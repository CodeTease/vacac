# Vaca v0.7 syntax

In addition to the by-value types described below, the compiler supports `&T`, `&mut T`, `*const T`, and `*mut T` as documented in [References and raw pointers](references-and-pointers.md).

## Program structure

A program is a collection of top-level declarations. Non-`unit` functions return via `return expr;` or a final expression, and all paths must produce a value.

```vaca
fn factorial(n: i64) -> i64 {
    if n <= 1 {
        return 1;
    } else {
        return n * factorial(n - 1);
    }
}

fn report(value: i64) -> unit {
    print("result = ");
    print_i64(value);
}

fn main() -> i64 {
    let result: i64 = factorial(10);
    report(result);
    0
}
```

## Value types

Value types are `i64`, `f32`, `f64`, `bool`, `str`, named enums, named structs, recursively nested fixed-size arrays `[element_type; length]`, and heap-backed vectors `Vec<element_type>`; functions may also return `unit`. Traits are top-level declarations and are not value types; generic parameters may use inline trait bounds.

Bindings, function parameters and results, unary negation, `+`, `-`, `*`, `/`, `%`, comparisons, and aggregate elements/payloads are supported for both floating-point types. Floating-point values use IEEE 754 behavior, including infinities, NaN, and unordered comparisons. Integers, `f32`, and `f64` are never converted implicitly.

## Floating-point literals

Floating-point literals use decimal notation with at least one digit before the decimal point; a fractional part and/or exponent are optional (for example, `3.125`, `2e-3`, `1.0e+4`). Unsuffixed floating-point literals have type `f64`; append `f32` to a numeric literal (for example, `3.125f32`, `2e-3f32`) to create a rounded `f32` literal.

Float `match` patterns use IEEE equality, treat signed zeros as equal for duplicate detection, and require a wildcard arm. Non-finite or out-of-range literal spellings are rejected; infinities and NaN can still result from arithmetic.

`print_f32(value)` prints a scalar `f32` followed by a newline using 9 significant digits; `print_f64(value)` writes the value followed by a newline using 17 significant digits.

## Integer literals and overflow

Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. Signed `i64` addition, subtraction, multiplication, and negation trap on overflow and terminate with status 1; the direct minimum-literal spelling remains valid.

## Unit

`unit` is a zero-sized value spelled `()` or `unit` as a type (`()` also works as a type). Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements, bound to locals, or returned. Unit values are valid generic arguments and enum payloads, so `Result<unit, E>` with postfix `?` supports fallible side-effect-only operations. Unit parameters, unit struct fields, unit tuple or array elements, unit vector elements, and unit globals are not supported.

## Bindings and assignment

A `let` type annotation is optional and inferred from its initializer. `let` bindings are mutable; `const` creates an immutable binding. Assignment uses `name = expression;`, array and vector elements may be assigned with `array[index] = expression;`, and fields of mutable local structs may be updated with `name.field = expression;`. Immutable bindings cannot be reassigned or mutated through an array element or struct field.

## Control flow

`if`/`else`, `while`, and `for` loops are statements. An `if` with an `else` can also produce a value, and `loop { ... }` can produce a value carried by `break expression;`. See [Conditionals, matching, and loops](control-flow.md).

## Strings

Strings are immutable and compare by content. Their operations and printing are documented in [Strings](strings.md).

## Entry point

The required Vaca entry point, process exit status, and input behavior are documented in [Runtime and entry point](runtime.md).
