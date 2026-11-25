# Changelog

All notable changes to the Vaca compiler (`vacac`) are documented in this
file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-25

Closures can now mutate captured state, completing the persistent mutable
capture item on the TODO roadmap.

### Added

- Persistent mutable closure captures. A lambda now stores its captured values
  in a heap environment shared by every copy of that closure, so assignments to
  captured mutable locals persist across calls while separately evaluated
  lambdas remain independent. Nested lambdas snapshot their parent's current
  state, and `const` bindings and loop variables stay immutable in captures.

### Fixed

- Vector element assignment now reloads the vector data pointer when it applies
  the store, so a right-hand side that grows the vector cannot leave the store
  targeting freed storage. Target indexes and bounds checks are still evaluated
  exactly once, before the right-hand side.

## [0.1.0] - 2026-11-24

Initial public release. `vacac` is a handwritten compiler for Vaca v0.1 that
checks source in Rust and emits native Linux ELF executables through LLVM 23.

### Added

#### Compiler pipeline

- A handwritten lexer, Pratt parser, hierarchical module loader and resolver,
  semantic analyzer, and LLVM code generator built with Inkwell.
- Native object-file generation and linking through the system `cc`, producing
  an executable whose exit status is the low 8 bits of `main`.
- LLVM module verification before artifacts are written.
- Trait validation and coherence checking, monomorphized generic
  specialization, conservative borrow and lifetime checking, and rejection of
  recursive by-value aggregate layouts.

#### Command-line interface

- Executable output with `-o`, explicit object paths with `--emit-obj`, and
  temporary build-and-run mode with `--run`.
- Check-only mode (`--check`) and verified LLVM IR emission (`--emit-llvm`).
- Repeatable native linker inputs: `--link-object`, `--link-search`, and
  `--link-lib` for executable and `--run` modes.
- `--json` diagnostics, `-q`/`--quiet`, `--help`, and `--version`.
- Options accepted before or after the source path, `--` to end option parsing,
  and output-path collision checks that prevent overwriting compiler inputs.

#### Language

- Scalar types `i64`, `f32`, `f64`, `bool`, and `str`; no implicit numeric
  conversions; IEEE 754 floating-point behavior; checked signed-integer
  overflow.
- Zero-sized `unit` (`()`), usable in locals, generic arguments, and enum
  payloads, including `Result<unit, E>`.
- `let` and `const` bindings, mutable assignment, struct-field updates,
  array/vector element assignment, and enforcement of immutable bindings.
- Expression `if`/`else`, `while`, range and collection `for` loops, and
  value-producing `loop` with `break` values; short-circuiting `&&` and `||`.
- `match` over enums, `bool`, `i64`, and floating-point values, with
  exhaustiveness checks, duplicate and unreachable-arm rejection, and
  recursive payload destructuring.
- Tuples and destructuring patterns for tuples, fixed arrays, and structs.
- Fixed-size arrays `[T; N]` with nesting, indexing, iteration, copying, and
  recursive equality.
- Heap-backed `Vec<T>` sequences with `vec_new`, `vec_push`, indexing,
  iteration, and length queries.
- Named structs with nesting, field access, mutation, and recursive equality.
- Enums with optional ordered payloads, construction, exhaustive matching, and
  recursive equality.
- Immutable strings with content equality, `+` concatenation, printing, and
  rejection of embedded NUL characters.
- Built-ins `print_i64`, `print_f32`, `print_f64`, `print`, `println`,
  `read_i64`, `len`, `vec_new`, and `vec_push`.
- Monomorphized generics for functions, structs, and enums, with argument
  inference, turbofish syntax, inline trait bounds, and a 512-instance
  specialization limit.
- Nominal traits with required functions, explicit implementations, coherence
  and visibility rules, and fully static dispatch, usable as bounds on generic
  declarations.
- Trait default function bodies, inherited and specialized per implementation.
- First-class function values and lambdas, including closures that capture
  locals by value into heap-backed environments.
- Shared and mutable references with conservative lexical borrow checking,
  named lifetimes, shared-reference reborrows, and lifetime-aware returns.
- Typed raw pointers (`*const T` and `*mut T`) with explicit `&raw const` and
  `&raw mut` address formation and unchecked dereference.
- Recoverable errors through the prelude `Result<T, E>` enum and the postfix
  `?` operator.
- Module-level `const` and `let` globals with compile-time constant
  initializers and static storage.
- Hierarchical modules (`mod name;`) with `pub` visibility, qualified paths,
  and private `use path::name [as alias];` imports for modules, functions,
  extern functions, globals, structs, enums, traits, and enum variants.
- Scalar C FFI through `extern "C"` declarations with `i64`, `f32`, and `f64`
  parameters and `i64`, `f32`, `f64`, or `unit` returns, together with native
  linker inputs.

#### Diagnostics

- Structured diagnostics in human-readable and JSON forms, with one-based
  source locations, source excerpts, and carets aligned for tabs and Unicode
  scalar values.
- Parser recovery at top-level declaration boundaries and within function
  bodies and match arms, reporting multiple errors where possible.
- JSON diagnostics containing `severity`, `file`, `line`, `column`,
  `end_column`, and `message`.
- Source identity preserved through module loading and code generation so
  backend errors point at the relevant declaration.

#### Documentation, examples, and testing

- User documentation for the CLI, diagnostics, scalar C FFI, testing, and the
  Vaca v0.1 language, plus a documentation index.
- Example programs covering arithmetic, control flow, arrays, vectors, structs,
  enums, tuples, generics, traits, closures, references, pointers, modules,
  globals, `Result`, floating-point values, and C FFI.
- Feature-grouped Rust unit and integration tests.
- `scripts/test-examples.sh` to check and run every example entry point.
- A deterministic adversarial corpus covering references and lifetimes, generic
  specialization limits, trait coherence, parser recovery and JSON diagnostics,
  deep modules, imports, overflow, IEEE floating-point values, closures, scalar
  C FFI, and LLVM emission.
- CI for formatting, tests, release builds, examples, and the adversarial
  corpus, plus a tag-driven release workflow that publishes statically linked
  Linux x86_64 binaries and `SHA256SUMS`.
- Apache License 2.0.

[0.2.0]: https://github.com/CodeTease/vacac/releases/tag/v0.2.0
[0.1.0]: https://github.com/CodeTease/vacac/releases/tag/v0.1.0
