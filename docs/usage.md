# Using the compiler

`vacac` compiles a Vaca source file to a native ELF executable. Options may appear before or after the source path, and `--` ends option parsing.

## Requirements

- Rust and Cargo
- LLVM 23
- A C linker available as `cc`

## Building

Build with:

```sh
cargo build --release
```

The release compiler is written to `target/release/vacac`.

## Compiling a program

Compile a Vaca source file:

```sh
target/release/vacac examples/factorial.vca -o factorial
./factorial
```

The object file is retained next to the executable. If `-o` is omitted, the executable is `a.out`; the object file is derived from the executable path unless `--emit-obj` is given (`-o bar` writes `bar.o`). Use `--emit-obj path.o` to choose the object-file path explicitly.

## Compiling and running

For a quick compile-and-run without leaving build artifacts:

```sh
target/release/vacac examples/factorial.vca --run
```

`--run` builds in a temporary directory and runs the result. Linker inputs are supported in this mode.

## Checking a program

Run the lexer, parser, and semantic checker without producing build artifacts:

```sh
target/release/vacac examples/factorial.vca --check
```

Check mode also builds and verifies the LLVM module, but writes no object or executable.

## Emitting LLVM IR

Write the verified LLVM IR module to a text file without linking an executable:

```sh
target/release/vacac examples/factorial.vca --emit-llvm factorial.ll
```

## Output modes and common options

`--check` and `--emit-llvm` are exclusive output modes and cannot be combined with `-o` or `--emit-obj`. `--run` is also exclusive with other output modes and output paths.

Use `--quiet` (or `-q`) to suppress success messages, and `--json` to emit errors as JSON for tools. `--help` lists all options and `--version` prints the compiler version.

## Linking native code

Vaca can call scalar C functions using declarations such as `extern "C" fn labs(value: i64) -> i64;`. See [Scalar C FFI](c-ffi.md) for supported types and limitations.

For native object files or libraries, pass repeatable linker inputs while building or running:

```sh
vacac app.vca --link-object support.o --link-search ./lib --link-lib support -o app
vacac app.vca --run --link-lib m
```

`--link-object <path>`, `--link-search <directory>`, and `--link-lib <name>` are available for executable and `--run` modes only; they are not used by `--check` or `--emit-llvm`.

## Output path safety

Every output path is compared against the compilation inputs before anything is written. A run is refused when the executable, the object file, or the `--emit-llvm` path would overwrite the root source, any module source, or a `--link-object` input, and when the executable and object paths resolve to the same file. This keeps `vacac main.vca -o main.vca` and `vacac main.vca --link-object helper.o -o helper` from destroying the inputs (`helper.o` is the derived object path there).
