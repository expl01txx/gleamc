# gleam-to-c

A [Gleam](https://gleam-lang.io/) → C compiler written in Rust. It parses a `.gleam` source file and emits a small C project (`main.c` plus an embedded C runtime) that compiles with any C11 compiler on a POSIX system.

No external crates — the whole project is `std`-only Rust plus two hand-written C files.

## Features

- Tokenizer, AST, and recursive-descent parser for Gleam
- Code generator that translates the AST to portable C11
- All values are a single tagged union (`GValue`); heap types (strings, lists, tuples, records, closures) are refcounted
- Native C implementations of the standard library modules: `gleam/io`, `gleam/fs`, `gleam/string`, `gleam/int`
- End-to-end tests that compile the generated C with `cc` and run it

## Quick start

```sh
# 1. Build the compiler
cargo build

# 2. Compile a Gleam program to C
./target/debug/gleam-parser tests/hello.gleam -o /tmp/out

# 3. Build the generated C project with any C11 compiler
cc -std=c11 -Wall -Wextra /tmp/out/main.c /tmp/out/gleam_runtime.c -o /tmp/prog

# 4. Run it
/tmp/prog
```

The output directory contains `main.c`, `gleam_runtime.c`, and `gleam_runtime.h`. Only `main.c` and `gleam_runtime.c` need to be compiled.

### Example

```gleam
import gleam/io

pub fn main() {
  let x = 40 + 2
  io.println("the answer is " <> int.to_string(x))
}
```

## How it works

```
lexer.rs → tokens → parser.rs → Module AST → codegen.rs → C source
```

- User functions compile to `static GValue gleam_fn_<name>(GClosure *, GValue *, size_t)`.
- Records are `GRecord` with a string tag; constructors like `Ok(x)` compile to an inline `g_record("Ok", 1, fields)`.
- Case clauses compile to sequential `if (!g_N_matched)` blocks; non-exhaustive cases end in a panic.
- Standard library functions are **native**: a call like `string.split(x, on: y)` resolves to a direct C call such as `gleam_string_split(args, argc)`. There is a single table (`std_function` in `src/codegen.rs`) mapping `(module, function)` → `(C name, arity, labels)`.

## Supported standard library

| Module | Coverage |
|---|---|
| `gleam/io` | `print`, `println`, `debug`, `read_line` |
| `gleam/fs` | `read`, `write`, `append`, `remove`, `is_file`, `is_directory`, `create_directory`, `list_directory` |
| `gleam/string` | 31 functions (grapheme/UTF functions excluded) |
| `gleam/int` | all 21 functions |

These modules work without an explicit import.

## Repository layout

```
src/
  lib.rs          crate root: re-exports parse, generate_c, write_c_project
  lexer.rs        tokenizer (Token, TokenKind)
  ast.rs          AST types (Module, Definition, Expr, Pattern, ...)
  parser.rs       recursive-descent parser producing a Module
  codegen.rs      AST → C translation; embeds the runtime via include_str!
  main.rs         CLI: gleam-parser <file.gleam> -o <dir>
  runtime/
    gleam_runtime.h   GValue model + runtime/stdlib declarations
    gleam_runtime.c   runtime implementation + stdlib function bodies
tests/
  tour.rs         parser-level tests (no C involved)
  runtime.rs      end-to-end tests: parse → codegen → cc → run → assert stdout
  hello.gleam     small demo program used by an e2e test
  string_test.gleam / int_test.gleam   stdlib test programs
gleam_std_*.html  vendored stdlib docs (string, int, list, ...)
```

## Testing

```sh
cargo test
```

- **Parser tests** (`tests/tour.rs`) are pure Rust and assert on AST shape.
- **End-to-end tests** (`tests/runtime.rs`) parse a Gleam program, generate C, compile it with `cc -std=c11 -Wall -Wextra`, run it, and assert on stdout. They skip cleanly when no C compiler is available.

## License

MIT — see [Cargo.toml](Cargo.toml).
