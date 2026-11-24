# Named records

Named records (structs) have top-level declarations and named fields of primitive (including `f32` and `f64`), fixed-array, vector, enum, or struct type:

```vaca
struct User {
    id: i64,
    active: bool,
    name: str,
}

fn lookup(id: i64) -> User {
    User { id: id, active: true, name: "Ada" }
}
```

Struct values can be bound, passed to functions, and returned. A literal must initialize every field exactly once; its fields may appear in any order. Fields can be read with `user.name` and updated on mutable local bindings with `user.name = "Grace";`. Nested fields and array elements in fields may also be read or updated, for example `account.owner.name` or `account.scores[0]`.

Struct fields may use `i64`, `f32`, `f64`, `bool`, `str`, tuples, recursively nested fixed-size arrays and vectors of non-`unit` values, an enum, or another struct. `unit` is not a valid struct-field type, and recursive by-value struct layouts are rejected.

Enum-valued fields can be constructed, passed and returned as part of the struct, and inspected with `match`. Struct equality and inequality compare every field recursively, using IEEE equality for floats, string contents for `str` fields, and handle identity for vector fields.

Generic structs are documented in [Generics](generics.md).
