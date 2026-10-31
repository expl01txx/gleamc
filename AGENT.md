# AGENT.md — gleam-to-c

A Gleam → C compiler written in Rust. It parses a `.gleam` source file and
emits a small C project (`main.c` + an embedded C runtime) that compiles with
any C11 compiler on a POSIX system. No external crates are used; the whole
project is `std`-only Rust plus two hand-written C files.

## Repository layout

```
src/
  lib.rs          crate root: re-exports parse, generate_c, write_c_project
  lexer.rs        tokenizer (Token, TokenKind)
  ast.rs          AST types (Module, Definition, Expr, Pattern, ...)
  parser.rs       recursive-descent parser producing a Module
  codegen.rs      AST → C translation; also embeds the runtime via include_str!
  main.rs         CLI: `gleam-parser <file.gleam> -o <dir>`
  runtime/
    gleam_runtime.h   GValue model + all runtime/stdlib declarations
    gleam_runtime.c   runtime implementation + all stdlib function bodies
tests/
  tour.rs         parser-level tests (no C involved)
  runtime.rs      end-to-end tests: parse → codegen → cc → run → assert stdout
  hello.gleam     small demo program used by an e2e test
  string_test.gleam / int_test.gleam   stdlib test programs (see below)
gleam_std_*.html  vendored stdlib docs (string, int, list, ...)
```

## Build & run workflow

```sh
cargo build
./target/debug/gleam-parser tests/hello.gleam -o /tmp/out
cc -std=c11 -Wall -Wextra /tmp/out/main.c /tmp/out/gleam_runtime.c -o /tmp/prog
/tmp/prog
cargo test    # parser tests + e2e runtime tests (skipped if no cc)
```

`write_c_project` writes `main.c`, `gleam_runtime.c`, and `gleam_runtime.h`
into the output directory. The generated `main.c` includes
`"gleam_runtime.h"`; only `main.c` and `gleam_runtime.c` need to be compiled.

## Architecture

### Pipeline

`lexer.rs` → tokens → `parser.rs` → `Module` AST → `codegen.rs` → C source.
`translate(source)` does parse+codegen in one call; `write_c_project` also
writes the runtime files next to `main.c`.

### Generated C model

All values are `GValue`: a tagged union (`G_INT`, `G_FLOAT`, `G_BOOL`,
`G_NIL`, `G_STRING`, `G_LIST`, `G_TUPLE`, `G_RECORD`, `G_CLOSURE`,
`G_BYTES`). Heap types (`GString`, `GList`, `GTuple`, `GRecord`, `GClosure`)
are refcounted via `g_retain`/`g_release`. The generated code is responsible
for balancing retains/releases around temporaries.

- User functions compile to `static GValue gleam_fn_<name>(GClosure *, GValue *, size_t)`.
- Records are `GRecord` with a string tag; constructors like `Ok(x)` compile
  to an inline `g_record("Ok", 1, fields)`.
- Generated locals are named `g_0`, `g_1`, ... (monotonically increasing per
  function).
- Case clauses compile to sequential `if (!g_N_matched)` blocks; a
  non-exhaustive case ends in `g_panic("panic", ...)`.

### Codegen structure (`src/codegen.rs`)

- `Compiler` holds symbol tables: `constructors`, `functions` (user),
  `imports`, `unqualified` (imports), `constants`, `visible`.
- `gen_expr` is the main expression compiler; helpers: `gen_call`,
  `eval_call_args` (label resolution), `gen_case`, `gen_guard`,
  `gen_closure`, `gen_pipeline`, `gen_binary`, ...
- **Stdlib functions are native**: they are not compiled from Gleam. A call
  like `string.split(x, on: y)` resolves through `direct_function` →
  `std_function(module, name)` and emits a direct C call
  `gleam_string_split(args, argc)`.
- `std_function(module, name) -> Option<(&'static str, usize, &'static [Option<&'static str>])>`
  is the single table mapping `(module, function)` → `(C name, arity, labels)`.
  Labels are `None` for positional-only parameters.
