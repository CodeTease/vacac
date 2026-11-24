# Conditionals, matching, and loops

Enum matching and the general exhaustive-pattern rules are covered in [Enums and exhaustive matching](enums.md).

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```vaca
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Scalar matching

`match` also accepts `bool` and `i64` values. Boolean matches must cover both `true` and `false`, or include a wildcard; integer matches require a wildcard because the integer domain is open-ended. Duplicate and unreachable patterns are rejected. Integer patterns are signed decimal literals, including `-9223372036854775808`:

```vaca
let label = match code {
    0 => { "success" },
    -1 => { "failure" },
    _ => { "unknown" },
};
```

## Value-producing loops

An unconditional `loop` expression produces the value carried by its `break expression;` statement. All reachable value breaks must have the same non-`unit` type. `continue;` restarts the innermost loop.

Ordinary `while` and `for` loops still use bare `break;` and do not produce values:

```vaca
let first_even = loop {
    candidate = candidate + 1;
    if candidate % 2 == 0 {
        break candidate;
    } else {
        continue;
    }
};
```

Fixed-size arrays and vectors can also be traversed directly with `for value in collection { ... }`; see [Fixed-size arrays](arrays.md#iteration) and [Runtime-sized vectors](vectors.md#iteration).
