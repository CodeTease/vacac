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

Lambdas may capture local bindings; each captured value is copied when the closure is created into a heap-backed environment retained until process exit. Captures are snapshots, not shared mutable cells, and a closure cannot assign to a captured binding. This allows a returned closure to outlive the scope that created it:

```vaca
fn make_adder(amount: i64) -> fn(i64) -> i64 {
    fn(value: i64) -> i64 { value + amount }
}

fn main() -> i64 {
    let add_two = make_adder(2);
    add_two(40) - 42
}
```

## Equality, storage, and limitations

Function values compare equal when both their code and environment pointers match; separately created closures therefore need not compare equal even when they capture identical values. Module functions used as values obey the same `pub` visibility rules as direct calls. Fixed arrays and vectors store the complete closure value, so captured environments remain available when values are indexed, copied, or iterated. Global function-value initializers are not supported.
