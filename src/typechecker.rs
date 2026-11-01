//! Type checker for the parsed Gleam subset.
//!
//! The checker is Hindley–Milner style: every expression gets an inferred
//! type, and constraints from annotations, patterns, operators and calls
//! are solved by unification over a global substitution. Top-level functions
//! are generalized after their bodies are checked (they may be polymorphic)
//! and instantiated at each use site; values bound with `let` and anonymous
//! functions are not generalized.
//!
//! Name resolution mirrors `codegen.rs`: locals shadow constants, constants
//! shadow functions, module-qualified names resolve to native stdlib
//! functions only, and record fields are looked up across all constructors
//! (first match wins).

use crate::ast::*;
use crate::codegen::is_stdlib_module;
use std::collections::{HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A type error with a source location.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeCheckError {
    pub message: String,
    pub span: Span,
    /// 1-based line number (only when the source text is available).
    pub line: Option<u32>,
    /// 1-based column number (only when the source text is available).
    pub col: Option<u32>,
}

impl TypeCheckError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            line: None,
            col: None,
        }
    }
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.col) {
            (Some(line), Some(col)) => write!(f, "{line}:{col}: {}", self.message),
            _ => write!(
                f,
                "at byte {}..{}: {}",
                self.span.start, self.span.end, self.message
            ),
        }
    }
}

impl std::error::Error for TypeCheckError {}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A monomorphic type with (possibly unresolved) type variables.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// An unbound type variable.
    Var(u32),
    /// A named type application: `Int`, `List(Int)`, `order.Order`.
    App {
        module: Option<String>,
        name: String,
        args: Vec<Type>,
    },
    /// A function type: `fn(Int, String) -> Bool`.
    Fn(Vec<Type>, Box<Type>),
    /// A tuple type: `#(Int, String)`.
    Tuple(Vec<Type>),
    /// A bit array type with optional size and unit (in bits).
    BitArray {
        size: Option<u64>,
        unit: Option<u64>,
    },
}

impl Type {
    /// A nullary named type with no module qualifier.
    fn app(name: &str) -> Self {
        Type::App {
            module: None,
            name: name.into(),
            args: Vec::new(),
        }
    }

    fn app_args(module: Option<String>, name: &str, args: Vec<Type>) -> Self {
        Type::App {
            module,
            name: name.into(),
            args,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Var(id) => write!(f, "?t{id}"),
            Type::App { module, name, args } => {
                let prefix = match module {
                    Some(module) => format!("{module}.{name}"),
                    None => name.clone(),
                };
                match args.len() {
                    0 => f.write_str(&prefix),
                    _ => write!(f, "{prefix}({})", args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
                }
            }
            Type::Fn(args, ret) => write!(
                f,
                "fn({}) -> {}",
                args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "),
                ret
            ),
            Type::Tuple(elems) => write!(
                f,
                "#({})",
                elems.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
            ),
            Type::BitArray { size, unit } => match (size, unit) {
                (None, None) | (None, Some(_)) => f.write_str("BitArray"),
                (Some(size), None) => write!(f, "BitArray({size})"),
                (Some(size), Some(unit)) => write!(f, "BitArray({size}, {unit})"),
            },
        }
    }
}

// Convenience constructors for the built-in types.
fn t_int() -> Type { Type::app("Int") }
fn t_float() -> Type { Type::app("Float") }
fn t_bool() -> Type { Type::app("Bool") }
fn t_string() -> Type { Type::app("String") }
fn t_nil() -> Type { Type::app("Nil") }
fn t_list(elem: Type) -> Type { Type::app_args(None, "List", vec![elem]) }
fn t_fn(args: Vec<Type>, ret: Type) -> Type { Type::Fn(args, Box::new(ret)) }

// ---------------------------------------------------------------------------
// Unification
// ---------------------------------------------------------------------------

/// A substitution from type variables to types.
#[derive(Default)]
struct Subst(Vec<Option<Box<Type>>>);

impl Subst {
    fn new() -> Self {
        Self(Vec::new())
    }

    /// Allocate a fresh type variable.
    fn fresh(&mut self) -> Type {
        let id = self.0.len() as u32;
        self.0.push(None);
        Type::Var(id)
    }

    /// Allocate a fresh variable id without returning the type.
    fn new_var(&mut self) -> u32 {
        let id = self.0.len() as u32;
        self.0.push(None);
        id
    }

    /// Resolve all bindings in a type.
    fn apply(&self, t: &Type) -> Type {
        match t {
            Type::Var(id) => match self.0.get(*id as usize) {
                Some(Some(target)) => self.apply(target),
                _ => t.clone(),
            },
            Type::App { module, name, args } => Type::App {
                module: module.clone(),
                name: name.clone(),
                args: args.iter().map(|a| self.apply(a)).collect(),
            },
            Type::Fn(args, ret) => Type::Fn(
                args.iter().map(|a| self.apply(a)).collect(),
                Box::new(self.apply(ret)),
            ),
            Type::Tuple(elems) => Type::Tuple(elems.iter().map(|e| self.apply(e)).collect()),
            Type::BitArray { size, unit } => Type::BitArray { size: *size, unit: *unit },
        }
    }

    /// Unify two types, extending the substitution.
    fn unify(&mut self, a: &Type, b: &Type) -> Result<(), String> {
        match (a, b) {
            (Type::Var(i), Type::Var(j)) if i == j => Ok(()),
            (Type::Var(i), t) => self.bind(*i, t),
            (t, Type::Var(j)) => self.bind(*j, t),
            (
                Type::App { module: ma, name: na, args: aa },
                Type::App { module: mb, name: nb, args: ab },
            ) if ma == mb && na == nb && aa.len() == ab.len() => {
                for (x, y) in aa.iter().zip(ab.iter()) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            (Type::Fn(aa, ar), Type::Fn(ba, br)) if aa.len() == ba.len() => {
                for (x, y) in aa.iter().zip(ba.iter()) {
                    self.unify(x, y)?;
                }
                self.unify(ar, br)
            }
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b.iter()) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            (
                Type::BitArray { size: sa, unit: su },
                Type::BitArray { size: ba, unit: bu },
            ) => {
                if let (Some(x), Some(y)) = (*sa, *ba) {
                    if x != y {
                        return Err(format!("cannot unify `{a}` with `{b}`"));
                    }
                }
                if let (Some(x), Some(y)) = (*su, *bu) {
                    if x != y {
                        return Err(format!("cannot unify `{a}` with `{b}`"));
                    }
                }
                Ok(())
            }
            _ => Err(format!("cannot unify `{a}` with `{b}`")),
        }
    }

