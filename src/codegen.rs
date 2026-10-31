//! C code generation for the parsed Gleam subset.
//!
//! Generated programs use tagged values and reference-counted heap objects.
//! The runtime is embedded in this crate so a generated project has no
//! dependencies beyond a C11 compiler and the POSIX C library.

use crate::ast::*;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

pub const RUNTIME_HEADER: &str = include_str!("runtime/gleam_runtime.h");
pub const RUNTIME_SOURCE: &str = include_str!("runtime/gleam_runtime.c");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
    pub span: Span,
}

impl CodegenError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C codegen at byte {}: {}", self.span.start, self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Parse and translate one Gleam module to a C source file.
pub fn translate(source: &str) -> Result<String, CompileError> {
    let module = crate::parse(source).map_err(CompileError::Parse)?;
    generate_c(&module).map_err(CompileError::Codegen)
}

/// Translate a parsed module. The output includes a C `main` wrapper when the
/// Gleam module defines `main`.
pub fn generate_c(module: &Module) -> Result<String, CodegenError> {
    Compiler::new(module)?.compile()
}

/// Write `main.c`, `gleam_runtime.c`, and `gleam_runtime.h` into a directory.
pub fn write_c_project(module: &Module, directory: impl AsRef<Path>) -> Result<(), ProjectError> {
    let source = generate_c(module).map_err(ProjectError::Codegen)?;
    let directory = directory.as_ref();
    fs::create_dir_all(directory).map_err(ProjectError::Io)?;
    fs::write(directory.join("main.c"), source).map_err(ProjectError::Io)?;
    fs::write(directory.join("gleam_runtime.c"), RUNTIME_SOURCE).map_err(ProjectError::Io)?;
    fs::write(directory.join("gleam_runtime.h"), RUNTIME_HEADER).map_err(ProjectError::Io)?;
    Ok(())
}

#[derive(Debug)]
pub enum CompileError {
    Parse(ParseError),
    Codegen(CodegenError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::Codegen(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Debug)]
pub enum ProjectError {
    Codegen(CodegenError),
    Io(io::Error),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codegen(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ProjectError {}

#[derive(Clone)]
struct ConstructorInfo {
    fields: Vec<RecordField>,
}

#[derive(Clone)]
struct FunctionInfo {
    c_name: String,
    arity: usize,
    labels: Vec<Option<String>>,
}

#[derive(Default)]
struct Scope {
    locals: Vec<String>,
}

struct Compiler<'a> {
    module: &'a Module,
    constructors: HashMap<String, ConstructorInfo>,
    functions: HashMap<String, FunctionInfo>,
    imports: HashMap<String, String>,
    unqualified: HashMap<String, String>,
    constants: Vec<String>,
    visible: HashMap<String, String>,
    scopes: Vec<Scope>,
    counter: usize,
    extra_functions: String,
}

impl<'a> Compiler<'a> {
    fn new(module: &'a Module) -> Result<Self, CodegenError> {
        let mut constructors = HashMap::new();
        let mut functions = HashMap::new();
        let mut imports = HashMap::new();
        let mut unqualified = HashMap::new();
        let mut constants = Vec::new();

        for definition in &module.definitions {
            match definition {
                Definition::Type(ty) => {
                    if let TypeDefKind::Record(variants) = &ty.kind {
                        for variant in variants {
                            if constructors
                                .insert(
                                    variant.name.clone(),
                                    ConstructorInfo {
                                        fields: variant.fields.clone(),
                                    },
                                )
                                .is_some()
                            {
                                return Err(CodegenError::new(
                                    format!("duplicate constructor `{}`", variant.name),
                                    ty.span,
                                ));
                            }
                        }
                    }
                }
                Definition::Fn(function) => {
                    let c_name = format!("gleam_fn_{}", function.name);
                    functions.insert(
                        function.name.clone(),
                        FunctionInfo {
                            c_name,
                            arity: function.args.len(),
                            labels: function
                                .args
                                .iter()
                                .map(|arg| arg.label.clone().or_else(|| Some(arg.name.clone())))
                                .collect(),
                        },
                    );
                }
                Definition::Constant(constant) => constants.push(constant.name.clone()),
                Definition::Import(import) => {
                    let module_name = import.module.join("/");
                    let alias = import
                        .alias
                        .clone()
                        .or_else(|| import.module.last().cloned())
                        .unwrap();
                    imports.insert(alias, module_name.clone());
                    for item in &import.unqualified {
                        if let UnqualifiedImport::Value(name) = item {
                            unqualified.insert(name.clone(), module_name.clone());
                        }
                    }
                }
            }
        }

        // Built-in constructors from the standard library. `Eq`, `Lt`, and
        // `Gt` are the variants of `gleam/order.Order`, returned by
        // `string.compare`; `InvalidBase` is the error of `gleam/int` base
        // conversion functions.
        for (name, fields) in [("Ok", 1), ("Error", 1), ("Some", 1), ("None", 0),
                               ("Eq", 0), ("Lt", 0), ("Gt", 0), ("InvalidBase", 0)] {
            constructors.entry(name.into()).or_insert_with(|| ConstructorInfo {
                fields: (0..fields)
                    .map(|_| RecordField {
                        label: None,
                        name: None,
                        type_annotation: TypeAnnotation::Named {
                            module: None,
                            name: "a".into(),
                            args: Vec::new(),
                        },
                    })
                    .collect(),
            });
        }

        Ok(Self {
            module,
            constructors,
            functions,
            imports,
            unqualified,
            constants,
            visible: HashMap::new(),
            scopes: Vec::new(),
            counter: 0,
            extra_functions: String::new(),
        })
    }

    fn compile(mut self) -> Result<String, CodegenError> {
        let mut declarations = String::new();
        let mut definitions = String::new();
        let functions: Vec<&FnDef> = self
            .module
            .definitions
            .iter()
            .filter_map(|definition| match definition {
                Definition::Fn(function) => Some(function),
                _ => None,
            })
            .collect();

        for function in &functions {
            let info = &self.functions[&function.name];
            declarations.push_str(&format!(
                "static GValue {}(GClosure *, GValue *, size_t);\n",
                info.c_name
            ));
        }
        declarations.push('\n');
        for (tag, info) in &self.constructors {
            if info.fields.is_empty() {
                continue;
            }
            let name = format!("gleam_ctor_{tag}");
            declarations.push_str(&format!(
                "static GValue {name}(GClosure *, GValue *, size_t);\n"
            ));
            definitions.push_str(&format!(
                "static GValue __attribute__((unused)) {name}(GClosure *closure, GValue *args, size_t argc) {{\n  (void)closure;\n  if (argc != {}) g_panic(\"incorrect argument count\", __FILE__, __LINE__);\n  GValue fields[{}];\n",
                info.fields.len(), info.fields.len()
            ));
            for index in 0..info.fields.len() {
                definitions.push_str(&format!("  fields[{index}] = g_retain(args[{index}]);\n"));
            }
            definitions.push_str(&format!(
                "  return g_record({}, {}, fields);\n}}\n\n",
                c_string(tag), info.fields.len()
            ));
        }

        for function in functions {
            definitions.push_str(&self.compile_function(function)?);
        }

        let mut constants = String::new();
        for name in &self.constants {
            constants.push_str(&format!("static GValue gleam_const_{name};\n"));
        }
        if !self.constants.is_empty() {
            constants.push_str("\nstatic void gleam_init_constants(void) {\n");
            let constants_to_compile: Vec<&ConstantDef> = self
                .module
                .definitions
                .iter()
                .filter_map(|definition| match definition {
                    Definition::Constant(constant) => Some(constant),
                    _ => None,
                })
                .collect();
            for constant in constants_to_compile {
                let value = self.compile_constant(&constant.value, constant.span)?;
                constants.push_str(&format!("  gleam_const_{} = {value};\n", constant.name));
            }
            constants.push_str("}\n\n");
        }

        let has_main = self.functions.contains_key("main");
        let mut output = String::from(
            "/* Generated by gleam-parser. Requires C11 and a POSIX system. */\n#include \"gleam_runtime.h\"\n#include <limits.h>\n\n",
        );
        output.push_str(&constants);
        output.push_str(&declarations);
        output.push_str(&self.extra_functions);
        output.push_str(&definitions);
        if has_main {
            output.push_str(
                "int main(void) {\n  GValue gleam_result;\n",
            );
            if !self.constants.is_empty() {
                output.push_str("  gleam_init_constants();\n");
            }
            output.push_str(
                "  gleam_result = gleam_fn_main(NULL, NULL, 0);\n  g_release(gleam_result);\n",
            );
            for name in &self.constants {
                output.push_str(&format!("  g_release(gleam_const_{name});\n"));
            }
            output.push_str("  return 0;\n}\n");
        }
        Ok(output)
    }

