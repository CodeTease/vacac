# Runtime-sized vectors

`Vec<T>` is a heap-backed, runtime-sized sequence with `i64`, `f32`, `f64`, `bool`, `str`, named enum/struct, fixed-array, and nested-vector element types (but not `unit`). Create an empty vector with a type annotation and append values with `vec_push`:

```vaca
fn main() -> i64 {
    let values: Vec<i64> = vec_new();
    vec_push(values, 10);
    vec_push(values, 20);
    values[1] = 42;
    print_i64(len(values));
    print_i64(values[1]);
    0
}
```

## Creation and mutation

`vec_new()` requires an expected `Vec<T>` type (usually a binding annotation), while `vec_push(vector, value)` appends one type-checked element and returns `unit`.

Vector handles are copied shallowly: assigning, passing, or returning a vector aliases the same contents, and vector equality/inequality compares handle identity. `const` makes only the handle binding immutable; appending and indexed element mutation through that handle are permitted, but rebinding it is not.

## Indexing

Indexing uses signed `i64` indexes and invalid indexes exit with status 1.

## Iteration

Vector iteration evaluates the handle once and captures its length at loop entry, so elements appended during the loop are not visited; element bindings remain immutable. Vector allocations persist until process exit.