    fn bind(&mut self, id: u32, target: &Type) -> Result<(), String> {
        if occurs(id, target) {
            return Err("infinite type".into());
        }
        // Substitute the variable in all existing bindings.
        let mut entries = Vec::with_capacity(self.0.len());
        for entry in self.0.drain(..) {
            entries.push(entry.map(|t| Box::new(replace_in(*t, id, target))));
        }
        self.0 = entries;
        self.0[id as usize] = Some(Box::new(target.clone()));
        Ok(())
    }
}

fn occurs(id: u32, t: &Type) -> bool {
    match t {
        Type::Var(i) => *i == id,
        Type::App { args, .. } => args.iter().any(|a| occurs(id, a)),
        Type::Fn(args, ret) => args.iter().any(|a| occurs(id, a)) || occurs(id, ret),
        Type::Tuple(elems) => elems.iter().any(|e| occurs(id, e)),
        Type::BitArray { .. } => false,
    }
}

fn replace_in(t: Type, from: u32, to: &Type) -> Type {
    match t {
        Type::Var(i) if i == from => to.clone(),
        Type::Var(i) => Type::Var(i),
        Type::App { module, name, args } => Type::App {
            module,
            name,
            args: args.into_iter().map(|a| replace_in(a, from, to)).collect(),
        },
        Type::Fn(args, ret) => Type::Fn(
            args.into_iter().map(|a| replace_in(a, from, to)).collect(),
            Box::new(replace_in(*ret, from, to)),
        ),
        Type::Tuple(elems) => Type::Tuple(elems.into_iter().map(|e| replace_in(e, from, to)).collect()),
        other => other,
    }
}

fn collect_free(t: &Type, out: &mut HashSet<u32>) {
    match t {
        Type::Var(id) => { out.insert(*id); }
        Type::App { args, .. } => args.iter().for_each(|a| collect_free(a, out)),
        Type::Fn(args, ret) => {
            args.iter().for_each(|a| collect_free(a, out));
            collect_free(ret, out);
        }
        Type::Tuple(elems) => elems.iter().for_each(|e| collect_free(e, out)),
        Type::BitArray { .. } => {}
    }
}

fn replace_map(t: Type, map: &HashMap<u32, u32>) -> Type {
    match t {
        Type::Var(id) => Type::Var(map.get(&id).copied().unwrap_or(id)),
        Type::App { module, name, args } => Type::App {
            module,
            name,
            args: args.into_iter().map(|a| replace_map(a, map)).collect(),
        },
        Type::Fn(args, ret) => Type::Fn(
            args.into_iter().map(|a| replace_map(a, map)).collect(),
            Box::new(replace_map(*ret, map)),
        ),
        Type::Tuple(elems) => Type::Tuple(elems.into_iter().map(|e| replace_map(e, map)).collect()),
        other => other,
    }
}

/// Replace all free variables of a type with fresh ones (let-polymorphism).
fn generalize(t: Type, sub: &mut Subst) -> Type {
    let mut free = HashSet::new();
    collect_free(&t, &mut free);
    if free.is_empty() {
        return t;
    }
    let map: HashMap<u32, u32> = free.iter().map(|&id| (id, sub.new_var())).collect();
    replace_map(t, &map)
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TypeInfo {
    params: Vec<String>,
    kind: TypeKind,
}

#[derive(Clone)]
enum TypeKind {
    /// A plain type with no Gleam constructors (Int, List, external types).
    Bare,
    /// A type alias: transparently expanded at the use site.
    Alias(TypeAnnotation),
}

#[derive(Clone)]
struct Ctor {
    /// The name of the type this constructor belongs to.
    type_name: String,
    type_module: Option<String>,
    /// The type parameters of the parent type, in declaration order.
    params: Vec<String>,
    /// Field types as written (resolved against the parameter environment).
    fields: Vec<RecordField>,
}

struct FnInfo {
    labels: Vec<Option<String>>,
    ty: Type,
}

#[derive(Default)]
struct Env {
    /// Types keyed by `Name` or `module/Name`.
    types: HashMap<String, TypeInfo>,
    /// Constructors keyed by variant name (unqualified, as in codegen).
    constructors: HashMap<String, Ctor>,
    functions: HashMap<String, FnInfo>,
    constants: HashMap<String, Type>,
    /// Import aliases: `alias -> module path`.
    imports: HashMap<String, String>,
    /// Unqualified function imports: `name -> module path`.
    unqualified_fns: HashMap<String, String>,
    /// Unqualified type imports: `name -> module path`.
    unqualified_types: HashMap<String, String>,
}

impl Env {
    fn new() -> Self {
        let mut env = Self::default();
        for name in ["Int", "Float", "Bool", "String", "Nil"] {
            env.types.insert(name.into(), TypeInfo { params: vec![], kind: TypeKind::Bare });
        }
        env.types.insert("List".into(), TypeInfo { params: vec!["a".into()], kind: TypeKind::Bare });

        // option.Option
        env.types.insert("Option".into(), TypeInfo { params: vec!["a".into()], kind: TypeKind::Bare });
        env.insert_ctor("Some", "Option", None, &["a"], &[ann_var("a")]);
        env.insert_ctor("None", "Option", None, &["a"], &[]);

        // result.Result
        env.types.insert("Result".into(), TypeInfo { params: vec!["a".into(), "b".into()], kind: TypeKind::Bare });
        env.insert_ctor("Ok", "Result", None, &["a", "b"], &[ann_var("a")]);
        env.insert_ctor("Error", "Result", None, &["a", "b"], &[ann_var("b")]);

        // order.Order (returned by the compare functions).
        env.types.insert("order/Order".into(), TypeInfo { params: vec![], kind: TypeKind::Bare });
        for name in ["Eq", "Lt", "Gt"] {
            env.insert_ctor(name, "Order", Some("order"), &[], &[]);
        }

        // int.IntError (returned by base conversion functions).
        env.types.insert("int/IntError".into(), TypeInfo { params: vec![], kind: TypeKind::Bare });
        env.insert_ctor("InvalidBase", "IntError", Some("int"), &[], &[]);

        // list.ContinueOrStop (returned by the `list.fold_until` function).
        env.types.insert("list/ContinueOrStop".into(), TypeInfo { params: vec!["a".into()], kind: TypeKind::Bare });
        for name in ["Continue", "Stop"] {
            env.insert_ctor(name, "ContinueOrStop", Some("list"), &["a"], &[ann_var("a")]);
        }

        // fs.Error
        env.types.insert("fs/Error".into(), TypeInfo { params: vec![], kind: TypeKind::Bare });
        for name in ["NotFound", "PermissionDenied"] {
            env.insert_ctor(name, "Error", Some("fs"), &[], &[]);
        }
        env
    }

    fn insert_ctor(
        &mut self,
        name: &str,
        type_name: &str,
        module: Option<&str>,
        params: &[&str],
        fields: &[TypeAnnotation],
    ) {
        self.constructors.insert(
            name.into(),
            Ctor {
                type_name: type_name.into(),
                type_module: module.map(|m| m.into()),
                params: params.iter().map(|p| p.to_string()).collect(),
                fields: fields
                    .iter()
                    .map(|f| RecordField { label: None, name: None, type_annotation: f.clone() })
                    .collect(),
            },
        );
    }
}

fn ann_var(name: &str) -> TypeAnnotation {
    TypeAnnotation::Named {
        module: None,
        name: name.into(),
        args: vec![],
    }
}

// ---------------------------------------------------------------------------
// Standard library function types
//
// Keep in sync with `codegen::std_function` (C names, arities and labels).
// ---------------------------------------------------------------------------

fn std_function(sub: &mut Subst, module: &str, name: &str) -> Option<(Type, Vec<Option<String>>)> {
    let int = |n: usize| vec![t_int(); n];
    let s = || t_string();
    match (
        matches!(module, "gleam/io" | "io"),
        matches!(module, "gleam/fs" | "gleam/filesystem" | "fs" | "filesystem"),
        matches!(module, "gleam/string" | "string"),
        matches!(module, "gleam/int" | "int"),
        matches!(module, "gleam/list" | "list"),
        matches!(module, "gleam/cli" | "cli"),
        name,
    ) {
        (true, _, _, _, _, _, "print") => Some((t_fn(vec![s()], t_nil()), vec![Some("message".into())])),
        (true, _, _, _, _, _, "println") => Some((t_fn(vec![s()], t_nil()), vec![Some("message".into())])),
        (true, _, _, _, _, _, "debug") => {
            let a = sub.fresh(); // opaque parameter, fresh per lookup
            Some((t_fn(vec![a], t_nil()), vec![Some("value".into())]))
        }
        (true, _, _, _, _, _, "read_line" | "readln") => {
            Some((t_fn(vec![], t_result(s(), t_nil())), vec![]))
        }
        (_, true, _, _, _, _, "read" | "read_file") => Some((
            t_fn(vec![s()], t_result(s(), t_app("fs/Error"))),
            vec![Some("path".into())],
        )),
        (_, true, _, _, _, _, "write" | "write_file") => Some((
            t_fn(vec![s(), s()], t_result(t_nil(), t_app("fs/Error"))),
            vec![Some("path".into()), Some("contents".into())],
        )),
        (_, true, _, _, _, _, "append" | "append_file") => Some((
            t_fn(vec![s(), s()], t_result(t_nil(), t_app("fs/Error"))),
            vec![Some("path".into()), Some("contents".into())],
        )),
        (_, true, _, _, _, _, "remove" | "delete") => Some((
            t_fn(vec![s()], t_result(t_nil(), t_app("fs/Error"))),
            vec![Some("path".into())],
        )),
        (_, true, _, _, _, _, "is_file") => Some((t_fn(vec![s()], t_bool()), vec![Some("path".into())])),
        (_, true, _, _, _, _, "is_directory") => Some((t_fn(vec![s()], t_bool()), vec![Some("path".into())])),
        (_, true, _, _, _, _, "create_directory") => Some((
            t_fn(vec![s()], t_result(t_nil(), t_app("fs/Error"))),
            vec![Some("path".into())],
        )),
        (_, true, _, _, _, _, "list_directory" | "list") => Some((
            t_fn(vec![s()], t_result(t_list(s()), t_app("fs/Error"))),
            vec![Some("path".into())],
        )),
        // gleam/string (grapheme and UTF code point functions excluded).
        (_, _, true, _, _, _, "append") => Some((t_fn(vec![s(), s()], s()), vec![Some("to".into()), Some("suffix".into())])),
        (_, _, true, _, _, _, "byte_size") => Some((t_fn(vec![s()], t_int()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "capitalise") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "compare") => {
            Some((t_fn(vec![s(), s()], t_app("order/Order")), vec![None, None]))
        }
        (_, _, true, _, _, _, "concat") => Some((t_fn(vec![t_list(s())], s()), vec![Some("strings".into())])),
        (_, _, true, _, _, _, "contains") => Some((t_fn(vec![s(), s()], t_bool()), vec![Some("does".into()), Some("contain".into())])),
        (_, _, true, _, _, _, "crop") => Some((t_fn(vec![s(), s()], s()), vec![Some("from".into()), Some("before".into())])),
        (_, _, true, _, _, _, "ends_with") => Some((t_fn(vec![s(), s()], t_bool()), vec![None, None])),
        (_, _, true, _, _, _, "first") => Some((t_fn(vec![s()], t_result(s(), t_nil())), vec![Some("string".into())])),
        (_, _, true, _, _, _, "inspect") => {
            let a = sub.fresh(); // opaque parameter, fresh per lookup
            Some((t_fn(vec![a], s()), vec![Some("term".into())]))
        }
        (_, _, true, _, _, _, "is_empty") => Some((t_fn(vec![s()], t_bool()), vec![Some("str".into())])),
        (_, _, true, _, _, _, "join") => Some((t_fn(vec![t_list(s()), s()], s()), vec![Some("strings".into()), Some("with".into())])),
        (_, _, true, _, _, _, "last") => Some((t_fn(vec![s()], t_result(s(), t_nil())), vec![Some("string".into())])),
        (_, _, true, _, _, _, "length") => Some((t_fn(vec![s()], t_int()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "lowercase") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "pad_end") => Some((t_fn(vec![s(), t_int(), s()], s()), vec![Some("string".into()), Some("to".into()), Some("with".into())])),
        (_, _, true, _, _, _, "pad_start") => Some((t_fn(vec![s(), t_int(), s()], s()), vec![Some("string".into()), Some("to".into()), Some("with".into())])),
        (_, _, true, _, _, _, "remove_prefix") => Some((t_fn(vec![s(), s()], s()), vec![Some("from".into()), Some("matching".into())])),
        (_, _, true, _, _, _, "remove_suffix") => Some((t_fn(vec![s(), s()], s()), vec![Some("from".into()), Some("matching".into())])),
        (_, _, true, _, _, _, "repeat") => Some((t_fn(vec![s(), t_int()], s()), vec![Some("string".into()), Some("times".into())])),
        (_, _, true, _, _, _, "replace") => Some((t_fn(vec![s(), s(), s()], s()), vec![Some("in".into()), Some("each".into()), Some("with".into())])),
        (_, _, true, _, _, _, "reverse") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "slice") => Some((t_fn(vec![s(), t_int(), t_int()], s()), vec![Some("from".into()), Some("at_index".into()), Some("length".into())])),
        (_, _, true, _, _, _, "split") => Some((t_fn(vec![s(), s()], t_list(s())), vec![None, Some("on".into())])),
        (_, _, true, _, _, _, "split_once") => Some((
            t_fn(vec![s(), s()], t_result(Type::Tuple(vec![s(), s()]), t_nil())),
            vec![Some("string".into()), Some("on".into())],
        )),
        (_, _, true, _, _, _, "starts_with") => Some((t_fn(vec![s(), s()], t_bool()), vec![None, None])),
        (_, _, true, _, _, _, "to_option") => Some((t_fn(vec![s()], t_option(s())), vec![Some("string".into())])),
        (_, _, true, _, _, _, "trim") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "trim_end") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "trim_start") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        (_, _, true, _, _, _, "uppercase") => Some((t_fn(vec![s()], s()), vec![Some("string".into())])),
        // gleam/int.
        (_, _, _, true, _, _, "absolute_value") => Some((t_fn(vec![t_int()], t_int()), vec![Some("num".into())])),
        (_, _, _, true, _, _, "clamp") => Some((t_fn(int(3), t_int()), vec![None, Some("min".into()), Some("max".into())])),
        (_, _, _, true, _, _, "compare") => {
            Some((t_fn(vec![t_int(), t_int()], t_app("order/Order")), vec![None, Some("with".into())]))
        }
        (_, _, _, true, _, _, "digits") => Some((
            t_fn(vec![t_int(), t_int()], t_result(t_list(t_int()), t_app("int/IntError"))),
            vec![Some("number".into()), Some("base".into())],
        )),
        (_, _, _, true, _, _, "is_even") => Some((t_fn(vec![t_int()], t_bool()), vec![Some("x".into())])),
        (_, _, _, true, _, _, "is_odd") => Some((t_fn(vec![t_int()], t_bool()), vec![Some("x".into())])),
        (_, _, _, true, _, _, "max") => Some((t_fn(vec![t_int(), t_int()], t_int()), vec![None, None])),
        (_, _, _, true, _, _, "min") => Some((t_fn(vec![t_int(), t_int()], t_int()), vec![None, None])),
        (_, _, _, true, _, _, "negate") => Some((t_fn(vec![t_int()], t_int()), vec![Some("x".into())])),
        (_, _, _, true, _, _, "parse") => Some((t_fn(vec![s()], t_result(t_int(), t_nil())), vec![Some("string".into())])),
        (_, _, _, true, _, _, "product") => Some((t_fn(vec![t_list(t_int())], t_int()), vec![Some("numbers".into())])),
        (_, _, _, true, _, _, "random") => Some((t_fn(vec![t_int(), t_int()], t_int()), vec![None, None])),
        (_, _, _, true, _, _, "sum") => Some((t_fn(vec![t_list(t_int())], t_int()), vec![Some("numbers".into())])),
        (_, _, _, true, _, _, "to_base16") => Some((t_fn(vec![t_int()], s()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "to_base2") => Some((t_fn(vec![t_int()], s()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "to_base36") => Some((t_fn(vec![t_int()], s()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "to_base8") => Some((t_fn(vec![t_int()], s()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "to_base_string") => Some((
            t_fn(vec![t_int(), t_int()], t_result(s(), t_app("int/IntError"))),
            vec![Some("int".into()), Some("base".into())],
        )),
        (_, _, _, true, _, _, "to_float") => Some((t_fn(vec![t_int()], t_float()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "to_string") => Some((t_fn(vec![t_int()], s()), vec![Some("int".into())])),
        (_, _, _, true, _, _, "undigits") => Some((
            t_fn(vec![t_list(t_int()), t_int()], t_result(t_int(), t_app("int/IntError"))),
            vec![Some("numbers".into()), Some("base".into())],
        )),
        // gleam/list. `group` is excluded: it requires `gleam/dict`.
        (_, _, _, _, true, _, "all") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_bool()),
                vec![Some("in".into()), Some("satisfying".into())],
            ))
        }
        (_, _, _, _, true, _, "any") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_bool()),
                vec![Some("in".into()), Some("satisfying".into())],
            ))
        }
        (_, _, _, _, true, _, "append") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_list(a.clone())], t_list(a.clone())),
                vec![],
            ))
        }
        (_, _, _, _, true, _, "chunk") => {
            let a = sub.fresh();
            let k = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], k.clone())], t_list(t_list(a.clone()))),
                vec![Some("in".into()), Some("by".into())],
            ))
        }
        (_, _, _, _, true, _, "combination_pairs") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone())], t_list(Type::Tuple(vec![a.clone().clone(), a.clone()]))),
                vec![None],
            ))
        }
        (_, _, _, _, true, _, "combinations") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(t_list(a.clone()))),
                vec![None, Some("by".into())],
            ))
        }
        (_, _, _, _, true, _, "contains") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone()), a.clone()], t_bool()), vec![]))
        }
        (_, _, _, _, true, _, "count") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_int()),
                vec![Some("list".into()), Some("where".into())],
            ))
        }
        (_, _, _, _, true, _, "drop") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(a.clone())),
                vec![Some("from".into()), Some("up_to".into())],
            ))
        }
        (_, _, _, _, true, _, "drop_while") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_list(a.clone())),
                vec![Some("in".into()), Some("satisfying".into())],
            ))
        }
        (_, _, _, _, true, _, "each") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_nil())], t_nil()), vec![]))
        }
        (_, _, _, _, true, _, "filter") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_list(a.clone())),
                vec![Some("list".into()), Some("keeping".into())],
            ))
        }
        (_, _, _, _, true, _, "filter_map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            let e = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_result(b.clone().clone(), e.clone()))], t_list(b.clone())),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "find") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_result(a.clone(), t_nil())),
                vec![Some("in".into()), Some("one_that".into())],
            ))
        }
        (_, _, _, _, true, _, "find_map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            let c = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_result(b.clone().clone(), c.clone()))], t_result(b.clone(), t_nil())),
                vec![Some("in".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "first") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_result(a.clone(), t_nil())), vec![None]))
        }
        (_, _, _, _, true, _, "flat_map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_list(b.clone().clone()))], t_list(b.clone())),
                vec![Some("over".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "flatten") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(t_list(a.clone().clone()))], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "fold") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone()], acc.clone().clone())],
                    acc.clone(),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "fold_right") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone()], acc.clone().clone())],
                    acc.clone(),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "fold_until") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            let step = Type::app_args(Some("list".into()), "ContinueOrStop", vec![acc.clone().clone()]);
            Some((
                t_fn(vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone()], step)], acc.clone()),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        // `group` needs `gleam/dict`, which this compiler does not have.
        (_, _, _, _, true, _, "index_fold") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone(), t_int()], acc.clone().clone())],
                    acc.clone(),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "index_map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![a.clone(), t_int()], b.clone())], t_list(b.clone())),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "interleave") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(t_list(a.clone().clone()))], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "intersperse") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), a.clone()], t_list(a.clone())),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "is_empty") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone())], t_bool()), vec![None]))
        }
        (_, _, _, _, true, _, "key_filter") => {
            let k = sub.fresh();
            let v = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(Type::Tuple(vec![k.clone().clone(), v.clone().clone()])), k.clone()],
                    t_list(v.clone()),
                ),
                vec![Some("in".into()), Some("find".into())],
            ))
        }
        (_, _, _, _, true, _, "key_find") => {
            let k = sub.fresh();
            let v = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(Type::Tuple(vec![k.clone(), v.clone()])), k.clone()],
                    t_result(v.clone(), t_nil()),
                ),
                vec![Some("in".into()), Some("find".into())],
            ))
        }
        (_, _, _, _, true, _, "key_pop") => {
            let k = sub.fresh();
            let v = sub.fresh();
            let pair = Type::Tuple(vec![k.clone().clone(), v.clone().clone()]);
            Some((
                t_fn(
                    vec![t_list(pair.clone()), k.clone()],
                    t_result(Type::Tuple(vec![v.clone(), t_list(pair)]), t_nil()),
                ),
                vec![None, Some("key".into())],
            ))
        }
        (_, _, _, _, true, _, "key_set") => {
            let k = sub.fresh();
            let v = sub.fresh();
            let pair = Type::Tuple(vec![k.clone().clone(), v.clone().clone()]);
            Some((
                t_fn(vec![t_list(pair.clone()), k.clone(), v.clone()], t_list(pair)),
                vec![None, Some("key".into()), Some("value".into())],
            ))
        }
        (_, _, _, _, true, _, "last") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_result(a.clone(), t_nil())), vec![None]))
        }
        (_, _, _, _, true, _, "length") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone())], t_int()), vec![Some("of".into())]))
        }
        (_, _, _, _, true, _, "map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            // The closure must take the list element type (`a`) and may
            // return a new type (`b`), matching `gleam/list.map`.
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![a], b.clone())], t_list(b)),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "map2") => {
            let a = sub.fresh();
            let b = sub.fresh();
            let c = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_list(b.clone()), t_fn(vec![a.clone(), b.clone()], c.clone())], t_list(c.clone())),
                vec![None, None, Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "map_fold") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(
                    vec![
                        t_list(a.clone()),
                        acc.clone().clone(),
                        t_fn(vec![acc.clone().clone(), a.clone()], Type::Tuple(vec![acc.clone().clone(), b.clone().clone()])),
                    ],
                    Type::Tuple(vec![acc.clone(), t_list(b.clone())]),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "max") => {
            let a = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone().clone()), t_fn(vec![a.clone().clone(), a.clone()], t_app("order/Order"))],
                    t_result(a.clone(), t_nil()),
                ),
                vec![Some("over".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "new") => {
            let a = sub.fresh();
            Some((t_fn(vec![], t_list(a.clone())), vec![]))
        }
        (_, _, _, _, true, _, "partition") => {
            let a = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())],
                    Type::Tuple(vec![t_list(a.clone().clone()), t_list(a.clone())]),
                ),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "permutations") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_list(t_list(a.clone()))), vec![None]))
        }
        (_, _, _, _, true, _, "prepend") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), a.clone()], t_list(a.clone())),
                vec![Some("to".into()), Some("this".into())],
            ))
        }
        (_, _, _, _, true, _, "reduce") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone().clone(), a.clone()], a.clone())], t_result(a.clone(), t_nil())),
                vec![Some("over".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "repeat") => {
            let a = sub.fresh();
            Some((t_fn(vec![a.clone().clone(), t_int()], t_list(a.clone())), vec![Some("item".into()), Some("times".into())]))
        }
        (_, _, _, _, true, _, "rest") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone())], t_result(t_list(a.clone()), t_nil())),
                vec![None],
            ))
        }
        (_, _, _, _, true, _, "reverse") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "sample") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(a.clone())),
                vec![Some("from".into()), Some("up_to".into())],
            ))
        }
        (_, _, _, _, true, _, "scan") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone()], acc.clone().clone())],
                    t_list(acc.clone()),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "shuffle") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "sized_chunk") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(t_list(a.clone()))),
                vec![Some("in".into()), Some("into".into())],
            ))
        }
        (_, _, _, _, true, _, "sort") => {
            let a = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone().clone()), t_fn(vec![a.clone().clone(), a.clone()], t_app("order/Order"))],
                    t_list(a.clone()),
                ),
                vec![Some("list".into()), Some("by".into())],
            ))
        }
        (_, _, _, _, true, _, "split") => {
            let a = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone().clone()), t_int()],
                    Type::Tuple(vec![t_list(a.clone().clone()), t_list(a.clone())]),
                ),
                vec![Some("list".into()), Some("at".into())],
            ))
        }
        (_, _, _, _, true, _, "split_while") => {
            let a = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())],
                    Type::Tuple(vec![t_list(a.clone().clone()), t_list(a.clone())]),
                ),
                vec![Some("list".into()), Some("satisfying".into())],
            ))
        }
        (_, _, _, _, true, _, "strict_zip") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), t_list(b.clone())],
                    t_result(t_list(Type::Tuple(vec![a.clone(), b.clone()])), t_nil()),
                ),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "take") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(a.clone())),
                vec![Some("from".into()), Some("up_to".into())],
            ))
        }
        (_, _, _, _, true, _, "take_while") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_fn(vec![a.clone()], t_bool())], t_list(a.clone())),
                vec![Some("in".into()), Some("satisfying".into())],
            ))
        }
        (_, _, _, _, true, _, "transpose") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(t_list(a.clone().clone()))], t_list(t_list(a.clone()))),
                vec![None],
            ))
        }
        (_, _, _, _, true, _, "try_each") => {
            let a = sub.fresh();
            let b = sub.fresh();
            let e = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_result(b.clone(), e.clone().clone()))], t_result(t_nil(), e.clone())),
                vec![Some("over".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "try_fold") => {
            let a = sub.fresh();
            let acc = sub.fresh();
            let e = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), acc.clone().clone(), t_fn(vec![acc.clone().clone(), a.clone()], t_result(acc.clone().clone(), e.clone().clone()))],
                    t_result(acc.clone(), e.clone()),
                ),
                vec![Some("over".into()), Some("from".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "try_map") => {
            let a = sub.fresh();
            let b = sub.fresh();
            let e = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone()), t_fn(vec![b.clone()], t_result(b.clone().clone(), e.clone().clone()))], t_result(t_list(b.clone()), e.clone())),
                vec![Some("over".into()), Some("with".into())],
            ))
        }
        (_, _, _, _, true, _, "unique") => {
            let a = sub.fresh();
            Some((t_fn(vec![t_list(a.clone().clone())], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "unzip") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(Type::Tuple(vec![a.clone().clone(), b.clone().clone()]))],
                    Type::Tuple(vec![t_list(a.clone()), t_list(b.clone())]),
                ),
                vec![None],
            ))
        }
        (_, _, _, _, true, _, "window") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone()), t_int()], t_list(t_list(a.clone()))),
                vec![Some("list".into()), Some("by".into())],
            ))
        }
        (_, _, _, _, true, _, "window_by_2") => {
            let a = sub.fresh();
            Some((
                t_fn(vec![t_list(a.clone().clone())], t_list(Type::Tuple(vec![a.clone().clone(), a.clone()]))),
                vec![None],
            ))
        }
        (_, _, _, _, true, _, "wrap") => {
            let a = sub.fresh();
            Some((t_fn(vec![a.clone()], t_list(a.clone())), vec![None]))
        }
        (_, _, _, _, true, _, "zip") => {
            let a = sub.fresh();
            let b = sub.fresh();
            Some((
                t_fn(
                    vec![t_list(a.clone()), t_list(b.clone())],
                    t_list(Type::Tuple(vec![a.clone(), b.clone()])),
                ),
                vec![Some("list".into()), Some("with".into())],
            ))
        }
        // gleam/cli
        (_, _, _, _, _, true, "args") => Some((t_fn(vec![], t_list(s())), vec![])),
        _ => None,
    }
}