    fn compile_function(&mut self, function: &FnDef) -> Result<String, CodegenError> {
        let info = self.functions[&function.name].clone();
        self.visible.clear();
        self.scopes.clear();
        self.scopes.push(Scope::default());

        if matches!(function.body.expr, Expr::Block(ref body) if body.is_empty()) {
            let external = function.attributes.iter().find_map(|attribute| match attribute {
                Attribute::External(external) if external.lang == "c" => Some(external),
                _ => None,
            });
            if let Some(external) = external {
                return Ok(format!(
                    "static GValue {}(GClosure *closure, GValue *args, size_t argc) {{\n  (void)closure;\n  return {}(args, argc);\n}}\n\n",
                    info.c_name, external.function
                ));
            }
            return Err(CodegenError::new(
                "external functions must use @external(c, \"module\", \"function\")",
                function.span,
            ));
        }

        let mut body = format!(
            "static GValue {}(GClosure *closure, GValue *args, size_t argc) {{\n  (void)closure;\n  (void)args;\n",
            info.c_name
        );
        body.push_str(&format!(
            "  if (argc != {}) g_panic(\"incorrect argument count\", __FILE__, __LINE__);\n",
            function.args.len()
        ));
        for (index, arg) in function.args.iter().enumerate() {
            let name = self.temp();
            body.push_str(&format!("  GValue {name} = g_retain(args[{index}]);\n"));
            self.visible.insert(arg.name.clone(), name.clone());
            self.scopes.last_mut().unwrap().locals.push(name);
        }
        let result = self.gen_expr(&function.body, &mut body)?;
        body.push_str(&format!("  GValue gleam_return = {result};\n"));
        self.release_scope(&mut body);
        body.push_str("  return gleam_return;\n}\n\n");
        self.visible.clear();
        self.scopes.clear();
        Ok(body)
    }

