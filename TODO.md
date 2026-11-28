# TODO

## Language features

* [x] Define and test signed `i64` overflow semantics: addition, subtraction, multiplication, and negation trap at runtime; the minimum literal remains valid.
* [x] Scalar `f32` / `f64` support, including suffixed literals, arithmetic, comparisons, functions, and printing.
* [x] Floating-point values in arrays, vectors, structs, enums, and match patterns.
* [x] User-defined generics / parametric polymorphism for functions, structs, and enums (monomorphized functions, structs, and enums; inferred arguments, full explicit turbofish arguments, and inline trait bounds).
* [x] Non-capturing lambdas and first-class function values (locals, parameters, returns, indirect calls, and function types).
* [x] Capturing closures with immutable by-value environments retained until process exit.
* [x] Function values and captured closures in fixed arrays and vectors.
* [x] Global and module-level constants or variables (compile-time initializers, static storage, qualified module access).
* [x] Module imports and aliases with `use path::name [as alias];`, including collision, privacy, and parser-recovery checks across functions, globals, types, traits, and variants.
* [x] Destructuring patterns for structs, arrays, and tuples; tuple types and values.
* [x] Make `unit` a zero-sized value with a `()` literal, locals, generic arguments, and enum payloads, enabling `Result<unit, E>` and `?` for fallible side-effect-only operations.
* [x] Typed raw pointers (`*const T` / `*mut T`) with explicit raw address-taking and dereference; restricted lexical `&mut T` borrows.
* [x] Bounded named lifetimes and safe-reference borrowing (shared references can be returned from matching shared-reference parameters and propagated through direct named-function calls, local aliases, reborrows, and `if`/`match` joins; local loans use conservative lexical place tracking; references remain unavailable in aggregates, globals, closures, and function-value returns; NLL is deferred).
* [x] Scalar external C FFI declarations and calls (`i64`, `f32`, `f64`, and `unit` returns), with native linker inputs for executables and `--run`.
* [x] Interfaces / traits / protocols.
* [x] Default function bodies in traits, including inherited body specialization, visibility, and static-dispatch tests.
* [x] Generic traits and generic trait functions, including inference, coherence, visibility, and specialization-limit coverage.
* [x] Persistent mutable owned closure captures, with closure aliases sharing environment state and separately created closures remaining independent.
* [x] Unit-producing statement matches that skip result PHI construction.
* [x] Generalized aggregate and nested matching with multiple dispatch arms, nested variant and scalar patterns, and exhaustive/unreachable-arm analysis.
* [x] Mutable-reference reborrowing and named-lifetime returns, preserving overlap and local-escape checks.
* [ ] Non-lexical lifetimes that end shared and mutable loans at last use across branches and loops.
* [ ] ~~Object-oriented features such as methods, classes, inheritance, and polymorphism.~~
* [ ] Macros and compile-time metaprogramming.

## Runtime and libraries

* [x] Practical raw-pointer and C-string interoperability: `str` and typed raw pointers in extern signatures, typed null pointers and pointer equality, and raw address-taking for aggregate and vector subplaces, with documented ABI and string-lifetime rules.
* [ ] Automatic memory management (reference counting or garbage collection); currently heap-backed vectors and concatenated strings are retained until process exit.
* [ ] Standard library beyond the current built-ins (`print_i64`, `print_f32`, `print_f64`, `print`, `println`, `read_i64`, `len`, `vec_new`, and `vec_push`).
* [x] Recoverable error handling with the built-in generic `Result<T, E>` prelude enum and postfix `?` propagation; runtime faults still terminate the process.
* [ ] Async/await and concurrency (threads / multithreading).

## Compiler

* [ ] Cross-platform support (macOS, Windows).
* [x] User-selectable emitted-code optimization levels, with an LLVM module-pass pipeline and matching target-machine settings for executable, `--run`, and `--emit-llvm` modes.

## Diagnostic Engine

* [x] Carry structured diagnostics through code generation and the CLI instead of embedding source locations in error strings and parsing them back.
* [x] Preserve source-file and module identity through codegen so backend errors point to the file containing the relevant declaration.
* [x] Add source spans to entry-point signature validation and other user-facing errors that have a useful location.
* [x] Make human-readable, `Display`, and JSON diagnostic output consistent; define and test the JSON output contract and escaping.
* [x] Align source underlines for tabs and Unicode scalar-value columns, with a four-column tab stop and regression tests.
* [x] Expand diagnostic tests across lexer, parser, semantic analysis, modules, and codegen to cover locations, excerpts, underlines, and JSON output.
* [x] Recover from malformed top-level declarations and report multiple parser diagnostics across declarations and loaded modules; malformed declarations are skipped and prevent later compiler phases.
* [x] Recover within function bodies and match arms to report multiple syntax errors while preserving later statements, arms, and declarations where possible.
* [ ] Rich Diagnostic with hints/suggestions.

## Future

* [ ] Consider a package manager and reusable library distribution once the standard library and module system mature.
* [ ] Consider a self-hosting compiler written in Vaca, although this is not strictly necessary.
* [ ] Consider a linter and a formatter.
