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
fn returned_closures_keep_by_value_captures_alive() {
    let result = compile_and_run(
        r#"
        fn make(value: i64) -> fn() -> i64 {
            let captured = value;
            let closure: fn() -> i64 = fn() -> i64 { captured };
            captured = 0;
            closure
        }

        fn main() -> i64 {
            let first = make(40);
            let copy = first;
            let second = make(40);
            if first == copy && first != second { first() - 40 } else { 1 }
        }
        "#,
        "function-values-captured-closure",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn mutable_locals_are_captured_as_creation_time_snapshots() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 40;
            let closure: fn() -> i64 = fn() -> i64 { value };
            value = 2;
            closure() - 40
        }
        "#,
        "function-values-capture-snapshot",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn lambda_parameters_and_locals_shadow_capture_candidates() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 40;
            let parameter_shadow: fn(i64) -> i64 = fn(value: i64) -> i64 { value + 1 };
            let local_shadow: fn() -> i64 = fn() -> i64 {
                let value = 2;
                value
            };
            parameter_shadow(40) + local_shadow() - 43
        }
        "#,
        "function-values-capture-shadowing",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn nested_returned_closures_propagate_captures() {
    let result = compile_and_run(
        r#"
        fn make(value: i64) -> fn() -> fn() -> i64 {
            fn() -> fn() -> i64 {
                fn() -> i64 { value }
            }
        }

        fn main() -> i64 {
            let outer = make(42);
            let inner = outer();
            inner() - 42
        }
        "#,
        "function-values-nested-capture",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn captured_closures_can_be_stored_in_aggregates() {
    let result = compile_and_run(
        r#"
        struct Handler { call: fn(i64) -> i64 }

        fn main() -> i64 {
            let amount = 2;
            let handler = Handler {
                call: fn(value: i64) -> i64 { value + amount },
            };
            handler.call(40) - 42
        }
        "#,
        "function-values-captured-closure-aggregate",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn captured_bindings_cannot_be_assigned_in_a_closure() {
    assert_compile_error(
        "fn main() -> i64 { let captured = 1; let f = fn() -> i64 { captured = 2; 0 }; f() }",
        "function-values-capture-assignment-rejected",
        "cannot assign to captured binding `captured`",
    );
    assert_compile_error(
        "struct Item { value: i64 } fn main() -> i64 { let item = Item { value: 1 }; let f = fn() -> i64 { item.value = 2; 0 }; f() }",
        "function-values-capture-field-assignment-rejected",
        "cannot assign to captured binding `item`",
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
fn function_values_work_in_fixed_arrays_with_captures_mutation_and_iteration() {
    let result = compile_and_run(
        r#"
        fn increment(value: i64) -> i64 { value + 1 }

        fn main() -> i64 {
            let amount = 2;
            let captured: fn(i64) -> i64 = fn(value: i64) -> i64 { value + amount };
            let named: fn(i64) -> i64 = increment;
            let mut_functions: [fn(i64) -> i64; 4] = [
                named,
                captured,
                captured,
                fn(value: i64) -> i64 { value + amount },
            ];
            print_i64(if mut_functions[1] == mut_functions[2] { 1 } else { 0 });
            mut_functions[0] = captured;
            for function in mut_functions {
                print_i64(function(40));
            }
            if mut_functions[0](40) == 42 { 0 } else { 2 }
        }
        "#,
        "function-values-array-storage",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "1\n42\n42\n42\n42\n"
    );
}

#[test]
fn generic_functions_accept_arrays_of_function_values() {
    let result = compile_and_run(
        r#"
        fn increment(value: i64) -> i64 { value + 1 }
        fn apply_first<T>(functions: [fn(T) -> T; 2], value: T) -> T {
            functions[0](value)
        }

        fn main() -> i64 {
            apply_first([increment, increment], 41) - 42
        }
        "#,
        "function-values-generic-array",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn function_array_signatures_must_match_exactly() {
    assert_compile_error(
        "fn increment(value: i64) -> i64 { value + 1 } fn main() -> i64 { let functions: [fn(i64) -> i64; 2] = [increment, fn(value: bool) -> bool { value }]; 0 }",
        "function-values-array-signature-mismatch",
        "array element has type fn(bool) -> bool, expected fn(i64) -> i64",
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

#[test]
fn builtin_functions_use_the_closure_abi_when_used_as_values() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let printer: fn(i64) -> unit = print_i64;
            printer(42);
            0
        }
        "#,
        "function-values-builtin-adapter",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout, b"42\n");
}
