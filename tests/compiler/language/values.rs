use crate::common::*;

#[test]
fn recursive_factorial_compiles_to_native_executable() {
    let result = compile_and_run(
        r#"
        fn factorial(n: i64) -> i64 {
            if n <= 1 { return 1; }
            return n * factorial(n - 1);
        }
        fn main() -> i64 {
            print_i64(factorial(10));
            0
        }
        "#,
        "factorial",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "3628800\n");
}

#[test]
fn mutation_loops_and_short_circuit_boolean_operators_work() {
    let result = compile_and_run(
        r#"
        fn should_not_run() -> bool {
            print_i64(99);
            true
        }
        fn main() -> i64 {
            let i = 0;
            let total = 0;
            while i < 6 {
                total = total + i;
                i = i + 1;
            }
            let truth: bool = total == 15 && !(total < 0);
            let shorted: bool = false && should_not_run();
            if truth && !shorted { print_i64(total); } else { print_i64(0); }
            0
        }
        "#,
        "loops",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "15\n");
}

#[test]
fn boolean_operators_short_circuit_or_and_evaluate_required_operands() {
    let result = compile_and_run(
        r#"
        fn mark(value: i64) -> bool {
            print_i64(value);
            true
        }
        fn main() -> i64 {
            let skipped = true || mark(99);
            let evaluated_or = false || mark(1);
            let evaluated_and = true && mark(2);
            if skipped && evaluated_or && evaluated_and { return 0; } else { return 1; }
        }
        "#,
        "boolean-short-circuit",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n2\n");
}

#[test]
fn operator_precedence_and_left_associativity_are_preserved() {
    let result = compile_and_run(
        "fn main() -> i64 {\n    if 2 + 3 * 4 == 14 && !(1 > 2) || false { return 0; } else { return 1; }\n}\n",
        "operator-precedence",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        "fn main() -> i64 { if 10 - 3 - 2 == 5 { return 0; } else { return 1; } }\n",
        "operator-associativity",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn if_expressions_produce_values_and_compose_with_other_expressions() {
    let result = compile_and_run(
        r#"
        fn choose(flag: bool, left: i64, right: i64) -> i64 {
            if flag { left } else { right }
        }
        fn main() -> i64 {
            let first = if 3 < 4 { choose(true, 19, 23) } else { 99 };
            let second = if false { 5 } else { if true { 7 } else { 11 } };
            print_i64(first + second);
            0
        }
        "#,
        "if-expression-values",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "26\n");
}

#[test]
fn if_expression_branches_support_scoped_statements() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let selected = if true {
                let local = 41;
                local + 1
            } else {
                let local = 8;
                local
            };
            print_i64(selected);
            0
        }
        "#,
        "if-expression-scopes",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn if_expression_requires_matching_non_unit_value_branches() {
    assert_compile_error(
        "fn main() -> i64 { let result = if true { 1 } else { false }; 0 }\n",
        "if-expression-branch-types",
        "`if` expression branches have different types: i64 and bool",
    );
    assert_compile_error(
        "fn main() -> i64 { let result = if true { print_i64(1) } else { print_i64(2) }; 0 }\n",
        "if-expression-unit-branches",
        "`if` expression branches must produce a value",
    );
}

#[test]
fn if_expression_requires_an_else_branch() {
    assert_compile_error(
        "fn main() -> i64 { let result = if true { 1 }; 0 }\n",
        "if-expression-missing-else",
        "expected `else` in an `if` expression",
    );
}

#[test]
fn if_expression_can_be_discarded_as_an_expression_statement() {
    let result = compile_and_run(
        "fn main() -> i64 { if true { print_i64(1); 2 } else { print_i64(3); 4 }; 0 }\n",
        "if-expression-statement",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n");
}

#[test]
fn lexical_scopes_allow_shadowing_without_mutating_outer_bindings() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 10;
            if true {
                let value = 20;
                value = 21;
                print_i64(value);
            }
            print_i64(value);

            let index = 50;
            for index in 0..2 {
                print_i64(index);
            }
            print_i64(index);
            0
        }
        "#,
        "scope-shadowing",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "21\n10\n0\n1\n50\n"
    );
}
