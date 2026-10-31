//! End-to-end runtime tests: parse Gleam source, generate C, compile with
//! `cc`, run the program, and check its output.
//!
//! These tests are skipped when a C compiler is not available.

use std::process::Command;

fn cc_available() -> bool {
    Command::new("cc")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Parse, generate C, compile, and run a Gleam program. Returns stdout.
fn run_gleam(src: &str, name: &str) -> String {
    let module = gleam_parser::parse(src).expect("should parse");
    let dir = std::env::temp_dir().join(format!("gleam-parser-test-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    gleam_parser::write_c_project(&module, &dir).expect("should generate C");

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

    let run = Command::new(&bin).output().expect("failed to run program");
    assert!(
        run.status.success(),
        "program failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
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
