# Deterministic adversarial compiler corpus

This directory contains small Vaca v0.1 reproducers for reference/lifetime
checking, generic specialization limits, parser diagnostics, code generation,
runtime traps, closures, modules, and the scalar C ABI. Expected rejection and
runtime-trap cases are intentional; `run.py` checks their diagnostics/statuses
so that they are distinct from a compiler crash or unexpected success.

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

- References are intentionally more limited than Rust: loans are lexical
  (there is no NLL), mutable-reference reborrowing is rejected, references
  cannot be captured or stored in aggregates, and lifetime contracts accept
  only the documented origins/propagation paths. The fixtures check supported
  shared reborrowing and representative rejected origins/aliases. Here
  `&*mutable` is rejected at the earlier “mutable borrowing requires a named
  local binding” check; `&mut *mutable` similarly cannot form a reborrow. These
  limits are intentional, not findings of Rust borrow-checker unsoundness.
  Place tracking permits simultaneous work on known disjoint array indexes,
  but an unknown index is treated as possibly overlapping an active loan.
- The generic reproducer grows `Vec<T>` recursively until the documented
  512-concrete-instance limit is reached. The language has neither generic
  enums nor trait bounds.
- Parser cases exercise multiple malformed top-level/body constructs and a
  malformed match arm under `--check --json`. Diagnostics are valid JSON and
  recovery terminates; multiple diagnostics currently appear in recovery order,
  not necessarily source order.
- Deep module nesting is tested at a bounded depth of 48 to stress recursive
  loading/name resolution without intentionally exhausting the host stack.
- C interop covers only the documented scalar ABI (`i64`, `f32`, `f64`,
  `unit`); Vaca v0.1 does not support pointer/string/aggregate/variadic C FFI.

No compiler internal panic, miscompilation, or broken JSON recovery was
reproduced by this corpus in the recorded run. `run.py` prints individual
case results and exits nonzero on a mismatch.