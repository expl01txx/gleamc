# gleamc

A [Gleam](https://gleam.run/) → C compiler written in Rust. It parses a `.gleam` source file and emits a small C project (`main.c` plus an embedded C runtime) that compiles with any C11 compiler on a POSIX system.

No external crates — the whole project is `std`-only Rust plus two hand-written C files.

## Features

- Tokenizer, AST, and recursive-descent parser for Gleam
- Hindley–Milner style type checker with unification (runs before codegen)
- Code generator that translates the AST to portable C11
- All values are a single tagged union (`GValue`); heap types (strings, lists, tuples, records, closures) are refcounted
- Native C implementations of the standard library modules: `gleam/io`, `gleam/fs`, `gleam/string`, `gleam/int`, `gleam/list`
- End-to-end tests that compile the generated C with `cc` and run it

## Quick start

```sh
# 1. Build the compiler
cargo build

# 2. Compile a Gleam program to C
./target/debug/gleamc tests/hello.gleam -o /tmp/out

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

### Examples

The `examples/` directory contains runnable demo programs, each covered by an
end-to-end test in `tests/runtime.rs`:

| File | Shows |
|---|---|
| `examples/basic.gleam` | core language features: functions, labels, `let`, `case` with guards, tuples, lists with spread, records, closures, pipelines, `Result` |
| `examples/int.gleam` | the `gleam/int` module (conversions, parity, clamping, bases, digits) |
| `examples/string.gleam` | the `gleam/string` module (case changes, trimming, splitting, slicing, predicates) |
| `examples/list.gleam` | the `gleam/list` module (map/filter/fold, zip, sort, chunking) |

Run one:

```sh
./target/debug/gleamc examples/basic.gleam -o /tmp/out
cc -std=c11 -Wall -Wextra /tmp/out/main.c /tmp/out/gleam_runtime.c -o /tmp/prog
/tmp/prog
```

## How it works

```
lexer.rs → tokens → parser.rs → Module AST → typechecker.rs → codegen.rs → C source
```

The type checker infers types for the whole module before any C is generated:
local variables, `let` bindings, patterns, guards, case clauses, pipelines,
closures, and record updates are all checked by unification. Top-level functions
are generalized (Hindley–Milner let-generalization); values bound with `let`
and anonymous functions keep their inferred monomorphic type. Type errors
carry source spans and, when the source text is available, line/column numbers.

- User functions compile to `static GValue gleam_fn_<name>(GClosure *, GValue *, size_t)`.
- Records are `GRecord` with a string tag; constructors like `Ok(x)` compile to an inline `g_record("Ok", 1, fields)`.
- Case clauses compile to sequential `if (!g_N_matched)` blocks; non-exhaustive cases end in a panic.
- Standard library functions are **native**: a call like `string.split(x, on: y)` resolves to a direct C call such as `gleam_string_split(args, argc)`. There is a single table (`std_function` in `src/codegen.rs`) mapping `(module, function)` → `(C name, arity, labels)`.

## Supported standard library

| Module | Coverage |
|---|---|
| `gleam/io` | `print`, `println`, `debug`, `read_line`, `readln` |
| `gleam/cli` | `args` (command-line arguments as a list of strings) |
| `gleam/fs` | `read`, `write`, `append`, `remove`, `is_file`, `is_directory`, `create_directory`, `list_directory` |
| `gleam/string` | 31 functions (grapheme/UTF functions excluded) |
| `gleam/int` | all 21 functions |
| `gleam/list` | 64 functions (`group` excluded — it needs `gleam/dict`) |

These modules work without an explicit import.

## Repository layout

```
src/
  lib.rs          crate root: re-exports parse, typecheck_source, generate_c, write_c_project
  lexer.rs        tokenizer (Token, TokenKind)
  ast.rs          AST types (Module, Definition, Expr, Pattern, ...)
  parser.rs       recursive-descent parser producing a Module
  typechecker.rs  Hindley–Milner type inference with unification
  codegen.rs      AST → C translation; embeds the runtime via include_str!
  main.rs         CLI: gleamc <file.gleam> -o <dir>
  runtime/
    gleam_runtime.h   GValue model + runtime/stdlib declarations
    gleam_runtime.c   runtime implementation + stdlib function bodies
examples/
  basic.gleam     demo of core language features
  int.gleam       demo of the gleam/int module
  string.gleam    demo of the gleam/string module
  list.gleam      demo of the gleam/list module
tests/
  tour.rs         parser-level tests (no C involved)
  typecheck.rs    type checker tests: programs that must check, and ones that must fail
  runtime.rs      end-to-end tests: parse → typecheck → codegen → cc → run → assert stdout
  hello.gleam     small demo program used by an e2e test
  string_test.gleam / int_test.gleam / list_test.gleam   stdlib test programs
gleam_std_*.html  vendored stdlib docs (string, int, list, ...)
```

## Testing

```sh
cargo test
```

- **Parser tests** (`tests/tour.rs`) are pure Rust and assert on AST shape.
- **Type checker tests** (`tests/typecheck.rs`) feed programs to the type checker and assert that valid ones pass while invalid ones produce a specific error message.
- **End-to-end tests** (`tests/runtime.rs`) parse a Gleam program, type check it, generate C, compile it with `cc -std=c11 -Wall -Wextra`, run it, and assert on stdout. They skip cleanly when no C compiler is available.

## License

MIT — see [Cargo.toml](Cargo.toml).
