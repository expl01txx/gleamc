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
    let module = gleam_to_c::parse(src).expect("should parse");
    let dir = std::env::temp_dir().join(format!("gleam-parser-test-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    gleam_to_c::write_c_project(src, &module, &dir).expect("should generate C");

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
