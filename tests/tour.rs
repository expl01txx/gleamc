//! End-to-end tests for the Gleam parser, following the sections of the
//! official Gleam language tour.

use gleam_parser::ast::*;
use gleam_parser::parse;

/// Parse and assert success.
fn ok(src: &str) -> Module {
    match parse(src) {
        Ok(m) => m,
        Err(e) => panic!("failed to parse:\n{src}\n\nerror: {e}"),
    }
}

/// Parse and assert failure.
fn err(src: &str) -> ParseError {
    match parse(src) {
        Ok(_) => panic!("expected a parse error, but it succeeded:\n{src}"),
        Err(e) => e,
    }
}

/// The body of a function, unwrapping a single-statement block.
fn single_body(f: &FnDef) -> &Expr {
    match &f.body.expr {
        Expr::Block(b) if b.len() == 1 => &b[0].expr,
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Basics
// ---------------------------------------------------------------------------

#[test]
fn hello_world() {
    let m = ok(
        r#"
import gleam/io

pub fn main() {
  io.println("Hello, Joe!")
}
"#,
    );
    assert_eq!(m.definitions.len(), 2);
    match &m.definitions[1] {
        Definition::Fn(f) => {
            assert_eq!(f.name, "main");
            assert!(f.public);
            match single_body(f) {
                Expr::Call(c) => match &c.fun.expr {
                    Expr::FieldAccess { base, label } => {
                        assert_eq!(label, "println");
                        assert_eq!(&base.expr, &Expr::Var("io".into()));
                    }
                    other => panic!("expected a field access, got {other:?}"),
                },
                other => panic!("expected a call, got {other:?}"),
            }
        }
        other => panic!("expected a fn definition, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Modules & imports
// ---------------------------------------------------------------------------

#[test]
fn qualified_import() {
    let m = ok("import gleam/io\n");
    match &m.definitions[0] {
        Definition::Import(i) => {
            assert_eq!(i.module, vec!["gleam", "io"]);
            assert_eq!(i.alias, None);
            assert!(i.unqualified.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn aliased_import() {
    let m = ok("import gleam/list as l\n");
    match &m.definitions[0] {
        Definition::Import(i) => {
            assert_eq!(i.module, vec!["gleam", "list"]);
            assert_eq!(i.alias.as_deref(), Some("l"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn unqualified_imports() {
    let m = ok("import gleam/list {map, filter, type List}\n");
    match &m.definitions[0] {
        Definition::Import(i) => {
            assert_eq!(
                i.unqualified,
                vec![
                    UnqualifiedImport::Value("map".into()),
                    UnqualifiedImport::Value("filter".into()),
                    UnqualifiedImport::Type("List".into()),
                ]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn unqualified_import_with_alias() {
    let m = ok("import gleam/io as i {println}\n");
    match &m.definitions[0] {
        Definition::Import(i) => {
            assert_eq!(i.alias.as_deref(), Some("i"));
            assert_eq!(i.unqualified, vec![UnqualifiedImport::Value("println".into())]);
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[test]
fn int_literals() {
    let m = ok(
        r#"
pub fn main() {
  let a = 123
  let b = -456
  let c = 0xF
  let d = 0b1010
  let e = 0o17
  let f = 1_000_000
  a
}
"#,
    );
    // Check the raw text is preserved.
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().expect("block body");
            assert_eq!(block.len(), 7);
            if let Expr::Let(l) = &block[2].expr {
                if let Expr::Int(i) = &l.value.expr {
                    assert_eq!(i.raw, "0xF");
                    assert_eq!(i.as_u64(), Some(15));
                } else {
                    panic!("not an int")
                }
            }
            if let Expr::Let(l) = &block[4].expr {
                if let Expr::Int(i) = &l.value.expr {
                    assert_eq!(i.raw, "0o17");
                    assert_eq!(i.as_u64(), Some(15));
                } else {
                    panic!("not an int")
                }
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn float_literals() {
    let m = ok("pub fn main() { 3.14 }\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Float(fl) => {
                assert!((fl.value - 3.14).abs() < f64::EPSILON);
                assert_eq!(fl.raw, "3.14");
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn number_underscore_formats() {
    let m = ok("pub fn main() { 123_456.789_012 }\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Float(fl) => assert_eq!(fl.raw, "123_456.789_012"),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Strings & equality
// ---------------------------------------------------------------------------

#[test]
fn string_concatenation() {
    let m = ok(r#"pub fn main() { "Hello, " <> "Joe!" }"#);
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Binary { op, lhs, rhs } => {
                assert_eq!(*op, BinOp::Concat);
                assert_eq!(&lhs.expr, &Expr::String("Hello, ".into()));
                assert_eq!(&rhs.expr, &Expr::String("Joe!".into()));
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn string_escapes() {
    let m = ok(r#"pub fn main() { "a\nb\tc\u{1F600}" }"#);
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::String(s) => assert_eq!(s, "a\nb\tc\u{1F600}"),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn equality_operators() {
    let m = ok("pub fn main(x: Int, y: Int) {\n  x == y\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Binary { op, .. } => assert_eq!(*op, BinOp::Eq),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Bools, assignments, discards
// ---------------------------------------------------------------------------

#[test]
fn booleans() {
    let m = ok("pub fn main() {\n  let a = True\n  let b = False\n  a && b\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[2].expr {
                Expr::Binary { op, .. } => assert_eq!(*op, BinOp::And),
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn discard_patterns() {
    let m = ok("pub fn main(x: Int) {\n  let _ = x\n  let _y = x\n  0\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[0].expr {
                Expr::Let(l) => assert_eq!(l.pattern, Pattern::Discard("_".into())),
                other => panic!("{other:?}"),
            }
            match &block[1].expr {
                Expr::Let(l) => assert_eq!(l.pattern, Pattern::Discard("_y".into())),
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Type annotations
// ---------------------------------------------------------------------------

#[test]
fn all_type_annotations() {
    let m = ok(
        r#"
pub fn f(
  a: Int,
  b: Float,
  c: String,
  d: Bool,
  e: List(Int),
  g: #(Int, String),
  h: BitArray,
  i: BitArray(8, 8),
  j: fn(Int) -> String,
  k: fn(Int, String) -> Bool,
  l: Result(String, Int),
) -> List(#(Int, String)) {
  Nil
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            assert_eq!(f.args.len(), 11);
            // Check a few specific annotations.
            match &f.args[4].type_annotation.as_ref().unwrap() {
                TypeAnnotation::Named { name, args, .. } => {
                    assert_eq!(name, "List");
                    assert_eq!(args.len(), 1);
                }
                other => panic!("{other:?}"),
            }
            match &f.args[6].type_annotation.as_ref().unwrap() {
                TypeAnnotation::BitArray { size, unit } => {
                    assert_eq!(*size, None);
                    assert_eq!(*unit, None);
                }
                other => panic!("{other:?}"),
            }
            match &f.args[7].type_annotation.as_ref().unwrap() {
                TypeAnnotation::BitArray { size, unit } => {
                    assert!(size.is_some());
                    assert!(unit.is_some());
                }
                other => panic!("{other:?}"),
            }
            match &f.args[9].type_annotation.as_ref().unwrap() {
                TypeAnnotation::Fn { args, ret } => {
                    assert_eq!(args.len(), 2);
                    if let TypeAnnotation::Named { name, .. } = &**ret {
                        assert_eq!(name, "Bool");
                    } else {
                        panic!("not a named type")
                    }
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn qualified_type_annotation() {
    let m = ok("pub fn f(x: bytes_tree.BytesTree) -> Nil {\n  Nil\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match &f.args[0].type_annotation.as_ref().unwrap() {
            TypeAnnotation::Named { module, name, .. } => {
                assert_eq!(module.as_deref(), Some("bytes_tree"));
                assert_eq!(name, "BytesTree");
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Lists & blocks
// ---------------------------------------------------------------------------

#[test]
fn lists() {
    let m = ok("pub fn main() {\n  [1, 2, 3]\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::List { elements, spread } => {
                assert_eq!(elements.len(), 3);
                assert!(spread.is_none());
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn list_spread() {
    let m = ok("pub fn main(xs: List(Int)) {\n  [0, ..xs]\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::List { elements, spread } => {
                assert_eq!(elements.len(), 1);
                if let Some(s) = spread {
                    assert_eq!(&s.expr, &Expr::Var("xs".into()));
                } else {
                    panic!("no spread")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn empty_list() {
    let m = ok("pub fn main() {\n  []\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::List { elements, spread } => {
                assert!(elements.is_empty());
                assert!(spread.is_none());
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn constants() {
    let m = ok(
        r#"
const one = 1
const two: Int = 2
pub const three = 3
"#,
    );
    assert_eq!(m.definitions.len(), 3);
    match &m.definitions[0] {
        Definition::Constant(c) => {
            assert_eq!(c.name, "one");
            assert!(!c.public);
            assert_eq!(c.value, ConstValue::Int(IntValue { raw: "1".into() }));
        }
        other => panic!("{other:?}"),
    }
    match &m.definitions[1] {
        Definition::Constant(c) => {
            assert_eq!(c.name, "two");
            assert!(c.annotation.is_some());
        }
        other => panic!("{other:?}"),
    }
    match &m.definitions[2] {
        Definition::Constant(c) => {
            assert!(c.public);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn constant_list_and_tuple() {
    let m = ok(
        r#"
const xs = [1, 2, 3]
const t = #(1, "two", True)
"#,
    );
    match &m.definitions[0] {
        Definition::Constant(c) => match &c.value {
            ConstValue::List { elements, .. } => assert_eq!(elements.len(), 3),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    match &m.definitions[1] {
        Definition::Constant(c) => match &c.value {
            ConstValue::Tuple(t) => assert_eq!(t.len(), 3),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn constant_constructor() {
    let m = ok(
        r#"
type Color {
  Red
  Rgb(r: Int, g: Int, b: Int)
}

const color = Rgb(r: 255, g: 128, b: 0)
"#,
    );
    match &m.definitions[1] {
        Definition::Constant(c) => match &c.value {
            ConstValue::Constructor(ref_) => {
                assert_eq!(ref_.name, "Rgb");
                assert_eq!(ref_.module, None);
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn function_with_labels() {
    let m = ok("pub fn f(add a: Int, b: Int) -> Int {\n  a + b\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => {
            assert_eq!(f.args.len(), 2);
            assert_eq!(f.args[0].label.as_deref(), Some("add"));
            assert_eq!(f.args[0].name, "a");
            assert_eq!(f.args[1].label, None);
            assert_eq!(f.args[1].name, "b");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn anonymous_functions() {
    let m = ok(
        r#"
pub fn main() {
  let add = fn(a, b) { a + b }
  add(1, 2)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[0].expr {
                Expr::Let(l) => match &l.value.expr {
                    Expr::Fn(fl) => {
                        assert_eq!(fl.args, vec!["a".to_string(), "b".to_string()]);
                    }
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn function_capture() {
    let m = ok(
        r#"
pub fn main(xs: List(Int)) {
  xs
  |> list.map(add_one, _)
}

fn add_one(x: Int) -> Int {
  x + 1
}
"#,
    );
    // The capture `add_one, _` is inside the pipeline.
    match &m.definitions[0] {
        Definition::Fn(f) => {
            if let Expr::Pipeline { exprs, .. } = single_body(f) {
                if let Expr::Call(c) = &exprs[1].expr {
                    // Second arg is the capture.
                    assert_eq!(c.args.len(), 2);
                }
            } else {
                panic!("expected pipeline")
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn labelled_call_and_shorthand() {
    let m = ok(
        r#"
pub fn f(a: Int, b: Int) -> Int { a + b }

pub fn main(x: Int) {
  f(a: x, b: 2)
}
"#,
    );
    match &m.definitions[1] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Call(c) => {
                assert_eq!(c.args.len(), 2);
                match &c.args[0] {
                    CallArg::Labelled { label, value } => {
                        assert_eq!(label, "a");
                        assert_eq!(&value.expr, &Expr::Var("x".into()));
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Pipelines
// ---------------------------------------------------------------------------

#[test]
fn pipeline() {
    let m = ok(
        r#"
pub fn main() {
  1
  |> double
  |> double
  |> double
}

fn double(x: Int) -> Int { x * 2 }
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Pipeline { exprs, pipe_spans } => {
                assert_eq!(exprs.len(), 4);
                assert_eq!(pipe_spans.len(), 3);
                // First element is the literal 1.
                assert_eq!(&exprs[0].expr, &Expr::Int(IntValue { raw: "1".into() }));
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn pipeline_with_labelled_first_arg() {
    let m = ok(
        r#"
pub fn main(x: Int) {
  x
  |> f(a: _, b: 2)
}

fn f(a: Int, b: Int) -> Int { a + b }
"#,
    );
    // Should parse without error.
    assert!(m.definitions.len() == 2);
}

// ---------------------------------------------------------------------------
// Case expressions
// ---------------------------------------------------------------------------

#[test]
fn case_variable_patterns() {
    let m = ok(
        r#"
pub fn main(x: Int) {
  case x {
    0 -> "zero"
    y if y > 0 -> "positive"
    _ -> "negative"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                assert_eq!(c.subjects.len(), 1);
                assert_eq!(c.clauses.len(), 3);
                // Second clause has a guard.
                assert!(c.clauses[1].guard.is_some());
                match &c.clauses[2].patterns[0] {
                    Pattern::Discard(d) => assert_eq!(d, "_"),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_string_patterns() {
    let m = ok(
        r#"
pub fn main(s: String) {
  case s {
    "Hello, " <> name -> "Hi " <> name
    "" -> "empty"
    _ -> "other"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => match &c.clauses[0].patterns[0] {
                Pattern::StringPrefix { prefix, rest } => {
                    assert_eq!(prefix, "Hello, ");
                    if let Pattern::Var(v) = &**rest {
                        assert_eq!(v, "name");
                    } else {
                        panic!("not a var")
                    }
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_list_patterns() {
    let m = ok(
        r#"
pub fn main(xs: List(Int)) {
  case xs {
    [] -> "empty"
    [first, ..] -> first
    [1, 2, 3] -> "one two three"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                assert_eq!(c.clauses.len(), 3);
                // Second clause: [first, ..]
                match &c.clauses[1].patterns[0] {
                    Pattern::List { elements, rest } => {
                        assert_eq!(elements.len(), 1);
                        match &elements[0] {
                            Pattern::Var(v) => assert_eq!(v, "first"),
                            other => panic!("{other:?}"),
                        }
                        // `..` is a discard of the remaining elements.
                        match rest {
                            Some(r) => match &**r {
                                Pattern::Discard(_) => {}
                                other => panic!("{other:?}"),
                            },
                            None => panic!("no rest"),
                        }
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_multiple_subjects() {
    let m = ok(
        r#"
pub fn main(a: Int, b: Int) {
  case a, b {
    0, 0 -> "both zero"
    _, _ -> "other"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                assert_eq!(c.subjects.len(), 2);
                for clause in &c.clauses {
                    assert_eq!(clause.patterns.len(), 2);
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_alternative_patterns() {
    let m = ok(
        r#"
pub fn main(x: Int) {
  case x {
    2 | 4 | 6 -> "even"
    _ -> "odd"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => match &c.clauses[0].patterns[0] {
                Pattern::Alternative(alts) => assert_eq!(alts.len(), 3),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_pattern_alias() {
    let m = ok(
        r#"
pub fn main(xs: List(Int)) {
  case xs {
    [1, ..] as first -> first
  }
}
"#,
    );
    // `[1, ..] as first` — the alias applies to the whole list pattern.
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => match &c.clauses[0].patterns[0] {
                Pattern::Alias { pattern, name } => {
                    assert_eq!(name, "first");
                    if let Pattern::List { .. } = &**pattern {
                        // ok
                    } else {
                        panic!("not a list")
                    }
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_guards() {
    let m = ok(
        r#"
pub fn main(x: Int, y: Int) {
  case x, y {
    a, b if a == b && a > 0 -> "equal and positive"
    _, _ -> "other"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                let guard = c.clauses[0].guard.as_ref().expect("guard");
                // Guard is a binary && of two comparisons.
                if let Guard::Binary { op, .. } = guard {
                    assert_eq!(*op, BinOp::And);
                } else {
                    panic!("not a binary guard")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tuples & custom types
// ---------------------------------------------------------------------------

#[test]
fn tuples() {
    let m = ok("pub fn main() {\n  #(1, \"two\", [3])\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Tuple(t) => assert_eq!(t.len(), 3),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn custom_type() {
    let m = ok(
        r#"
pub type Season {
  Spring
  Summer
  Autumn
  Winter
}
"#,
    );
    match &m.definitions[0] {
        Definition::Type(t) => {
            assert_eq!(t.name, "Season");
            assert!(t.public);
            if let TypeDefKind::Record(variants) = &t.kind {
                assert_eq!(variants.len(), 4);
                assert_eq!(variants[0].name, "Spring");
                assert!(variants[0].fields.is_empty());
            } else {
                panic!("not a record")
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn custom_type_with_fields() {
    let m = ok(
        r#"
pub type Person {
  Person(name: String, age: Int)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Type(t) => {
            if let TypeDefKind::Record(variants) = &t.kind {
                assert_eq!(variants[0].fields.len(), 2);
                // `name: String` has no separate external label.
                assert_eq!(variants[0].fields[0].label, None);
                assert_eq!(variants[0].fields[0].name.as_deref(), Some("name"));
            } else {
                panic!("not a record")
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn generic_type() {
    let m = ok(
        r#"
pub type Pair(a, b) {
  Pair(first: a, second: b)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Type(t) => {
            assert_eq!(t.params, vec!["a".to_string(), "b".to_string()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn type_alias() {
    let m = ok("type Number = Int\n");
    match &m.definitions[0] {
        Definition::Type(t) => {
            if let TypeDefKind::Alias(ty) = &t.kind {
                if let TypeAnnotation::Named { name, .. } = ty {
                    assert_eq!(name, "Int");
                } else {
                    panic!("not named")
                }
            } else {
                panic!("not an alias")
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn external_type() {
    let m = ok("pub type DateTime\n");
    match &m.definitions[0] {
        Definition::Type(t) => {
            assert!(matches!(t.kind, TypeDefKind::External));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn opaque_type() {
    let m = ok(
        r#"
pub opaque type Secret {
  Secret(value: String)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Type(t) => {
            assert!(t.opaque);
            assert!(t.public);
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[test]
fn record_access() {
    let m = ok(
        r#"
pub type Person {
  Person(name: String)
}

pub fn main() {
  let p = Person(name: "Joe")
  p.name
}
"#,
    );
    match &m.definitions[1] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            // p.name is a field access.
            match &block[1].expr {
                Expr::FieldAccess { base, label } => {
                    assert_eq!(label, "name");
                    assert_eq!(&base.expr, &Expr::Var("p".into()));
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn record_pattern_matching() {
    let m = ok(
        r#"
pub type Person {
  Person(name: String, age: Int)
}

pub fn main(p: Person) {
  case p {
    Person(name: "Joe", age: 30) -> "Joe is 30"
    Person(name: name, ..) -> name
  }
}
"#,
    );
    match &m.definitions[1] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                // First clause pattern.
                match &c.clauses[0].patterns[0] {
                    Pattern::Constructor(ref_, args) => {
                        assert_eq!(ref_.name, "Person");
                        assert_eq!(args.len(), 2);
                    }
                    other => panic!("{other:?}"),
                }
                // Second clause has a .. discard.
                match &c.clauses[1].patterns[0] {
                    Pattern::Constructor(_, args) => {
                        assert_eq!(args.len(), 2);
                        if let PatternArg::DiscardRest = &args[1] {
                            // ok
                        } else {
                            panic!("expected discard rest")
                        }
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn record_update() {
    let m = ok(
        r#"
pub type Teacher {
  Teacher(name: String, subject: String, floor: Int, room: Int)
}

pub fn main() {
  let t1 = Teacher(name: "Mr Dodd", subject: "ICT", floor: 2, room: 2)
  let t2 = Teacher(..t1, subject: "PE", room: 6)
  t2
}
"#,
    );
    match &m.definitions[1] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[1].expr {
                Expr::Let(l) => match &l.value.expr {
                    Expr::RecordUpdate(ru) => {
                        assert_eq!(ru.constructor.name, "Teacher");
                        assert_eq!(&ru.base.expr, &Expr::Var("t1".into()));
                        assert_eq!(ru.args.len(), 2);
                    }
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Bit arrays
// ---------------------------------------------------------------------------

#[test]
fn bit_array_literal() {
    let m = ok("pub fn main() {\n  <<3>>\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::BitArray(ba) => {
                assert_eq!(ba.segments.len(), 1);
                assert_eq!(
                    &ba.segments[0].value.expr,
                    &Expr::Int(IntValue { raw: "3".into() })
                );
                assert!(ba.segments[0].options.is_empty());
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}#[test]
fn bit_array_with_options() {
    let m = ok("pub fn main() {\n  <<6147:size(16), \"Hello\":utf8, x:unsigned-little-size(2)>>\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::BitArray(ba) => {
                assert_eq!(ba.segments.len(), 3);
                // First segment: size(16).
                match &ba.segments[0].options[0] {
                    BitOption::Size(e) => {
                        assert_eq!(&e.expr, &Expr::Int(IntValue { raw: "16".into() }));
                    }
                    other => panic!("{other:?}"),
                }
                // Second segment: utf8.
                assert_eq!(ba.segments[1].options[0], BitOption::Utf8);
                // Third segment: unsigned-little-size(2).
                assert_eq!(ba.segments[2].options[0], BitOption::Unsigned);
                assert_eq!(ba.segments[2].options[1], BitOption::Little);
                if let BitOption::Size(_) = ba.segments[2].options[2] {
                    // ok
                } else {
                    panic!("expected size")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn bit_array_bits_concat() {
    let m = ok(
        r#"
pub fn main() {
  let first = <<4>>
  let second = <<2>>
  <<first:bits, second:bits>>
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[2].expr {
                Expr::BitArray(ba) => {
                    assert_eq!(ba.segments.len(), 2);
                    assert_eq!(ba.segments[0].options[0], BitOption::Bits);
                    assert_eq!(ba.segments[1].options[0], BitOption::Bits);
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Advanced features
// ---------------------------------------------------------------------------

#[test]
fn use_expression() {
    let m = ok(
        r#"
pub fn main() -> Nil {
  use username <- get_username()
  use password <- get_password()
  log_in(username, password)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Use(u) => {
                // Nested use: outer is the first `use`.
                assert_eq!(u.patterns.len(), 1);
                if let Pattern::Var(v) = &u.patterns[0] {
                    assert_eq!(v, "username");
                } else {
                    panic!("not a var")
                }
                // The body contains the second use.
                if let Expr::Use(inner) = &u.body.expr {
                    assert_eq!(inner.patterns.len(), 1);
                } else {
                    panic!("expected nested use")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn todo_and_panic() {
    let m = ok(
        r#"
pub fn main(x: Int) {
  case x {
    0 -> todo as "zero not allowed"
    _ -> panic as "impossible!"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => {
                match &c.clauses[0].expression.expr {
                    Expr::Todo { message } => {
                        assert_eq!(message.as_deref(), Some("zero not allowed"));
                    }
                    other => panic!("{other:?}"),
                }
                match &c.clauses[1].expression.expr {
                    Expr::Panic { message } => {
                        assert_eq!(message.as_deref(), Some("impossible!"));
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn let_assert() {
    let m = ok(
        r#"
pub fn main(items: List(Int)) {
  let assert [first, ..] = items as "List should not be empty"
  first
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let stmts = f.body.as_block().expect("block body");
            assert_eq!(stmts.len(), 2);
            match &stmts[0].expr {
                Expr::LetAssert(la) => {
                    assert_eq!(la.message.as_deref(), Some("List should not be empty"));
                    if let Pattern::List { elements, rest } = &la.pattern {
                        assert_eq!(elements.len(), 1);
                        assert!(rest.is_some());
                    } else {
                        panic!("not a list pattern")
                    }
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bool_assert() {
    let m = ok(
        r#"
pub fn main() {
  assert 1 + 2 == 3
  assert 6 == 6 as "Addition should be commutative"
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[0].expr {
                Expr::Assert(a) => assert_eq!(a.message, None),
                other => panic!("{other:?}"),
            }
            match &block[1].expr {
                Expr::Assert(a) => {
                    assert_eq!(a.message.as_deref(), Some("Addition should be commutative"));
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn external_fn() {
    let m = ok(
        r#"
@external(erlang, "my_module", "do_thing")
pub fn do_thing(x: Int) -> Int

@external(javascript, "js-module", "doThing")
pub fn other_thing(y: String) -> Nil
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            assert_eq!(f.attributes.len(), 1);
            if let Attribute::External(ext) = &f.attributes[0] {
                assert_eq!(ext.lang, "erlang");
                assert_eq!(ext.module, "my_module");
                assert_eq!(ext.function, "do_thing");
            } else {
                panic!("not external")
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn deprecated_attribute() {
    let m = ok(
        r#"
@deprecated("Use the new function instead")
pub fn old_fn(x: Int) -> Int {
  x
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => {
            if let Attribute::Deprecated(msg) = &f.attributes[0] {
                assert_eq!(msg, "Use the new function instead");
            } else {
                panic!("not deprecated")
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Documentation comments
// ---------------------------------------------------------------------------

#[test]
fn doc_comments() {
    let m = ok(
        r#"
//// This is a module doc.

/// This is a function doc.
pub fn documented(x: Int) -> Int {
  x
}
"#,
    );
    // Module docs.
    assert!(m.docs.is_some());
    match &m.definitions[0] {
        Definition::Fn(f) => {
            assert_eq!(f.docs.as_deref(), Some("This is a function doc."));
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Echo
// ---------------------------------------------------------------------------

#[test]
fn echo_expression() {
    let m = ok("pub fn main(x: Int) {\n  echo x\n  x\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().unwrap();
            match &block[0].expr {
                Expr::Echo(e) => assert_eq!(&e.expr, &Expr::Var("x".into())),
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Operator precedence
// ---------------------------------------------------------------------------

#[test]
fn operator_precedence() {
    // 1 + 2 * 3 should be 1 + (2 * 3).
    let m = ok("pub fn main() {\n  1 + 2 * 3\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Binary { op, lhs: _, rhs } => {
                assert_eq!(*op, BinOp::Plus);
                // RHS should be the multiplication.
                if let Expr::Binary { op: op2, .. } = &rhs.expr {
                    assert_eq!(*op2, BinOp::Times);
                } else {
                    panic!("expected multiplication on the right")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn operator_left_associativity() {
    // 1 - 2 - 3 should be (1 - 2) - 3.
    let m = ok("pub fn main() {\n  1 - 2 - 3\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Binary { op, lhs, .. } => {
                assert_eq!(*op, BinOp::Minus);
                // LHS should be the first subtraction.
                if let Expr::Binary { op: op2, .. } = &lhs.expr {
                    assert_eq!(*op2, BinOp::Minus);
                } else {
                    panic!("expected subtraction on the left")
                }
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Unary operators
// ---------------------------------------------------------------------------

#[test]
fn unary_minus() {
    let m = ok("pub fn main(x: Int) {\n  -x\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Neg(inner) => assert_eq!(&inner.expr, &Expr::Var("x".into())),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn unary_not() {
    let m = ok("pub fn main(x: Bool) {\n  !x\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Not(inner) => assert_eq!(&inner.expr, &Expr::Var("x".into())),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Nil
// ---------------------------------------------------------------------------

#[test]
fn nil_value() {
    let m = ok("pub fn main() -> Nil {\n  Nil\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Nil => {}
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn unclosed_block() {
    let e = err("pub fn main() {\n  1\n");
    assert!(e.message.contains("}`"));
}

#[test]
fn empty_block_is_error() {
    let e = err("pub fn main() {\n}\n");
    assert!(e.message.contains("cannot be empty"));
}

#[test]
fn missing_definition_keyword() {
    let e = err("not a valid gleam file\n");
    assert!(!e.message.is_empty());
}

#[test]
fn bad_type_annotation() {
    let e = err("pub fn main(x: ) -> Int {\n  1\n}\n");
    assert!(!e.message.is_empty());
}

#[test]
fn case_pattern_count_mismatch() {
    let e = err(
        r#"
pub fn main(a: Int, b: Int) {
  case a, b {
    0 -> "only one pattern"
  }
}
"#,
    );
    assert!(e.message.contains("pattern"));
}

#[test]
fn bad_bit_array_option() {
    let e = err("pub fn main() {\n  <<3:badoption>>\n}\n");
    assert!(e.message.contains("unknown bit array option"));
}

#[test]
fn unterminated_string() {
    let e = err(r#"pub fn main() { "unterminated }"#);
    assert!(!e.message.is_empty());
}

// ---------------------------------------------------------------------------
// Full program from the tour
// ---------------------------------------------------------------------------

#[test]
fn full_tour_program() {
    // A larger, realistic Gleam module exercising many features at once.
    let m = ok(
        r#"
//// A module demonstrating many Gleam features.
import gleam/io
import gleam/list {map, filter}
import gleam/result

pub type Shape {
  Circle(radius: Float)
  Rectangle(width: Float, height: Float)
}

pub const pi = 3.14159

/// Compute the area of a shape.
pub fn area(shape: Shape) -> Float {
  case shape {
    Circle(radius) -> pi * radius * radius
    Rectangle(width, height) -> width * height
  }
}

pub fn main() {
  let shapes = [
    Circle(radius: 1.0),
    Rectangle(width: 2.0, height: 3.0),
  ]

  let areas =
    shapes
    |> list.map(area)
    |> list.filter(fn(a) { a > 0.0 })

  echo areas
  io.println("Done!")
}
"#,
    );
    assert!(m.docs.is_some());
    // 1 module import + 2 qualified imports + type + const + 2 fns = 7 defs
    assert_eq!(m.definitions.len(), 7);
}

// ---------------------------------------------------------------------------
// Extra edge cases
// ---------------------------------------------------------------------------

#[test]
fn guard_function_call() {
    let m = ok(
        r#"
pub fn f(xs: List(Int)) {
  case xs {
    x if list.length(x) > 3 -> "long"
    _ -> "short"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => match &c.clauses[0].guard {
                Some(Guard::Binary { op: BinOp::Gt, rhs, .. }) => {
                    assert!(matches!(&**rhs, Guard::Int(_)));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn guard_string_concat() {
    let m = ok(
        r#"
pub fn f(s: String) {
  case s {
    x if x == "a" <> "b" -> "yes"
    _ -> "no"
  }
}
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Case(c) => match &c.clauses[0].guard {
                Some(Guard::Binary { op: BinOp::Eq, rhs, .. }) => match &**rhs {
                    Guard::Binary { op: BinOp::Concat, .. } => {}
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn unnamed_record_fields() {
    let m = ok(
        r#"
pub opaque type Secret {
  Secret(String)
  Pair(Int, Int)
}
"#,
    );
    match &m.definitions[0] {
        Definition::Type(t) => {
            let TypeDefKind::Record(variants) = &t.kind else {
                panic!("expected a record type");
            };
            assert_eq!(variants.len(), 2);
            assert_eq!(variants[0].fields.len(), 1);
            assert_eq!(variants[0].fields[0].name, None);
            assert_eq!(variants[1].fields.len(), 2);
            assert_eq!(variants[1].fields[0].name, None);
            assert_eq!(variants[1].fields[1].name, None);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn trailing_commas_everywhere() {
    let m = ok(
        r#"
pub fn f() {
  let a = [1, 2, 3,]
  let b = #(1, 2,)
  let c = <<1, 2,>>
  let d = g(a: 1, b: 2,)
  a
}

fn g(a: Int, b: Int) -> Int { a }
"#,
    );
    // Parses without error; bodies contain the expected shapes.
    match &m.definitions[0] {
        Definition::Fn(f) => {
            let block = f.body.as_block().expect("block body");
            assert!(matches!(&block[0].expr, Expr::Let(_)));
            assert!(matches!(&block[1].expr, Expr::Let(_)));
            assert!(matches!(&block[2].expr, Expr::Let(_)));
            assert!(matches!(&block[3].expr, Expr::Let(_)));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn empty_tuple() {
    let m = ok("pub fn f() {\n  #()\n}\n");
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Tuple(elems) => assert!(elems.is_empty()),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn labelled_argument_shorthand() {
    let m = ok(
        r#"
pub fn f(a: Int) {
  g(a:)
}

fn g(a: Int) -> Int { a }
"#,
    );
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Call(call) => match &call.args[0] {
                CallArg::Labelled { label, value } => {
                    assert_eq!(label, "a");
                    assert!(matches!(&value.expr, Expr::Var(_)));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn module_doc_mid_file() {
    let m = ok(
        r#"
// a comment
//// module docs
pub fn f() -> Int {
  1
}
"#,
    );
    assert!(m.docs.is_some());
    assert_eq!(m.definitions.len(), 1);
}

#[test]
fn labelled_placeholder_capture() {
    let m = ok(
        r#"
pub fn f(xs: List(Int)) {
  xs |> list.map(f2(a: _, b: 2))
}

fn f2(a: Int, b: Int) -> Int { a + b }
"#,
    );
    // Parses as a pipeline whose second stage is a capture.
    match &m.definitions[0] {
        Definition::Fn(f) => match single_body(f) {
            Expr::Pipeline { exprs, .. } => assert_eq!(exprs.len(), 2),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}
