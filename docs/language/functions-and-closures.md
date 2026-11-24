# Function values and closures

## Function types and lambdas

Functions can be used as values and called indirectly. Function types use `fn(parameter_types...) -> result_type`; lambdas use the same fully annotated signature before their body:

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

Function values may be passed to and returned from functions, stored in local bindings, used as struct or tuple fields, and stored directly in fixed arrays and vectors.

## Closures and captures

Lambdas may capture local bindings. Each captured value is copied when the lambda is evaluated into a heap-backed environment retained until process exit. The copy is shallow: an aliased `Vec` handle still refers to the same contents, while each closure evaluation owns its own environment slots.

Mutable bindings can be assigned through the closure. Every call through the same closure or one of its copies reads and writes the same environment, so state persists after the creating function returns:

```vaca
fn make_counter(seed: i64) -> fn() -> i64 {
    let count = seed;
    fn() -> i64 {
        count = count + 1;
        count
    }
}

fn main() -> i64 {
    const first = make_counter(0);
    const alias = first;
    const second = make_counter(0);

    print_i64(first()); // 1
    print_i64(alias()); // 2
    print_i64(second()); // 1
    print_i64(first()); // 3
    0
}
```

Writes to the enclosing local after the lambda is evaluated do not change the snapshot, and writes through the closure do not write back to the enclosing local. A `const` binding or loop variable remains immutable in a capture, but mutating an element through a captured `const` vector handle is allowed. A closure handle can itself be `const`; that only prevents rebinding the handle, not updates through its mutable capture slots.

Nested lambdas take a fresh snapshot of the values visible when the inner lambda is evaluated, including the current state of the parent closure's captures. Separate lambdas, even ones created from the same source location with the same starting values, have independent environments. Global names are accessed directly rather than captured.

Safe references still cannot be captured, and a closure cannot take a shared borrow, mutable borrow, or raw address of a captured binding because another alias could mutate that slot through a callback. Capturing a value requires reading the whole source binding at creation time, so a mutable loan that overlaps any part of that binding prevents the capture; a shared loan does not.

## Equality, storage, and limitations

Function values compare equal when both their code and environment pointers match; separately created closures therefore need not compare equal even when they capture identical values, and mutations do not change closure identity. Module functions used as values obey the same `pub` visibility rules as direct calls. Fixed arrays and vectors store the complete closure value, so captured environments remain available when values are indexed, copied, or iterated. Global function-value initializers are not supported.
