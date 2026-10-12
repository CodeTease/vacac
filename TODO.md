# TODO

## Language features

* [x] Define and test signed `i64` overflow semantics: addition, subtraction, multiplication, and negation trap at runtime; the minimum literal remains valid.
* [ ] `f32` support and floating-point values in arrays, vectors, structs, enums, and match patterns.
* [ ] User-defined generics / parametric polymorphism for functions and structs (beyond the built-in `Vec<T>` type).
* [ ] Closures, lambda expressions, and first-class function values.
* [ ] Global and module-level constants or variables.
* [ ] Destructuring patterns for structs, arrays, and tuples; tuple types and values.
* [ ] Manual pointer and reference types (for example, `&T` and `*mut T`).
* [ ] External C FFI declarations and calls.
* [ ] Interfaces / traits / protocols.
* [ ] Object-oriented features such as methods, classes, inheritance, and polymorphism.
* [ ] Macros and compile-time metaprogramming.

## Runtime and libraries

* [ ] Automatic memory management (reference counting or garbage collection); currently heap-backed vectors and concatenated strings are retained until process exit.
* [ ] Standard library beyond the current built-ins (`print_i64`, `print_f64`, `print`, `println`, `read_i64`, `len`, `vec_new`, and `vec_push`).
* [ ] Recoverable error handling and propagation (for example, `Result`/`?` or try/catch); current runtime failures terminate the process.
* [ ] Async/await and concurrency (threads / multithreading).

## Future

* [ ] Consider a package manager and reusable library distribution once the standard library and module system mature.