    fn compile_constant(&mut self, value: &ConstValue, span: Span) -> Result<String, CodegenError> {
        match value {
            ConstValue::Int(value) => Ok(format!("g_int({})", self.integer(value, span)?)),
            ConstValue::Float(value) => Ok(format!("g_float({})", self.float(value))),
            ConstValue::String(value) => Ok(format!("g_string({})", c_string(value))),
            ConstValue::Bool(value) => Ok(format!("g_bool({})", *value as u8)),
            ConstValue::Nil => Ok("g_nil()".into()),
            ConstValue::Var(name) => {
                if self.constants.contains(name) {
                    Ok(format!("g_retain(gleam_const_{name})"))
                } else {
                    Err(CodegenError::new(format!("unknown constant `{name}`"), span))
                }
            }
            ConstValue::List { elements, spread } => {
                let mut result = match spread {
                    Some(rest) => self.compile_constant(rest, span)?,
                    None => "g_list_empty()".into(),
                };
                for element in elements.iter().rev() {
                    result = format!(
                        "g_list_prepend({}, {result})",
                        self.compile_constant(element, span)?
                    );
                }
                Ok(result)
            }
            ConstValue::Tuple(elements) => {
                if elements.is_empty() {
                    return Ok("g_tuple(0, NULL)".into());
                }
                let values = elements
                    .iter()
                    .map(|element| self.compile_constant(element, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!(
                    "g_tuple({}, (GValue[]){{{}}})",
                    values.len(),
                    values.join(", ")
                ))
            }
            ConstValue::Constructor(reference) => {
                let info = self.constructor(reference, span)?.clone();
                if !info.fields.is_empty() {
                    return Err(CodegenError::new(
                        "constructor constants with fields are not supported by this AST",
                        span,
                    ));
                }
                Ok(format!("g_record({}, 0, NULL)", c_string(&reference.name)))
            }
            ConstValue::BitArray(_) => Err(CodegenError::new(
                "constant bit arrays are not supported by the C backend",
                span,
            )),
        }
    }

    fn temp(&mut self) -> String {
        let name = format!("g_{}", self.counter);
        self.counter += 1;
        name
    }

    fn release_scope(&mut self, output: &mut String) {
        if let Some(scope) = self.scopes.pop() {
            for local in scope.locals.iter().rev() {
                output.push_str(&format!("  g_release({local});\n"));
            }
        }
    }

    fn gen_expr(&mut self, expression: &Expression, out: &mut String) -> Result<String, CodegenError> {
        match &expression.expr {
            Expr::Int(value) => Ok(format!("g_int({})", self.integer(value, expression.span)?)),
            Expr::Float(value) => Ok(format!("g_float({})", self.float(value))),
            Expr::String(value) => Ok(format!("g_string({})", c_string(value))),
            Expr::Bool(value) => Ok(format!("g_bool({})", *value as u8)),
            Expr::Nil => Ok("g_nil()".into()),
            Expr::Var(name) => self.gen_var(name, expression.span),
            Expr::FieldAccess { base, label } => self.gen_field(base, label, expression.span, out),
            Expr::Constructor(reference) => {
                let info = self.constructor(reference, expression.span)?.clone();
                if info.fields.is_empty() {
                    Ok(format!("g_record({}, 0, NULL)", c_string(&reference.name)))
                } else {
                    let name = format!("gleam_ctor_{}", reference.name);
                    Ok(format!("g_function({name}, {})", info.fields.len()))
                }
            }
            Expr::List { elements, spread } => {
                let rest = match spread {
                    Some(rest) => self.gen_expr(rest, out)?,
                    None => "g_list_empty()".into(),
                };
                let mut current = self.temp();
                out.push_str(&format!("  GValue {current} = {rest};\n"));
                for element in elements.iter().rev() {
                    let value = self.gen_expr(element, out)?;
                    let next = self.temp();
                    out.push_str(&format!(
                        "  GValue {next} = g_list_prepend({value}, {current});\n"
                    ));
                    current = next;
                }
                Ok(current)
            }
            Expr::Tuple(elements) => {
                let values = self.gen_values(elements, out)?;
                Ok(self.make_aggregate("g_tuple", &values, out))
            }
            Expr::BitArray(_) => Err(CodegenError::new(
                "bit arrays are not yet supported by the C backend",
                expression.span,
            )),
            Expr::Call(call) => self.gen_call(call, expression.span, out),
            Expr::Fn(function) => self.gen_closure(function, out),
            Expr::Capture(capture) => self.gen_capture(capture, expression.span, out),
            Expr::RecordUpdate(update) => self.gen_record_update(update, expression.span, out),
            Expr::Case(case) => self.gen_case(case, expression.span, out),
            Expr::Block(statements) => self.gen_block(statements, out),
            Expr::Let(let_) => self.gen_let(&let_.pattern, &let_.value, None, expression.span, out),
            Expr::LetAssert(assert) => self.gen_let(
                &assert.pattern,
                &assert.value,
                assert.message.as_deref(),
                expression.span,
                out,
            ),
            Expr::Assert(assert) => {
                let value = self.gen_expr(&assert.value, out)?;
                out.push_str(&format!(
                    "  if (({value}).tag != G_BOOL) g_type_error(\"Bool\", {value}, __FILE__, __LINE__);\n  if (!({value}).as.boolean) g_panic({}, __FILE__, __LINE__);\n  g_release({value});\n",
                    c_string(assert.message.as_deref().unwrap_or("assertion failed"))
                ));
                Ok("g_nil()".into())
            }
            Expr::Use(_) => Err(CodegenError::new(
                "`use` expressions are not yet supported by the C backend",
                expression.span,
            )),
            Expr::Echo(inner) => {
                let value = self.gen_expr(inner, out)?;
                out.push_str(&format!("  g_echo({value}, __FILE__, __LINE__);\n"));
                Ok(value)
            }
            Expr::Todo { message } => {
                out.push_str(&format!(
                    "  g_panic({}, __FILE__, __LINE__);\n",
                    c_string(message.as_deref().unwrap_or("todo"))
                ));
                Ok("g_nil()".into())
            }
            Expr::Panic { message } => {
                out.push_str(&format!(
                    "  g_panic({}, __FILE__, __LINE__);\n",
                    c_string(message.as_deref().unwrap_or("panic"))
                ));
                Ok("g_nil()".into())
            }
            Expr::Binary { op, lhs, rhs } => self.gen_binary(*op, lhs, rhs, out),
            Expr::Neg(inner) => {
                let value = self.gen_expr(inner, out)?;
                let result = self.temp();
                out.push_str(&format!(
                    "  GValue {result};\n  if (({value}).tag == G_INT) {{ {result} = g_int(-({value}).as.integer); }}\n  else if (({value}).tag == G_FLOAT) {{ {result} = g_float(-({value}).as.floating); }}\n  else g_type_error(\"Int or Float\", {value}, __FILE__, __LINE__);\n  g_release({value});\n"
                ));
                Ok(result)
            }
            Expr::Not(inner) => {
                let value = self.gen_expr(inner, out)?;
                let result = self.temp();
                out.push_str(&format!(
                    "  if (({value}).tag != G_BOOL) g_type_error(\"Bool\", {value}, __FILE__, __LINE__);\n  GValue {result} = g_bool(!({value}).as.boolean);\n  g_release({value});\n"
                ));
                Ok(result)
            }
            Expr::Pipeline { exprs, .. } => self.gen_pipeline(exprs, out),
        }
    }

    fn gen_var(&mut self, name: &str, span: Span) -> Result<String, CodegenError> {
        if let Some(value) = self.visible.get(name) {
            return Ok(format!("g_retain({value})"));
        }
        if let Some(info) = self.functions.get(name) {
            return Ok(format!("g_function({}, {})", info.c_name, info.arity));
        }
        if self.constants.iter().any(|constant| constant == name) {
            return Ok(format!("g_retain(gleam_const_{name})"));
        }
        if let Some(module) = self.unqualified.get(name).cloned() {
            if let Some((function, arity, _)) = std_function(&module, name) {
                return Ok(self.native_function(function, arity));
            }
        }
        Err(CodegenError::new(
            format!("unknown variable or function `{name}`"),
            span,
        ))
    }

    fn gen_field(
        &mut self,
        base: &Expression,
        label: &str,
        span: Span,
        out: &mut String,
    ) -> Result<String, CodegenError> {
        if let Expr::Var(module) = &base.expr {
            if let Some(path) = self.imports.get(module) {
                if let Some((function, arity, _)) = std_function(path, label) {
                    return Ok(self.native_function(function, arity));
                }
                return Err(CodegenError::new(
                    format!("module `{path}` has no C runtime function `{label}`"),
                    span,
                ));
            } else if is_stdlib_module(module) {
                // Built-in modules may be used without an import.
                if let Some((function, arity, _)) = std_function(module, label) {
                    return Ok(self.native_function(function, arity));
                }
            }
        }
        let value = self.gen_expr(base, out)?;
        let result = self.temp();
        if let Ok(index) = label.parse::<usize>() {
            out.push_str(&format!(
                "  GValue {result} = g_retain(g_tuple_get({value}, {index}));\n"
            ));
        } else {
            let index = self.field_index(label, span)?;
            out.push_str(&format!(
                "  GValue {result} = g_retain(g_record_get({value}, {index}));\n"
            ));
        }
        out.push_str(&format!("  g_release({value});\n"));
        Ok(result)
    }

    fn native_function(&mut self, function: &str, arity: usize) -> String {
        let id = self.counter;
        self.counter += 1;
        let name = format!("gleam_native_{id}");
        self.extra_functions.push_str(&format!(
            "static GValue {name}(GClosure *closure, GValue *args, size_t argc) {{\n  (void)closure;\n  return {function}(args, argc);\n}}\n\n"
        ));
        format!("g_function({name}, {arity})")
    }

    fn gen_values(&mut self, values: &[Expression], out: &mut String) -> Result<Vec<String>, CodegenError> {
        values.iter().map(|value| self.gen_expr(value, out)).collect()
    }

    fn make_aggregate(&mut self, function: &str, values: &[String], out: &mut String) -> String {
        let result = self.temp();
        if values.is_empty() {
            out.push_str(&format!("  GValue {result} = {function}(0, NULL);\n"));
        } else {
            out.push_str(&format!(
                "  GValue {result} = {function}({}, (GValue[]){{{}}});\n",
                values.len(),
                values.join(", ")
            ));
        }
        result
    }

    fn gen_block(&mut self, statements: &[Expression], out: &mut String) -> Result<String, CodegenError> {
        let saved_visible = self.visible.clone();
        self.scopes.push(Scope::default());
        let mut result = "g_nil()".to_string();
        for (index, statement) in statements.iter().enumerate() {
            let value = self.gen_expr(statement, out)?;
            if index + 1 == statements.len() {
                result = value;
            } else {
                out.push_str(&format!("  g_release({value});\n"));
            }
        }
        self.release_scope(out);
        self.visible = saved_visible;
        Ok(result)
    }

    fn gen_let(
        &mut self,
        pattern: &Pattern,
        value: &Expression,
        message: Option<&str>,
        span: Span,
        out: &mut String,
    ) -> Result<String, CodegenError> {
        let value = self.gen_expr(value, out)?;
        let condition = self.pattern_condition(pattern, &value, span)?;
        out.push_str(&format!(
            "  if (!({condition})) g_panic({}, __FILE__, __LINE__);\n",
            c_string(message.unwrap_or("pattern match failed"))
        ));
        self.bind_pattern(pattern, &value, out)?;
        out.push_str(&format!("  g_release({value});\n"));
        Ok("g_nil()".into())
    }

    fn gen_binary(
        &mut self,
        op: BinOp,
        lhs: &Expression,
        rhs: &Expression,
        out: &mut String,
    ) -> Result<String, CodegenError> {
        let left = self.gen_expr(lhs, out)?;
        if matches!(op, BinOp::And | BinOp::Or) {
            out.push_str(&format!(
                "  if (({left}).tag != G_BOOL) g_type_error(\"Bool\", {left}, __FILE__, __LINE__);\n"
            ));
            let result = self.temp();
            let short = if op == BinOp::And { "!" } else { "" };
            out.push_str(&format!(
                "  GValue {result};\n  if ({short}({left}).as.boolean) {{ {result} = g_bool({}); }} else {{\n",
                op == BinOp::Or
            ));
            let right = self.gen_expr(rhs, out)?;
            out.push_str(&format!(
                "    if (({right}).tag != G_BOOL) g_type_error(\"Bool\", {right}, __FILE__, __LINE__);\n    {result} = g_bool(({right}).as.boolean);\n    g_release({right});\n  }}\n  g_release({left});\n"
            ));
            return Ok(result);
        }
        let right = self.gen_expr(rhs, out)?;
        let result = self.temp();
        let line = match op {
            BinOp::Eq | BinOp::NotEq => format!(
                "GValue {result} = g_bool({}g_equal({left}, {right}));",
                if op == BinOp::NotEq { "!" } else { "" }
            ),
            BinOp::Concat => format!("GValue {result} = g_string_concat({left}, {right});"),
            BinOp::Slash => format!("GValue {result} = g_int_divide({left}, {right});"),
            BinOp::Percent => format!("GValue {result} = g_int_modulo({left}, {right});"),
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                self.require_type("G_INT", "Int", &left, out);
                self.require_type("G_INT", "Int", &right, out);
                let operator = match op {
                    BinOp::Lt => "<",
                    BinOp::Gt => ">",
                    BinOp::LtEq => "<=",
                    _ => ">=",
                };
                format!("GValue {result} = g_bool(({left}).as.integer {operator} ({right}).as.integer);")
            }
            BinOp::LtFloat | BinOp::GtFloat | BinOp::LtEqFloat | BinOp::GtEqFloat => {
                self.require_type("G_FLOAT", "Float", &left, out);
                self.require_type("G_FLOAT", "Float", &right, out);
                let operator = match op {
                    BinOp::LtFloat => "<",
                    BinOp::GtFloat => ">",
                    BinOp::LtEqFloat => "<=",
                    _ => ">=",
                };
                format!("GValue {result} = g_bool(({left}).as.floating {operator} ({right}).as.floating);")
            }
            BinOp::Plus | BinOp::Minus | BinOp::Times => {
                self.require_type("G_INT", "Int", &left, out);
                self.require_type("G_INT", "Int", &right, out);
                let operator = match op {
                    BinOp::Plus => "+",
                    BinOp::Minus => "-",
                    _ => "*",
                };
                format!("GValue {result} = g_int(({left}).as.integer {operator} ({right}).as.integer);")
            }
            BinOp::PlusFloat | BinOp::MinusFloat | BinOp::TimesFloat | BinOp::SlashFloat => {
                self.require_type("G_FLOAT", "Float", &left, out);
                self.require_type("G_FLOAT", "Float", &right, out);
                let operator = match op {
                    BinOp::PlusFloat => "+",
                    BinOp::MinusFloat => "-",
                    BinOp::TimesFloat => "*",
                    _ => "/",
                };
                format!("GValue {result} = g_float(({left}).as.floating {operator} ({right}).as.floating);")
            }
            BinOp::And | BinOp::Or => unreachable!(),
        };
        out.push_str(&format!("  {line}\n  g_release({left});\n  g_release({right});\n"));
        Ok(result)
    }

