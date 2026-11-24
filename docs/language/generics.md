# Generics

Vaca generics are monomorphized: the compiler specializes concrete instances, deduplicates repeated instances, and rejects programs requiring more than 512 concrete generic instances. Partial type-argument lists are not supported. Generic functions, structs, and enums can be public module declarations and retain normal module visibility checks.

## Generic structs

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

## Generic functions

Generic functions use the same declaration syntax. Type parameters are inferred from argument types when possible; use turbofish syntax `function::<Type, ...>(...)` when a parameter cannot be inferred. Explicit generic arguments must list every parameter.

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

Generic parameters may carry trait bounds as documented in [Traits and static dispatch](traits.md). Built-in `Vec<T>` remains supported as a generic function parameter and return type, with its existing built-in runtime behavior.
