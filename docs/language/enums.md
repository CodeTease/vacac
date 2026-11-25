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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. Match patterns use `EnumName::Variant`; payload variants bind or ignore every field, for example `Event::Record(id, _, label)`. Payload patterns can themselves be variants, tuples, arrays, or structs, so a test on a nested value appears anywhere a value of that type appears. The payload count and types must match the declaration at every level.

Arms are tried in order and only the first matching arm runs. A pattern may overlap earlier arms as long as some value still reaches it; an arm whose values are all covered by earlier arms is rejected as unreachable, and repeating an earlier pattern is reported as a duplicate.

Exhaustiveness is computed from the complete pattern, not from variant tags alone: `Reading::Sample(true, _)` and `Reading::Sample(false, _)` together cover every sample, while either arm alone does not. Boolean and enum positions are exhaustive once every constructor is covered. Tuple, array, and struct positions are exhaustive once every combination of their elements is covered. Integer and floating-point positions always need a catch-all because their domains are open-ended. A whole-match `_` or identifier binding is a catch-all, so it must stay unique and last. A wildcard or binding nested inside a larger pattern covers only its position, subject to the rest of the pattern, so it may appear in an earlier arm such as `Reading::Sample(false, _)` in the example below.

Scalar patterns may be booleans, signed integers, or floating-point literals (use the `f32` suffix for an `f32` pattern), including inside aggregate patterns. Float pattern equality follows IEEE `==`: `+0.0` and `-0.0` are equal and duplicate, while NaN matches no float literal.

```vaca
enum Reading { Sample(bool, i64), Missing }

fn classify(value: Reading) -> i64 {
    match value {
        Reading::Sample(true, 0) => { 0 },
        Reading::Sample(true, number) => { number },
        Reading::Sample(false, _) => { -1 },
        Reading::Missing => { -2 },
    }
}
```

Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same type. When every arm produces `unit`, the match is usable as an expression statement (`match ... { ... };`) and is lowered without a result PHI; a `unit` match result can also be bound.

## Values and equality

Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents and aggregate payloads compare recursively.

Payloads may be any value type, including `unit`, tuples, arrays, structs, and other enums. Recursive by-value aggregate layouts are rejected.

For tuple, array, and struct destructuring patterns in `match` arms, see [Tuples and destructuring](tuples-and-destructuring.md). Scalar `bool` and `i64` matching is covered in [Conditionals, matching, and loops](control-flow.md#scalar-matching).