    fn require_type(&self, tag: &str, name: &str, value: &str, out: &mut String) {
        out.push_str(&format!(
            "  if (({value}).tag != {tag}) g_type_error(\"{name}\", {value}, __FILE__, __LINE__);\n"
        ));
    }

    fn gen_call(&mut self, call: &Call, span: Span, out: &mut String) -> Result<String, CodegenError> {
        if let Expr::Constructor(reference) = &call.fun.expr {
            let info = self.constructor(reference, span)?.clone();
            let values = self.eval_call_args(&call.args, &info.labels(), out)?;
            let result = self.make_record(&reference.name, &values, out);
            return Ok(result);
        }
        if let Expr::FieldAccess { base, label } = &call.fun.expr {
            if matches!(&base.expr, Expr::Var(_)) && label.starts_with(|c: char| c.is_ascii_uppercase()) {
                let reference = ConstructorRef {
                    module: None,
                    name: label.clone(),
                };
                let info = self.constructor(&reference, span)?.clone();
                let values = self.eval_call_args(&call.args, &info.labels(), out)?;
                return Ok(self.make_record(label, &values, out));
            }
        }

        if let Some((name, info)) = self.direct_function(&call.fun) {
            let values = self.eval_call_args(&call.args, &info.labels, out)?;
            let result = self.temp();
            let arguments = c_arguments(&values);
            if name.starts_with("gleam_fn_") {
                out.push_str(&format!(
                    "  GValue {result} = {name}(NULL, {arguments}, {});\n",
                    values.len()
                ));
            } else {
                out.push_str(&format!(
                    "  GValue {result} = {name}({arguments}, {});\n",
                    values.len()
                ));
            }
            for value in values {
                out.push_str(&format!("  g_release({value});\n"));
            }
            return Ok(result);
        }

        let function = self.gen_expr(&call.fun, out)?;
        let values = self.eval_call_args(&call.args, &vec![None; call.args.len()], out)?;
        let result = self.temp();
        out.push_str(&format!(
            "  GValue {result} = g_call({function}, {}, {});\n  g_release({function});\n",
            c_arguments(&values),
            values.len()
        ));
        for value in values {
            out.push_str(&format!("  g_release({value});\n"));
        }
        Ok(result)
    }

    fn direct_function(&self, expression: &Expression) -> Option<(String, FunctionInfo)> {
        match &expression.expr {
            Expr::Var(name) if !self.visible.contains_key(name) => {
                if let Some(info) = self.functions.get(name) {
                    return Some((info.c_name.clone(), info.clone()));
                }
                let module = self.unqualified.get(name)?;
                let (function, arity, labels) = std_function(module, name)?;
                Some((
                    function.into(),
                    FunctionInfo {
                        c_name: function.into(),
                        arity,
                        labels: labels
                            .iter()
                            .map(|label| label.map(|l| l.to_string()))
                            .collect(),
                    },
                ))
            }
            Expr::FieldAccess { base, label } => {
                let Expr::Var(module) = &base.expr else { return None };
                let function = if let Some(path) = self.imports.get(module) {
                    std_function(path, label)
                } else {
                    // Built-in modules may be used without an import.
                    is_stdlib_module(module).then(|| std_function(module, label))
                        .flatten()
                };
                let (function, arity, labels) = function?;
                Some((
                    function.into(),
                    FunctionInfo {
                        c_name: function.into(),
                        arity,
                        labels: labels
                            .iter()
                            .map(|label| label.map(|l| l.to_string()))
                            .collect(),
                    },
                ))
            }
            _ => None,
        }
    }

