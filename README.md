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

Vaca can call scalar C functions using declarations such as
`extern "C" fn labs(value: i64) -> i64;`. The C symbol is the declared function
name. Parameters may be `i64`, `f32`, or `f64`; return values may also be
`unit`. These types correspond to `int64_t`, `float`, `double`, and `void` in C.
Pointers, strings, booleans, aggregates, and variadic functions are not part of
this initial FFI. For native object files or libraries, pass repeatable linker
inputs while building or running:

```sh
vacac app.vca --link-object support.o --link-search ./lib --link-lib support -o app
vacac app.vca --run --link-lib m
```

`--link-object <path>`, `--link-search <directory>`, and `--link-lib <name>` are
available for executable and `--run` modes only; they are not used by
`--check` or `--emit-llvm`.

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
- [`c_ffi.vca`](examples/c_ffi.vca) — call the C standard library's `labs()`
  through a scalar `extern "C"` declaration.
- [`array_stats.vca`](examples/array_stats.vca) — iterate over a fixed-size
  array.
- [`array_iteration.vca`](examples/array_iteration.vca) — use read-only array
  iteration to calculate a sum and maximum.
- [`struct_inventory.vca`](examples/struct_inventory.vca) — define, construct,
  pass, and read named structs.
- [`const_and_struct_fields.vca`](examples/const_and_struct_fields.vca) — use an
  immutable `const` value and update fields of a mutable local struct.
- [`pointers.rs`](tests/compiler/pointers.rs) — checked mutable references and
  explicitly raw pointer reads/writes.
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

## Global constants and variables

Declare module-level values with `const` (immutable) or `let` (mutable). Types
may be written explicitly or inferred from the initializer:

```vaca
const LIMIT: i64 = 40 + 2;
const NEXT = LIMIT + 1;
pub let total: i64 = NEXT;

fn main() -> i64 {
    total = total + 1;
    total - 44
}
```

Global initializers must be compile-time constant expressions. They may use
scalar and aggregate literals, constant arithmetic, and references to immutable
globals (including public globals in other modules); function calls, local
variables, mutable-global dependencies, and initializer cycles are rejected.
Globals use static LLVM storage—there is no startup initializer execution.
Mutable globals can be assigned, while constants cannot be rebound or mutated
through fields or array elements. As with other declarations, a global in a
different module must be marked `pub` to be accessed through its qualified
name.

## References and raw pointers

Shared references use `&T`; `&value` borrows an eligible immutable local, and
`*reference` reads through it. Mutable references use `&mut T` and `&mut value`:

```vaca
fn read(value: &i64) -> i64 { *value }
fn increment(value: &mut i64) -> unit { *value = *value + 1; }

fn main() -> i64 {
    const answer = 42;
    let count = 41;
    if true {
        const count_ref: &mut i64 = &mut count;
        increment(count_ref);
        0
    } else {
        0
    };
    read(&answer) + count - 84
}
```

Mutable references are restricted to named mutable local bindings (`let`, never
`const` or loop variables). A mutable reference must be held in a `const` local;
while that borrow's lexical block is active, direct reads/writes or another
mutable borrow of the source are rejected. The borrow must first be bound to a
local reference variable, which can then be passed to functions and written
through with `*reference`. It cannot be returned, captured by a
closure, stored in aggregates/globals, or escape through expressions. Shared
references remain read-only and keep the same local-only restrictions.

Raw pointers use `*const T` and `*mut T`. Form them explicitly with `&raw const
place` or `&raw mut place`, then dereference with `*pointer`. Raw pointers retain
their target and mutability in Vaca's type checker, are not implicitly
convertible to references or between const and mutable forms, and may be stored
in aggregates or passed/returned by functions. A `*const T` cannot be assigned
through; `*mut T` can. Dereferencing raw pointers has **no runtime validity or
lifetime check**: the programmer must ensure the pointer is valid and points to
an appropriately aligned live object. There is no null literal, pointer
arithmetic/comparison, raw-pointer global initializer, pointer FFI, or general
address-taking of aggregate subplaces yet. Raw pointers and safe references are
distinct and do not implicitly coerce between one another.

## Expression conditionals

In addition to statement-style `if`/`else`, an `if` with an `else` can produce a value:

```vaca
let larger = if left > right { left } else { right };
```

Both branches must end with a value expression of the same non-`unit` type. Branches may contain local bindings or expression statements before their final value.

## Function values and closures

Functions can be used as values and called indirectly. Function types use
`fn(parameter_types...) -> result_type`; lambdas use the same fully annotated
signature before their body:

```vaca
fn apply(function: fn(i64) -> i64, value: i64) -> i64 {
    function(value)
}

fn increment(value: i64) -> i64 { value + 1 }

fn main() -> i64 {
    let named: fn(i64) -> i64 = increment;
    let doubled: fn(i64) -> i64 = fn(value: i64) -> i64 { value * 2 };
    apply(named, doubled(21)) - 43
}
```

