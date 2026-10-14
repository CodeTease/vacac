# TODO

## Language features

* [x] Define and test signed `i64` overflow semantics: addition, subtraction, multiplication, and negation trap at runtime; the minimum literal remains valid.
* [x] Scalar `f32` / `f64` support, including suffixed literals, arithmetic, comparisons, functions, and printing.
* [x] Floating-point values in arrays, vectors, structs, enums, and match patterns.
* [x] User-defined generics / parametric polymorphism for functions and structs (monomorphized functions and structs; inferred arguments and full explicit turbofish arguments, without trait bounds or generic enums).
* [x] Non-capturing lambdas and first-class function values (locals, parameters, returns, indirect calls, and function types).
* [x] Capturing closures with immutable by-value environments retained until process exit.
* [x] Function values and captured closures in fixed arrays and vectors.
* [x] Global and module-level constants or variables (compile-time initializers, static storage, qualified module access).
* [x] Destructuring patterns for structs, arrays, and tuples; tuple types and values.
* [x] Typed raw pointers (`*const T` / `*mut T`) with explicit raw address-taking and dereference; restricted lexical `&mut T` borrows.
* [ ] Lifetimes and broader safe-reference borrowing (current safe references are local-only, non-escaping, and mutable borrows use a conservative lexical scope rule).
* [x] Scalar external C FFI declarations and calls (`i64`, `f32`, `f64`, and `unit` returns), with native linker inputs for executables and `--run`.
* [ ] Interfaces / traits / protocols.
* [ ] Object-oriented features such as methods, classes, inheritance, and polymorphism.
* [ ] Macros and compile-time metaprogramming.

## Runtime and libraries

* [ ] Automatic memory management (reference counting or garbage collection); currently heap-backed vectors and concatenated strings are retained until process exit.
* [ ] Standard library beyond the current built-ins (`print_i64`, `print_f32`, `print_f64`, `print`, `println`, `read_i64`, `len`, `vec_new`, and `vec_push`).
* [x] Recoverable error handling with the built-in generic `Result<T, E>` prelude enum and postfix `?` propagation; runtime faults still terminate the process.
* [ ] Async/await and concurrency (threads / multithreading).

## Diagnostic Engine

* [ ] Carry structured diagnostics through code generation and the CLI instead of embedding source locations in error strings and parsing them back.
* [ ] Preserve source-file and module identity through codegen so backend errors point to the file containing the relevant declaration.
* [ ] Add source spans to entry-point validation and other user-facing errors that currently lack a location when a useful location is available.
* [ ] Make human-readable, `Display`, and JSON diagnostic output consistent; define and test the JSON output contract and escaping.
* [ ] Improve source underline alignment for tabs and Unicode, define the column-counting convention, and add regression tests for it.
* [ ] Expand diagnostic tests across lexer, parser, semantic analysis, modules, and codegen to cover locations, excerpts, underlines, and JSON output.
* [ ] Error recovery instead of fail-fast.
* [ ] Rich Diagnostic with hints/suggestions.

## Future

* [ ] Consider a package manager and reusable library distribution once the standard library and module system mature.
* [ ] Consider a self-hosting compiler written in Vaca, although this is not strictly necessary.
* [ ] A Linter and a formatter.
