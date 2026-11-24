# Tuples and destructuring

## Tuple types and values

Tuple types and values use comma-separated parentheses: `(i64, bool)` and `(42, true)`. A trailing comma distinguishes a one-element tuple `(value,)` from a grouped expression `(value)`. Tuples can be nested, passed to and returned from functions, stored in structs, arrays and vectors, and compared recursively for equality.

## Destructuring bindings

`let` and `const` bindings can destructure tuples, fixed arrays, and structs with recursive patterns. Use `_` to ignore a tuple item or an individual fixed-array position. Array patterns list every position. Struct patterns name fields; add `..` to ignore omitted fields:

```vaca
struct User { id: i64, active: bool, name: str }

fn describe(user: User, values: [(i64, bool); 2]) -> i64 {
    let User { id, active, .. } = user;
    let [(first, _), (second, enabled)] = values;
    if active && enabled { id + first + second } else { 0 }
}
```

## Match patterns

Match arms accept the same tuple, array and struct patterns, and enum payload bindings may themselves be destructured. Aggregate-value matches currently use one destructuring arm; enum matches remain exhaustive. A bare identifier pattern binds the whole scalar or enum value as a catch-all arm.

Generic structs and functions are documented in [Generics](generics.md).
