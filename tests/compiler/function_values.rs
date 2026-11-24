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
fn mutable_captures_persist_and_aliases_share_environment_state() {
    let result = compile_and_run(
        r#"
        struct Holder { call: fn() -> i64 }
        fn apply(callback: fn() -> i64) -> i64 { callback() }

        fn main() -> i64 {
            let count = 0;
            const first: fn() -> i64 = fn() -> i64 {
                count = count + 1;
                count
            };
            const alias = first;
            const second: fn() -> i64 = fn() -> i64 {
                count = count + 1;
                count
            };
            print_i64(first());
            print_i64(alias());
            print_i64(second());
            print_i64(apply(first));
            const holder = Holder { call: first };
            print_i64(holder.call());
            const functions: Vec<fn() -> i64> = vec_new();
            vec_push(functions, first);
            print_i64(functions[0]());
            print_i64(if first == alias && first != second { 1 } else { 0 });
            print_i64(count);
            first() - 6
        }
        "#,
        "function-values-shared-capture-state",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "1\n2\n1\n3\n4\n5\n1\n0\n"
    );
}

#[test]
fn captured_aggregates_and_parameters_support_mutation() {
    let result = compile_and_run(
        r#"
        struct Counter { value: i64 }

        fn make_parameter_counter(value: i64) -> fn() -> i64 {
            fn() -> i64 {
                value = value + 1;
                value
            }
        }

        fn make_box<T>(initial: T) -> fn(T) -> T {
            let value = initial;
            fn(next: T) -> T {
                value = next;
                value
            }
        }

        fn main() -> i64 {
            let item = Counter { value: 40 };
            const item_counter: fn() -> i64 = fn() -> i64 {
                item.value = item.value + 1;
                item.value
            };
            print_i64(item_counter());
            print_i64(item_counter());
            print_i64(item.value);

            let numbers = [40, 0];
            const array_counter: fn() -> i64 = fn() -> i64 {
                numbers[0] = numbers[0] + 2;
                numbers[0]
            };
            print_i64(array_counter());
            print_i64(array_counter());
            print_i64(numbers[0]);

            const parameter_counter = make_parameter_counter(40);
            print_i64(parameter_counter());
            print_i64(parameter_counter());

            const int_box = make_box(0);
            const bool_box = make_box(false);
            print_i64(int_box(42));
            print_i64(if bool_box(true) { 1 } else { 0 });
            print_i64(if int_box(7) == 7 { 1 } else { 0 });

            const empty = ();
            const unit_capture: fn() -> i64 = fn() -> i64 { empty; 42 };
            print_i64(unit_capture() - 42);

            let zeroes: [i64; 0] = [];
            const zero_capture: fn() -> i64 = fn() -> i64 { len(zeroes) };
            zero_capture()
        }
        "#,
        "function-values-mutable-aggregate-captures",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "41\n42\n40\n42\n44\n40\n41\n42\n42\n1\n1\n0\n"
    );
}

#[test]
fn nested_closures_snapshot_current_state_without_write_back() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let count = 0;
            const outer: fn() -> fn() -> i64 = fn() -> fn() -> i64 {
                count = count + 5;
                fn() -> i64 {
                    count = count + 1;
                    count
                }
            };
            const first_inner = outer();
            print_i64(first_inner());
            print_i64(first_inner());
            const second_inner = outer();
            print_i64(second_inner());
            print_i64(count);
            second_inner() - 12
        }
        "#,
        "function-values-nested-mutable-captures",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "6\n7\n11\n0\n");
}

#[test]
fn reentrant_closure_calls_observe_updates_through_the_same_environment() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let count = 0;
            const functions: Vec<fn() -> i64> = vec_new();
            const bump: fn() -> i64 = fn() -> i64 {
                count = count + 1;
                if count == 1 {
                    functions[0]()
                } else {
                    count
                }
            };
            vec_push(functions, bump);
            bump() - 2
        }
        "#,
        "function-values-reentrant-capture",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn immutable_captures_still_reject_assignment() {
    assert_compile_error(
        "fn main() -> i64 { const captured = 1; let f = fn() -> i64 { captured = 2; 0 }; f() }",
        "function-values-const-capture-assignment-rejected",
        "cannot assign to immutable binding `captured`",
    );
    assert_compile_error(
        "struct Item { value: i64 } fn main() -> i64 { const item = Item { value: 1 }; let f = fn() -> i64 { item.value = 2; 0 }; f() }",
        "function-values-const-capture-field-assignment-rejected",
        "cannot assign through immutable binding `item`",
    );
    assert_compile_error(
        "fn main() -> i64 { for index in 0..1 { let f = fn() -> i64 { index = 2; index }; return f(); } 0 }",
        "function-values-loop-capture-assignment-rejected",
        "cannot assign to immutable loop variable `index`",
    );
}

#[test]
fn captures_require_a_whole_binding_snapshot_free_of_mutable_loans() {
    assert_compile_error(
        "struct Pair { left: i64, right: i64 } fn main() -> i64 { let pair = Pair { left: 1, right: 2 }; const reference: &mut i64 = &mut pair.left; let closure = fn() -> i64 { pair.right }; closure() }",
        "function-values-mutably-borrowed-capture",
        "cannot capture `pair` while it is mutably borrowed",
    );

    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }

        fn main() -> i64 {
            let pair = Pair { left: 1, right: 2 };
            const reference: &i64 = &pair.left;
            const closure: fn() -> i64 = fn() -> i64 { pair.right };
            closure() - 2 + *reference - 1
        }
        "#,
        "function-values-shared-borrowed-capture",
    );
    assert_eq!(result.status.code(), Some(0));
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
