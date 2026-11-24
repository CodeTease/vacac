# Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or an ordered tuple of one or more value payloads (primitives including `f32` and `f64`, `unit`, named structs and enums, fixed-size arrays, or vectors):

```vaca
enum MaybeInt {
    None,
    Some(i64),
}

enum Event {
    Record(i64, bool, str),
}

fn value_or_default(value: MaybeInt) -> i64 {
    match value {
        MaybeInt::None => { 0 },
        MaybeInt::Some(number) => { number },
    }
}

fn main() -> i64 {
    value_or_default(MaybeInt::Some(7))
}
```

## Construction and matching

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. Match patterns use `EnumName::Variant`; payload variants bind or ignore every field, for example `Event::Record(id, _, label)`, and tuple/array/struct payloads can be recursively destructured. The payload count and types must match the declaration.

Scalar patterns may be booleans, signed integers, or floating-point literals (use the `f32` suffix for an `f32` pattern). Float pattern equality follows IEEE `==`: `+0.0` and `-0.0` are equal and duplicate, while NaN matches no float literal. Floating-point matches always require a `_` wildcard because their pattern set is not exhaustive.

For booleans, integers, and enums a final `_` wildcard or identifier binding may handle remaining values; it is optional when every possible value or variant is otherwise covered. Wildcards and identifier catch-alls must be unique and last, and duplicate or unreachable arms are rejected.

Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same type. When every arm produces `unit`, the match is usable as an expression statement (`match ... { ... };`) and is lowered without a result PHI; a `unit` match result can also be bound.

## Values and equality

Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents and aggregate payloads compare recursively.

Payloads may be any value type, including `unit`, tuples, arrays, structs, and other enums. Recursive by-value aggregate layouts are rejected.

For tuple, array, and struct destructuring patterns in `match` arms, see [Tuples and destructuring](tuples-and-destructuring.md). Scalar `bool` and `i64` matching is covered in [Conditionals, matching, and loops](control-flow.md#scalar-matching).
