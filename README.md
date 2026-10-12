# Vaca compiler 🐄

`vacac` is a handwritten, statically compiled compiler for Vaca v0.1 🐮. It parses and checks source in Rust, generates native LLVM object code with Inkwell, then invokes `cc` to link an ELF executable.

A **CodeTease** project.

## Requirements

- Rust and Cargo
- LLVM 22 (including `llvm-config`)
- A C linker available as `cc`

Build with:

```sh
cargo build --release
```

Compile a Vaca source file (the object file is retained next to the executable):

```sh
target/release/vacac examples/factorial.vca -o factorial
./factorial
```

For a quick compile-and-run without leaving build artifacts:

```sh
target/release/vacac examples/factorial.vca --run
```

Use `--emit-obj path.o` to choose the object-file path explicitly.

Run the lexer, parser, and semantic checker without producing build artifacts:

```sh
target/release/vacac examples/factorial.vca --check
```

Write the verified LLVM IR module to a text file (without linking an executable):

```sh
target/release/vacac examples/factorial.vca --emit-llvm factorial.ll
```

`--check` and `--emit-llvm` are exclusive output modes and cannot be combined
with `-o` or `--emit-obj`. `--run` is also exclusive with other output modes and
output paths. Check mode also builds and verifies the LLVM module, but writes no
object or executable. Use `--quiet` (or `-q`) to suppress success messages, and
`--json` to emit errors as JSON for tools. `--help` lists all options and
`--version` prints the compiler version.

## Examples

The [`examples/`](examples/) directory contains complete programs that can be
checked or compiled with `vacac`. For instance:

```sh
target/release/vacac examples/enum_match.vca --check
target/release/vacac examples/enum_match.vca -o enum_match
./enum_match
```

Some examples to explore:

- [`if_expression.vca`](examples/if_expression.vca) — choose a value with an
  expression conditional.
- [`enum_match.vca`](examples/enum_match.vca) — construct enum variants and
  handle each one with an exhaustive `match`.
- [`loop_control.vca`](examples/loop_control.vca) — use `continue` and `break`
  in a range loop.
- [`read_number.vca`](examples/read_number.vca) — read and classify an integer
  from standard input.
- [`array_stats.vca`](examples/array_stats.vca) — iterate over a fixed-size
  array.
- [`array_iteration.vca`](examples/array_iteration.vca) — use read-only array
  iteration to calculate a sum and maximum.
- [`struct_inventory.vca`](examples/struct_inventory.vca) — define, construct,
  pass, and read named structs.
- [`const_and_struct_fields.vca`](examples/const_and_struct_fields.vca) — use an
  immutable `const` value and update fields of a mutable local struct.
- [`modules/`](examples/modules/) — split a program across nested source files
  and use qualified public structs, enums, and functions.

## Hierarchical modules

Declare a child module with `mod name;`. The compiler loads its source relative
to the declaring file, recursively checks all module files as one program, and
resolves names such as `geometry::distance_squared()` or
`geometry::point::Point`. Module files can use either `name.vca` or
`name/mod.vca`; nested declarations are resolved relative to their containing
module directory. For example, `mod geometry;` in `main.vca` can load
`geometry.vca`, and a `mod point;` declared in `geometry.vca` can load
`geometry/point.vca`.

Declarations in another module are accessible only when marked `pub`; private
functions, structs, and enums remain local to their declaring module. The root
source file must define the required `fn main() -> i64` entry point. The
[`modules/`](examples/modules/) example demonstrates nested modules, qualified
type and function names, and enum matching. From the repository root, check,
compile, or run it with:

