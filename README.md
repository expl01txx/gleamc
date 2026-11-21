# gleamc

A [Gleam](https://gleam.run/) → C compiler written in Rust. It loads a `.gleam` entry file and its local imports, translates them to C11 against an embedded C runtime, and compiles the result into a native binary with `cc`. With `-c` it instead emits a single self-contained C file you can compile yourself.

No external crates — the whole project is `std`-only Rust plus two hand-written C files.

## Features

- Tokenizer, AST, and recursive-descent parser for Gleam
- Hindley–Milner style type checker with unification and dependency-ordered inference
- C11 code generation for closures, captures, `use`, pipelines, records, guarded case alternatives, and bit-array construction and matching
- All values are a single tagged union (`GValue`); heap types (strings, lists, tuples, records, closures, bit arrays) are refcounted
- Native C implementations of the standard library modules: `gleam/io`, `gleam/fs`, `gleam/string`, `gleam/int`, `gleam/list`
- End-to-end tests that compile the generated C with `cc` and run it

## Quick start

```sh
# 1. Build the compiler
cargo build

# 2. Compile a Gleam program straight to a native binary
./target/debug/gleamc tests/hello.gleam
./program
```

By default `gleamc` generates the C files in a staging directory (`gleam-out`), compiles them with `cc -std=c11 -Wall -Wextra` into `./program`, and removes the staging directory again. If the compile fails, the C files are kept so you can fix and build them manually.

Pass `-release` to build an optimized binary (`-O3 -flto -ffunction-sections -fdata-sections -Wl,--gc-sections -DNDEBUG`):

```sh
./target/debug/gleamc tests/hello.gleam -release
```

To emit C instead of a binary, pass `-c`: this writes a single self-contained C file (runtime embedded) that compiles with one command:

```sh
./target/debug/gleamc tests/hello.gleam -c
cd gleam-out && cc -std=c11 -Wall -Wextra hello.c -o program && ./program
```

`-o <dir>` changes the staging/output directory in both modes (default: `gleam-out`). If the directory already exists during a native build, the compiler stages its files in a temporary subdirectory.

Local imports resolve under the entry file's nearest `src` ancestor, or its containing directory when there is no `src` ancestor. For example, `import app/model` loads `src/app/model.gleam` in a regular project. Both CLI modes follow the same import graph. The Rust APIs `translate_file` and `write_c_project_file` load imports from disk, while `translate` and `translate_standalone` accept the source text of a single module.

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
./target/debug/gleamc examples/basic.gleam
./program
```

## How it works

```
lexer.rs → tokens → parser.rs → Module AST → typechecker.rs → codegen.rs → C source
```

The type checker infers types for the whole module before any C is generated:
local variables, `let` bindings, patterns, guards, case clauses, pipelines,
closures, and record updates are checked by unification. Top-level functions are checked in dependency order and then generalized. Values bound with `let`
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

## Compatibility

The C target currently uses signed 64-bit integers, a subset of the standard library, and its own C foreign-function interface. Dependency resolution covers local source files and the built-in runtime modules. Hex package resolution remains outside the current build workflow. Pattern exhaustiveness and variants narrowed by earlier matches still differ from upstream. Generated programs can panic on a case that has no matching clause.

A Gleam function with foreign attributes can supply a Gleam body for the C target. `@external(c, "module", "symbol")` selects a function with the signature `GValue symbol(GValue *, size_t)`. The symbol must be supplied by the runtime or linked as part of the C build.

## Repository layout

```
src/
  lib.rs          crate root: re-exports parse, typecheck_source, generate_c, write_c_project
  lexer.rs        tokenizer (Token, TokenKind)
  ast.rs          AST types (Module, Definition, Expr, Pattern, ...)
  parser.rs       recursive-descent parser producing a Module
  project.rs      local module loading and name resolution
  typechecker.rs  Hindley–Milner type inference with unification
  codegen.rs      AST → C translation; embeds the runtime via include_str!
  main.rs         CLI: gleamc <file.gleam> [-o <dir>] [-c] [-release]; auto-compiles with cc
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
  runtime.rs      end-to-end tests for native execution and both CLI output modes
  language_test.gleam  core syntax exercised through generated C
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
