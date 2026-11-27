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

A fresh mutable reference borrows a place rooted at a named mutable local binding (`let`, never `const` or loop variables), including a struct field or fixed-array element. It must be bound to a `const` local before it can be passed to a function or written through with `*reference`. Mutable references cannot be copied into another local binding, and one call cannot receive overlapping mutable-reference origins in multiple parameters.

An existing reference can be reborrowed through a direct dereference. `&mut *reference` creates an exclusive child and `&*reference` creates a shared view; both accept field and constant-index projections such as `&mut (*reference).left` and `&(*reference).items[0]`. A reborrow may be bound directly or passed to a call, while a fresh `&mut place` still requires a local binding first.

While a child reborrow's lexical block is active, it blocks overlapping access through its parent. An exclusive child blocks parent reads, writes, and borrows; a shared child blocks parent writes and exclusive borrows but permits overlapping reads and other shared views. Access through the child itself is always allowed. Known-disjoint fields and constant array indices remain usable, while an unknown index conservatively overlaps. After the child's block ends, the parent is usable again.

Shared references can borrow either `let` or `const` locals, including struct fields and fixed-array elements. Overlapping shared loans are allowed, but prevent overlapping writes and exclusive borrows until the lexical block ends. Constant field names and constant array indices are tracked precisely; unknown indices conservatively overlap. Vector elements remain ineligible.

## Named lifetimes

Named lifetimes can be declared on function signatures (`fn identity<'a>(value: &'a i64) -> &'a i64`) to return a reference tied to an input lifetime. A shared return may originate from more than one shared-reference parameter when every possible origin has the declared lifetime. A mutable return (`-> &'a mut T`) is also supported when the declared lifetime appears on at least one `&'a mut` parameter; it may return that parameter directly, a reborrow such as `&mut *value`, a forwarding call result, or an `if`/`match` selection over eligible mutable parameters. A result may point at a subplace of a parameter, so the input and output pointee types need not match.

Generic type parameters can appear behind named references (for example, `fn identity<'a, T>(value: &'a mut T) -> &'a mut T`); specialization infers the referenced type while the named lifetime remains a checked signature contract. Shared returns still accept only shared-reference origins. A matching lifetime on an `&mut T` parameter does not authorize returning a shared reference, and a shared `&T` parameter does not authorize returning `&mut T`.

Calls that return references conservatively propagate every argument corresponding to an eligible parameter with the return lifetime. A local alias created from the result therefore keeps every possible caller place loaned until the alias's lexical block ends. An origin the checker cannot establish is rejected rather than treated as a local-safe value. Returning a local borrow, a reborrow of a local binding that does not originate from a parameter, or a reference with an origin from another or undeclared lifetime is rejected.

`if` and `match` results can be returned when every branch has only parameter origins with the function's declared lifetime; a branch that can refer to a local cannot escape. A branch-local reference binding may yield a reborrow of an outliving place, which is promoted to the enclosing scope. Function lifetime parameters are scoped to their declaration, and every reference type in a lifetime-generic signature must name one of those parameters.

## Reference limitations

References still cannot be captured by a closure, returned through a function value or lambda, stored in aggregates or globals, or borrowed from vector elements. Closure capture slots are not borrowable: a closure cannot take a shared reference, mutable reference, or raw address of a binding it captured. Provenance propagation is limited to direct named-function calls, local reference aliases, reborrows, and `if`/`match` joins; function-value calls and other expression forms do not propagate reference origins. Named-lifetime traits and trait functions are not supported. Last-use loan ending (NLL) is not implemented, so loans remain conservatively lexical.

## Raw pointers

Raw pointers use `*const T` and `*mut T`. Form them explicitly with `&raw const place` or `&raw mut place`, then dereference with `*pointer`. Raw pointers retain their target and mutability in Vaca's type checker, are not implicitly convertible to references or between const and mutable forms, and may be stored in aggregates or passed/returned by functions. A `*const T` cannot be assigned through; `*mut T` can.

Dereferencing raw pointers has **no runtime validity or lifetime check**: the programmer must ensure the pointer is valid and points to an appropriately aligned live object. There is no null literal, pointer arithmetic/comparison, raw-pointer global initializer, pointer FFI, or general address-taking of aggregate subplaces yet. Raw pointers and safe references are distinct and do not implicitly coerce between one another.
