# Vaca compiler 🐄

**PUBLIC DEVELOPMENT**

`vacac` is a handwritten, statically compiled compiler for Vaca 🐮. It parses and checks source in Rust, generates native LLVM object code with Inkwell, then invokes `cc` to link an ELF executable.

> This compiler currently supports Linux only. For Windows/macOS, please use Docker or WSL2.

A **CodeTease** project.

## Requirements

- Rust and Cargo
- LLVM 23
- A C linker available as `cc`

## Quick start

Build with:

```sh
cargo build --release
```

Compile a Vaca source file (the object file is retained next to the executable):

```sh
target/release/vacac examples/factorial.vca -o factorial
./factorial
```

For a quick compile-and-run without leaving build artifacts:

```sh
target/release/vacac examples/factorial.vca --run
```

Run the lexer, parser, and semantic checker without producing build artifacts:

```sh
target/release/vacac examples/factorial.vca --check
```

Write the verified LLVM IR module to a text file without linking an executable:

```sh
target/release/vacac examples/factorial.vca --emit-llvm factorial.ll
```

## Documentation

Start at the [documentation index](docs/README.md).

- [Using the compiler](docs/usage.md) — build, CLI modes, linker inputs, and output-path safety.
- [Diagnostics](docs/diagnostics.md) — parser recovery and human-readable/JSON error formats.
- [Examples](docs/examples.md) — complete programs to check, compile, and run.
- [Testing](docs/testing.md) — Rust tests, the example runner, and the adversarial corpus.
- [Scalar C FFI](docs/c-ffi.md) — `extern "C"` declarations and native linking.
- [Language reference](docs/language/syntax.md) — Vaca types, values, and statements, with links to the rest of the language guide.

## Examples

The [`examples/`](examples/) directory contains complete programs that can be checked or compiled with `vacac`:

```sh
target/release/vacac examples/enum_match.vca --check
target/release/vacac examples/enum_match.vca -o enum_match
./enum_match
```

See [Examples](docs/examples.md) for the full list, including traits, modules, arrays, structs, closures, and C FFI.

## Testing

Run the Rust unit and integration tests with:

```sh
cargo test
```

See [Testing](docs/testing.md) for the example runner and the deterministic adversarial corpus.
