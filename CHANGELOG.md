# Changelog

All notable changes to the Vaca compiler (`vacac`) are documented in this
file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-11-28

Practical raw-pointer and C-string interoperability completes the
corresponding TODO roadmap item.

### Added

- `extern "C"` parameters and results may use `str` and typed raw pointers.
  Strings cross as non-null, NUL-terminated `const char *` values without
  copying; a null C string result traps instead of entering Vaca as `str`.
- Typed null expressions (`null::<*const T>()` and `null::<*mut T>()`) and
  address equality (`==`/`!=`) for raw pointers of the same type.
- `&raw const place` and `&raw mut place` now accept struct fields, fixed-array
  elements, vector elements, and nested combinations, with single evaluation of
  selectors and vector-storage reloads when a later index grows the vector.
- Documented ABI and lifetime rules for C strings, nullable raw-pointer results,
  vector-backed addresses, and non-ABI Vaca aggregate layouts.

### Changed

- Raw-pointer comparisons now lower directly to LLVM pointer comparisons instead
  of being rejected.
- Mutable raw address-taking follows ordinary place mutability, including
  mutable vector elements through `const` handles.

### Fixed

- A stray `>` after a typed null pointer type is now rejected with an
  "unexpected `>` after the null pointer type" diagnostic.

## [0.5.0] - 2026-11-28

Mutable-reference reborrowing and named mutable returns complete the
corresponding TODO roadmap item.

### Added

- Mutable reborrows (`&mut *reference`) and shared views of mutable references
  (`&*reference`), including field and constant-index projections. Child loans
  block overlapping parent access until their lexical scope ends, while
  known-disjoint places remain usable.
- Named-lifetime mutable returns (`fn get<'a>(value: &'a mut T) -> &'a mut T`)
  through direct returns, reborrows, forwarding calls, and `if`/`match`
  selections. Callers retain loans on every eligible argument origin, and local
  borrows still cannot escape through a return.

### Changed

- Safe-reference tracking now uses a unified shared/mutable loan model with
  per-value loan lineages. Direct and indirect call arguments reserve reference
  origins during argument evaluation, and overlapping shared/mutable call
  arguments are rejected.

### Fixed

- Reference origins returned from lifetime-annotated calls are now opaque.
  Projecting a field through a returned reference keeps the whole argument
  borrowed, so mutating an argument's field while that reference is live is
  rejected instead of being allowed through a narrowed loan.
- Borrow checking is now scoped per function or lambda, so parameters of
  different callables at the same position no longer appear to alias and
  spurious overlapping-mutable-argument errors are gone.
- Reference-typed `if`/`match` results can no longer escape a block-local
  binding or a pattern binding, while scalar results computed by dereferencing
  a block-local reference are now accepted.
- Loans created inside a block end with that block, while references that
  escape keep their ancestor loans, so using the parent after a branch no
  longer reports a stale conflict.
- Reborrow index selectors are now type-checked, and non-constant indices
  remain usable in reborrows.

## [0.4.0] - 2026-11-27

Generic traits and generic trait functions complete the corresponding TODO
roadmap item.

### Added

- Generic traits and generic trait functions. Traits and their functions may
  declare type parameters, impl heads carry ordered trait arguments, and trait
  and function arguments are inferred from value arguments or supplied with
  qualified calls and `::<...>` turbofish. Coherence and overlap check the full
  impl head, inherited defaults specialize trait and function parameters with
  capture-avoiding renaming, and concrete bound proofs and specialization share
  the existing instance and proof-depth limits.

### Fixed

- Generic trait inference now finds trait arguments nested inside aggregate
  payloads, so an argument supplied deep in a tuple, array, or struct field
  still specializes the impl.
- Generic trait argument inference no longer depends on the order in which
  value arguments appear, so an impl head is selected correctly when an earlier
  parameter is only narrowed by a later one.

## [0.3.0] - 2026-11-25

Match arms can now combine aggregate and nested patterns, completing the
generalized aggregate and nested matching item on the TODO roadmap.

### Added

- Generalized aggregate and nested matching. Match arms can combine enum
  variants, scalar literals, tuples, fixed arrays, and struct fields at any
  depth, and an aggregate scrutinee can now use several arms. Reachability and
  exhaustiveness are computed from the whole pattern, so coverage of every
  payload combination is required instead of only every top-level variant.

### Fixed

- Generic specialization now normalizes the scrutinee type and infers match
  result types from every arm, including statements and `loop` break values, so
  a generic call can infer from a value bound by a nested pattern.
- Generic inference resolves an `if` branch tail type in its own binding scope,
  so a branch-local binding no longer shadows an outer binding of a different
  type.
- Pattern-matrix limit errors now point at the `match` expression instead of an
  internal location.

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

Initial public release. `vacac` is a handwritten compiler for the Vaca language that
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
  Vaca language, plus a documentation index.
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

[0.6.0]: https://github.com/CodeTease/vacac/releases/tag/v0.6.0
[0.5.0]: https://github.com/CodeTease/vacac/releases/tag/v0.5.0
[0.4.0]: https://github.com/CodeTease/vacac/releases/tag/v0.4.0
[0.3.0]: https://github.com/CodeTease/vacac/releases/tag/v0.3.0
[0.2.0]: https://github.com/CodeTease/vacac/releases/tag/v0.2.0
[0.1.0]: https://github.com/CodeTease/vacac/releases/tag/v0.1.0
