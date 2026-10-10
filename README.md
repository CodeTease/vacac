# Apex compiler

`apexc` is a handwritten, statically compiled compiler for Apex v0.1. It parses and checks source in Rust, generates native LLVM object code with Inkwell, then invokes `cc` to link an ELF executable.

## Requirements

- Rust and Cargo
- LLVM 22 (including `llvm-config`)
- A C linker available as `cc`

Build with:

```sh
cargo build --release
```

Compile an Apex source file (the object file is retained next to the executable):

```sh
target/release/apexc examples/factorial.apx -o factorial
./factorial
```

Use `--emit-obj path.o` to choose the object-file path explicitly.

Run the lexer, parser, and semantic checker without producing build artifacts:

```sh
target/release/apexc examples/factorial.apx --check
```

Write the verified LLVM IR module to a text file (without linking an executable):

```sh
target/release/apexc examples/factorial.apx --emit-llvm factorial.ll
```

`--check` and `--emit-llvm` are exclusive output modes and cannot be combined
with `-o` or `--emit-obj`. Check mode also builds and verifies the LLVM module,
but writes no object or executable.

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```apex
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Fixed-size arrays

Fixed-size, one-dimensional arrays contain `i64`, `bool`, or `str` elements. Array literals infer their element type and length from their contents; annotations use `[element_type; length]`:

```apex
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

Arrays are copied as values when bound, passed, or returned. Indexes are signed `i64`; negative indexes and indexes greater than or equal to the array length terminate the program with status 1. `len(array)` returns the fixed length as an `i64`. Array equality, nested arrays, zero-length arrays, and dynamically sized arrays are not supported.

## Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or one primitive payload (`i64`, `bool`, or `str`):

```apex
enum MaybeInt {
    None,
    Some(i64),
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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression)`. `match` is an expression. Variant patterns use `EnumName::Variant`; payload variants bind their payload as `EnumName::Variant(name)` or ignore it as `EnumName::Variant(_)`. A final `_` wildcard arm may handle all remaining variants; it is optional when every variant is otherwise covered. Wildcards must be unique and last, and duplicate or unreachable arms are rejected. Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same non-`unit` type. Enum values may be bound, assigned, passed to and returned from functions, but cannot be compared or used as array elements. Nested enums, multiple payload fields, array payloads, and matching across enums are not supported.

## Apex v0.1 syntax

```text
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

Value types are `i64`, `bool`, `str`, named enums, and fixed-size arrays `[i64; length]`, `[bool; length]`, or `[str; length]`; functions may also return `unit`. Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements but not bound or used as values. Unit parameters are not supported. Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. A `let` type annotation is optional and inferred from its initializer. Bindings are mutable; assignment uses `name = expression;`, and array elements may be assigned with `array[index] = expression;`. `if`/`else`, `while`, and integer range `for` loops are statements. A loop such as `for i in 0..6 { ... }` visits `0` through `5`; ranges are half-open, both bounds are evaluated once, and the loop variable is immutable and scoped to the loop. Functions returning a value must use `return expression;` or a final expression. All functions are top-level and have a declared return type. Operators include arithmetic (`+ - * / %`), signed comparisons, equality, boolean negation, and short-circuiting `&&` / `||`. Division and remainder by zero, as well as `i64::MIN / -1` and `i64::MIN % -1`, terminate the program with exit status 1. Line (`//`) and block (`/* ... */`) comments are supported. `print_i64(i64)` writes one decimal value and a newline to stdout; `read_i64() -> i64` reads one signed decimal integer token from stdin.

String literals are written `"..."` and have type `str`. They support the escapes `\n`, `\t`, `\r`, `\\`, and `\"`, may contain UTF-8 text, and must end on the line they start. Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`. Arithmetic, ordering comparisons, and concatenation are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime. `print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline; `print("x = "); print_i64(x);` prints a labelled value.

The required Apex entry point is `fn main() -> i64`. The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Apex result as the process exit status, following the host OS convention. `read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. A malformed token, end-of-file before a number, or a value outside the `i64` range terminates the program with status 1. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.