    fn eval_call_args(
        &mut self,
        args: &[CallArg],
        labels: &[Option<String>],
        out: &mut String,
    ) -> Result<Vec<String>, CodegenError> {
        let mut evaluated = Vec::new();
        for arg in args {
            match arg {
                CallArg::Value(value) => {
                    let value = self.gen_expr(value, out)?;
                    evaluated.push((None, value));
                }
                CallArg::Labelled { label, value } => {
                    let value = self.gen_expr(value, out)?;
                    evaluated.push((Some(label.clone()), value));
                }
            }
        }
        let mut ordered = vec![None; evaluated.len()];
        let mut position = 0;
        for (label, value) in evaluated {
            let index = if let Some(label) = label {
                labels.iter().position(|candidate| candidate.as_ref() == Some(&label))
            } else {
                while position < ordered.len() && ordered[position].is_some() {
                    position += 1;
                }
                (position < ordered.len()).then_some(position)
            };
            let Some(index) = index.filter(|index| *index < ordered.len()) else {
                return Err(CodegenError::new("unknown or misplaced call argument", Span::point(0)));
            };
            if ordered[index].replace(value).is_some() {
                return Err(CodegenError::new("duplicate call argument", Span::point(0)));
            }
        }
        Ok(ordered.into_iter().map(Option::unwrap).collect())
    }

    fn make_record(&mut self, tag: &str, values: &[String], out: &mut String) -> String {
        let result = self.temp();
        out.push_str(&format!(
            "  GValue {result} = g_record({}, {}, {});\n",
            c_string(tag),
            values.len(),
            c_arguments(values)
        ));
        result
    }

    fn gen_pipeline(&mut self, exprs: &[Expression], out: &mut String) -> Result<String, CodegenError> {
        let mut value = self.gen_expr(&exprs[0], out)?;
        for stage in &exprs[1..] {
            match &stage.expr {
                Expr::Var(_) | Expr::FieldAccess { .. } | Expr::Constructor(_) => {
                    let function = self.gen_expr(stage, out)?;
                    let result = self.temp();
                    out.push_str(&format!(
                        "  GValue {result} = g_call({function}, (GValue[]){{{value}}}, 1);\n  g_release({function});\n  g_release({value});\n"
                    ));
                    value = result;
                }
                Expr::Call(call) => {
                    let mut args = vec![CallArg::Value(Expression::synthetic(Expr::Nil))];
                    args.extend(call.args.clone());
                    let fake = Call {
                        fun: call.fun.clone(),
                        args,
                        fun_span: call.fun_span,
                    };
                    let values = self.eval_call_args(&fake.args[1..], &vec![None; fake.args.len() - 1], out)?;
                    let function = self.gen_expr(&call.fun, out)?;
                    let mut all = vec![value.clone()];
                    all.extend(values.iter().cloned());
                    let result = self.temp();
                    out.push_str(&format!(
                        "  GValue {result} = g_call({function}, {}, {});\n  g_release({function});\n  g_release({value});\n",
                        c_arguments(&all), all.len()
                    ));
                    for argument in values {
                        out.push_str(&format!("  g_release({argument});\n"));
                    }
                    value = result;
                }
                _ => {
                    let function = self.gen_expr(stage, out)?;
                    let result = self.temp();
                    out.push_str(&format!(
                        "  GValue {result} = g_call({function}, (GValue[]){{{value}}}, 1);\n  g_release({function});\n  g_release({value});\n"
                    ));
                    value = result;
                }
            }
        }
        Ok(value)
    }

    fn gen_case(&mut self, case: &Case, span: Span, out: &mut String) -> Result<String, CodegenError> {
        let subjects = self.gen_values(&case.subjects, out)?;
        let result = self.temp();
        out.push_str(&format!("  GValue {result};\n  int {result}_matched = 0;\n"));
        for clause in &case.clauses {
            let saved_visible = self.visible.clone();
            out.push_str(&format!("  if (!{result}_matched) {{\n"));
            self.scopes.push(Scope::default());
            let mut conditions = Vec::new();
            for (pattern, subject) in clause.patterns.iter().zip(&subjects) {
                conditions.push(self.pattern_condition(pattern, subject, span)?);
            }
            let condition = if conditions.is_empty() {
                "1".into()
            } else {
                conditions.join(" && ")
            };
            out.push_str(&format!("    if ({condition}) {{\n"));
            for (pattern, subject) in clause.patterns.iter().zip(&subjects) {
                self.bind_pattern(pattern, subject, out)?;
            }
            if let Some(guard) = &clause.guard {
                let guard = self.gen_guard(guard, out)?;
                out.push_str(&format!(
                    "      if (({guard}).tag != G_BOOL) g_type_error(\"Bool\", {guard}, __FILE__, __LINE__);\n      if (({guard}).as.boolean) {{\n      g_release({guard});\n"
                ));
            }
            let value = self.gen_expr(&clause.expression, out)?;
            out.push_str(&format!("      {result} = {value};\n      {result}_matched = 1;\n"));
            if clause.guard.is_some() {
                out.push_str("      }\n");
            }
            self.release_scope(out);
            out.push_str("    }\n  }\n");
            self.visible = saved_visible;
        }
        out.push_str(&format!(
            "  if (!{result}_matched) g_panic(\"no case clause matched\", __FILE__, __LINE__);\n"
        ));
        for subject in subjects {
            out.push_str(&format!("  g_release({subject});\n"));
        }
        Ok(result)
    }

    fn gen_guard(&mut self, guard: &Guard, out: &mut String) -> Result<String, CodegenError> {
        match guard {
            Guard::Int(value) => Ok(format!("g_int({})", self.integer(value, Span::point(0))?)),
            Guard::Float(value) => Ok(format!("g_float({})", self.float(value))),
            Guard::String(value) => Ok(format!("g_string({})", c_string(value))),
            Guard::Bool(value) => Ok(format!("g_bool({})", *value as u8)),
            Guard::Nil => Ok("g_nil()".into()),
            Guard::Var(name) => self.gen_var(name, Span::point(0)),
            Guard::FieldAccess { base, label } => {
                let value = self.gen_guard(base, out)?;
                let result = self.temp();
                if let Ok(index) = label.parse::<usize>() {
                    out.push_str(&format!(
                        "  GValue {result} = g_retain(g_tuple_get({value}, {index}));\n"
                    ));
                } else {
                    let index = self.field_index(label, Span::point(0))?;
                    out.push_str(&format!(
                        "  GValue {result} = g_retain(g_record_get({value}, {index}));\n"
                    ));
                }
                out.push_str(&format!("  g_release({value});\n"));
                Ok(result)
            }
            Guard::Call { .. } => Err(CodegenError::new(
                "function calls in guards are not supported by the C backend",
                Span::point(0),
            )),
            Guard::Binary { op, lhs, rhs } => {
                let left = self.gen_guard(lhs, out)?;
                let right = self.gen_guard(rhs, out)?;
                let result = self.temp();
                let expression = match op {
                    BinOp::And | BinOp::Or => {
                        let operator = if *op == BinOp::And { "&&" } else { "||" };
                        format!("g_bool(({left}).as.boolean {operator} ({right}).as.boolean)")
                    }
                    BinOp::Eq | BinOp::NotEq => format!(
                        "g_bool({}g_equal({left}, {right}))",
                        if *op == BinOp::NotEq { "!" } else { "" }
                    ),
                    BinOp::Concat => format!("g_string_concat({left}, {right})"),
                    _ => {
                        let operator = match op {
                            BinOp::Lt | BinOp::LtFloat => "<",
                            BinOp::Gt | BinOp::GtFloat => ">",
                            BinOp::LtEq | BinOp::LtEqFloat => "<=",
                            BinOp::GtEq | BinOp::GtEqFloat => ">=",
                            BinOp::Plus | BinOp::PlusFloat => "+",
                            BinOp::Minus | BinOp::MinusFloat => "-",
                            BinOp::Times | BinOp::TimesFloat => "*",
                            BinOp::Slash | BinOp::SlashFloat => "/",
                            BinOp::Percent => "%",
                            _ => return Err(CodegenError::new("invalid guard operator", Span::point(0))),
                        };
                        if matches!(op, BinOp::PlusFloat | BinOp::MinusFloat | BinOp::TimesFloat | BinOp::SlashFloat | BinOp::LtFloat | BinOp::GtFloat | BinOp::LtEqFloat | BinOp::GtEqFloat) {
                            if matches!(op, BinOp::LtFloat | BinOp::GtFloat | BinOp::LtEqFloat | BinOp::GtEqFloat) {
                                format!("g_bool(({left}).as.floating {operator} ({right}).as.floating)")
                            } else {
                                format!("g_float(({left}).as.floating {operator} ({right}).as.floating)")
                            }
                        } else if matches!(op, BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq) {
                            format!("g_bool(({left}).as.integer {operator} ({right}).as.integer)")
                        } else {
                            format!("g_int(({left}).as.integer {operator} ({right}).as.integer)")
                        }
                    }
                };
                out.push_str(&format!(
                    "  GValue {result} = {expression};\n  g_release({left});\n  g_release({right});\n"
                ));
                Ok(result)
            }
        }
    }