- `is_stdlib_module(module)` lists modules usable **without an import**
  (currently: `gleam/io`, `gleam/fs`, `gleam/string`, `gleam/int`).
- Built-in constructors are registered in `Compiler::new`
  (`Ok`, `Error`, `Some`, `None`, `Eq`, `Lt`, `Gt`, `InvalidBase`).
  Zero-field constructors do not get a `gleam_ctor_*` wrapper; records are
  built inline.

## How to implement a stdlib module

Worked example: `gleam/string` and `gleam/int` were added this way.

1. **Extract the docs.** The HTML is vendored in the repo root
   (`gleam_std_<module>.html`). Strip tags with a quick Python one-liner
   (regex `<[^>]+>` → newline, `html.unescape`) and read the function list,
   signatures, labels, and examples.

2. **Decide scope.** Skip functions that depend on modules this compiler does
   not have (e.g. `list.group` needs `gleam/dict`; `string.utf_codepoint`
   was excluded as a UTF/grapheme feature). Document the exclusion in a
   comment in `std_function`.

3. **Register in `src/codegen.rs`:**
   - Add an entry to `std_function` for every function:
     `Some(("gleam_<module>_<name>", arity, &[labels...]))`. The match is on
     a tuple of module booleans `(io, fs, string, int, name)` — add a new
     boolean + extend every existing pattern when adding a module.
   - Add the module to `is_stdlib_module` so it works without an import.
   - Register any new custom-type constructors (e.g. `InvalidBase`) in the
     built-in constructor loop in `Compiler::new`.

4. **Declare** each function in `src/runtime/gleam_runtime.h`:
   `GValue gleam_<module>_<name>(GValue *args, size_t argc);`

5. **Implement** in `src/runtime/gleam_runtime.c` (conventions below).

6. **Write a test program** `tests/<module>_test.gleam` covering every
   implemented function with the doc examples (see Testing below).

7. **Add an e2e test** to `tests/runtime.rs`:
   ```rust
   #[test]
   fn <module>_module_end_to_end() {
       if !cc_available() { eprintln!("skipping: no C compiler available"); return; }
       let out = run_gleam(include_str!("<module>_test.gleam"), "<module>");
       assert!(out.ends_with("all <module> tests passed\n"), "unexpected output:\n{out}");
   }
   ```

8. **Rebuild the Rust binary before regenerating C.** The runtime is embedded
   with `include_str!` — a stale binary emits a stale `gleam_runtime.c`.
   Always: `cargo build` → generate → `cc` → run.

9. **Run the full suite** (`cargo test`) and verify the C compiles clean:
   ```sh
   cc -std=c11 -Wall -Wextra -pedantic -c src/runtime/gleam_runtime.c -o /dev/null
   ```

## C runtime conventions (`src/runtime/gleam_runtime.c`)

- Every stdlib function starts with `require_args("<module>.<name>", N, argc);`
  and uses `require_string(...)` / `require_int(...)` for typed arguments.
- Return values are built with the `g_*` constructors:
  `g_int`, `g_float`, `g_bool`, `g_nil`, `g_string`, `g_string_n`,
  `g_list_empty`, `g_list_prepend`, `g_tuple`, `g_record`,
  `g_bytes`. Results are `g_record("Ok", 1, (GValue[]){...})` /
  `g_record("Error", 1, ...)`. Use compound literals for field arrays.
- Errors: `g_panic(message, __FILE__, __LINE__)` for programmer errors
  (bad arity, division by zero), `g_type_error(expected, value, ...)` for
  wrong-typed values. Both abort.
- **List construction order**: `g_list_prepend` builds backwards. To produce
  `[a, b, c]` from least-significant-first data, prepend in index order
  `0, 1, 2`. (This bug has bitten us twice: `string.split` and `int.digits`.)
- **UTF-8 back-walks**: walking back over continuation bytes
  (`(data[i] & 0xC0) == 0x80`) stops *right after* the lead byte, so the lead
  byte is at `index - 1`. Decode at `index - 1` and slice from there.
