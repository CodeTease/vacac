# Scalar C FFI

Vaca can call scalar C functions using declarations such as:

```vaca
extern "C" fn labs(value: i64) -> i64;
```

The C symbol is the declared function name. Parameters may be `i64`, `f32`, or `f64`; return values may also be `unit`. These types correspond to `int64_t`, `float`, `double`, and `void` in C.

Pointers, strings, booleans, aggregates, and variadic functions are not part of this initial FFI.

## Linking native code

For native object files or libraries, pass repeatable linker inputs while building or running:

```sh
vacac app.vca --link-object support.o --link-search ./lib --link-lib support -o app
vacac app.vca --run --link-lib m
```

`--link-object <path>`, `--link-search <directory>`, and `--link-lib <name>` are available for executable and `--run` modes only; they are not used by `--check` or `--emit-llvm`. See [Using the compiler](usage.md#linking-native-code) for the full CLI description.

[`examples/c_ffi.vca`](../examples/c_ffi.vca) calls the C standard library's `labs()` through a scalar `extern "C"` declaration.
