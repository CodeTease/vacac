# Vaca documentation

`vacac` is a handwritten, statically compiled compiler for Vaca. It parses and checks source in Rust, generates native LLVM object code with Inkwell, then invokes `cc` to link an ELF executable.

The repository [README](../README.md) is the short landing page. This index links to the user-facing guides in `docs/`.

## Using Vaca

- [Using the compiler](usage.md) — requirements, building, compilation modes, linker inputs, and output-path safety.
- [Diagnostics](diagnostics.md) — parser recovery, source locations, and JSON error fields.
- [Scalar C FFI](c-ffi.md) — declaring and calling scalar C functions, and linking native code.
- [Examples](examples.md) — complete programs to check, compile, and run.
- [Testing](testing.md) — Rust tests, the example runner, and the adversarial corpus.

## Language reference

- [Vaca syntax](language/syntax.md) — program structure, value types, literals, bindings, and control-flow overview.
- [Runtime and entry point](language/runtime.md) — `main`, process exit status, `read_i64()`, and fatal runtime errors.
- [Strings](language/strings.md) — immutable strings, concatenation, comparison, and printing.
- [Conditionals, matching, and loops](language/control-flow.md) — expression conditionals, scalar `match`, and value-producing `loop`.
- [Fixed-size arrays](language/arrays.md) — array types, indexing, iteration, copying, and equality.
- [Runtime-sized vectors](language/vectors.md) — `Vec<T>`, `vec_new`, `vec_push`, indexing, and iteration.
- [Named records](language/structs.md) — struct declarations, literals, field access, and equality.
- [Enums and exhaustive matching](language/enums.md) — variants, payloads, `match`, and reachability rules.
- [Tuples and destructuring](language/tuples-and-destructuring.md) — tuple values, binding patterns, and match patterns.
- [Generics](language/generics.md) — generic structs, enums, functions, inference, turbofish, and specialization.
- [Traits and static dispatch](language/traits.md) — trait declarations, default bodies, coherence, and monomorphized dispatch.
- [Function values and closures](language/functions-and-closures.md) — function types, lambdas, captures, and equality.
- [References and raw pointers](language/references-and-pointers.md) — shared and mutable references, named lifetimes, and raw pointers.
- [Recoverable errors](language/recoverable-errors.md) — `Result<T, E>` and the `?` operator.
- [Global constants and variables](language/globals.md) — module-level values and constant initializers.
- [Modules and imports](language/modules.md) — hierarchical modules, visibility, and `use` aliases.