    fn pattern_condition(&self, pattern: &Pattern, value: &str, span: Span) -> Result<String, CodegenError> {
        Ok(match pattern {
            Pattern::Var(_) | Pattern::Discard(_) => "1".into(),
            Pattern::Int(expected) => format!(
                "({value}).tag == G_INT && ({value}).as.integer == {}",
                expected.as_u64().ok_or_else(|| CodegenError::new("invalid integer", span))?
            ),
            Pattern::Float(expected) => format!(
                "({value}).tag == G_FLOAT && ({value}).as.floating == {}",
                self.float(expected)
            ),
            Pattern::String(expected) => {
                format!("g_string_equals({value}, {})", c_string(expected))
            },
            Pattern::Bool(expected) => format!(
                "({value}).tag == G_BOOL && ({value}).as.boolean == {}",
                *expected as u8
            ),
            Pattern::Nil => format!("({value}).tag == G_NIL"),
            Pattern::StringPrefix { prefix, .. } => {
                format!("g_string_starts_with({value}, {})", c_string(prefix))
            }
            Pattern::Constructor(reference, args) => {
                let info = self.constructor(reference, span)?;
                let mut checks = vec![format!("g_record_is({value}, {})", c_string(&reference.name))];
                let mut position = 0;
                for arg in args {
                    match arg {
                        PatternArg::Named { label, pattern } => {
                            let index = info.fields.iter().position(|field| {
                                field.label.as_ref().or(field.name.as_ref()).map(String::as_str) == Some(label)
                            }).ok_or_else(|| CodegenError::new(format!("unknown field `{label}`"), span))?;
                            checks.push(self.pattern_condition(pattern, &format!("g_record_get({value}, {index})"), span)?);
                        }
                        PatternArg::Unnamed(pattern) => {
                            checks.push(self.pattern_condition(pattern, &format!("g_record_get({value}, {position})"), span)?);
                            position += 1;
                        }
                        PatternArg::DiscardRest => {}
                    }
                }
                checks.join(" && ")
            }
            Pattern::List { elements, rest } => {
                let minimum = elements.len();
                let length_check = if rest.is_some() { ">=" } else { "==" };
                let mut checks = vec![format!(
                    "(({value}).tag == G_NIL || ({value}).tag == G_LIST) && g_list_length({value}) {length_check} {minimum}"
                )];
                for (index, element) in elements.iter().enumerate() {
                    checks.push(self.pattern_condition(element, &list_item(value, index), span)?);
                }
                checks.join(" && ")
            }
            Pattern::Tuple(elements) => {
                let mut checks = vec![format!("({value}).tag == G_TUPLE && ((GTuple *)({value}).as.pointer)->length == {}", elements.len())];
                for (index, element) in elements.iter().enumerate() {
                    checks.push(self.pattern_condition(element, &format!("g_tuple_get({value}, {index})"), span)?);
                }
                checks.join(" && ")
            }
            Pattern::BitArray(_) => return Err(CodegenError::new("bit array patterns are not supported", span)),
            Pattern::Alias { pattern, .. } => self.pattern_condition(pattern, value, span)?,
            Pattern::Alternative(patterns) => {
                let checks = patterns
                    .iter()
                    .map(|pattern| self.pattern_condition(pattern, value, span))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("({})", checks.join(" || "))
            }
        })
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &str, out: &mut String) -> Result<(), CodegenError> {
        match pattern {
            Pattern::Var(name) => self.bind(name, format!("g_retain({value})"), out),
            Pattern::Discard(_) => {}
            Pattern::StringPrefix { prefix, rest } => {
                self.bind_pattern(rest, &format!("g_string_drop_prefix({value}, {})", prefix.len()), out)?;
            }
            Pattern::Constructor(reference, args) => {
                let info = self.constructor(reference, Span::point(0))?.clone();
                let mut position = 0;
                for arg in args {
                    match arg {
                        PatternArg::Named { label, pattern } => {
                            let index = info.fields.iter().position(|field| {
                                field.label.as_ref().or(field.name.as_ref()).map(String::as_str) == Some(label)
                            }).unwrap();
                            self.bind_pattern(pattern, &format!("g_record_get({value}, {index})"), out)?;
                        }
                        PatternArg::Unnamed(pattern) => {
                            self.bind_pattern(pattern, &format!("g_record_get({value}, {position})"), out)?;
                            position += 1;
                        }
                        PatternArg::DiscardRest => {}
                    }
                }
            }
            Pattern::List { elements, rest } => {
                for (index, element) in elements.iter().enumerate() {
                    self.bind_pattern(element, &list_item(value, index), out)?;
                }
                if let Some(rest) = rest {
                    self.bind_pattern(rest, &list_tail(value, elements.len()), out)?;
                }
            }
            Pattern::Tuple(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    self.bind_pattern(element, &format!("g_tuple_get({value}, {index})"), out)?;
                }
            }
            Pattern::Alias { pattern, name } => {
                self.bind(name, format!("g_retain({value})"), out);
                self.bind_pattern(pattern, value, out)?;
            }
            Pattern::Alternative(patterns) => {
                let condition = self.pattern_condition(&patterns[0], value, Span::point(0))?;
                out.push_str(&format!("  if ({condition}) {{\n"));
                self.bind_pattern(&patterns[0], value, out)?;
                for alternative in &patterns[1..] {
                    out.push_str("  } else {\n");
                    self.bind_pattern(alternative, value, out)?;
                }
                out.push_str("  }\n");
            }
            _ => {}
        }
        Ok(())
    }

    fn bind(&mut self, name: &str, value: String, out: &mut String) {
        let variable = self.temp();
        out.push_str(&format!("  GValue {variable} = {value};\n"));
        self.visible.insert(name.into(), variable.clone());
        self.scopes.last_mut().unwrap().locals.push(variable);
    }

    fn field_index(&self, label: &str, span: Span) -> Result<usize, CodegenError> {
        let mut found = None;
        for info in self.constructors.values() {
            if let Some(index) = info.fields.iter().position(|field| {
                field.label.as_ref().or(field.name.as_ref()).map(String::as_str) == Some(label)
            }) {
                if found.is_some_and(|previous| previous != index) {
                    return Err(CodegenError::new(format!("ambiguous record field `{label}`"), span));
                }
                found = Some(index);
            }
        }
        found.ok_or_else(|| CodegenError::new(format!("unknown record field `{label}`"), span))
    }

    fn constructor(&self, reference: &ConstructorRef, span: Span) -> Result<&ConstructorInfo, CodegenError> {
        self.constructors.get(&reference.name).ok_or_else(|| {
            CodegenError::new(format!("unknown constructor `{}`", reference.name), span)
        })
    }

