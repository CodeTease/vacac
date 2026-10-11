# Apex compiler

`apexc` is a handwritten, statically compiled compiler for Apex v0.1. It parses and checks source in Rust, generates native LLVM object code with Inkwell, then invokes `cc` to link an ELF executable.

A **CodeTease** project.

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
- [`modules/`](examples/modules/) — split a program across nested source files
  and use qualified public structs, enums, and functions.

## Hierarchical modules

Declare a child module with `mod name;`. The compiler loads its source relative
to the declaring file, recursively checks all module files as one program, and
resolves names such as `geometry::distance_squared()` or
`geometry::point::Point`. Module files can use either `name.apx` or
`name/mod.apx`; nested declarations are resolved relative to their containing
module directory. For example, `mod geometry;` in `main.apx` can load
`geometry.apx`, and a `mod point;` declared in `geometry.apx` can load
`geometry/point.apx`.

Declarations in another module are accessible only when marked `pub`; private
functions, structs, and enums remain local to their declaring module. The root
source file must define the required `fn main() -> i64` entry point. The
[`modules/`](examples/modules/) example demonstrates nested modules, qualified
type and function names, and enum matching. From the repository root, check,
compile, or run it with:

```sh
target/release/apexc examples/modules/main.apx --check
target/release/apexc examples/modules/main.apx -o modules-example
./modules-example
# Or compile and run without leaving artifacts:
target/release/apexc examples/modules/main.apx --run
```

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```apex
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Scalar matching and value-producing loops

`match` also accepts `bool` and `i64` values. Boolean matches must cover both
`true` and `false`, or include a wildcard; integer matches require a wildcard
because the integer domain is open-ended. Duplicate and unreachable patterns
are rejected. Integer patterns are signed decimal literals, including
`-9223372036854775808`:

```apex
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

```apex
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

Arrays are copied as values when bound, passed, or returned. Arrays of structs, enums, and vectors can be indexed, assigned, and iterated like primitive arrays; enum elements can be inspected with `match`, and struct elements support field access and assignment. Nested arrays are supported to any syntactic depth, including within struct fields, parameters, and return values. Zero-length arrays such as `[i64; 0]`, `[User; 0]`, or `[[i64; 2]; 0]` are supported; their empty literal must have an explicit array type annotation because no element type can be inferred. Indexes are signed `i64`; negative indexes and indexes greater than or equal to the array length terminate the program with status 1, so indexing a zero-length array always exits with status 1. `len(array)` returns the outer fixed length as an `i64`. Equality and inequality compare arrays element by element, recursively comparing nested aggregates and string contents; vector values inside arrays compare by handle identity.

Arrays may also be traversed directly with a read-only element binding:

```apex
for value in values {
    print_i64(value);
}
```

The array expression is evaluated once and copied before iteration. The binding is immutable and scoped to the loop body; `break` and `continue` work as they do in range loops. Iteration over a zero-length array executes the body zero times.

## Runtime-sized vectors

`Vec<T>` is a heap-backed, runtime-sized sequence with `i64`, `bool`, `str`, named enum/struct, fixed-array, and nested-vector element types (but not `unit`). Create an empty vector with a type annotation and append values with `vec_push`:

```apex
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

Struct values can be bound, passed to functions, and returned. A literal must initialize every field exactly once; its fields may appear in any order. Fields can be read with `user.name` and updated on mutable local bindings with `user.name = "Grace";`. Nested fields and array elements in fields may also be read or updated, for example `account.owner.name` or `account.scores[0]`. Struct fields may use `i64`, `bool`, `str`, recursively nested fixed-size arrays and vectors of non-`unit` values, an enum, or another struct. `unit` is not a valid struct-field type, and recursive by-value struct layouts are rejected. Enum-valued fields can be constructed, passed and returned as part of the struct, and inspected with `match`. Struct equality and inequality compare every field recursively, using string contents for `str` fields and handle identity for vector fields.

## Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or an ordered tuple of one or more non-`unit` value payloads (primitives, named structs and enums, fixed-size arrays, or vectors):

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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. `match` is an expression. Variant patterns use `EnumName::Variant`; payload variants bind or ignore each field independently, for example `Event::Record(id, _, label)`. The number and types of supplied payload values and pattern fields must match the declaration. A final `_` wildcard arm may handle all remaining variants; it is optional when every variant is otherwise covered. Wildcards must be unique and last, and duplicate or unreachable arms are rejected. Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same non-`unit` type. Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents and aggregate payloads compare recursively. Payloads may be any non-`unit` value type, including arrays, structs, and other enums. Recursive by-value aggregate layouts are rejected.

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

Value types are `i64`, `bool`, `str`, named enums, named structs, recursively nested fixed-size arrays `[element_type; length]`, and heap-backed vectors `Vec<element_type>`; functions may also return `unit`. Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements but not bound or used as values. Unit parameters are not supported. Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. A `let` type annotation is optional and inferred from its initializer. `let` bindings are mutable; `const` creates an immutable binding. Assignment uses `name = expression;`, array and vector elements may be assigned with `array[index] = expression;`, and fields of mutable local structs may be updated with `name.field = expression;`. Immutable bindings cannot be reassigned or mutated through an array element or struct field. `if`/`else`, `while`, and `for` loops are statements; `loop { ... }` is an expression whose `break expression;` exits with a value, and all such breaks must agree on one non-`unit` type. `continue;` targets the innermost loop. A range loop such as `for i in 0..6 { ... }` visits `0` through `5`; ranges are half-open, both bounds are evaluated once, and the loop variable is immutable and scoped to the loop. An array loop such as `for value in values { ... }` visits each copied element once; vector loops capture the vector length at entry. Both use a read-only binding scoped to the loop. Functions returning a value must use `return expression;` or a final expression. All functions are top-level and have a declared return type. `match` supports enums, booleans, and signed `i64` literals; boolean matches are exhaustive over both values, while `i64` matches require a wildcard arm. Operators include arithmetic (`+ - * / %`), signed comparisons, equality, boolean negation, and short-circuiting `&&` / `||`. Integer bitwise operators `&`, `^`, and `|` operate on `i64` values. Shifts use `<<` and arithmetic `>>`; their right operand must be in `0..64`, otherwise the program exits with status 1. Bitwise AND binds more tightly than XOR, which binds more tightly than OR; shifts bind more tightly than comparisons and less tightly than addition. Division and remainder by zero, as well as `i64::MIN / -1` and `i64::MIN % -1`, terminate the program with exit status 1. Line (`//`) and block (`/* ... */`) comments are supported. `print_i64(i64)` writes one decimal value and a newline to stdout; `read_i64() -> i64` reads one signed decimal integer token from stdin.

Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`. The `+` operator concatenates two strings. Concatenation copies both inputs into a newly allocated NUL-terminated string; these allocations remain alive until the process exits. Concatenation uses UTF-8 bytes without changing their encoding. Other arithmetic and ordering comparisons are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime. `print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline; `print("x = "); print_i64(x);` prints a labelled value.

The required Apex entry point is `fn main() -> i64`. The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Apex result as the process exit status, following the host OS convention. `read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. A malformed token, end-of-file before a number, or a value outside the `i64` range terminates the program with status 1. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.