- **Strict C11 + POSIX**: the file defines `_POSIX_C_SOURCE 200809L` at the
  top. No C++ features (no lambdas, no auto). `random()` is unavailable in
  strict mode — use `rand()` (+ `srand` once with `time(NULL)` for
  randomness). New `<...>` includes go alphabetically after `gleam_runtime.h`.
- Indentation is 2 spaces; comments explain *why*, matching the existing
  style. No global mutable state except explicit `static` helpers.
- Keep helper functions `static` and near their use site (e.g. `trim_edges`,
  `int_to_base`, `random_draw`).

## Codegen conventions (`src/codegen.rs`)

- Errors carry a source `Span`; construct with `CodegenError::new(msg, span)`.
- Labeled call arguments resolve through `FunctionInfo.labels`
  (`Vec<Option<String>>`) in `eval_call_args`; native stdlib functions must
  provide real labels or labeled calls fail with
  "unknown or misplaced call argument".
- When adding a module to `std_function`, update **all** match-arm tuples and
  the two `FunctionInfo` construction sites that convert labels
  (`label.map(|l| l.to_string())`).
- Don't add dependencies; keep everything in `std`.

## Testing conventions

- **Parser tests** (`tests/tour.rs`): pure Rust, use the `ok(src)` /
  `err(src)` helpers; assert on AST shape. No C involved.
- **E2E tests** (`tests/runtime.rs`): `run_gleam(src, name)` parses, writes a
  project to `/tmp/gleam-parser-test-<name>`, compiles with
  `cc -std=c11 -Wall -Wextra`, runs, and returns stdout. Tests skip cleanly
  when `cc` is missing. Each test uses its own directory name.
- **Gleam test programs** (`tests/*_test.gleam`) follow a fixed shape:
  ```gleam
  fn check(condition: Bool, name: String) {
    case condition {
      True -> io.println(name <> " ok")
      False -> {
        io.println(name <> " FAILED")
        panic as "check failed"
      }
    }
  }
  ```
  `main` runs every check and ends with `io.println("all <module> tests passed")`.
  One line per check makes failures easy to locate.

### Gleam language gotchas (when writing test programs)

- `panic as` only accepts a **string literal**, not a variable.
- Pipelines bind looser than operators: `"a" |> f == b` parses as
  `"a" |> (f == b)` — parenthesize: `("a" |> f) == b`.
- Labeled and positional arguments can be mixed freely
  (`int.clamp(40, min: 50, max: 60)`).
- Unqualified imports work: `import gleam/int {sum}`.
- Blocks `{ ... }` are expressions; the last expression is the value.

## Current stdlib coverage

| Module | Status |
|---|---|
| `gleam/io` | `print`, `println`, `debug`, `read_line` |
| `gleam/fs` | `read`, `write`, `append`, `remove`, `is_file`, `is_directory`, `create_directory`, `list_directory` |
| `gleam/string` | 31 functions; grapheme/UTF functions excluded (`drop_start`, `drop_end`, `pop_grapheme`, `to_graphemes`, `utf_codepoint`, `utf_codepoint_to_int`, `to_utf_codepoints`, `from_utf_codepoints`). Grapheme counting is approximated via a combining-mark table. |
| `gleam/int` | all 21 functions |
| `gleam/list` | **not started** — 61 functions; `group` needs `gleam/dict` (exclude it) |

## Known pitfalls (learned the hard way)

1. **Stale embedded runtime** — after editing `src/runtime/*.c|h`, run
   `cargo build` before generating C, or you debug yesterday's runtime.
2. **Prepend order** — see C conventions; verify list-producing functions
   against a known expected list, not just length.
3. **UTF-8 off-by-one** — see C conventions; test with multi-byte endings
   (e.g. `"xe̊"`) and ASCII endings separately.
4. **Label typing** — `std_function` labels are
   `[Option<&'static str>]`; a function with an unlabeled first parameter and
   labeled second (`split(x, on: y)`) needs `[None, Some("on")]`, not `&[]`.
5. **Doc examples can be wrong in your head** — verify arithmetic in the
   test itself (e.g. 8 in binary is `1000`, not `100`).
6. **`hello.gleam` is a live file** — its contents may change; the e2e test
   asserts exact stdout, so keep it in sync.
