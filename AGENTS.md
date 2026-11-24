# vacac - Developer Notes

User-facing docs live in `README.md` (the landing page) and `docs/` (the full
guides, indexed by `docs/README.md`). This file covers building, the compiler
pipeline, and rules that are easy to break.

## Requirements

- Rust with the 2024 edition; CI pins 1.99.1
- **LLVM 23**; inkwell uses feature `llvm23-1`. Provide `llvm-config` or
  `llvm-config-23`, or set `LLVM_SYS_231_PREFIX` (CI uses `/usr/lib/llvm-23`)
- C linker as `cc`
- Python 3 for the adversarial corpus

## Commands

| Action | Command |
|---|---|
| Build | `cargo build --release` |
| Test | `cargo test` (CI uses `cargo test --locked`) |
| Format | `cargo fmt` |
| Check/run all examples | `scripts/test-examples.sh` (after `cargo build --release`) |
| Adversarial corpus | `python3 tests/fuzz/run.py target/release/vacac` |

No linter config exists. GitHub Actions (`.github/workflows/ci.yml`) checks
`cargo fmt --all -- --check`, runs tests, builds the release compiler, checks and
runs all examples, and runs `tests/fuzz/run.py`.

## Architecture

Pipeline stages, all entered through `src/main.rs`:

