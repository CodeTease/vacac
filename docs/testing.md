# Testing

Run the Rust unit and integration tests with:

```sh
cargo test
```

## Checking and running examples

To check and run every example program, first build the release compiler and then run:

```sh
cargo build --release
scripts/test-examples.sh
```

The script checks and runs every top-level `examples/*.vca` and nested `main.vca` entry point (module implementation files are not independent programs), using `--check` and `--run`. It supplies `42` to `read_number.vca` and accepts `VACAC=/path/to/vacac`.

## Adversarial corpus

The deterministic adversarial corpus in [`tests/fuzz/`](../tests/fuzz/) targets reference/lifetime diagnostics, generic specialization limits, generic trait inference and proof-depth limits, malformed-source recovery and JSON diagnostics, deep module loading, module import resolution, checked integer overflow, IEEE floating-point values, returned and stateful closure captures, C scalar FFI, and LLVM emission. Run it against the release compiler with:

```sh
python3 tests/fuzz/run.py target/release/vacac
```

Expected compile errors and runtime traps are part of the corpus; its runner checks their diagnostics and exit statuses and fails on unexpected results. See [`tests/fuzz/README.md`](../tests/fuzz/README.md) for coverage details and language limitations.

Parser recovery reports multiple diagnostics when possible; their order reflects recovery and is not guaranteed to match source order. See [Diagnostics](diagnostics.md#parser-recovery).