Function values may be passed to and returned from functions, stored in local
bindings, used as struct or tuple fields, and stored directly in fixed arrays and
vectors. Lambdas may capture local bindings;
each captured value is copied when the closure is created into a heap-backed
environment retained until process exit. Captures are snapshots, not shared
mutable cells, and a closure cannot assign to a captured binding. This allows a
returned closure to outlive the scope that created it:

```vaca
fn make_adder(amount: i64) -> fn(i64) -> i64 {
    fn(value: i64) -> i64 { value + amount }
}

fn main() -> i64 {
    let add_two = make_adder(2);
    add_two(40) - 42
}
```

Function values compare equal when both their code and environment pointers
match; separately created closures therefore need not compare equal even when
they capture identical values. Module functions used as values obey the same
`pub` visibility rules as direct calls. Fixed arrays and vectors store the
complete closure value, so captured environments remain available when values
are indexed, copied, or iterated. Global function-value initializers are not
supported.

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

Fixed-size arrays contain `i64`, `f32`, `f64`, `bool`, `str`, values of a named struct or enum type, vectors, or other fixed-size arrays. Array literals infer their element type and length from their contents; annotations use `[element_type; length]`:

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

`Vec<T>` is a heap-backed, runtime-sized sequence with `i64`, `f32`, `f64`, `bool`, `str`, named enum/struct, fixed-array, and nested-vector element types (but not `unit`). Create an empty vector with a type annotation and append values with `vec_push`:

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

Struct values can be bound, passed to functions, and returned. A literal must initialize every field exactly once; its fields may appear in any order. Fields can be read with `user.name` and updated on mutable local bindings with `user.name = "Grace";`. Nested fields and array elements in fields may also be read or updated, for example `account.owner.name` or `account.scores[0]`. Struct fields may use `i64`, `f32`, `f64`, `bool`, `str`, tuples, recursively nested fixed-size arrays and vectors of non-`unit` values, an enum, or another struct. `unit` is not a valid struct-field type, and recursive by-value struct layouts are rejected. Enum-valued fields can be constructed, passed and returned as part of the struct, and inspected with `match`. Struct equality and inequality compare every field recursively, using IEEE equality for floats, string contents for `str` fields and handle identity for vector fields.

## Tuples and destructuring

Tuple types and values use comma-separated parentheses: `(i64, bool)` and `(42, true)`. A trailing comma distinguishes a one-element tuple `(value,)` from a grouped expression `(value)`. Tuples can be nested, passed to and returned from functions, stored in structs, arrays and vectors, and compared recursively for equality.

`let` and `const` bindings can destructure tuples, fixed arrays, and structs with recursive patterns. Use `_` to ignore a tuple item or an individual fixed-array position. Array patterns list every position. Struct patterns name fields; add `..` to ignore omitted fields:

```vaca
struct User { id: i64, active: bool, name: str }

fn describe(user: User, values: [(i64, bool); 2]) -> i64 {
    let User { id, active, .. } = user;
    let [(first, _), (second, enabled)] = values;
    if active && enabled { id + first + second } else { 0 }
}
```

Match arms accept the same tuple, array and struct patterns, and enum payload bindings may themselves be destructured. Aggregate-value matches currently use one destructuring arm; enum matches remain exhaustive. A bare identifier pattern binds the whole scalar or enum value as a catch-all arm.

Generic structs are monomorphized into concrete layouts at each use. Supply type arguments in a type annotation or struct literal; nested applications are supported:

```vaca
struct Box<T> { value: T }
struct Pair<L, R> { left: L, right: R }

fn main() -> i64 {
    let pair: Pair<i64, Box<str>> = Pair<i64, Box<str>> {
        left: 42,
        right: Box<str> { value: "generic" },
    };
    println(pair.right.value);
    pair.left - 42
}
```

Generic functions use the same declaration syntax. Type parameters are inferred from argument types when possible; use turbofish syntax `function::<Type, ...>(...)` when a parameter cannot be inferred. Explicit generic arguments must list every parameter. Generic functions and structs can be public module declarations and retain normal module visibility checks. The compiler specializes concrete instances, deduplicates repeated instances, and rejects programs requiring more than 512 concrete generic function/struct instances. Generic enums, trait bounds, and partial type-argument lists are not supported. Built-in `Vec<T>` remains supported as a generic function parameter and return type, with its existing built-in runtime behavior.

```vaca
fn identity<T>(value: T) -> T { value }
fn make<T>() -> i64 { 42 }

fn main() -> i64 {
    let value = identity(42);       // inferred as i64
    let explicit = identity::<str>("vaca");
    println(explicit);
    value - make::<bool>()
}
```

## Enums and exhaustive matching

Enums are top-level declarations with uniquely named variants. A variant has no payload or an ordered tuple of one or more non-`unit` value payloads (primitives including `f32` and `f64`, named structs and enums, fixed-size arrays, or vectors):

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

