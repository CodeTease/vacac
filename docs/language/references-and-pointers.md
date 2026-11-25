# References and raw pointers

## Shared and mutable references

Shared references use `&T`; `&value` borrows an eligible local, including a mutable `let` binding, and `*reference` reads through it. Mutable references use `&mut T` and `&mut value`:

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

## Borrow rules

Mutable references may borrow a place rooted at a named mutable local binding (`let`, never `const` or loop variables), including a struct field or fixed-array element. They must be held in a `const` local; while a borrow's lexical block is active, overlapping reads, writes, or borrows are rejected. The borrow must first be bound to a local reference variable, which can then be passed to functions and written through with `*reference`. Mutable references cannot be copied into another local binding, and one call cannot receive overlapping mutable-reference origins in multiple `&mut` parameters. These call borrows are temporary.

Shared references can borrow either `let` or `const` locals, including struct fields and fixed-array elements. Overlapping shared loans are allowed, but prevent overlapping writes and mutable borrows until the lexical block ends. Constant field names and constant array indices are tracked precisely; unknown indices conservatively overlap. Vector elements remain ineligible.

An existing shared reference can be reborrowed through a direct dereference with `&*reference`; the reborrow retains the original loan and named-lifetime provenance. Mutable-reference reborrowing is not supported.

## Named lifetimes

Named lifetimes can be declared on function signatures (`fn identity<'a>(value: &'a i64) -> &'a i64`) to return a reference tied to an input lifetime. A return may originate from more than one parameter when every possible parameter origin has the declared lifetime, for example when choosing between two `&'a` inputs.

Generic type parameters can appear behind named references (for example, `fn identity<'a, T>(value: &'a T) -> &'a T`); specialization infers the referenced type while the named lifetime remains a checked signature contract. Only shared-reference parameters (`&T`) can be origins for a shared-reference return; a matching lifetime on an `&mut T` parameter alone does not authorize returning a shared reference.

Calls that return references conservatively propagate all arguments corresponding to shared-reference parameters with the return lifetime through local reference aliases, so the caller retains shared loans on every possible borrowed place. An origin the checker cannot establish is rejected rather than treated as a local-safe value. Returning a local borrow or a reference with an origin from another or undeclared lifetime is rejected.

`if` and `match` results can be returned when every branch has only parameter origins with the function's declared lifetime; a branch that can refer to a local cannot escape. Function lifetime parameters are scoped to their declaration, and every reference type in a lifetime-generic signature must name one of those parameters.

## Reference limitations

References still cannot be captured by a closure or stored in aggregates or globals. Closure capture slots are not borrowable: a closure cannot take a shared reference, mutable reference, or raw address of a binding it captured. Provenance propagation is limited to direct named-function calls, local reference aliases, supported shared-reference reborrows, and `if`/`match` joins; function-value calls and other expression forms do not propagate reference origins. Last-use loan ending (NLL) is not implemented, so loans remain conservatively lexical.

## Raw pointers

Raw pointers use `*const T` and `*mut T`. Form them explicitly with `&raw const place` or `&raw mut place`, then dereference with `*pointer`. Raw pointers retain their target and mutability in Vaca's type checker, are not implicitly convertible to references or between const and mutable forms, and may be stored in aggregates or passed/returned by functions. A `*const T` cannot be assigned through; `*mut T` can.

Dereferencing raw pointers has **no runtime validity or lifetime check**: the programmer must ensure the pointer is valid and points to an appropriately aligned live object. There is no null literal, pointer arithmetic/comparison, raw-pointer global initializer, pointer FFI, or general address-taking of aggregate subplaces yet. Raw pointers and safe references are distinct and do not implicitly coerce between one another.
