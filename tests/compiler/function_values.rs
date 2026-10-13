use crate::common::*;

#[test]
fn function_values_support_lambdas_parameters_returns_and_generics() {
    let result = compile_and_run(
        r#"
        fn apply(function: fn(i64) -> i64, value: i64) -> i64 { function(value) }
        fn add_two(value: i64) -> i64 { value + 2 }
        fn identity_function(function: fn(i64) -> i64) -> fn(i64) -> i64 { function }
        fn invoke<T>(function: T, value: i64) -> i64 { function(value) }
        fn apply_same<T>(function: fn(T) -> T, value: T) -> T { function(value) }
        fn identity_lambda<T>(value: T) -> T {
            let identity: fn(T) -> T = fn(item: T) -> T { item };
            identity(value)
        }

        fn main() -> i64 {
            let increment: fn(i64) -> i64 = fn(value: i64) -> i64 { value + 1 };
            let named: fn(i64) -> i64 = add_two;
            let returned = identity_function(increment);
            apply(returned, 40) - 41 + invoke(named, 40) - 42
                + apply_same(fn(value: i64) -> i64 { value + 1 }, 41) - 42
                + identity_lambda(42) - 42
        }
        "#,
        "function-values-higher-order",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn function_values_can_be_stored_in_tuples_and_structs() {
    let result = compile_and_run(
        r#"
        struct Handler { call: fn(i64) -> i64 }
        fn add_one(value: i64) -> i64 { value + 1 }

        fn main() -> i64 {
            let (function, value) = (add_one, 41);
            let handler = Handler { call: function };
            handler.call(value) - 42
        }
        "#,
        "function-values-aggregate-storage",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn nested_lambdas_are_non_capturing_and_have_unique_functions() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let outer: fn(i64) -> i64 = fn(value: i64) -> i64 {
                let inner: fn(i64) -> i64 = fn(n: i64) -> i64 { n + 1 };
                inner(value)
            };
            outer(41) - 42
        }
        "#,
        "function-values-nested-lambdas",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn lambdas_reject_lexical_captures() {
    assert_compile_error(
        "fn main() -> i64 { let captured = 1; let f = fn(value: i64) -> i64 { value + captured }; f(0) }",
        "function-values-capture-rejected",
        "lambda captures are not supported (`captured` is captured)",
    );
    assert_compile_error(
        "fn main() -> i64 { let captured = 1; let f = fn() -> i64 { captured = 2; 0 }; f() }",
        "function-values-capture-assignment-rejected",
        "lambda captures are not supported (`captured` is captured)",
    );
}

#[test]
fn function_types_check_call_arity_and_parameter_types() {
    assert_compile_error(
        "fn main() -> i64 { let f: fn(i64) -> i64 = fn(value: i64) -> i64 { value }; f() }",
        "function-values-arity",
        "function value expects 1 argument(s), got 0",
    );
    assert_compile_error(
        "fn main() -> i64 { let f: fn(i64) -> i64 = fn(value: i64) -> i64 { value }; f(true) }",
        "function-values-argument-type",
        "argument 1 to function value has type bool, expected i64",
    );
}

#[test]
fn function_values_cannot_be_stored_in_fixed_arrays_or_vectors() {
    assert_compile_error(
        "fn increment(value: i64) -> i64 { value + 1 } fn main() -> i64 { let functions = [increment, increment]; 0 }",
        "function-values-array-storage",
        "function values cannot be stored in arrays or vectors",
    );
    assert_compile_error(
        "fn increment(value: i64) -> i64 { value + 1 } fn main() -> i64 { let functions: Vec<fn(i64) -> i64> = vec_new(); 0 }",
        "function-values-vector-storage",
        "function values cannot be stored in arrays or vectors",
    );
}

#[test]
fn function_values_support_unit_returns_and_pointer_equality() {
    let result = compile_and_run(
        r#"
        fn noop() -> unit { }
        fn main() -> i64 {
            let first: fn() -> unit = noop;
            let second: fn() -> unit = noop;
            if first == second { first(); 0 } else { 1 }
        }
        "#,
        "function-values-unit-and-equality",
    );
    assert_eq!(result.status.code(), Some(0));
}