Construct variants with `EnumName::Variant` or `EnumName::Variant(expression, ...)`. Match patterns use `EnumName::Variant`; payload variants bind or ignore every field, for example `Event::Record(id, _, label)`, and tuple/array/struct payloads can be recursively destructured. The payload count and types must match the declaration. Scalar patterns may be booleans, signed integers, or floating-point literals (use the `f32` suffix for an `f32` pattern). Float pattern equality follows IEEE `==`: `+0.0` and `-0.0` are equal and duplicate, while NaN matches no float literal. Floating-point matches always require a `_` wildcard because their pattern set is not exhaustive. For booleans, integers, and enums a final `_` wildcard or identifier binding may handle remaining values; it is optional when every possible value or variant is otherwise covered. Wildcards and identifier catch-alls must be unique and last, and duplicate or unreachable arms are rejected. Each arm uses `=> { ... }` and its block must end in a value expression. All arm values must have the same non-`unit` type. Enum values may be bound, assigned, passed to and returned from functions, stored in fixed-size arrays, inspected with `match`, and compared for equality or inequality. Two enums are equal when they have the same variant and equal payloads; payload strings compare by contents and aggregate payloads compare recursively. Payloads may be any non-`unit` value type, including tuples, arrays, structs, and other enums. Recursive by-value aggregate layouts are rejected.

## Vaca v0.1 syntax

In addition to the by-value types described below, the compiler supports
`&T`, `&mut T`, `*const T`, and `*mut T` as documented in
[References and raw pointers](#references-and-raw-pointers).

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

Value types are `i64`, `f32`, `f64`, `bool`, `str`, named enums, named structs, recursively nested fixed-size arrays `[element_type; length]`, and heap-backed vectors `Vec<element_type>`; functions may also return `unit`. Floating-point literals use decimal notation with at least one digit before the decimal point; a fractional part and/or exponent are optional (for example, `3.125`, `2e-3`, `1.0e+4`). Unsuffixed floating-point literals have type `f64`; append `f32` to a numeric literal (for example, `3.125f32`, `2e-3f32`) to create a rounded `f32` literal. Bindings, function parameters and results, unary negation, `+`, `-`, `*`, `/`, `%`, comparisons, and aggregate elements/payloads are supported for both types. They use IEEE 754 behavior, including infinities, NaN, and unordered comparisons; integers, `f32`, and `f64` are never converted implicitly. Float `match` patterns use IEEE equality, treat signed zeros as equal for duplicate detection, and require a wildcard arm. Non-finite or out-of-range literal spellings are rejected; infinities and NaN can still result from arithmetic. `print_f32(value)` prints a scalar `f32` followed by a newline using 9 significant digits; `print_f64(value)` writes the value followed by a newline using 17 significant digits. Unit-returning functions can finish without a value or use `return;`, and their results may be used as expression statements but not bound or used as values. Unit parameters are not supported. Integer literals must fit in `i64`; the magnitude `9223372036854775808` is accepted only as the operand of unary minus, allowing `-9223372036854775808` to spell `i64::MIN`. Signed `i64` addition, subtraction, multiplication, and negation trap on overflow and terminate with status 1; the direct minimum-literal spelling remains valid. A `let` type annotation is optional and inferred from its initializer. `let` bindings are mutable; `const` creates an immutable binding. Assignment uses `name = expression;`, array and vector elements may be assigned with `array[index] = expression;`, and fields of mutable local structs may be updated with `name.field = expression;`. Immutable bindings cannot be reassigned or mutated through an array element or struct field. `if`/`else`, `while`, and `for` loops are statements; `loop { ... }` [line truncated]

Strings are immutable; they can be bound, assigned, passed, returned, and compared by content with `==` and `!=`. The `+` operator concatenates two strings. Concatenation copies both inputs into a newly allocated NUL-terminated string; these allocations remain alive until the process exits. Concatenation uses UTF-8 bytes without changing their encoding. Other arithmetic and ordering comparisons are not supported for `str`. `\0` is rejected because strings are NUL-terminated at runtime. `print(str)` writes a string to stdout as-is, and `println(str)` writes it followed by a newline; `print("x = "); print_i64(x);` prints a labelled integer, while `print_f32(x);` and `print_f64(y);` print floating-point values.

The required Vaca entry point is `fn main() -> i64`. The compiler emits a small C-ABI `main` wrapper that returns the low 8 bits of the Vaca result as the process exit status, following the host OS convention. `read_i64()` accepts optional leading ASCII whitespace and an optional `+` or `-`, followed by at least one decimal digit and then whitespace or EOF. A malformed token, end-of-file before a number, a value outside the `i64` range, division or remainder by zero, signed division or remainder overflow, invalid shift count, or signed arithmetic overflow terminates the program with status 1. The first whitespace after a number is consumed as its delimiter; any following token remains available to the next call.
