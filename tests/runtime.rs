//! End-to-end runtime tests: parse Gleam source, generate C, compile with
//! `cc`, run the program, and check its output.
//!
//! These tests are skipped when a C compiler is not available.

use std::io::Write;
use std::process::{Command, Stdio};

fn cc_available() -> bool {
    Command::new("cc")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Parse, generate C, compile, and run a Gleam program with the given
/// command-line arguments and stdin. Returns stdout.
fn run_gleam_configured(src: &str, name: &str, args: &[&str], stdin: &str) -> String {
    let module = gleamc::parse(src).expect("should parse");
    let dir = std::env::temp_dir().join(format!("gleam-parser-test-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    gleamc::write_c_project(src, &module, &dir).expect("should generate C");

    let main_c = dir.join("main.c");
    let runtime_c = dir.join("gleam_runtime.c");
    let bin = dir.join("prog");

    let compile = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra"])
        .arg(&main_c)
        .arg(&runtime_c)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("failed to run cc");
    assert!(
        compile.status.success(),
        "cc failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut child = Command::new(&bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run program");
    if !stdin.is_empty() {
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
    }
    drop(child.stdin.take());
    let run = child.wait_with_output().expect("failed to wait for program");
    assert!(
        run.status.success(),
        "program failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Parse, generate C, compile, and run a Gleam program with no arguments
/// and empty stdin. Returns stdout.
fn run_gleam(src: &str, name: &str) -> String {
    run_gleam_configured(src, name, &[], "")
}

#[test]
fn string_module_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("string_test.gleam"), "string");
    assert!(
        out.ends_with("all string tests passed\n"),
        "unexpected output:\n{out}"
    );
}

#[test]
fn int_module_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("int_test.gleam"), "int");
    assert!(
        out.ends_with("all int tests passed\n"),
        "unexpected output:\n{out}"
    );
}

#[test]
fn list_module_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("list_test.gleam"), "list");
    assert!(
        out.ends_with("all list tests passed\n"),
        "unexpected output:\n{out}"
    );
}

#[test]
fn io_module_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam_configured(include_str!("io_test.gleam"), "io", &[], "hello\n");
    assert!(
        out == "got: hello\neof\nall io tests passed\n",
        "unexpected output:\n{out}"
    );
}

#[test]
fn cli_module_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam_configured(
        include_str!("cli_test.gleam"),
        "cli-args",
        &["hello", "world"],
        "",
    );
    assert!(
        out == "count=2\nfirst=hello\nall cli tests passed\n",
        "unexpected output:\n{out}"
    );
    let out = run_gleam_configured(include_str!("cli_test.gleam"), "cli-empty", &[], "");
    assert!(
        out == "count=0\nempty ok\nall cli tests passed\n",
        "unexpected output:\n{out}"
    );
}

#[test]
fn standalone_c_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let c = gleamc::translate_standalone(include_str!("hello.gleam"))
        .expect("should translate to a single C file");
    assert!(
        !c.contains("#include \"gleam_runtime.h\""),
        "standalone C must not reference the runtime header"
    );

    let dir = std::env::temp_dir().join("gleam-parser-test-standalone");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let c_file = dir.join("hello.c");
    std::fs::write(&c_file, c).expect("write C file");
    let bin = dir.join("prog");

    let compile = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra"])
        .arg(&c_file)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("failed to run cc");
    assert!(
        compile.status.success(),
        "cc failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&bin).output().expect("failed to run program");
    assert!(
        run.status.success(),
        "program failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("push 5"), "unexpected output:\n{out}");
}

#[test]
fn const_constructor_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("const_test.gleam"), "const");
    assert!(
        out.ends_with("all const tests passed\n"),
        "unexpected output:\n{out}"
    );
}

// ---------------------------------------------------------------------------
// Examples (examples/*.gleam): demo programs that must compile and run.
// ---------------------------------------------------------------------------

#[test]
fn basic_example_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("../examples/basic.gleam"), "example-basic");
    assert_eq!(
        out,
        "Hello from Gleam!\n\
         answer: 42\n\
         point sum: 7\n\
         list length: 4\n\
         label: positive\n\
         total area: 9.14159\n\
         the answer is 42\n\
         parsed: 123\n\
         done!\n"
    );
}

#[test]
fn int_example_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("../examples/int.gleam"), "example-int");
    // The final line prints a random integer, so assert on the
    // deterministic prefix.
    assert!(
        out.starts_with(
            "123\n\
             parsed: 42\n\
             to_float: 7\n\
             is_even(4): True\n\
             is_odd(7): True\n\
             negate(5): -5\n\
             abs(-9): 9\n\
             max: 9\n\
             min: 3\n\
             clamp: 10\n\
             Lt\nGt\nEq\n\
             sum: 10\n\
             product: 24\n\
             binary(8): 1000\n\
             octal(64): 100\n\
             hex(255): FF\n\
             base36(35): Z\n\
             base16(45): 2D\n\
             digits: [4, 5, 6]\n\
             undigits: 456\n\
             roll: "
        ),
        "unexpected output:\n{out}"
    );
}

#[test]
fn string_example_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("../examples/string.gleam"), "example-string");
    assert_eq!(
        out,
        "gleam\n\
         abc\n\
         lower: gleam\n\
         upper: GLEAM\n\
         capitalise: Hello world\n\
         length: 5\n\
         byte_size: 5\n\
         [padded]\n[left]\n[right]\n\
         empty: True\n\
         starts_with: True\n\
         ends_with: True\n\
         contains: True\n\
         split: [\"one\", \"two\", \"three\"]\n\
         join: one, two, three\n\
         replace: LoLLipops\n\
         repeat: hahaha\n\
         slice: world\n\
         crop: world\n\
         remove_prefix: world\n\
         remove_suffix: hello\n\
         first/last: Ok(\"h\") / Ok(\"o\")\n\
         Lt\n\
         to_option(\"\"): None()\n"
    );
}

#[test]
fn list_example_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("../examples/list.gleam"), "example-list");
    assert_eq!(
        out,
        "[1, 2, 3, 4, 5]\n\
         length: 5\n\
         is_empty: False\n\
         reversed: [5, 4, 3, 2, 1]\n\
         spread: [0, 1, 2, 3, 4, 5]\n\
         append: [1, 2, 3, 4]\n\
         drop: [3, 4, 5]\n\
         take: [1, 2, 3]\n\
         doubled: [2, 4, 6, 8, 10]\n\
         evens: [2, 4]\n\
         sum: 15\n\
         zip: [#(1, 10), #(2, 20), #(3, 30)]\n\
         unzip: [1, 2, 3] / [10, 20, 30]\n\
         contains: True\n\
         first above 2: 3\n\
         sorted: [1, 2, 3]\n\
         unique: [1, 2, 3]\n\
         chunked: [[1, 2], [3, 4], [5]]\n\
         interspersed: [1, 0, 2, 0, 3]\n\
         first/last: Ok(1) / Ok(5)\n\
         rest: Ok([2, 3, 4, 5])\n"
    );
}

#[test]
fn hello_end_to_end() {
    if !cc_available() {
        eprintln!("skipping: no C compiler available");
        return;
    }
    let out = run_gleam(include_str!("hello.gleam"), "hello");
    // main() splits a program into lines and prints the list.
    assert!(
        out == "[\"push 5\", \"push 3\", \"add\", \"print\"]\n",
        "unexpected output:\n{out}"
    );
}
