# Deterministic adversarial compiler corpus

This directory contains small Vaca reproducers for reference/lifetime
checking, generic specialization limits, generic traits, trait coherence,
bounds, default bodies, parser diagnostics, code generation, runtime traps,
closures, modules, module imports, and the scalar C ABI. Expected rejection and runtime-trap cases are intentional;
`run.py` checks their diagnostics/statuses so that they are distinct from a
compiler crash or unexpected success.

Run against the release compiler from the repository root:

```sh
cargo build --release
python3 tests/fuzz/run.py target/release/vacac
```

The runner creates a reproducible 48-module-deep project in a temporary
directory and removes it, as well as temporary native build artifacts, after
the run. `--check` also constructs and verifies LLVM IR; the closure case is
additionally exercised with `--emit-llvm`.

## Coverage and limits

- Closure fixtures cover both a returned aggregate snapshot and a mutable
  counter whose aliases share environment state while separately created
  closures remain independent.
- References are intentionally more limited than Rust: loans are lexical
  (there is no NLL), references cannot be captured or stored in aggregates, and
  lifetime contracts accept only the documented origins/propagation paths.
  The fixtures check shared and mutable reborrows, named mutable returns, and
  representative rejected origins/aliases. A child reborrow blocks overlapping
  access through its parent until the child's lexical scope ends, while
  disjoint field and constant-index projections remain usable. Place tracking
  permits simultaneous work on known disjoint array indexes, but an unknown
  index is treated as possibly overlapping an active loan.
- The generic reproducer grows `Vec<T>` recursively until the documented
  512-concrete-instance limit is reached. Trait impls share that limit and
  overlapping impl targets are rejected before specialization. A separate case
  inherits a default trait body through a generic impl while a concrete impl
  overrides it, checking static dispatch end to end.
- Generic-trait cases cover inferred and explicit trait/function arguments,
  trait-argument arity, the rejected `Trait::<Args>::function` shorthand, and a
  deliberately expanding bound proof that must stop at the trait-proof depth
  limit instead of overflowing the host stack.
- Parser cases exercise multiple malformed top-level/body constructs and a
  malformed match arm under `--check --json`. Diagnostics are valid JSON and
  recovery terminates; multiple diagnostics currently appear in recovery order,
  not necessarily source order.
- Import cases cover aliases for modules, types, traits, and variants (including
  direct variant imports in values and patterns), a duplicate alias, an alias
  that collides with a declaration in the same module, an unknown path, and
  malformed `use` declarations.
- Deep module nesting is tested at a bounded depth of 48 to stress recursive
  loading/name resolution without intentionally exhausting the host stack. Each
  level reaches its child through an aliased `use` import.
- C interop covers the scalar ABI (`i64`, `f32`, `f64`, `unit`) plus `str` and
  typed raw-pointer parameters and results. The pointer/string fixture checks a
  NUL-terminated string return, a `str` argument, mutation through a projected
  raw pointer, typed null equality, and a nullable raw-pointer result. Struct,
  enum, vector, and closure layouts remain non-ABI, safe references and
  booleans are not extern types, and variadics are unsupported.

No compiler internal panic, miscompilation, or broken JSON recovery was
reproduced by this corpus in the recorded run. `run.py` prints individual
case results and exits nonzero on a mismatch.