1. **lexer.rs** - tokenizes `.vca` source into `Vec<Token>` with line/column tracking
2. **parser.rs** - Pratt parser producing `Program`; split across `parser/declarations.rs`, `expressions.rs`, `matching.rs`, `statements.rs`
3. **modules/loader.rs** - loads the root and nested module sources into one `Program`
4. **modules/resolve.rs** - resolves per-module `use` imports, qualifies names, and enforces module visibility
5. **modules/traits.rs** - validates traits and impls, checks coherence, lowers impl functions, and materializes inherited default bodies as specialized impl functions
6. **modules/generics/** - specializes functions, structs, and enums; inference, validation, trait bounds, and static trait dispatch (512 instance limit)
7. **sema.rs** - type checker: scopes, signatures, immutability, place-based borrows, named lifetimes, return paths and unreachable code; split across `sema/` (declarations, expressions, matching, places, statements, types)
8. **aggregate_validation.rs** - rejects recursive by-value struct/enum/array layouts; called from sema
9. **codegen.rs** - Inkwell/LLVM 23 backend; builds and verifies the module, emits an object file, then links with external `cc`; split across `codegen/`
10. **diagnostic.rs** - structured diagnostics with human-readable and JSON rendering

Trait calls and bounds are fully resolved to ordinary direct calls during module
loading and generic specialization; there are no trait objects or runtime
vtables. Checkers exist in **both sema.rs and codegen.rs** for immutability,
place assignment, and overlapping mutable arguments. If you add a rule, update
both layers.

## Compiler usage

```
vacac <source.vca> [-o <executable>] [--emit-obj <object.o>]
vacac <source.vca> --run
vacac <source.vca> --check
vacac <source.vca> --emit-llvm <module.ll>
```

Options may appear before or after the source path; `--` ends option parsing.
Add `--link-object <path>`, `--link-search <dir>`, and `--link-lib <name>` for
native inputs (repeatable, executable and `--run` only). `--json` renders errors
as JSON, `-q`/`--quiet` suppresses success messages, and `--help`/`--version` are
available.

The default executable is `a.out`. The object file is derived from the
executable path unless `--emit-obj` is given (`-o bar` writes `bar.o`).
`--run` builds in a temporary directory and runs the result. `--check` runs
sema and builds and verifies the LLVM module without writing artifacts.
`--emit-llvm` verifies the module and writes IR. `--run`, `--check`, and
`--emit-llvm` are mutually exclusive and cannot be combined with `-o` or
`--emit-obj`.

Entry point must be exactly `fn main() -> i64` in the root source. The emitted
C-ABI wrapper returns its low 8 bits as the process exit status.

Types: `i64`, `f32`, `f64`, `bool`, `str`, tuples, fixed arrays `[T; N]`,
vectors `Vec<T>`, named structs and enums, function types `fn(T) -> U`, shared
and mutable references `&T`/`&mut T`, and raw pointers `*const T`/`*mut T`.
`unit` is a zero-sized value: `()` is its literal and `()`/`unit` name the
type. Unit values can be bound to locals, used as generic arguments, and stored
in enum payloads (`Result<unit, E>` works with `?`). Unit parameters, struct
fields, tuple/array/vector elements, and globals remain unsupported. Unsuffixed
float literals are `f64`; an `f32` suffix such
as `1.25f32` produces an `f32`. There are no implicit numeric conversions.
`Result<T, E>` is a compiler-provided prelude enum; `?` propagates its error.
Built-ins are `print_i64(i64)`, `print_f32(f32)` (9 significant digits plus
newline), `print_f64(f64)` (17 significant digits plus newline), `print(str)`
(no newline), `println(str)`, `read_i64() -> i64` (signed decimal token from
stdin; invalid or out-of-range input exits 1), `len(array_or_vector) -> i64`,
`vec_new()` (needs an expected `Vec<T>` type), and `vec_push(vector, value)`
(returns unit).

## Testing

Integration tests are rooted at `tests/compiler.rs` and grouped by feature in
`tests/compiler/` (`cli`, `runtime`, `language` with `control_flow`,
`diagnostics`, and `semantics`/`values` submodules, `strings`, `arrays`,
`vectors`, `structs`, `enums`, `tuples`, `generics`, `traits`, `function_values`,
`globals`, `pointers`, `references`, `results`, `ffi`, `floats`, `bitwise`,
`equality`, `matching_loops`, and `modules`). Shared helpers are in
`tests/compiler/common/mod.rs`. Each test invokes the already-built binary via
`env!("CARGO_BIN_EXE_vacac")`, writes a temporary `.vca` under
`std::env::temp_dir()`, compiles, runs, asserts stdout/status, and removes the
directory. Unit tests also live beside sema and CLI code.

`scripts/test-examples.sh` checks and runs every top-level `examples/*.vca` and
nested `main.vca` entry point (module implementation files are not independent
programs), using `--check` and `--run`. It supplies `42` to `read_number.vca`
and accepts `VACAC=/path/to/vacac`.

`tests/fuzz/run.py` is a deterministic adversarial corpus for references,
lifetimes, generics limits, trait coherence, parser recovery/JSON diagnostics,
deep modules, overflow, IEEE floats, closures, and scalar C FFI. Expected
compile failures and runtime traps are intentional; the runner checks messages
and exit statuses and fails on a compiler panic or unexpected success. Its
`README.md` documents current limitations.

## Gotchas

- Integer literal `9223372036854775808` (u64 max) is accepted only under unary minus, producing `i64::MIN`. Positive use outside `-` errors.
- Signed `i64` add, subtract, multiply, and negate trap on overflow. Division/remainder by zero, `i64::MIN / -1` or `% -1`, a negative shift count, and a shift count of 64 or more exit with status 1.
- `const` bindings, `for` loop variables, and reference bindings are immutable. Direct assignment and struct-field or array-element assignment through them are rejected at both sema and codegen layers. Reference bindings must be `const` and cannot be reassigned. `let` bindings remain mutable. A `const` vector handle can still be appended to or have elements mutated; only rebinding is forbidden.
- Function bodies return via `return expr;` or a final expression; all paths must produce a value. `unit` functions may finish without a value or use `return;`.
- `print_i64`, `print`, `println`, `vec_push` return unit; their results cannot be bound or used as values. `unit` is not a parameter, struct field, vector element, or enum payload type.
- Empty array literals need an explicit array type. Array and vector indexes are `i64`; negative or out-of-range indexes exit with status 1.
- Structs and enums support by-value fields containing arrays, tuples, enums, and other structs. Recursive by-value layouts are rejected; vectors and pointers break the cycle because they are indirect.
- Reserved names: `print_i64`, `print_f32`, `print_f64`, `print`, `println`, `read_i64`, `len`, `vec_new`, `vec_push`, `printf`, `strcmp`, `vaca_main`, `getchar`, `exit`, `strlen`, `malloc`, `realloc`, `memcpy`, `vaca_alloc`, `vaca_str_concat`, `vaca_vec_new`, `vaca_vec_push`. `Result` is a reserved prelude enum, `Self` cannot name a user type, and extern C symbols may not collide with these or `main`.
- Shift and bitwise operators require `i64` operands; arithmetic and comparisons are type-matched with no float/int mixing. `&&` and `||` short-circuit. Float `match` patterns use IEEE equality, treat signed zeros as equal for duplicate checks, never match NaN, and require a wildcard arm.
- Traits use nominal `impl Trait for Type` blocks and static dispatch. Bounds are checked per concrete instantiation, impl functions are rewritten to unspellable internal names, and overlapping impl targets are rejected conservatively. Trait functions may end in a default body; an impl that omits the function inherits a copy specialized with `Self` replaced by the impl target and the impl's generic parameters. Inherited bodies keep the trait's module for privacy checks, call sibling functions through `<Self as Trait>::function(...)`, and are type-checked once per implementation (a trait with no impls only has its bodies resolved). Explicit impls override defaults and must match the trait signature. No `dyn Trait`, method syntax, orphan rule, generic traits, supertraits, or associated items yet.
- Generics are monomorphized. Type arguments are inferred when possible; turbofish requires every parameter. More than 512 concrete instances is rejected.
- Lambdas use a fully annotated `fn(T) -> U { ... }` signature. Captures are copied by value into a heap environment when the lambda is evaluated and retain it until process exit. Closure copies share that environment, so mutable captures persist across calls and remain independent from separately evaluated lambdas; `const` bindings and loop variables stay immutable inside the closure. Capture slots cannot be borrowed or addressed. Function values compare by code and environment pointer. Global function-value initializers are unsupported.
- Globals use `const` or `let` with compile-time constant initializers. Function calls, mutable-global dependencies, and initializer cycles are rejected, and there is no startup initializer execution.
- `str` is an LLVM `ptr` to a NUL-terminated global constant, deduplicated per module. `+` calls a runtime helper and returns a heap allocation retained until process exit; `==`/`!=` call `strcmp`. `\0` escapes and raw `0x00` bytes are rejected in the lexer. Other operators on `str` are type errors.
- References are checked lexically: no NLL, no mutable-reference reborrowing, and references cannot be captured, stored in aggregates or globals, or returned through function-value calls. Mutable references must initialize a `const` local from a fresh borrow of a mutable local place, and one call cannot receive overlapping `&mut` origins in multiple parameters. Raw pointers are typed but unchecked and cannot implicitly become references.
- Scalar C FFI only: `extern "C" fn name(...) -> T;` with `i64`, `f32`, `f64`, or `unit` results. No pointer, string, aggregate, or variadic FFI yet. The C symbol is the declared name.
- Hierarchical modules use `mod name;`; child source may be `name.vca` or `name/mod.vca`, relative to the declaring module file. Cross-module declarations must be `pub`, names are qualified with `::`, and only the root defines `main`. `use path::name [as alias];` imports a module, function, extern function, global, struct, enum, trait, or enum variant into one module; paths resolve relative to the declaring module and then the root. Imports are private to that module (no `pub use`, groups, or globs), aliases are collision-checked against local declarations and submodules, private targets are rejected, and directly imported enum variants work as values, call targets, and match patterns.
- Parser recovery reports multiple diagnostics where possible in recovery order, not necessarily source order. Any parser or module-loading diagnostic stops compilation before sema and codegen.
- Object files land next to the output executable by default. Output paths are checked before anything is written: the executable, object file, and `--emit-llvm` path may not resolve to the root source, a module source, or a `--link-object` input, and the executable and object paths must differ. Hard links and symlinks count as collisions.
