# Fixed-size arrays

## Array types and literals

Fixed-size arrays contain `i64`, `f32`, `f64`, `bool`, `str`, values of a named struct or enum type, vectors, or other fixed-size arrays. Array literals infer their element type and length from their contents; annotations use `[element_type; length]`:

```vaca
fn first(values: [i64; 3]) -> i64 {
    values[0]
}

fn main() -> i64 {
    let values = [10, 20, 30];
    values[1] = 42;
    for i in 0..len(values) {
        print_i64(values[i]);
    }
    first(values) - 10
}
```

Arrays are copied as values when bound, passed, or returned. Arrays of structs, enums, and vectors can be indexed, assigned, and iterated like primitive arrays; enum elements can be inspected with `match`, and struct elements support field access and assignment. Nested arrays are supported to any syntactic depth, including within struct fields, parameters, and return values.

Zero-length arrays such as `[i64; 0]`, `[User; 0]`, or `[[i64; 2]; 0]` are supported; their empty literal must have an explicit array type annotation because no element type can be inferred.

## Indexing and length

Indexes are signed `i64`; negative indexes and indexes greater than or equal to the array length terminate the program with status 1, so indexing a zero-length array always exits with status 1. `len(array)` returns the outer fixed length as an `i64`.

## Equality

Equality and inequality compare arrays element by element, recursively comparing nested aggregates and string contents; vector values inside arrays compare by handle identity.

## Iteration

Arrays may also be traversed directly with a read-only element binding:

```vaca
for value in values {
    print_i64(value);
}
```

The array expression is evaluated once and copied before iteration. The binding is immutable and scoped to the loop body; `break` and `continue` work as they do in range loops. Iteration over a zero-length array executes the body zero times.
