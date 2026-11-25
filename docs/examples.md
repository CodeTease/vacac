# Examples

The [`examples/`](../examples/) directory contains complete programs that can be checked or compiled with `vacac`. For instance:

```sh
target/release/vacac examples/enum_match.vca --check
target/release/vacac examples/enum_match.vca -o enum_match
./enum_match
```

Some examples to explore:

- [`if_expression.vca`](../examples/if_expression.vca) — choose a value with an expression conditional.
- [`enum_match.vca`](../examples/enum_match.vca) — construct enum variants and handle each one with an exhaustive `match`.
- [`result_propagation.vca`](../examples/result_propagation.vca) — return recoverable errors with `Result<T, E>` and propagate them with `?`.
- [`loop_control.vca`](../examples/loop_control.vca) — use `continue` and `break` in a range loop.
- [`read_number.vca`](../examples/read_number.vca) — read and classify an integer from standard input.
- [`c_ffi.vca`](../examples/c_ffi.vca) — call the C standard library's `labs()` through a scalar `extern "C"` declaration.
- [`array_stats.vca`](../examples/array_stats.vca) — iterate over a fixed-size array.
- [`array_iteration.vca`](../examples/array_iteration.vca) — use read-only array iteration to calculate a sum and maximum.
- [`struct_inventory.vca`](../examples/struct_inventory.vca) — define, construct, pass, and read named structs.
- [`const_and_struct_fields.vca`](../examples/const_and_struct_fields.vca) — use an immutable `const` value and update fields of a mutable local struct.
- [`traits.vca`](../examples/traits.vca) — define traits with default functions, implement them for primitives and generic structs, and dispatch through a trait bound.
- [`stateful_closures.vca`](../examples/stateful_closures.vca) — keep mutable captured state in a closure environment shared by its aliases.
- [`pointers.rs`](../tests/compiler/pointers.rs) — checked mutable references and explicitly raw pointer reads/writes.
- [`modules/`](../examples/modules/) — split a program across nested source files and use qualified public structs, enums, and functions, plus `use` imports and aliases.

The modules example is described in more detail in [Modules and imports](language/modules.md#hierarchical-modules). To check and run every example in the repository, see [Testing](testing.md#checking-and-running-examples).
