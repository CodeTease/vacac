# vacac — Developer Notes

## Requirements

- Rust + Cargo
- **LLVM 23** (must provide `llvm-config` or `llvm-config-23`; inkwell uses feature `llvm23-1`)
- C linker as `cc`

## Commands

| Action | Command |
|---|---|
| Build | `cargo build --release` |
| Test | `cargo test` |
| Format | `cargo fmt` |
| Check/run all examples | `scripts/test-examples.sh` (after `cargo build --release`) |

No linter config exists. CircleCI checks formatting, runs tests, builds the
release compiler, and checks/runs all examples.

## Architecture

Four modules, one pipeline stage each:

1. **lexer.rs** – tokenizes `.vca` source into `Vec<Token>` (line/column tracking)
2. **parser.rs** – Pratt parser producing `Program` AST (expressions via precedence climbing)
3. **sema.rs** – type checker (scopes, signatures, immutable bindings and loop vars, return-path analysis)
4. **codegen.rs** – Inkwell/LLVM22 backend; emits object file, linked by external `cc`

All entry through `src/main.rs`. Checkers exist in **both** sema.rs and codegen.rs — if you add a rule, update both.

## Compiler usage

```
vacac <source.vca> [-o <executable>] [--emit-obj <object.o>]
```

Entry point must be exactly `fn main() -> i64`. The emitted ELF wrapper returns its low 8 bits as the process exit status.

Types: `i64`, `f32`, `f64`, `bool`, `str`, named enums, named structs, fixed-size arrays, and vectors. Floating-point values may be used in arrays, vectors, structs, and enum payloads. Unsuffixed float literals are `f64`; use an `f32` suffix, such as `1.25f32`, for a rounded `f32` literal. There are no implicit conversions between integers or float widths. Float match patterns use IEEE equality and require a wildcard arm. Functions may also return `unit`. Built-ins are `print_i64(i64)`, `print_f32(f32)` (9 significant digits plus newline), `print_f64(f64)` (17 significant digits plus newline), `print(str)` (no newline), `println(str)`, `read_i64() -> i64` (signed decimal token from stdin; invalid or out-of-range input exits 1), and `len(array) -> i64`.

## Testing

Integration tests are rooted at `tests/compiler.rs` and grouped by feature in `tests/compiler/` (`cli`, `runtime`, `language`, `strings`, `arrays`, `structs`, and `enums`). Shared compile/run helpers are in `tests/compiler/common/mod.rs`. Each test invokes the already-built binary via `env!("CARGO_BIN_EXE_vacac")`, writes temporary `.vca` to `$TMPDIR`, compiles, runs, and asserts stdout/status. Clean up temp dirs on completion.

`scripts/test-examples.sh` checks and runs all top-level `examples/*.vca` files and nested `main.vca` entry points (including module projects); nested module implementation files are not treated as independent programs. It supplies `42` to `read_number.vca` and accepts `VACAC=/path/to/vacac` to select another compiler binary. Hierarchical modules use `mod name;`; child source may be `name.vca` or `name/mod.vca`, relative to the declaring module file. Cross-module declarations must be `pub`, names are qualified with `::`, and only the root source defines `fn main() -> i64`.

## Gotchas

- Integer literal `9223372036854775808` (u64 max) is accepted only under unary minus → `i64::MIN`. Positive use outside `-` errors.
- `const` bindings and `for` loop variables are immutable; direct assignment, array-element assignment, and struct-field assignment through them are rejected at both sema and codegen layers. `let` bindings remain mutable.
- Function bodies return via `return expr;` or final expression; all paths must produce a value.
- `print_i64`, `print`, `println` return unit — their results cannot be bound with `let` or `const`.
- Division/remainder by zero and `i64::MIN / -1` or `% -1` exit with status 1.
- Structs support by-value fields containing arrays, enums, and other structs; recursive by-value struct layouts are rejected.
- Reserved names: `print_i64`, `print`, `println`, `read_i64`, `len`, `printf`, `strcmp`, `vaca_main`, `getchar`, `exit`, `strlen`, `malloc`, `realloc`, `memcpy`, `vaca_alloc`, `vaca_str_concat`, `vaca_vec_new`, `vaca_vec_push`. User may not define these.
- `str` is lowered to an LLVM `ptr` to a NUL-terminated global constant (deduplicated per module); concatenation calls a generated runtime helper and returns a heap allocation retained until process exit. `==`/`!=` call `strcmp`; `\0` escapes are rejected in the lexer for this reason. Other operators on `str` are type errors.
- Object files land next to the output executable by default (`target/release/vacac foo.vca -o bar` → `bar.o`). Use `--emit-obj` to override.
