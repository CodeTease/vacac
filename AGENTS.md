# apexc — Developer Notes

## Requirements

- Rust + Cargo
- **LLVM 22** (must provide `llvm-config` or `llvm-config-22`; inkwell uses feature `llvm22-1`)
- C linker as `cc`

## Commands

| Action | Command |
|---|---|
| Build | `cargo build --release` |
| Test | `cargo test` |

No formatter, linter, or CI config exists. This is a bare Cargo project.

## Architecture

Four modules, one pipeline stage each:

1. **lexer.rs** – tokenizes `.apx` source into `Vec<Token>` (line/column tracking)
2. **parser.rs** – Pratt parser producing `Program` AST (expressions via precedence climbing)
3. **sema.rs** – type checker (scopes, signatures, immutable loop vars, return-path analysis)
4. **codegen.rs** – Inkwell/LLVM22 backend; emits object file, linked by external `cc`

All entry through `src/main.rs`. Checkers exist in **both** sema.rs and codegen.rs — if you add a rule, update both.

## Compiler usage

```
apexc <source.apx> [-o <executable>] [--emit-obj <object.o>]
```

Entry point must be exactly `fn main() -> i64`. The emitted ELF wrapper returns its low 8 bits as the process exit status.

Types: `i64`, `bool`, `str`. Only primitive ops, function calls, and the built-ins `print_i64(i64)`, `print(str)` (no newline), `println(str)`, and `read_i64() -> i64` (signed decimal token from stdin; invalid or out-of-range input exits 1).

## Testing

Tests are in `tests/compiler.rs`. Each integration test invokes the already-built binary via `env!("CARGO_BIN_EXE_apexc")`, writes temporary `.apx` to `$TMPDIR`, compiles, runs, and asserts stdout/status. Clean up temp dirs on completion.

## Gotchas

- Integer literal `9223372036854775808` (u64 max) is accepted only under unary minus → `i64::MIN`. Positive use outside `-` errors.
- `for` loop variable is immutable; assignment in body is rejected at both sema and codegen layers.
- Function bodies return via `return expr;` or final expression; all paths must produce a value.
- `print_i64`, `print`, `println` return unit — their results cannot be bound with `let`.
- Division/remainder by zero and `i64::MIN / -1` or `% -1` exit with status 1.
- Reserved names: `print_i64`, `print`, `println`, `read_i64`, `printf`, `strcmp`, `apex_main`, `getchar`, `exit`. User may not define these.
- `str` is lowered to an LLVM `ptr` to a NUL-terminated global constant (deduplicated per module). `==`/`!=` call `strcmp`; `\0` escapes are rejected in the lexer for this reason. All other operators on `str` are type errors.
- Object files land next to the output executable by default (`target/release/apexc foo.apx -o bar` → `bar.o`). Use `--emit-obj` to override.