```sh
target/release/vacac examples/modules/main.vca --check
target/release/vacac examples/modules/main.vca -o modules-example
./modules-example
# Or compile and run without leaving artifacts:
target/release/vacac examples/modules/main.vca --run
```

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```vaca
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Scalar matching and value-producing loops

`match` also accepts `bool` and `i64` values. Boolean matches must cover both
`true` and `false`, or include a wildcard; integer matches require a wildcard
because the integer domain is open-ended. Duplicate and unreachable patterns
are rejected. Integer patterns are signed decimal literals, including
`-9223372036854775808`:

```vaca
let label = match code {
    0 => { "success" },
    -1 => { "failure" },
    _ => { "unknown" },
};
```

An unconditional `loop` expression produces the value carried by its
`break expression;` statement. All reachable value breaks must have the same
non-`unit` type. `continue;` restarts the innermost loop. Ordinary `while` and
`for` loops still use bare `break;` and do not produce values:

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

## Fixed-size arrays

Fixed-size arrays contain `i64`, `bool`, `str`, values of a named struct or enum type, vectors, or other fixed-size arrays. Array literals infer their element type and length from their contents; annotations use `[element_type; length]`:

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

Arrays are copied as values when bound, passed, or returned. Arrays of structs, enums, and vectors can be indexed, assigned, and iterated like primitive arrays; enum elements can be inspected with `match`, and struct elements support field access and assignment. Nested arrays are supported to any syntactic depth, including within struct fields, parameters, and return values. Zero-length arrays such as `[i64; 0]`, `[User; 0]`, or `[[i64; 2]; 0]` are supported; their empty literal must have an explicit array type annotation because no element type can be inferred. Indexes are signed `i64`; negative indexes and indexes greater than or equal to the array length terminate the program with status 1, so indexing a zero-length array always exits with status 1. `len(array)` returns the outer fixed length as an `i64`. Equality and inequality compare arrays element by element, recursively comparing nested aggregates and string contents; vector values inside arrays compare by handle identity.

Arrays may also be traversed directly with a read-only element binding:

```vaca
for value in values {
    print_i64(value);
}
```

The array expression is evaluated once and copied before iteration. The binding is immutable and scoped to the loop body; `break` and `continue` work as they do in range loops. Iteration over a zero-length array executes the body zero times.

## Runtime-sized vectors

`Vec<T>` is a heap-backed, runtime-sized sequence with `i64`, `bool`, `str`, named enum/struct, fixed-array, and nested-vector element types (but not `unit`). Create an empty vector with a type annotation and append values with `vec_push`:

```vaca
fn main() -> i64 {
    let values: Vec<i64> = vec_new();
    vec_push(values, 10);
    vec_push(values, 20);
    values[1] = 42;
    print_i64(len(values));
    print_i64(values[1]);
    0
}
```

`vec_new()` requires an expected `Vec<T>` type (usually a binding annotation), while `vec_push(vector, value)` appends one type-checked element and returns `unit`. Vector handles are copied shallowly: assigning, passing, or returning a vector aliases the same contents, and vector equality/inequality compares handle identity. `const` makes only the handle binding immutable; appending and indexed element mutation through that handle are permitted, but rebinding it is not. Indexing uses signed `i64` indexes and invalid indexes exit with status 1. Vector iteration evaluates the handle once and captures its length at loop entry, so elements appended during the loop are not visited; element bindings remain immutable. Vector allocations persist until process exit.

## Named records

Named records (structs) have top-level declarations and named fields of primitive, fixed-array, vector, enum, or struct type:

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

Struct values can be bound, passed to functions, and returned. A literal must initialize every field exactly once; its fields may appear in any order. Fields can be read with `user.name` and updated on mutable local bindings with `user.name = "Grace";`. Nested fields and array elements in fields may also be read or updated, for example `account.owner.name` or `account.scores[0]`. Struct fields may use `i64`, `bool`, `str`, recursively nested fixed-size arrays and vectors of non-`unit` values, an enum, or another struct. `unit` is not a valid struct-field type, and recursive by-value struct layouts are rejected. Enum-valued fields can be constructed, passed and returned as part of the struct, and inspected with `match`. Struct equality and inequality compare every field recursively, using string contents for `str` fields and handle identity for vector fields.

## Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or an ordered tuple of one or more non-`unit` value payloads (primitives, named structs and enums, fixed-size arrays, or vectors):

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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. `match` is an expression. Variant patterns use `EnumName::Variant`; payload variants bind or ignore each field independently, for example `Event::Record(id, _, label)`. The number and types of supplied payload values and pattern fields must match the declaration. A final `_` wildcard arm may handle all remaining variants; it is optional when every variant is otherwise covered. Wildcards must be unique and last, and duplicate or unreachable arms are rejected. Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same non-`unit` type. Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents and aggregate payloads compare recursively. Payloads may be any non-`unit` value type, including arrays, structs, and other enums. Recursive by-value aggregate layouts are rejected.

## Vaca v0.1 syntax

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

Value types are `i64`, scalar `f32` and `f64`, `bool`, `str`, named enums, named structs, recursively nested fixed-size arrays `[element_type; length]`, and heap-backed vectors `Vec<element_type>`; functions may also return `unit`. Floating-point literals use decimal notation with at least one digit before the decimal point; a fractional part and/or exponent are optional (for example, `3.125`, `2e-3`, `1.0e+4`). Unsuffixed floating-point literals have type `f64`; append `f32` to a numeric literal (for example, `3.125f32`, `2e-3f32`) to create a rounded `f32` literal. Bindings, function parameters and results, unary negation, `+`, `-`, `*`, `/`, `%`, and comparisons are supported for both types. They use IEEE 754 behavior, including infinities, NaN, and unordered comparisons; integers, `f32`, and `f64` are never converted implicitly. Non-finite or out-of-range literal spellings are rejected; infinities and NaN can still result from arithmetic. Both float types are scalar-only and are not currently supported in arrays, vectors, struct fields, enum payloads, or match patterns. `print_f32(value)` prints a scalar `f32` followed by a newline using 9 significant digits; `print_f64(value)` writes the value followed by a newline using 17 significant digits. Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements but not bound or used as values. Unit parameters are not supported. Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. Signed `i64` addition, subtraction, multiplication, and negation trap on overflow and terminate with status 1; the direct minimum-literal spelling remains valid. A `let` type annotation is optional and inferred from its initializer. `let` bindings are mutable; `const` creates an immutable binding. Assignment uses `name = expression;`, array and vector elements may be assigned with `array[index] = expression;`, and fields of mutable local structs may be updated with `name.field = expression;`. Immutable bindings cannot be reassigned or mutated through an array element or struct field. `if`/`else`, `while`, and `for` loops are statements; `loop { ... }` [line truncated]

Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`. The `+` operator concatenates two strings. Concatenation copies both inputs into a newly allocated NUL-terminated string; these allocations remain alive until the process exits. Concatenation uses UTF-8 bytes without changing their encoding. Other arithmetic and ordering comparisons are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime. `print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline; `print("x = "); print_i64(x);` prints a labelled integer, while `print_f32(x);` and `print_f64(y);` print floating-point values.

The required Vaca entry point is `fn main() -> i64`. The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Vaca result as the process exit status, following the host OS convention. `read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. A malformed token, end-of-file before a number, a value outside the `i64` range, division or remainder by zero, signed division or remainder overflow, invalid shift count, or signed arithmetic overflow terminates the program with status 1. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.
