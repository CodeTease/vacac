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

For a quick compile-and-run without leaving build artifacts:

```sh
target/release/apexc examples/factorial.apx --run
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
with `-o` or `--emit-obj`. `--run` is also exclusive with other output modes and
output paths. Check mode also builds and verifies the LLVM module, but writes no
object or executable. Use `--quiet` (or `-q`) to suppress success messages, and
`--json` to emit errors as JSON for tools. `--help` lists all options and
`--version` prints the compiler version.

## Examples

The [`examples/`](examples/) directory contains complete programs that can be
checked or compiled with `apexc`. For instance:

```sh
target/release/apexc examples/enum_match.apx --check
target/release/apexc examples/enum_match.apx -o enum_match
./enum_match
```

Some examples to explore:

- [`if_expression.apx`](examples/if_expression.apx) — choose a value with an
  expression conditional.
- [`enum_match.apx`](examples/enum_match.apx) — construct enum variants and
  handle each one with an exhaustive `match`.
- [`loop_control.apx`](examples/loop_control.apx) — use `continue` and `break`
  in a range loop.
- [`read_number.apx`](examples/read_number.apx) — read and classify an integer
  from standard input.
- [`array_stats.apx`](examples/array_stats.apx) — iterate over a fixed-size
  array.
- [`array_iteration.apx`](examples/array_iteration.apx) — use read-only array
  iteration to calculate a sum and maximum.
- [`struct_inventory.apx`](examples/struct_inventory.apx) — define, construct,
  pass, and read named structs.
- [`const_and_struct_fields.apx`](examples/const_and_struct_fields.apx) — use an
  immutable `const` value and update fields of a mutable local struct.

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```apex
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Fixed-size arrays

Fixed-size arrays contain `i64`, `bool`, `str`, values of a named struct or enum type, or other fixed-size arrays. Array literals infer their element type and length from their contents; annotations use `[element_type; length]`:

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

Arrays are copied as values when bound, passed, or returned. Arrays of structs and enums can be indexed, assigned, and iterated like primitive arrays; enum elements can be inspected with `match`, and struct elements support field access and assignment. Nested arrays are supported to any syntactic depth, including within struct fields, parameters, and return values. Zero-length arrays such as `[i64; 0]`, `[User; 0]`, or `[[i64; 2]; 0]` are supported; their empty literal must have an explicit array type annotation because no element type can be inferred. Indexes are signed `i64`; negative indexes and indexes greater than or equal to the array length terminate the program with status 1, so indexing a zero-length array always exits with status 1. `len(array)` returns the outer fixed length as an `i64`. Equality and inequality compare arrays element by element, recursively comparing nested aggregates and string contents; dynamically sized arrays are not supported.

Arrays may also be traversed directly with a read-only element binding:

```apex
for value in values {
    print_i64(value);
}
```

The array expression is evaluated once and copied before iteration. The binding is immutable and scoped to the loop body; `break` and `continue` work as they do in range loops. Iteration over a zero-length array executes the body zero times.

## Named records

Named records (structs) have top-level declarations and named fields of primitive, fixed-array, enum, or struct type:

```apex
struct User {
    id: i64,
    active: bool,
    name: str,
}

fn lookup(id: i64) -> User {
    User { id: id, active: true, name: "Ada" }
}
```

Struct values can be bound, passed to functions, and returned. A literal must initialize every field exactly once; its fields may appear in any order. Fields can be read with `user.name` and updated on mutable local bindings with `user.name = "Grace";`. Nested fields and array elements in fields may also be read or updated, for example `account.owner.name` or `account.scores[0]`. Struct fields may use `i64`, `bool`, `str`, recursively nested fixed-size arrays of primitive, enum, or struct values, an enum, or another struct. `unit` is not a valid struct-field type, and recursive by-value struct layouts are rejected. Enum-valued fields can be constructed, passed and returned as part of the struct, and inspected with `match`. Struct equality and inequality compare every field recursively, using string contents for `str` fields.

## Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or an ordered tuple of one or more primitive payloads (`i64`, `bool`, or `str`):

```apex
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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. `match` is an expression. Variant patterns use `EnumName::Variant`; payload variants bind or ignore each field independently, for example `Event::Record(id, _, label)`. The number and types of supplied payload values and pattern fields must match the declaration. A final `_` wildcard arm may handle all remaining variants; it is optional when every variant is otherwise covered. Wildcards must be unique and last, and duplicate or unreachable arms are rejected. Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same non-`unit` type. Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents. Payloads remain restricted to primitive `i64`, `bool`, and `str` fields; nested enums, arrays, and structs are not supported as payloads.

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

Value types are `i64`, `bool`, `str`, named enums, named structs, and recursively nested fixed-size arrays `[element_type; length]`; functions may also return `unit`. Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements but not bound or used as values. Unit parameters are not supported. Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. A `let` type annotation is optional and inferred from its initializer. `let` bindings are mutable; `const` creates an immutable binding. Assignment uses `name = expression;`, array elements may be assigned with `array[index] = expression;`, and fields of mutable local structs may be updated with `name.field = expression;`. Immutable bindings cannot be reassigned or mutated through an array element or struct field. `if`/`else`, `while`, and `for` loops are statements. A range loop such as `for i in 0..6 { ... }` visits `0` through `5`; ranges are half-open, both bounds are evaluated once, and the loop variable is immutable and scoped to the loop. An array loop such as `for value in values { ... }` visits each copied element once, and its read-only binding is scoped to the loop. Functions returning a value must use `return expression;` or a final expression. All functions are top-level and have a declared return type. Operators include arithmetic (`+ - * / %`), signed comparisons, equality, boolean negation, and short-circuiting `&&` / `||`. Division and remainder by zero, as well as `i64::MIN / -1` and `i64::MIN % -1`, terminate the program with exit status 1. Line (`//`) and block (`/* ... */`) comments are supported. `print_i64(i64)` writes one decimal value and a newline to stdout; `read_i64() -> i64` reads one signed decimal integer token from stdin.

String literals are written `"..."` and have type `str`. They support the escapes `\n`, `\t`, `\r`, `\\`, and `\"`, may contain UTF-8 text, and must end on the line they start. Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`. Arithmetic, ordering comparisons, and concatenation are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime. `print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline; `print("x = "); print_i64(x);` prints a labelled value.

The required Apex entry point is `fn main() -> i64`. The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Apex result as the process exit status, following the host OS convention. `read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. A malformed token, end-of-file before a number, or a value outside the `i64` range terminates the program with status 1. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.
