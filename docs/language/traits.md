# Traits and static dispatch

Traits are nominal interfaces with required functions, optional default bodies, and explicit implementations. A trait function writes `Self` where the implementing type belongs; call trait functions through the trait name:

```vaca
trait Show {
    fn show(value: Self) -> str;
    fn describe(value: Self) -> str { "value=" + <Self as Show>::show(value) }
}

trait Zero {
    fn zero() -> Self;
}

struct Box<T> {
    value: T,
}

impl Show for i64 {
    fn show(value: Self) -> str { "integer" }
}

impl<T: Show> Show for Box<T> {
    fn show(value: Self) -> str { Show::show(value.value) }
}

impl Zero for i64 {
    fn zero() -> Self { 0 }
}

fn describe<T: Show>(value: T) -> str {
    Show::show(value)
}

fn main() -> i64 {
    println(describe(Box<i64> { value: 42 }));
    println(Show::describe(42));
    <i64 as Zero>::zero()
}
```

## Generic traits and generic trait functions

Traits and their functions may declare type parameters. Trait parameters describe the interface, while function parameters are chosen at each call:

```vaca
trait Choose<T> {
    fn pick(value: Self, input: T) -> T;
    fn relay<U>(value: Self, input: T, fallback: U) -> U {
        let selected = <Self as Choose<T>>::pick(value, input);
        if true { fallback } else { fallback }
    }
}

impl<A> Choose<A> for i64 {
    fn pick(value: i64, input: A) -> A { input }
}

fn main() -> i64 {
    <i64 as Choose<bool>>::pick(0, true);
    Choose::relay::<i64>(0, true, 42) - 42
}
```

An impl is identified by its trait, the implementing type, and its ordered trait arguments. Two impls that differ only in `Self` or in a trait argument are distinct; `Choose<i64>` and `Choose<bool>` can both be implemented for `i64`. Function type parameters do not participate in impl selection.

Trait arguments are inferred from value arguments, not from the implementation table or the expected return type. `Self` is inferred from values that mention it. Function type parameters are inferred the same way, and explicit function arguments can be supplied with `::<...>` after the function name. If a parameter appears only in a return type, provide it explicitly:

```vaca
<i64 as Zero>::zero()
<i64 as Choose<i64>>::pick(0, 42)
```

The qualified form supplies `Self` and every trait argument explicitly: `<Type as Trait<A, B>>::function(...)`. The unqualified `Trait::<A>::function(...)` shorthand is rejected because a turbofish after the trait selects function arguments, not trait arguments.

Generic trait declarations, impl headers, and bounds may nest type applications. All generic parameters in a list are in scope in its bounds, including forward references such as `fn f<T: Convert<U>, U>(...)`. A trait function parameter may not shadow a trait parameter, and an impl function parameter may not shadow an impl parameter.

## Calling trait functions

`Show::show(value)` infers the implementing type from the arguments. `Choose::relay(value, input, fallback)` additionally infers the trait and function arguments. Contextual argument types are propagated once the relevant arguments are known. For example, `vec_new()` can be passed where an inferred `Vec<T>` is expected and a generic enum constructor such as `Maybe::<i64>::None` is resolved before the call.

When inference cannot determine a parameter, the diagnostic names it and describes the explicit form to use. Partial type-argument lists are not supported.

Generic type parameters may carry inline bounds such as `T: Show + Other`, and the same bounds may be written on generic structs, enums, impls, traits, and trait functions. Bounds on a trait declaration constrain the trait's type arguments; bounds on a function constrain its own type parameters. A bound may be parameterized, such as `S: Choose<T>`. Bounds cannot expose a private trait from a public declaration.

## Default implementations

A trait function that ends in a body instead of a semicolon is a default implementation. An impl may omit that function and inherit the body; omitting a function without a default is still an error.

Inherited bodies are cloned per impl and specialized exactly like explicit methods: `Self` is replaced by the impl target, trait parameters are replaced by the impl's trait arguments, and function parameters are carried over with fresh names when they collide with impl parameters. The body is checked once per implementation. Because the body is lexically part of the trait module, it may use that module's private items but not private items from the impl's module. Other trait functions are called with the qualified `<Self as Trait<...>>::function(...)` form, which resolves to the same impl. An explicit implementation of a default function overrides the body and must match the trait signature, including the number and bounds of its function type parameters. A default body in a trait with no implementations is resolved syntactically but not type-checked.

## Coherence and visibility

Trait implementations are nominal and global. Implementing the same trait twice for the same full head is rejected. Coherence compares `Self`, all trait arguments, and the impl type parameters together: `impl<T> Tag<T> for i64` conflicts with `impl Tag<bool> for i64`, while `impl Tag<bool> for i64` and `impl Tag<i64> for i64` do not conflict. Generic impl parameters must all be constrained by `Self` or by a trait argument. Traits, their functions, and the types used in public bounds obey normal module visibility, while impls have no separate visibility. There is currently no orphan rule.

Trait argument arity and declaration shape are checked eagerly. Whether concrete arguments satisfy the trait's parameter bounds, and whether a type implements a trait used as a bound, is checked at each concrete use, consistent with Vaca's generic checking.

## Static dispatch

Trait dispatch is fully static. The compiler selects the unique implementation while specializing concrete generic instances and rewrites each trait call to an ordinary direct call to the specialized impl function. No vtable, dictionary, or runtime metadata is emitted. Distinct trait arguments produce distinct direct callees, while repeated equivalent calls reuse one specialized function.

Trait calls and bounds are checked per concrete instantiation, consistent with the rest of Vaca's generic checking; an unused generic function body is not rejected merely because no implementation exists for a type it is never instantiated with. Expanding trait-bound proofs stop at a fixed proof-depth limit instead of recursing indefinitely.

## Current limitations

Traits are not value types. Vaca does not currently support `dyn Trait`, trait objects, dot-method syntax, supertraits, associated types or constants, `where` clauses, lifetime-generic traits or trait functions, operator overloading, or trait functions used as function values. `Self` is permitted in trait and impl function signatures, in trait function bounds, and inside trait default bodies, but not in local type annotations inside ordinary function bodies, including explicit impl functions. Marker traits with no functions are allowed, and functions with default bodies never have to be implemented. Trait arguments cannot contain references or named lifetimes yet.