fn t_option(elem: Type) -> Type {
    Type::app_args(None, "Option", vec![elem])
}

fn t_result(ok: Type, err: Type) -> Type {
    Type::app_args(None, "Result", vec![ok, err])
}

fn t_app(key: &str) -> Type {
    // Keys like "order/Order" as registered in the environment.
    match key.split_once('/') {
        Some((module, name)) => Type::app_args(Some(module.into()), name, vec![]),
        None => Type::app(key),
    }
}

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// A resolved callable: parameter types, argument labels and result type.
struct Callee {
    params: Vec<Type>,
    labels: Vec<Option<String>>,
    ret: Type,
}

struct Checker<'a> {
    source: Option<&'a str>,
    module: &'a Module,
    env: Env,
    sub: Subst,
    scopes: Vec<HashMap<String, Type>>,
}

/// Type check a parsed module. Line and column numbers are not available
/// (no source text), so errors carry byte spans only.
pub fn typecheck(module: &Module) -> Result<(), TypeCheckError> {
    Checker::new(module, None).and_then(|checker| checker.check())
}

/// Type check a parsed module with the original source text available, so
/// errors can report line and column numbers.
pub fn typecheck_source(source: &str, module: &Module) -> Result<(), TypeCheckError> {
    Checker::new(module, Some(source)).and_then(|checker| checker.check())
}