    fn gen_record_update(&mut self, update: &RecordUpdate, span: Span, out: &mut String) -> Result<String, CodegenError> {
        let info = self.constructor(&update.constructor, span)?.clone();
        let base = self.gen_expr(&update.base, out)?;
        let mut replacements = HashMap::new();
        for arg in &update.args {
            let CallArg::Labelled { label, value } = arg else {
                return Err(CodegenError::new("record updates require labelled fields", span));
            };
            let index = info.fields.iter().position(|field| {
                field.label.as_ref().or(field.name.as_ref()).map(String::as_str) == Some(label)
            }).ok_or_else(|| CodegenError::new(format!("unknown field `{label}`"), span))?;
            let value = self.gen_expr(value, out)?;
            replacements.insert(index, value);
        }
        let values: Vec<String> = (0..info.fields.len())
            .map(|index| {
                replacements.remove(&index).unwrap_or_else(|| {
                    format!("g_retain(g_record_get({base}, {index}))")
                })
            })
            .collect();
        let result = self.make_record(&update.constructor.name, &values, out);
        out.push_str(&format!("  g_release({base});\n"));
        for value in replacements.into_values() {
            out.push_str(&format!("  g_release({value});\n"));
        }
        Ok(result)
    }

    fn gen_closure(&mut self, function: &FnLiteral, _out: &mut String) -> Result<String, CodegenError> {
        let mut captures: Vec<(String, String)> = self
            .visible
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        captures.sort_by(|left, right| left.0.cmp(&right.0));
        let id = self.counter;
        self.counter += 1;
        let name = format!("gleam_lambda_{id}");
        let saved_visible = std::mem::take(&mut self.visible);
        let saved_scopes = std::mem::take(&mut self.scopes);
        self.scopes.push(Scope::default());
        let mut body = format!(
            "static GValue {name}(GClosure *closure, GValue *args, size_t argc) {{\n  if (argc != {}) g_panic(\"incorrect argument count\", __FILE__, __LINE__);\n",
            function.args.len()
        );
        for (index, (variable, _)) in captures.iter().enumerate() {
            let value = self.temp();
            body.push_str(&format!(
                "  GValue {value} = g_retain(closure->captures[{index}]);\n"
            ));
            self.visible.insert(variable.clone(), value.clone());
            self.scopes.last_mut().unwrap().locals.push(value);
        }
        for (index, argument) in function.args.iter().enumerate() {
            if !argument.starts_with('_') {
                let value = self.temp();
                body.push_str(&format!("  GValue {value} = g_retain(args[{index}]);\n"));
                self.visible.insert(argument.clone(), value.clone());
                self.scopes.last_mut().unwrap().locals.push(value);
            }
        }
        let result = self.gen_expr(&function.body, &mut body)?;
        body.push_str(&format!("  GValue gleam_return = {result};\n"));
        self.release_scope(&mut body);
        body.push_str("  return gleam_return;\n}\n\n");
        self.extra_functions.push_str(&body);
        self.visible = saved_visible;
        self.scopes = saved_scopes;
        let retained: Vec<String> = captures.iter().map(|(_, value)| format!("g_retain({value})")).collect();
        Ok(format!(
            "g_closure_new({name}, {}, {}, {})",
            function.args.len(),
            retained.len(),
            c_arguments(&retained)
        ))
    }

    fn gen_capture(&mut self, capture: &Capture, span: Span, out: &mut String) -> Result<String, CodegenError> {
        let function = self.gen_expr(&capture.fun, out)?;
        let mut captured_args = Vec::new();
        let mut placeholders = 0;
        for arg in &capture.args {
            match arg {
                CaptureArg::Value(expression) => {
                    captured_args.push(Some(self.gen_expr(expression, out)?));
                }
                CaptureArg::Placeholder => {
                    placeholders += 1;
                    captured_args.push(None);
                }
            }
        }
        if placeholders != 1 {
            return Err(CodegenError::new("function captures require one placeholder", span));
        }
        let id = self.counter;
        self.counter += 1;
        let name = format!("gleam_capture_{id}");
        let mut definition = format!(
            "static GValue {name}(GClosure *closure, GValue *args, size_t argc) {{\n  if (argc != 1) g_panic(\"incorrect argument count\", __FILE__, __LINE__);\n  GValue captured[{arity}];\n",
            arity = captured_args.len()
        );
        let mut placeholder = 0;
        for (index, argument) in captured_args.iter().enumerate() {
            if argument.is_some() {
                definition.push_str(&format!("  captured[{index}] = closure->captures[{}];\n", index + 1));
            } else {
                definition.push_str(&format!("  captured[{index}] = args[{placeholder}];\n"));
                placeholder += 1;
            }
        }
        definition.push_str(&format!(
            "  return g_call(closure->captures[0], captured, {});\n}}\n\n",
            captured_args.len()
        ));
        self.extra_functions.push_str(&definition);
        let mut captures = vec![function];
        captures.extend(captured_args.into_iter().flatten());
        Ok(format!(
            "g_closure_new({name}, 1, {}, {})",
            captures.len(),
            c_arguments(&captures)
        ))
    }

    fn integer(&self, value: &IntValue, span: Span) -> Result<String, CodegenError> {
        let value = value
            .as_u64()
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| CodegenError::new("integer does not fit in a C int64_t", span))?;
        Ok(format!("INT64_C({value})"))
    }

    fn float(&self, value: &FloatValue) -> String {
        value.raw.replace('_', "")
    }
}

impl ConstructorInfo {
    fn labels(&self) -> Vec<Option<String>> {
        self.fields
            .iter()
            .map(|field| field.label.clone().or_else(|| field.name.clone()))
            .collect()
    }
}

