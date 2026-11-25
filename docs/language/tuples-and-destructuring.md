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

Match arms accept the same tuple, array, and struct patterns, and enum payload bindings may themselves be destructured. An aggregate value can be matched with several arms, and any position may hold a nested enum variant or scalar pattern:

```vaca
enum Reading { Sample(bool, i64), Missing }
struct Packet { reading: Reading, flags: [bool; 2] }

fn classify(packet: Packet) -> i64 {
    match packet {
        Packet { reading: Reading::Sample(true, 0), flags: [true, _] } => { 1 },
        Packet { reading: Reading::Sample(true, number), .. } => { number },
        Packet { reading: Reading::Sample(false, _), .. } => { 2 },
        Packet { reading: Reading::Missing, .. } => { 3 },
    }
}
```

The first matching arm runs, so later arms only need to cover values that earlier arms left open. A bare identifier pattern binds the whole value as a catch-all arm. Coverage under a `..` or `_` still counts, so the enum match above is exhaustive without a final wildcard. See [Enums and exhaustive matching](enums.md#construction-and-matching) for the reachability rules.

Generic structs and functions are documented in [Generics](generics.md).