impl<'a> Checker<'a> {
    /// Pass 1: register types, constants, function signatures and imports.
    fn new(module: &'a Module, source: Option<&'a str>) -> Result<Self, TypeCheckError> {
        let mut checker = Checker {
            source,
            module,
            env: Env::new(),
            sub: Subst::new(),
            scopes: Vec::new(),
        };

        // Constants may reference each other; register names up front.
        for definition in module.definitions.iter().filter_map(|d| match d {
            Definition::Constant(c) => Some(c),
            _ => None,
        }) {
            if checker.env.constants.insert(definition.name.clone(), checker.sub.fresh()).is_some() {
                return Err(checker.error(
                    format!("duplicate constant `{}`", definition.name),
                    definition.span,
                ));
            }
        }

        for definition in &module.definitions {
            match definition {
                Definition::Type(ty) => checker.register_type(ty)?,
                Definition::Constant(c) => {
                    let value = checker.const_value_type(&c.value, c.span)?;
                    if let Some(annotation) = &c.annotation {
                        let expected = checker.resolve_annotation(annotation, &HashMap::new(), 0)?;
                        checker.unify_with(&expected, &value, c.span)?;
                    }
                    *checker.env.constants.get_mut(&c.name).expect("registered above") = value;
                }
                Definition::Fn(f) => checker.register_function(f)?,
                Definition::Import(import) => {
                    let path = import.module.join("/");
                    let alias = import
                        .alias
                        .clone()
                        .or_else(|| import.module.last().cloned())
                        .unwrap();
                    if checker.env.imports.insert(alias.clone(), path.clone()).is_some() {
                        return Err(checker.error(
                            format!("duplicate import alias `{alias}`"),
                            import.span,
                        ));
                    }
                    for item in &import.unqualified {
                        match item {
                            UnqualifiedImport::Value(name) => {
                                if checker
                                    .env
                                    .unqualified_fns
                                    .insert(name.clone(), path.clone())
                                    .is_some()
                                {
                                    return Err(checker.error(
                                        format!("duplicate unqualified import `{name}`"),
                                        import.span,
                                    ));
                                }
                            }
                            UnqualifiedImport::Type(name) => {
                                if checker
                                    .env
                                    .unqualified_types
                                    .insert(name.clone(), path.clone())
                                    .is_some()
                                {
                                    return Err(checker.error(
                                        format!("duplicate unqualified import `type {name}`"),
                                        import.span,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(checker)
    }

    /// Pass 2: check every function body against its signature.
    fn check(mut self) -> Result<(), TypeCheckError> {
        for definition in &self.module.definitions {
            let Definition::Fn(f) = definition else {
                continue;
            };
            // @external functions have no body to check.
            if matches!(f.body.expr, Expr::Block(ref body) if body.is_empty()) {
                continue;
            }
            self.check_function_body(f)?;
        }
        Ok(())
    }

    fn register_type(&mut self, ty: &TypeDef) -> Result<(), TypeCheckError> {
        if self.env.types.contains_key(&ty.name) {
            return Err(self.error(format!("duplicate type `{}`", ty.name), ty.span));
        }
        let kind = match &ty.kind {
            TypeDefKind::Record(variants) => {
                for variant in variants {
                    // A true duplicate is a second variant of the *same*
                    // type. Constructors of other types may share a bare
                    // name (e.g. `list.ContinueOrStop.Continue` vs a local
                    // `Continue`); the local definition then shadows the
                    // built-in in unqualified lookup, matching codegen.
                    let is_duplicate = self.env.constructors.get(&variant.name)
                        .is_some_and(|c| c.type_name == ty.name && c.type_module.is_none());
                    if is_duplicate {
                        return Err(self.error(format!("duplicate constructor `{}`", variant.name), ty.span));
                    }
                    self.env.constructors.insert(
                        variant.name.clone(),
                        Ctor {
                            type_name: ty.name.clone(),
                            type_module: None,
                            params: ty.params.clone(),
                            fields: variant.fields.clone(),
                        },
                    );
                }
                TypeKind::Bare
            }
            TypeDefKind::Alias(annotation) => TypeKind::Alias(annotation.clone()),
            TypeDefKind::External => TypeKind::Bare,
        };
        self.env
            .types
            .insert(ty.name.clone(), TypeInfo { params: ty.params.clone(), kind });
        Ok(())
    }

    fn register_function(&mut self, f: &FnDef) -> Result<(), TypeCheckError> {
        if self.env.functions.contains_key(&f.name) {
            return Err(self.error(format!("duplicate function `{}`", f.name), f.span));
        }
        let mut param_env = HashMap::new();
        // Lowercase names in annotations are type variables; give each name
        // one fresh variable for the whole signature.
        let mut names = Vec::new();
        for arg in &f.args {
            if let Some(annotation) = &arg.type_annotation {
                collect_annotation_names(annotation, &mut names);
            }
        }
        if let Some(annotation) = &f.return_annotation {
            collect_annotation_names(annotation, &mut names);
        }
        for name in names {
            param_env.entry(name).or_insert_with(|| self.sub.fresh());
        }

        let arg_types: Vec<Type> = f
            .args
            .iter()
            .map(|arg| match &arg.type_annotation {
                Some(annotation) => self.resolve_annotation(annotation, &param_env, 0),
                None => Ok(self.sub.fresh()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match &f.return_annotation {
            Some(annotation) => self.resolve_annotation(annotation, &param_env, 0)?,
            None => self.sub.fresh(),
        };

        self.env.functions.insert(
            f.name.clone(),
            FnInfo {
                labels: f
                    .args
                    .iter()
                    .map(|arg| arg.label.clone().or_else(|| Some(arg.name.clone())))
                    .collect(),
                ty: t_fn(arg_types, ret),
            },
        );
        Ok(())
    }

    fn check_function_body(&mut self, f: &FnDef) -> Result<(), TypeCheckError> {
        let signature = self.sub.apply(&self.env.functions[&f.name].ty);
        let (arg_types, ret) = match signature {
            Type::Fn(args, ret) => (args, *ret),
            _ => unreachable!("function signatures are always function types"),
        };

        self.scopes.push(HashMap::new());
        for (arg, ty) in f.args.iter().zip(arg_types.iter()) {
            if !arg.name.starts_with('_') {
                self.bind_var(&arg.name, ty.clone(), f.span)?;
            }
        }
        let body_ty = self.infer(&f.body)?;
        self.unify_with(&ret, &body_ty, f.body.span)?;
        self.scopes.pop();

        // Generalize the signature so later use sites get fresh instances.
        let final_signature = self.sub.apply(&t_fn(arg_types, ret));
        if let Some(info) = self.env.functions.get_mut(&f.name) {
            info.ty = generalize(final_signature, &mut self.sub);
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Errors & unification helpers
    // ------------------------------------------------------------------

    fn error(&self, message: impl Into<String>, span: Span) -> TypeCheckError {
        let mut error = TypeCheckError::new(message, span);
        if let Some(source) = self.source {
            let (line, col) = span.line_col(source);
            error.line = Some(line);
            error.col = Some(col);
        }
        error
    }

    /// Unify `found` against an expected type with a readable message.
    fn unify_with(&mut self, expected: &Type, found: &Type, span: Span) -> Result<(), TypeCheckError> {
        let (expected, found) = (self.sub.apply(expected), self.sub.apply(found));
        self.sub
            .unify(&expected, &found)
            .map_err(|_| self.error(format!("type mismatch: expected {expected}, found {found}"), span))
    }

    /// Unify two inferred types with each other.
    fn unify_pair(&mut self, a: &Type, b: &Type, span: Span) -> Result<(), TypeCheckError> {
        let (a, b) = (self.sub.apply(a), self.sub.apply(b));
        self.sub
            .unify(&a, &b)
            .map_err(|_| self.error(format!("type mismatch: cannot unify {a} with {b}"), span))
    }

    fn bind_var(&mut self, name: &str, ty: Type, span: Span) -> Result<(), TypeCheckError> {
        let scope = self.scopes.last_mut().expect("a scope is always open");
        if scope.contains_key(name) {
            return Err(self.error(format!("variable `{name}` is already bound in this scope"), span));
        }
        scope.insert(name.into(), ty);
        Ok(())
    }

    /// Look up a value: locals, then constants, then functions, then
    /// unqualified stdlib imports. Functions are instantiated on lookup.
    fn lookup_value(&mut self, name: &str) -> Option<Type> {
        if let Some(ty) = self.scopes.iter().rev().find_map(|scope| scope.get(name)) {
            return Some(self.sub.apply(ty));
        }
        if let Some(ty) = self.env.constants.get(name) {
            return Some(self.sub.apply(ty));
        }
        if let Some(ty) = self.env.functions.get(name).map(|info| info.ty.clone()) {
            return Some(self.instantiate(&ty));
        }
        if let Some(module) = self.env.unqualified_fns.get(name) {
            if let Some((ty, _)) = std_function(&mut self.sub, module, name) {
                return Some(ty);
            }
        }
        None
    }

    /// Replace all free variables of a type with fresh ones so each use site
    /// of a (possibly polymorphic) function gets its own instance.
    fn instantiate(&mut self, ty: &Type) -> Type {
        let mut free = HashSet::new();
        collect_free(ty, &mut free);
        if free.is_empty() {
            return self.sub.apply(ty);
        }
        let map: HashMap<u32, u32> = free.iter().map(|&id| (id, self.sub.new_var())).collect();
        replace_map(self.sub.apply(ty), &map)
    }

    // ------------------------------------------------------------------
    // Type annotations
    // ------------------------------------------------------------------

    fn resolve_annotation(
        &mut self,
        annotation: &TypeAnnotation,
        params: &HashMap<String, Type>,
        depth: u32,
    ) -> Result<Type, TypeCheckError> {
        match annotation {
            TypeAnnotation::Named { module, name, args } => {
                // Lowercase names are type variables of the enclosing signature.
                if name.starts_with(|c: char| c.is_ascii_lowercase()) && module.is_none() {
                    return params
                        .get(name)
                        .cloned()
                        .ok_or_else(|| self.error(format!("unknown type `{name}`"), Span::point(0)));
                }

                // Candidate keys, in priority order.
                let mut candidates: Vec<String> = Vec::new();
                if let Some(module) = module {
                    candidates.push(format!("{module}/{name}"));
                    candidates.push(name.clone());
                } else if let Some(path) = self.env.unqualified_types.get(name) {
                    candidates.push(format!("{path}/{name}"));
                    candidates.push(name.clone());
                } else {
                    candidates.push(name.clone());
                }

                let key = candidates
                    .iter()
                    .find(|key| self.env.types.contains_key(*key))
                    .ok_or_else(|| {
                        self.error(
                            format!(
                                "unknown type `{}`",
                                module.as_ref().map(|m| format!("{m}.{name}")).unwrap_or(name.clone())
                            ),
                            Span::point(0),
                        )
                    })?;
                let info = self.env.types.get(key).expect("checked above").clone();
                if info.params.len() != args.len() {
                    return Err(self.error(
                        format!(
                            "type `{name}` expects {} type parameter(s), got {}",
                            info.params.len(),
                            args.len()
                        ),
                        Span::point(0),
                    ));
                }

                // Bind the type's parameters to its actual arguments.
                let mut param_env = HashMap::new();
                for (param, arg) in info.params.iter().zip(args.iter()) {
                    param_env.insert(param.clone(), self.resolve_annotation(arg, params, depth)?);
                }

                match &info.kind {
                    TypeKind::Alias(inner) => {
                        if depth > 16 {
                            return Err(self.error(format!("cyclic type alias `{name}`"), Span::point(0)));
                        }
                        self.resolve_annotation(inner, &param_env, depth + 1)
                    }
                    TypeKind::Bare => {
                        // Arguments in declaration order (HashMap order is random).
                        let args = info.params.iter().map(|p| param_env[p].clone()).collect();
                        Ok(Type::app_args(module.clone(), name, args))
                    }
                }
            }
            TypeAnnotation::Fn { args, ret } => {
                let mut resolved = Vec::with_capacity(args.len());
                for arg in args {
                    resolved.push(self.resolve_annotation(arg, params, depth)?);
                }
                Ok(t_fn(resolved, self.resolve_annotation(ret, params, depth)?))
            }
            TypeAnnotation::Tuple(elems) => {
                let mut resolved = Vec::with_capacity(elems.len());
                for elem in elems {
                    resolved.push(self.resolve_annotation(elem, params, depth)?);
                }
                Ok(Type::Tuple(resolved))
            }
            TypeAnnotation::BitArray { size, unit } => Ok(Type::BitArray {
                size: size.as_ref().and_then(|v| v.as_u64()),
                unit: unit.as_ref().and_then(|v| v.as_u64()),
            }),
        }
    }

    /// The type of a record field, resolved against the constructor's
    /// parameters (which may be fresh variables).
    fn field_type(&mut self, ctor: &Ctor, index: usize) -> Result<Type, TypeCheckError> {
        let mut param_env = HashMap::new();
        for param in &ctor.params {
            param_env.insert(param.clone(), self.sub.fresh());
        }
        self.resolve_annotation(&ctor.fields[index].type_annotation, &param_env, 0)
    }

    /// The record type produced by a constructor, with fresh type arguments.
    fn ctor_record_type(&mut self, ctor: &Ctor) -> Type {
        let args = (0..ctor.params.len()).map(|_| self.sub.fresh()).collect();
        Type::app_args(ctor.type_module.clone(), &ctor.type_name, args)
    }

    // ------------------------------------------------------------------
    // Constants
    // ------------------------------------------------------------------

    fn const_value_type(&mut self, value: &ConstValue, span: Span) -> Result<Type, TypeCheckError> {
        match value {
            ConstValue::Int(_) => Ok(t_int()),
            ConstValue::Float(_) => Ok(t_float()),
            ConstValue::String(_) => Ok(t_string()),
            ConstValue::Bool(_) => Ok(t_bool()),
            ConstValue::Nil => Ok(t_nil()),
            ConstValue::Var(name) => self
                .env
                .constants
                .get(name)
                .map(|ty| self.sub.apply(ty))
                .ok_or_else(|| self.error(format!("unknown constant `{name}`"), span)),
            ConstValue::Constructor(reference, args) => {
                let ctor = self
                    .env
                    .constructors
                    .get(&reference.name)
                    .cloned()
                    .ok_or_else(|| self.error(format!("unknown constructor `{}`", reference.name), span))?;
                if args.len() != ctor.fields.len() {
                    return Err(self.error(
                        format!(
                            "`{}` expects {} field(s), but the constant has {}",
                            reference.name,
                            ctor.fields.len(),
                            args.len()
                        ),
                        span,
                    ));
                }
                let record = self.ctor_record_type(&ctor);
                for (index, arg) in args.iter().enumerate() {
                    if let Some(label) = &arg.label {
                        let expected = ctor.fields[index]
                            .label
                            .as_deref()
                            .or(ctor.fields[index].name.as_deref());
                        if expected != Some(label.as_str()) {
                            return Err(self.error(
                                format!("no field labeled `{label}` in `{}`", reference.name),
                                span,
                            ));
                        }
                    }
                    let ty = self.field_type(&ctor, index)?;
                    let value_ty = self.const_value_type(&arg.value, span)?;
                    self.unify_with(&ty, &value_ty, span)?;
                }
                Ok(record)
            }
            ConstValue::List { elements, spread } => {
                let elem = self.sub.fresh();
                for element in elements {
                    let ty = self.const_value_type(element, span)?;
                    self.unify_with(&elem, &ty, span)?;
                }
                if let Some(rest) = spread {
                    let ty = self.const_value_type(rest, span)?;
                    match self.sub.apply(&ty) {
                        Type::App { name: n, args, .. } if n == "List" && args.len() == 1 => {
                            self.unify_with(&elem, &args[0], span)?;
                        }
                        other => {
                            return Err(self.error(format!("expected a list for the `..` spread, found {other}"), span));
                        }
                    }
                }
                Ok(t_list(elem))
            }
            ConstValue::Tuple(elems) => {
                let mut resolved = Vec::with_capacity(elems.len());
                for elem in elems {
                    resolved.push(self.const_value_type(elem, span)?);
                }
                Ok(Type::Tuple(resolved))
            }
            ConstValue::BitArray(_) => Ok(Type::BitArray { size: None, unit: None }),
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn infer(&mut self, expression: &Expression) -> Result<Type, TypeCheckError> {
        let span = expression.span;
        match &expression.expr {
            Expr::Int(_) => Ok(t_int()),
            Expr::Float(_) => Ok(t_float()),
            Expr::String(_) => Ok(t_string()),
            Expr::Bool(_) => Ok(t_bool()),
            Expr::Nil => Ok(t_nil()),

            Expr::Var(name) => self
                .lookup_value(name)
                .ok_or_else(|| {
                    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                        self.error(format!("unknown constructor `{name}`"), span)
                    } else {
                        self.error(format!("unknown variable `{name}`"), span)
                    }
                }),

            Expr::FieldAccess { base, label } => self.infer_field(base, label, span),

            Expr::Constructor(reference) => {
                let callee = self.resolve_callee(&Expression { expr: Expr::Constructor(reference.clone()), span })?;
                Ok(t_fn(callee.params, callee.ret))
            }

            Expr::List { elements, spread } => {
                let elem = self.sub.fresh();
                for element in elements {
                    let ty = self.infer(element)?;
                    self.unify_with(&elem, &ty, span)?;
                }
                if let Some(rest) = spread {
                    let ty = self.infer(rest)?;
                    match self.sub.apply(&ty) {
                        Type::App { name: n, args, .. } if n == "List" && args.len() == 1 => {
                            self.unify_with(&elem, &args[0], span)?;
                        }
                        other => {
                            return Err(self.error(format!("expected a list for the `..` spread, found {other}"), span));
                        }
                    }
                }
                Ok(t_list(elem))
            }

            Expr::Tuple(elems) => {
                let mut resolved = Vec::with_capacity(elems.len());
                for elem in elems {
                    resolved.push(self.infer(elem)?);
                }
                Ok(Type::Tuple(resolved))
            }

            Expr::BitArray(bit_array) => {
                for segment in &bit_array.segments {
                    let ty = self.infer(&segment.value)?;
                    match self.sub.apply(&ty) {
                        Type::App { name, .. } if matches!(name.as_str(), "Int" | "Float" | "String") => {}
                        other => {
                            return Err(self.error(
                                format!("bit array segments must be Int, Float or String, found {other}"),
                                span,
                            ));
                        }
                    }
                    for option in &segment.options {
                        match option {
                            BitOption::Size(e) | BitOption::Unit(e) => {
                                let ty = self.infer(e)?;
                                self.unify_with(&t_int(), &ty, e.span)?;
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Type::BitArray { size: None, unit: None })
            }

            Expr::Call(call) => self.check_call(&call.fun, &call.args, span),

            Expr::Fn(literal) => {
                self.scopes.push(HashMap::new());
                let mut params = Vec::with_capacity(literal.args.len());
                for name in &literal.args {
                    if name.starts_with('_') {
                        params.push(self.sub.fresh());
                        continue;
                    }
                    let ty = self.sub.fresh();
                    self.bind_var(name, ty.clone(), span)?;
                    params.push(ty);
                }
                let body = self.infer(&literal.body)?;
                self.scopes.pop();
                Ok(t_fn(params, body))
            }

            Expr::Capture(capture) => {
                let callee = self.resolve_callee(&capture.fun)?;
                let mut placeholder: Option<usize> = None;
                let mut count = 0usize;
                let mut args: Vec<(Option<String>, Option<&Expression>)> =
                    Vec::with_capacity(capture.args.len());
                for (index, arg) in capture.args.iter().enumerate() {
                    match arg {
                        CaptureArg::Value(value) => args.push((None, Some(value))),
                        CaptureArg::Placeholder => {
                            count += 1;
                            placeholder = Some(index);
                            args.push((None, None));
                        }
                    }
                }
                if count != 1 {
                    return Err(self.error(
                        "function captures require exactly one `_` placeholder",
                        span,
                    ));
                }
                let ph = placeholder.expect("placeholder checked above");
                self.check_args(&callee.params, &callee.labels, &args, None, span)?;
                Ok(t_fn(vec![callee.params[ph].clone()], callee.ret))
            }

            Expr::RecordUpdate(update) => {
                let ctor = self
                    .env
                    .constructors
                    .get(&update.constructor.name)
                    .cloned()
                    .ok_or_else(|| self.error(format!("unknown constructor `{}`", update.constructor.name), span))?;
                let record = self.ctor_record_type(&ctor);
                let base = self.infer(&update.base)?;
                self.unify_with(&record, &base, span)?;
                for arg in &update.args {
                    let CallArg::Labelled { label, value } = arg else {
                        return Err(self.error("record updates require labelled fields", span));
                    };
                    let index = ctor
                        .fields
                        .iter()
                        .position(|field| field.label.as_deref().or(field.name.as_deref()) == Some(label.as_str()))
                        .ok_or_else(|| self.error(format!("unknown field `{label}` in `{}`", update.constructor.name), span))?;
                    let ty = self.infer(value)?;
                    let field = self.field_type(&ctor, index)?;
                    self.unify_with(&field, &ty, value.span)?;
                }
                Ok(record)
            }

            Expr::Case(case) => self.infer_case(case, span),

            Expr::Block(statements) => {
                let mut result = None;
                for statement in statements {
                    result = Some(self.infer(statement)?);
                }
                result.ok_or_else(|| self.error("a block cannot be empty", span))
            }

            Expr::Let(let_) => {
                let value = self.infer(&let_.value)?;
                if let Some(annotation) = &let_.annotation {
                    let expected = self.resolve_annotation(annotation, &HashMap::new(), 0)?;
                    self.unify_with(&expected, &value, span)?;
                }
                self.check_pattern(&let_.pattern, &value, span)?;
                Ok(t_nil())
            }

            Expr::LetAssert(assert) => {
                let value = self.infer(&assert.value)?;
                self.check_pattern(&assert.pattern, &value, span)?;
                Ok(t_nil())
            }

            Expr::Assert(assert) => {
                let value = self.infer(&assert.value)?;
                self.unify_with(&t_bool(), &value, span)?;
                Ok(t_nil())
            }

            Expr::Use(use_) => self.infer_use(use_, span),

            Expr::Echo(inner) => self.infer(inner),

            Expr::Todo { .. } | Expr::Panic { .. } => Ok(self.sub.fresh()),

            Expr::Binary { op, lhs, rhs } => {
                let left = self.infer(lhs)?;
                let right = self.infer(rhs)?;
                self.binary_result(*op, &left, &right, span)
            }

            Expr::Neg(inner) => {
                let ty = self.infer(inner)?;
                match self.sub.apply(&ty) {
                    Type::App { name, .. } if name == "Int" => Ok(t_int()),
                    Type::App { name, .. } if name == "Float" => Ok(t_float()),
                    other => Err(self.error(format!("expected an Int or Float to negate, found {other}"), span)),
                }
            }

            Expr::Not(inner) => {
                let ty = self.infer(inner)?;
                self.unify_with(&t_bool(), &ty, span)?;
                Ok(t_bool())
            }

            Expr::Pipeline { exprs, .. } => self.infer_pipeline(exprs, span),
        }
    }

    fn infer_field(&mut self, base: &Expression, label: &str, span: Span) -> Result<Type, TypeCheckError> {
        // Module qualified reference: `io.println`, `string.split`.
        if let Expr::Var(module) = &base.expr {
            if let Some(path) = self.env.imports.get(module) {
                return std_function(&mut self.sub, path, label)
                    .map(|(ty, _)| ty)
                    .ok_or_else(|| self.error(format!("module `{path}` has no function `{label}`"), span));
            }
            if is_stdlib_module(module) {
                return std_function(&mut self.sub, module, label)
                    .map(|(ty, _)| ty)
                    .ok_or_else(|| self.error(format!("module `{module}` has no function `{label}`"), span));
            }
        }

        let base_ty = self.infer(base)?;
        // Tuple index: `tuple.0`.
        if let Ok(index) = label.parse::<usize>() {
            match self.sub.apply(&base_ty) {
                Type::Tuple(elems) if index < elems.len() => return Ok(elems[index].clone()),
                Type::Tuple(elems) => {
                    return Err(self.error(
                        format!("tuple has {} element(s), no index `{label}`", elems.len()),
                        span,
                    ));
                }
                other => return Err(self.error(format!("`{other}` is not a tuple"), span)),
            }
        }

        // Record field: search all constructors (mirrors codegen).
        for ctor in self.env.constructors.values() {
            if let Some(index) = ctor
                .fields
                .iter()
                .position(|field| field.label.as_deref().or(field.name.as_deref()) == Some(label))
            {
                let ctor = ctor.clone();
                return self.field_type(&ctor, index);
            }
        }
        Err(self.error(format!("unknown record field `{label}`"), span))
    }

    fn resolve_callee(&mut self, fun: &Expression) -> Result<Callee, TypeCheckError> {
        match &fun.expr {
            Expr::Constructor(reference) => {
                let ctor = self
                    .env
                    .constructors
                    .get(&reference.name)
                    .cloned()
                    .ok_or_else(|| self.error(format!("unknown constructor `{}`", reference.name), fun.span))?;
                let mut params = Vec::with_capacity(ctor.fields.len());
                for index in 0..ctor.fields.len() {
                    params.push(self.field_type(&ctor, index)?);
                }
                Ok(Callee {
                    params,
                    labels: ctor
                        .fields
                        .iter()
                        .map(|field| field.label.clone().or_else(|| field.name.clone()))
                        .collect(),
                    ret: self.ctor_record_type(&ctor),
                })
            }
            Expr::FieldAccess { base, label } => {
                if let Expr::Var(module) = &base.expr {
                    if let Some(path) = self.env.imports.get(module) {
                        return std_function(&mut self.sub, path, label)
                            .map(|(ty, labels)| Callee { params: fn_params(&ty), labels, ret: fn_ret(&ty) })
                            .ok_or_else(|| self.error(format!("module `{path}` has no function `{label}`"), fun.span));
                    }
                    if is_stdlib_module(module) {
                        return std_function(&mut self.sub, module, label)
                            .map(|(ty, labels)| Callee { params: fn_params(&ty), labels, ret: fn_ret(&ty) })
                            .ok_or_else(|| self.error(format!("module `{module}` has no function `{label}`"), fun.span));
                    }
                }
                let ty = self.infer(fun)?;
                match self.sub.apply(&ty) {
                    Type::Fn(args, ret) => Ok(Callee { params: args, labels: vec![], ret: *ret }),
                    other => Err(self.error(format!("`{other}` is not a function"), fun.span)),
                }
            }
            Expr::Var(name) => {
                let ty = self
                    .lookup_value(name)
                    .ok_or_else(|| self.error(format!("unknown variable `{name}`"), fun.span))?;
                let labels = self.callee_labels(name);
                match self.sub.apply(&ty) {
                    Type::Fn(args, ret) => Ok(Callee { params: args, labels, ret: *ret }),
                    other => Err(self.error(
                        format!("`{name}` is not a function (it has type {other})"),
                        fun.span,
                    )),
                }
            }
            _ => {
                let ty = self.infer(fun)?;
                match self.sub.apply(&ty) {
                    Type::Fn(args, ret) => Ok(Callee { params: args, labels: vec![], ret: *ret }),
                    other => Err(self.error(format!("`{other}` is not a function"), fun.span)),
                }
            }
        }
    }

    /// Argument labels for a named callee: local functions first, then
    /// unqualified stdlib imports.
    fn callee_labels(&mut self, name: &str) -> Vec<Option<String>> {
        if let Some(f) = self.env.functions.get(name) {
            return f.labels.clone();
        }
        if let Some(module) = self.env.unqualified_fns.get(name) {
            if let Some((_, labels)) = std_function(&mut self.sub, module, name) {
                return labels;
            }
        }
        vec![]
    }

    fn check_call(&mut self, fun: &Expression, args: &[CallArg], span: Span) -> Result<Type, TypeCheckError> {
        let callee = self.resolve_callee(fun)?;
        let typed_args: Vec<(Option<String>, Option<&Expression>)> = args
            .iter()
            .map(|arg| match arg {
                CallArg::Value(value) => (None, Some(value)),
                CallArg::Labelled { label, value } => (Some(label.clone()), Some(value)),
            })
            .collect();
        self.check_args(&callee.params, &callee.labels, &typed_args, None, span)?;
        Ok(self.sub.apply(&callee.ret))
    }

    /// Check call arguments against parameter types using the same label
    /// resolution rules as codegen: positional arguments fill the first free
    /// slot in order, labelled arguments must match a declared label.
    fn check_args(
        &mut self,
        params: &[Type],
        labels: &[Option<String>],
        args: &[(Option<String>, Option<&Expression>)],
        piped: Option<Type>,
        span: Span,
    ) -> Result<(), TypeCheckError> {
        let mut typed = Vec::with_capacity(args.len());
        for (label, value) in args {
            match value {
                Some(value) => typed.push((label.clone(), Some(self.infer(value)?))),
                None => typed.push((label.clone(), None)),
            }
        }
        self.check_arg_types(params, labels, &typed, piped, span)
    }

    fn check_arg_types(
        &mut self,
        params: &[Type],
        labels: &[Option<String>],
        args: &[(Option<String>, Option<Type>)],
        piped: Option<Type>,
        span: Span,
    ) -> Result<(), TypeCheckError> {
        let n = params.len();
        let mut filled = vec![false; n];
        let mut position = 0;

        if let Some(value) = piped {
            if n == 0 {
                return Err(self.error("cannot pipe a value into a function that takes no arguments", span));
            }
            self.unify_with(&params[0], &value, span)?;
            filled[0] = true;
            position = 1;
        }

        for (label, ty) in args {
            let index = match label {
                None => {
                    while position < n && filled[position] {
                        position += 1;
                    }
                    if position >= n {
                        return Err(self.error(
                            format!("too many arguments: function expects {n}"),
                            span,
                        ));
                    }
                    position
                }
                Some(label) => labels
                    .iter()
                    .position(|candidate| candidate.as_deref() == Some(label.as_str()))
                    .ok_or_else(|| self.error(format!("function has no argument labeled `{label}`"), span))?,
            };
            if filled[index] {
                return Err(self.error(
                    match label {
                        Some(label) => format!("duplicate argument `{label}`"),
                        None => "duplicate positional argument".to_string(),
                    },
                    span,
                ));
            }
            filled[index] = true;
            if let Some(ty) = ty {
                self.unify_with(&params[index], &ty, span)?;
            }
        }

        let given = filled.iter().filter(|&&f| f).count();
        if given < n {
            return Err(self.error(
                format!("function expects {n} argument(s), got {given}"),
                span,
            ));
        }
        Ok(())
    }

    fn infer_pipeline(&mut self, exprs: &[Expression], _span: Span) -> Result<Type, TypeCheckError> {
        let mut value = self.infer(&exprs[0])?;
        for stage in &exprs[1..] {
            value = match &stage.expr {
                Expr::Call(call) => {
                    let callee = self.resolve_callee(&call.fun)?;
                    let typed_args: Vec<(Option<String>, Option<&Expression>)> = call
                        .args
                        .iter()
                        .map(|arg| match arg {
                            CallArg::Value(value) => (None, Some(value)),
                            CallArg::Labelled { label, value } => (Some(label.clone()), Some(value)),
                        })
                        .collect();
                    self.check_args(&callee.params, &callee.labels, &typed_args, Some(value), stage.span)?;
                    self.sub.apply(&callee.ret)
                }
                _ => {
                    let callee = self.resolve_callee(stage)?;
                    self.check_args(&callee.params, &callee.labels, &[], Some(value), stage.span)?;
                    self.sub.apply(&callee.ret)
                }
            };
        }
        Ok(value)
    }

    fn infer_case(&mut self, case: &Case, span: Span) -> Result<Type, TypeCheckError> {
        let mut subjects = Vec::with_capacity(case.subjects.len());
        for subject in &case.subjects {
            subjects.push(self.infer(subject)?);
        }

        let mut result: Option<Type> = None;
        for clause in &case.clauses {
            self.scopes.push(HashMap::new());
            for (pattern, subject) in clause.patterns.iter().zip(subjects.iter()) {
                self.check_pattern(pattern, subject, clause.span)?;
            }
            if let Some(guard) = &clause.guard {
                let ty = self.type_guard(guard)?;
                self.unify_with(&t_bool(), &ty, clause.span)?;
            }
            let body = self.infer(&clause.expression)?;
            match &result {
                None => result = Some(body),
                Some(previous) => self.unify_with(previous, &body, clause.span)?,
            }
            self.scopes.pop();
        }

        match result {
            Some(ty) => Ok(self.sub.apply(&ty)),
            None => Err(self.error("case expression has no clauses", span)),
        }
    }

    fn infer_use(&mut self, use_: &Use, span: Span) -> Result<Type, TypeCheckError> {
        // Bind the callback's parameters to fresh variables.
        self.scopes.push(HashMap::new());
        let mut params = Vec::with_capacity(use_.patterns.len());
        for pattern in &use_.patterns {
            let subject = self.sub.fresh();
            self.check_pattern(pattern, &subject, span)?;
            params.push(match pattern {
                Pattern::Var(name) if !name.starts_with('_') => self
                    .scopes
                    .last()
                    .expect("scope open")
                    .get(name)
                    .expect("bound above")
                    .clone(),
                _ => subject,
            });
        }
        let callback_ret = self.sub.fresh();
        let callback = t_fn(params, callback_ret.clone());

        // The call is applied to the callback as its final argument: check
        // the explicit arguments plus the callback against the callee.
        let empty: Vec<CallArg> = Vec::new();
        let (fun, explicit) = match &use_.call.expr {
            Expr::Call(call) => (&call.fun, &call.args),
            _ => (&use_.call, &empty),
        };
        let callee = self.resolve_callee(fun)?;
        let mut arg_tys: Vec<(Option<String>, Type)> = Vec::with_capacity(explicit.len() + 1);
        for arg in explicit {
            match arg {
                CallArg::Value(value) => arg_tys.push((None, self.infer(value)?)),
                CallArg::Labelled { label, value } => {
                    arg_tys.push((Some(label.clone()), self.infer(value)?))
                }
            }
        }
        arg_tys.push((None, callback));
        let args: Vec<(Option<String>, Option<Type>)> =
            arg_tys.into_iter().map(|(label, ty)| (label, Some(ty))).collect();
        self.check_arg_types(&callee.params, &callee.labels, &args, None, span)?;

        let body = self.infer(&use_.body)?;
        self.unify_with(&callback_ret, &body, span)?;
        self.scopes.pop();
        Ok(self.sub.apply(&callee.ret))
    }

    // ------------------------------------------------------------------
    // Patterns
    // ------------------------------------------------------------------

    fn check_pattern(&mut self, pattern: &Pattern, subject: &Type, span: Span) -> Result<(), TypeCheckError> {
        let mut bound = HashSet::new();
        self.check_pattern_inner(pattern, subject, span, &mut bound)
    }

    /// `bound` tracks the names introduced by this pattern. Binding the same
    /// name twice within one pattern is an error, while shadowing a variable
    /// from an outer scope (e.g. `let x = f(x)`) is allowed and simply
    /// overwrites the binding in the current scope.
    fn check_pattern_inner(
        &mut self,
        pattern: &Pattern,
        subject: &Type,
        span: Span,
        bound: &mut HashSet<String>,
    ) -> Result<(), TypeCheckError> {
        let subject = self.sub.apply(subject);
        match pattern {
            Pattern::Var(name) => {
                if !name.starts_with('_') {
                    self.bind_pattern_var(name, subject, span, bound)?;
                }
                Ok(())
            }
            Pattern::Discard(_) => Ok(()),

            Pattern::Int(_) => self.pattern_literal(&subject, &t_int(), span),
            Pattern::Float(_) => self.pattern_literal(&subject, &t_float(), span),
            Pattern::String(_) => self.pattern_literal(&subject, &t_string(), span),
            Pattern::Bool(_) => self.pattern_literal(&subject, &t_bool(), span),
            Pattern::Nil => self.pattern_literal(&subject, &t_nil(), span),

            Pattern::StringPrefix { rest, .. } => {
                self.pattern_literal(&subject, &t_string(), span)?;
                if let Pattern::Var(name) = &**rest {
                    if !name.starts_with('_') {
                        self.bind_pattern_var(name, t_string(), span, bound)?;
                    }
                }
                Ok(())
            }

            Pattern::Constructor(reference, args) => {
                let ctor = self
                    .env
                    .constructors
                    .get(&reference.name)
                    .cloned()
                    .ok_or_else(|| self.error(format!("unknown constructor `{}`", reference.name), span))?;
                let record = self.ctor_record_type(&ctor);
                self.unify_with(&record, &subject, span)?;

                // Use the subject's actual type arguments when known.
                let mut param_env: HashMap<String, Type> = HashMap::new();
                if let Type::App { name, args: actual, .. } = self.sub.apply(&subject) {
                    if name == ctor.type_name && actual.len() == ctor.params.len() {
                        for (param, arg) in ctor.params.iter().zip(actual.iter()) {
                            param_env.insert(param.clone(), arg.clone());
                        }
                    }
                }
                for param in &ctor.params {
                    param_env.entry(param.clone()).or_insert_with(|| self.sub.fresh());
                }

                let mut position = 0;
                for arg in args {
                    match arg {
                        PatternArg::Named { label, pattern } => {
                            let index = ctor
                                .fields
                                .iter()
                                .position(|field| field.label.as_deref().or(field.name.as_deref()) == Some(label.as_str()))
                                .ok_or_else(|| self.error(format!("unknown field `{label}` in `{}`", reference.name), span))?;
                            let ty = self.resolve_annotation(&ctor.fields[index].type_annotation, &param_env, 0)?;
                            self.check_pattern_inner(pattern, &ty, span, bound)?;
                        }
                        PatternArg::Unnamed(pattern) => {
                            if position >= ctor.fields.len() {
                                return Err(self.error(
                                    format!("pattern for `{}` has too many fields", reference.name),
                                    span,
                                ));
                            }
                            let ty = self.resolve_annotation(&ctor.fields[position].type_annotation, &param_env, 0)?;
                            self.check_pattern_inner(pattern, &ty, span, bound)?;
                            position += 1;
                        }
                        PatternArg::DiscardRest => {}
                    }
                }
                Ok(())
            }

            Pattern::List { elements, rest } => {
                let elem = match &subject {
                    Type::App { name, args, .. } if name == "List" && args.len() == 1 => args[0].clone(),
                    other => {
                        return Err(self.error(format!("expected a list, found {other}"), span));
                    }
                };
                for element in elements {
                    self.check_pattern_inner(element, &elem, span, bound)?;
                }
                if let Some(rest) = rest {
                    self.check_pattern_inner(rest, &t_list(elem), span, bound)?;
                }
                Ok(())
            }

            Pattern::Tuple(elems) => match &subject {
                Type::Tuple(subject_elems) => {
                    if subject_elems.len() != elems.len() {
                        return Err(self.error(
                            format!(
                                "tuple pattern expects {} element(s), but the tuple has {}",
                                elems.len(),
                                subject_elems.len()
                            ),
                            span,
                        ));
                    }
                    for (pattern, ty) in elems.iter().zip(subject_elems.iter()) {
                        self.check_pattern_inner(pattern, ty, span, bound)?;
                    }
                    Ok(())
                }
                other => Err(self.error(format!("expected a tuple, found {other}"), span)),
            },

            Pattern::BitArray(bit_array) => {
                if !matches!(subject, Type::BitArray { .. }) {
                    return Err(self.error(format!("expected a bit array, found {subject}"), span));
                }
                for segment in &bit_array.segments {
                    let ty = self.sub.fresh();
                    self.check_pattern_inner(&segment.value, &ty, span, bound)?;
                }
                Ok(())
            }

            Pattern::Alias { pattern, name } => {
                self.check_pattern_inner(pattern, &subject, span, bound)?;
                if !name.starts_with('_') {
                    self.bind_pattern_var(name, subject, span, bound)?;
                }
                Ok(())
            }

            Pattern::Alternative(alternatives) => {
                if alternatives.is_empty() {
                    return Err(self.error("empty pattern alternative", span));
                }
                // The first alternative binds in the current scope; the rest
                // must bind exactly the same variables with compatible types.
                let base_len = self.scopes.last().expect("scope open").len();
                let mut first_bound = HashSet::new();
                self.check_pattern_inner(&alternatives[0], &subject, span, &mut first_bound)?;
                let scope = self.scopes.last().expect("scope open");
                let mut expected: Vec<(String, Type)> =
                    scope.iter().skip(base_len).map(|(k, v)| (k.clone(), v.clone())).collect();
                expected.sort_by(|a, b| a.0.cmp(&b.0));

                for alternative in &alternatives[1..] {
                    self.scopes.push(HashMap::new());
                    let mut alt_bound = HashSet::new();
                    self.check_pattern_inner(alternative, &subject, span, &mut alt_bound)?;
                    let mut actual: Vec<(String, Type)> =
                        self.scopes.last().expect("scope open").iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    self.scopes.pop();
                    actual.sort_by(|a, b| a.0.cmp(&b.0));

                    let expected_names: Vec<&str> = expected.iter().map(|(n, _)| n.as_str()).collect();
                    let actual_names: Vec<&str> = actual.iter().map(|(n, _)| n.as_str()).collect();
                    if expected_names != actual_names {
                        return Err(self.error(
                            "alternative patterns must bind the same variables",
                            span,
                        ));
                    }
                    for ((_, e_ty), (_, a_ty)) in expected.iter().zip(actual.iter()) {
                        self.unify_with(e_ty, a_ty, span)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn bind_pattern_var(
        &mut self,
        name: &str,
        ty: Type,
        span: Span,
        bound: &mut HashSet<String>,
    ) -> Result<(), TypeCheckError> {
        if !bound.insert(name.into()) {
            return Err(self.error(format!("variable `{name}` is already bound in this scope"), span));
        }
        self.scopes
            .last_mut()
            .expect("a scope is always open")
            .insert(name.into(), ty);
        Ok(())
    }

    fn pattern_literal(&mut self, subject: &Type, literal: &Type, span: Span) -> Result<(), TypeCheckError> {
        self.sub
            .unify(subject, literal)
            .map_err(|_| self.error(format!("pattern expects {literal}, but the subject is {subject}"), span))
    }

    // ------------------------------------------------------------------
    // Guards
    // ------------------------------------------------------------------

    fn type_guard(&mut self, guard: &Guard) -> Result<Type, TypeCheckError> {
        match guard {
            Guard::Int(_) => Ok(t_int()),
            Guard::Float(_) => Ok(t_float()),
            Guard::String(_) => Ok(t_string()),
            Guard::Bool(_) => Ok(t_bool()),
            Guard::Nil => Ok(t_nil()),

            Guard::Var(name) => self
                .lookup_value(name)
                .ok_or_else(|| self.error(format!("unknown variable `{name}` in guard"), Span::point(0))),

            Guard::FieldAccess { base, label } => {
                let base_ty = self.type_guard(base)?;
                if let Ok(index) = label.parse::<usize>() {
                    match self.sub.apply(&base_ty) {
                        Type::Tuple(elems) if index < elems.len() => return Ok(elems[index].clone()),
                        other => return Err(self.error(format!("`{other}` is not a tuple with index `{label}`"), Span::point(0))),
                    }
                }
                for ctor in self.env.constructors.values() {
                    if let Some(index) = ctor
                        .fields
                        .iter()
                        .position(|field| field.label.as_deref().or(field.name.as_deref()) == Some(label))
                    {
                        let ctor = ctor.clone();
                        return self.field_type(&ctor, index);
                    }
                }
                Err(self.error(format!("unknown record field `{label}` in guard"), Span::point(0)))
            }

            Guard::Call { fun, args } => {
                let callee = match &**fun {
                    Guard::Var(name) => {
                        let ty = self
                            .lookup_value(name)
                            .ok_or_else(|| self.error(format!("unknown variable `{name}` in guard"), Span::point(0)))?;
                        match self.sub.apply(&ty) {
                            Type::Fn(args, ret) => Callee { params: args, labels: vec![], ret: *ret },
                            other => return Err(self.error(format!("`{other}` is not a function"), Span::point(0))),
                        }
                    }
                    Guard::FieldAccess { base, label } => {
                        let Guard::Var(module) = &**base else {
                            return Err(self.error("guards only support calls to module functions", Span::point(0)));
                        };
                        let path = self
                            .env
                            .imports
                            .get(module)
                            .cloned()
                            .or_else(|| is_stdlib_module(module).then(|| module.clone()));
                        let Some(path) = path else {
                            return Err(self.error(format!("`{module}` is not a module"), Span::point(0)));
                        };
                        std_function(&mut self.sub, &path, label)
                            .map(|(ty, labels)| Callee { params: fn_params(&ty), labels, ret: fn_ret(&ty) })
                            .ok_or_else(|| self.error(format!("module `{path}` has no function `{label}`"), Span::point(0)))?
                    }
                    _ => return Err(self.error("guards only support calls to plain functions", Span::point(0))),
                };
                let mut typed = Vec::with_capacity(args.len());
                for arg in args {
                    typed.push((None, Some(self.type_guard(arg)?)));
                }
                self.check_arg_types(&callee.params, &callee.labels, &typed, None, Span::point(0))?;
                Ok(self.sub.apply(&callee.ret))
            }

            Guard::Binary { op, lhs, rhs } => {
                let left = self.type_guard(lhs)?;
                let right = self.type_guard(rhs)?;
                self.binary_result(*op, &left, &right, Span::point(0))
            }
        }
    }

    // ------------------------------------------------------------------
    // Operators
    // ------------------------------------------------------------------

    fn binary_result(&mut self, op: BinOp, left: &Type, right: &Type, span: Span) -> Result<Type, TypeCheckError> {
        let require = |this: &mut Self, ty: &Type, expected: Type| -> Result<(), TypeCheckError> {
            let applied = this.sub.apply(ty);
            this.sub.unify(&expected, &applied).map_err(|_| {
                this.error(format!("`{op}` expects {expected} operands, found {applied}"), span)
            })
        };

        match op {
            BinOp::And | BinOp::Or => {
                require(self, left, t_bool())?;
                require(self, right, t_bool())?;
                Ok(t_bool())
            }
            BinOp::Eq | BinOp::NotEq => {
                self.unify_pair(left, right, span)?;
                Ok(t_bool())
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                require(self, left, t_int())?;
                require(self, right, t_int())?;
                Ok(t_bool())
            }
            BinOp::LtFloat | BinOp::GtFloat | BinOp::LtEqFloat | BinOp::GtEqFloat => {
                require(self, left, t_float())?;
                require(self, right, t_float())?;
                Ok(t_bool())
            }
            BinOp::Concat => {
                require(self, left, t_string())?;
                require(self, right, t_string())?;
                Ok(t_string())
            }
            BinOp::Plus | BinOp::Minus | BinOp::Times | BinOp::Slash | BinOp::Percent => {
                require(self, left, t_int())?;
                require(self, right, t_int())?;
                Ok(t_int())
            }
            BinOp::PlusFloat | BinOp::MinusFloat | BinOp::TimesFloat | BinOp::SlashFloat => {
                require(self, left, t_float())?;
                require(self, right, t_float())?;
                Ok(t_float())
            }
        }
    }
}

/// Extract the parameter list of a function type.
fn fn_params(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Fn(args, _) => args.clone(),
        _ => vec![],
    }
}

/// Extract the return type of a function type.
fn fn_ret(ty: &Type) -> Type {
    match ty {
        Type::Fn(_, ret) => (**ret).clone(),
        _ => Type::app("Nil"),
    }
}

/// Collect the lowercase (type variable) names in a type annotation.
fn collect_annotation_names(annotation: &TypeAnnotation, out: &mut Vec<String>) {
    match annotation {
        TypeAnnotation::Named { module, name, args } => {
            if module.is_none() && name.starts_with(|c: char| c.is_ascii_lowercase()) {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            for arg in args {
                collect_annotation_names(arg, out);
            }
        }
        TypeAnnotation::Fn { args, ret } => {
            for arg in args {
                collect_annotation_names(arg, out);
            }
            collect_annotation_names(ret, out);
        }
        TypeAnnotation::Tuple(elems) => {
            for elem in elems {
                collect_annotation_names(elem, out);
            }
        }
        TypeAnnotation::BitArray { .. } => {}
    }
}