/// A standard library function: C name, arity, and argument labels.
fn std_function(
    module: &str,
    name: &str,
) -> Option<(&'static str, usize, &'static [Option<&'static str>])> {
    let io = matches!(module, "gleam/io" | "io");
    let fs = matches!(module, "gleam/fs" | "gleam/filesystem" | "fs" | "filesystem");
    let string = matches!(module, "gleam/string" | "string");
    let int = matches!(module, "gleam/int" | "int");
    match (io, fs, string, int, name) {
        (true, _, _, _, "print") => Some(("gleam_io_print", 1, &[Some("message")])),
        (true, _, _, _, "println") => Some(("gleam_io_println", 1, &[Some("message")])),
        (true, _, _, _, "debug") => Some(("gleam_io_debug", 1, &[Some("value")])),
        (true, _, _, _, "read_line") => Some(("gleam_io_read_line", 0, &[])),
        (_, true, _, _, "read" | "read_file") => Some(("gleam_fs_read", 1, &[Some("path")])),
        (_, true, _, _, "write" | "write_file") => {
            Some(("gleam_fs_write", 2, &[Some("path"), Some("contents")]))
        }
        (_, true, _, _, "append" | "append_file") => {
            Some(("gleam_fs_append", 2, &[Some("path"), Some("contents")]))
        }
        (_, true, _, _, "remove" | "delete") => Some(("gleam_fs_remove", 1, &[Some("path")])),
        (_, true, _, _, "is_file") => Some(("gleam_fs_is_file", 1, &[Some("path")])),
        (_, true, _, _, "is_directory") => Some(("gleam_fs_is_directory", 1, &[Some("path")])),
        (_, true, _, _, "create_directory") => {
            Some(("gleam_fs_create_directory", 1, &[Some("path")]))
        }
        (_, true, _, _, "list_directory" | "list") => {
            Some(("gleam_fs_list_directory", 1, &[Some("path")]))
        }
        // gleam/string (grapheme and UTF code point functions excluded).
        (_, _, true, _, "append") => {
            Some(("gleam_string_append", 2, &[Some("to"), Some("suffix")]))
        }
        (_, _, true, _, "byte_size") => Some(("gleam_string_byte_size", 1, &[Some("string")])),
        (_, _, true, _, "capitalise") => {
            Some(("gleam_string_capitalise", 1, &[Some("string")]))
        }
        (_, _, true, _, "compare") => Some(("gleam_string_compare", 2, &[None, None])),
        (_, _, true, _, "concat") => Some(("gleam_string_concat", 1, &[Some("strings")])),
        (_, _, true, _, "contains") => {
            Some(("gleam_string_contains", 2, &[Some("does"), Some("contain")]))
        }
        (_, _, true, _, "crop") => Some(("gleam_string_crop", 2, &[Some("from"), Some("before")])),
        (_, _, true, _, "ends_with") => Some(("gleam_string_ends_with", 2, &[None, None])),
        (_, _, true, _, "first") => Some(("gleam_string_first", 1, &[Some("string")])),
        (_, _, true, _, "inspect") => Some(("gleam_string_inspect", 1, &[Some("term")])),
        (_, _, true, _, "is_empty") => Some(("gleam_string_is_empty", 1, &[Some("str")])),
        (_, _, true, _, "join") => {
            Some(("gleam_string_join", 2, &[Some("strings"), Some("with")]))
        }
        (_, _, true, _, "last") => Some(("gleam_string_last", 1, &[Some("string")])),
        (_, _, true, _, "length") => Some(("gleam_string_length", 1, &[Some("string")])),
        (_, _, true, _, "lowercase") => Some(("gleam_string_lowercase", 1, &[Some("string")])),
        (_, _, true, _, "pad_end") => {
            Some(("gleam_string_pad_end", 3,
                  &[Some("string"), Some("to"), Some("with")]))
        }
        (_, _, true, _, "pad_start") => {
            Some(("gleam_string_pad_start", 3,
                  &[Some("string"), Some("to"), Some("with")]))
        }
        (_, _, true, _, "remove_prefix") => {
            Some(("gleam_string_remove_prefix", 2, &[Some("from"), Some("matching")]))
        }
        (_, _, true, _, "remove_suffix") => {
            Some(("gleam_string_remove_suffix", 2, &[Some("from"), Some("matching")]))
        }
        (_, _, true, _, "repeat") => {
            Some(("gleam_string_repeat", 2, &[Some("string"), Some("times")]))
        }
        (_, _, true, _, "replace") => {
            Some(("gleam_string_replace", 3,
                  &[Some("in"), Some("each"), Some("with")]))
        }
        (_, _, true, _, "reverse") => Some(("gleam_string_reverse", 1, &[Some("string")])),
        (_, _, true, _, "slice") => {
            Some(("gleam_string_slice", 3,
                  &[Some("from"), Some("at_index"), Some("length")]))
        }
        (_, _, true, _, "split") => Some(("gleam_string_split", 2, &[None, Some("on")])),
        (_, _, true, _, "split_once") => {
            Some(("gleam_string_split_once", 2, &[Some("string"), Some("on")]))
        }
        (_, _, true, _, "starts_with") => Some(("gleam_string_starts_with", 2, &[None, None])),
        (_, _, true, _, "to_option") => Some(("gleam_string_to_option", 1, &[Some("string")])),
        (_, _, true, _, "trim") => Some(("gleam_string_trim", 1, &[Some("string")])),
        (_, _, true, _, "trim_end") => Some(("gleam_string_trim_end", 1, &[Some("string")])),
        (_, _, true, _, "trim_start") => Some(("gleam_string_trim_start", 1, &[Some("string")])),
        (_, _, true, _, "uppercase") => Some(("gleam_string_uppercase", 1, &[Some("string")])),
        // gleam/int.
        (_, _, _, true, "absolute_value") => {
            Some(("gleam_int_absolute_value", 1, &[Some("num")]))
        }
        (_, _, _, true, "clamp") => {
            Some(("gleam_int_clamp", 3, &[None, Some("min"), Some("max")]))
        }
        (_, _, _, true, "compare") => Some(("gleam_int_compare", 2, &[None, Some("with")])),
        (_, _, _, true, "digits") => {
            Some(("gleam_int_digits", 2, &[Some("number"), Some("base")]))
        }
        (_, _, _, true, "is_even") => Some(("gleam_int_is_even", 1, &[Some("x")])),
        (_, _, _, true, "is_odd") => Some(("gleam_int_is_odd", 1, &[Some("x")])),
        (_, _, _, true, "max") => Some(("gleam_int_max", 2, &[None, None])),
        (_, _, _, true, "min") => Some(("gleam_int_min", 2, &[None, None])),
        (_, _, _, true, "negate") => Some(("gleam_int_negate", 1, &[Some("x")])),
        (_, _, _, true, "parse") => Some(("gleam_int_parse", 1, &[Some("string")])),
        (_, _, _, true, "product") => Some(("gleam_int_product", 1, &[Some("numbers")])),
        (_, _, _, true, "random") => Some(("gleam_int_random", 2, &[None, None])),
        (_, _, _, true, "sum") => Some(("gleam_int_sum", 1, &[Some("numbers")])),
        (_, _, _, true, "to_base16") => Some(("gleam_int_to_base16", 1, &[Some("int")])),
        (_, _, _, true, "to_base2") => Some(("gleam_int_to_base2", 1, &[Some("int")])),
        (_, _, _, true, "to_base36") => Some(("gleam_int_to_base36", 1, &[Some("int")])),
        (_, _, _, true, "to_base8") => Some(("gleam_int_to_base8", 1, &[Some("int")])),
        (_, _, _, true, "to_base_string") => {
            Some(("gleam_int_to_base_string", 2, &[Some("int"), Some("base")]))
        }
        (_, _, _, true, "to_float") => Some(("gleam_int_to_float", 1, &[Some("int")])),
        (_, _, _, true, "to_string") => Some(("gleam_int_to_string", 1, &[Some("int")])),
        (_, _, _, true, "undigits") => {
            Some(("gleam_int_undigits", 2, &[Some("numbers"), Some("base")]))
        }
        _ => None,
    }
}

/// Built-in standard library modules that may be referenced without an import.
fn is_stdlib_module(module: &str) -> bool {
    matches!(
        module,
        "gleam/io" | "io" | "gleam/fs" | "gleam/filesystem" | "fs" | "filesystem"
            | "gleam/string" | "string" | "gleam/int" | "int"
    )
}

fn c_arguments(values: &[String]) -> String {
    if values.is_empty() {
        "NULL".into()
    } else {
        format!("(GValue[]){{{}}}", values.join(", "))
    }
}

fn c_string(value: &str) -> String {
    let mut result = String::from("\"");
    for byte in value.as_bytes() {
        match byte {
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            b'\t' => result.push_str("\\t"),
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            0x20..=0x7e => result.push(*byte as char),
            _ => result.push_str(&format!("\\{byte:03o}")),
        }
    }
    result.push('"');
    result
}

fn list_item(value: &str, index: usize) -> String {
    if index == 0 {
        format!("g_list_head({value})")
    } else {
        format!("g_list_head({})", list_tail(value, index))
    }
}

fn list_tail(value: &str, count: usize) -> String {
    let mut result = value.to_string();
    for _ in 0..count {
        result = format!("g_list_tail({result})");
    }
    result
}
