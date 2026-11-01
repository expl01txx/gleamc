//! Type checker tests: programs that must type check, and programs that must
//! be rejected with a sensible error message.

use gleam_to_c::{parse, typecheck_source};

fn ok(source: &str) {
    let module = parse(source).expect("should parse");
    match typecheck_source(source, &module) {
        Ok(()) => {}
        Err(e) => panic!("should type check: {e}"),
    }
}

/// Assert the source is rejected and the error message contains `needle`.
fn err(source: &str, needle: &str) {
    let module = match parse(source) {
        Ok(m) => m,
        Err(e) => panic!("should parse, got: {e}"),
    };
    let e = typecheck_source(source, &module).expect_err("should fail");
    assert!(
        e.message.contains(needle),
        "expected error containing `{needle}`, got: `{}`",
        e.message
    );
}

// ---------------------------------------------------------------------------
// Positive tests
// ---------------------------------------------------------------------------

#[test]
fn arithmetic() {
    ok(r#"
        pub fn main() -> Int {
            1 + 2 * 3 - 4 / 2 % 3
        }
    "#);
}

#[test]
fn float_arithmetic() {
    ok(r#"
        pub fn main() -> Float {
            let a = 1.0
            let b = 2.5
            a +. b
        }
    "#);
}

#[test]
fn strings_and_bools() {
    ok(r#"
        pub fn main() -> Bool {
            let s = "hello" <> " world"
            let b = True
            s == "hello world" && b
        }
    "#);
}

#[test]
fn lists_and_tuples() {
    ok(r#"
        pub fn main() -> Bool {
            let xs = [1, 2, 3]
            let t = #(xs, "a", True)
            t.0 == [1, 2, 3]
        }
    "#);
}

#[test]
fn custom_types_and_fields() {
    ok(r#"
        type Point {
          Point(x: Int, y: Int)
        }

        pub fn main() -> Int {
            let p = Point(1, y: 2)
            p.x + p.y
        }
    "#);
}

#[test]
fn case_with_guards() {
    ok(r#"
        pub fn classify(n: Int) -> String {
          case n {
            0 -> "zero"
            n if n > 0 -> "positive"
            _ -> "negative"
          }
        }
    "#);
}

#[test]
fn let_assert() {
    ok(r#"
        fn result(x: Int) -> Result(Int, Int) {
          Ok(x)
        }

        pub fn main() -> Int {
          let assert Ok(y) = result(1)
          y
        }
    "#);
}

#[test]
fn polymorphic_function() {
    // Top-level functions are generalized: identity works for any type.
    ok(r#"
        pub fn id(x) {
          x
        }

        pub fn main() -> String {
          id(1)
          id("a")
        }
    "#);
}

#[test]
fn closures_are_monomorphic() {
    // `let` bindings are not generalized: a closure inferred as Int -> Int
    // cannot be applied to a String.
    err(
        r#"
            pub fn main() -> Int {
              let add = fn(a, b) { a + b }
              add("a", "b")
            }
        "#,
        "type mismatch",
    );
}

#[test]
fn labeled_arguments() {
    ok(r#"
        pub fn greet(name: String, punct: String) -> String {
          name <> punct
        }

        pub fn main() -> String {
          greet(name: "Evan", punct: "!")
        }
    "#);
}

#[test]
fn pipelines() {
    ok(r#"
        pub fn double(x: Int) -> Int { x * 2 }
        pub fn show(x: Int) -> String { "n" }

        pub fn main() -> String {
          let s =
            1
            |> double
            |> show
          s
        }
    "#);
}

#[test]
fn bit_arrays() {
    ok(r#"
        pub fn main() -> Int {
          let b = <<1, 2:size(8)>>
          1
        }
    "#);
}

#[test]
fn use_callbacks() {
    ok(r#"
        type Person {
          Person(name: String)
        }

        fn with_person(f: fn(Person) -> String) -> String {
          f(Person("Evan"))
        }

        pub fn main() -> String {
          use p <- with_person()
          p.name
        }
    "#);
}

#[test]
fn stdlib_functions() {
    ok(r#"
        import gleam/io
        import gleam/string
        import gleam/int

        pub fn main() -> Nil {
          io.println(string.uppercase("hi"))
          io.debug(int.to_string(42))
        }
    "#);
}

#[test]
fn io_readln_and_cli_args() {
    ok(r#"
        pub fn main() -> Nil {
          case io.readln() {
            Ok(line) -> io.println(line)
            Error(nil) -> io.println("eof")
          }
          print_args(cli.args())
        }

        fn print_args(args: List(String)) -> Nil {
          io.println(string.concat(args))
        }
    "#);
}

#[test]
fn cli_args_arity() {
    err(
        "pub fn main() -> Nil {\n  cli.args(1)\n}",
        "expects 0",
    );
}

#[test]
fn record_update() {
    ok(r#"
        type Point {
          Point(x: Int, y: Int)
        }

        pub fn main() -> Int {
          let p = Point(1, 2)
          let q = Point(..p, x: 3)
          q.x + q.y
        }
    "#);
}

#[test]
fn generic_types() {
    ok(r#"
        type Wrapper(a) {
          Wrap(inner: a)
        }

        pub fn main() -> Int {
          let w = Wrap(1)
          case w {
            Wrap(i) -> i
          }
        }
    "#);
}

// ---------------------------------------------------------------------------
// Negative tests
// ---------------------------------------------------------------------------

#[test]
fn arithmetic_type_mismatch() {
    err("pub fn main() -> Int {\n  1 + \"a\"\n}", "expects Int operands");
}

#[test]
fn unknown_variable() {
    err("pub fn main() -> Int {\n  nope\n}", "unknown variable `nope`");
}

#[test]
fn too_many_arguments() {
    err(
        "pub fn f(a: Int) -> Int { a }\npub fn main() -> Int {\n  f(1, 2)\n}",
        "too many arguments",
    );
}

#[test]
fn too_few_arguments() {
    err(
        "pub fn f(a: Int, b: Int) -> Int { a }\npub fn main() -> Int {\n  f(1)\n}",
        "expects 2 argument(s)",
    );
}

#[test]
fn unknown_field() {
    err(
        "type Point { Point(x: Int) }\npub fn main() -> Int {\n  let p = Point(1)\n  p.y\n}",
        "unknown record field `y`",
    );
}

#[test]
fn guard_type_mismatch() {
    err(
        "pub fn main() -> Int {\n  case 1 {\n    x if x > \"a\" -> 1\n    _ -> 2\n  }\n}",
        "expects Int operands",
    );
}

#[test]
fn let_assert_on_plain_value() {
    err(
        "pub fn main() -> Int {\n  let x = 1\n  let assert Ok(y) = x\n  y\n}",
        "expected Result",
    );
}

#[test]
fn unknown_constructor() {
    err("pub fn main() -> Int {\n  Nope(1)\n}", "unknown constructor `Nope`");
}

#[test]
fn bad_label() {
    err(
        "pub fn f(a: Int) -> Int { a }\npub fn main() -> Int {\n  f(b: 1)\n}",
        "no argument labeled `b`",
    );
}

#[test]
fn record_update_wrong_base() {
    err(
        "type Point { Point(x: Int, y: Int) }\npub fn main() -> Point {\n  Point(..1, x: 2)\n}",
        "expected Point",
    );
}

#[test]
fn pipeline_mismatch() {
    err(
        "pub fn f(x: Int) -> Int { x }\npub fn main() -> String {\n  1 |> f |> g\n}\npub fn g(x: String) -> String { x }",
        "type mismatch",
    );
}

#[test]
fn tuple_index_out_of_range() {
    err("pub fn main() -> Int {\n  #(1, 2).5\n}", "no index `5`");
}

#[test]
fn not_on_int() {
    err("pub fn main() -> Bool {\n  !1\n}", "type mismatch");
}

#[test]
fn unknown_stdlib_function() {
    err(
        "import gleam/io\npub fn main() -> Nil {\n  io.nope(1)\n}",
        "has no function `nope`",
    );
}

#[test]
fn stdlib_arity() {
    err(
        "import gleam/string\npub fn main() -> String {\n  string.append(\"a\")\n}",
        "expects 2 argument(s)",
    );
}

#[test]
fn case_branch_type_mismatch() {
    err(
        "pub fn main() -> Int {\n  case 1 {\n    _ -> \"a\"\n  }\n}",
        "type mismatch",
    );
}

#[test]
fn binary_operator_non_numeric() {
    err("pub fn main() -> Bool {\n  True + False\n}", "expects Int operands");
}

#[test]
fn errors_carry_spans() {
    let source = "pub fn main() -> Int {\n  1 + \"a\"\n}";
    let module = parse(source).expect("should parse");
    let e = typecheck_source(source, &module).expect_err("should fail");
    assert!(e.span.start < e.span.end, "span should be non-empty");
    // The source is available, so line/col are populated.
    assert!(e.line.is_some() && e.col.is_some(), "line/col should be set");
}
