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

## Calling trait functions

`Show::show(value)` infers the implementing type from the arguments. Use the explicit form `<Type as Trait>::function(...)` when inference is impossible, including functions whose only use of `Self` is their return type.

Generic type parameters may carry inline bounds such as `T: Show + Other`, and the same bounds may be written on generic structs, enums, and impls. Bounds cannot expose a private trait from a public declaration.

## Default implementations

A trait function that ends in a body instead of a semicolon is a default implementation. An impl may omit that function and inherit the body; omitting a function without a default is still an error.

Inherited bodies are cloned per impl and specialized exactly like explicit methods: `Self` is replaced by the impl target, impl generic parameters carry over, and the body is checked once per implementation. Because the body is lexically part of the trait module, it may use that module's private items but not private items from the impl's module. Other trait functions are called with the qualified `<Self as Trait>::function(...)` form, which resolves to the same impl. An explicit implementation of a default function overrides the body and must match the trait signature. A default body in a trait with no implementations is resolved syntactically but not type-checked.

## Coherence and visibility

Trait implementations are nominal and global. Implementing the same trait twice for the same type is rejected, as are generic impls whose targets may overlap, such as `Box<T>` and `Box<i64>`. Generic impl parameters must all be constrained by the impl target. Traits and their functions obey normal module visibility, while impls have no separate visibility. There is currently no orphan rule.

## Static dispatch

Trait dispatch is fully static. The compiler selects the unique implementation while specializing concrete generic instances and rewrites each trait call to an ordinary direct call to the specialized impl function. No vtable, dictionary, or runtime metadata is emitted.

Trait calls and bounds are checked per concrete instantiation, consistent with the rest of Vaca's generic checking; an unused generic function body is not rejected merely because no implementation exists for a type it is never instantiated with.

## Current limitations

Traits are not value types. Vaca does not currently support `dyn Trait`, trait objects, dot-method syntax, supertraits, associated types or constants, generic traits, per-function generics, operator overloading, or trait functions used as function values. `Self` is permitted in trait and impl function signatures and inside trait default bodies, but not in local type annotations inside ordinary function bodies, including explicit impl functions. Marker traits with no functions are allowed, and functions with default bodies never have to be implemented.
