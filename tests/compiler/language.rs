use crate::common::*;
use std::fs;

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

#[test]
fn bindings_cannot_be_redeclared_in_one_scope_or_escape_a_for_loop() {
    for (source, test_name, expected) in [
        (
            "fn main() -> i64 { let value = 1; let value = 2; 0 }\n",
            "duplicate-local",
            "`value` is already declared in this scope",
        ),
        (
            "fn main() -> i64 { for index in 0..1 { } index }\n",
            "for-index-out-of-scope",
            "undefined variable `index`",
        ),
        (
            "fn identity(value: i64, value: i64) -> i64 { value }\nfn main() -> i64 { 0 }\n",
            "duplicate-parameter",
            "duplicate parameter `value`",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn while_break_and_continue_change_loop_flow() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let i = 0;
            let total = 0;
            while i < 10 {
                i = i + 1;
                if i == 3 { continue; }
                if i == 7 { break; }
                total = total + i;
            }
            print_i64(total);
            0
        }
        "#,
        "while-break-continue",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "18\n");
}

#[test]
fn for_continue_runs_the_increment_and_break_targets_the_innermost_loop() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let total = 0;
            for i in 0..5 {
                if i == 1 { continue; }
                for j in 0..3 {
                    if j == 1 { break; }
                    total = total + i;
                }
            }
            print_i64(total);
            0
        }
        "#,
        "for-break-continue",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "9\n");
}

#[test]
fn break_and_continue_are_rejected_outside_loops() {
    for (statement, diagnostic, test_name) in [
        (
            "break;",
            "`break` is only valid inside a loop",
            "break-outside-loop",
        ),
        (
            "continue;",
            "`continue` is only valid inside a loop",
            "continue-outside-loop",
        ),
    ] {
        let source = format!("fn main() -> i64 {{\n    {statement}\n    0\n}}\n");
        let (dir, compile) = compile_source(&source, test_name);
        let stderr = String::from_utf8_lossy(&compile.stderr);
        assert!(!compile.status.success());
        assert!(
            stderr.contains(diagnostic),
            "unexpected diagnostic: {stderr}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn for_ranges_are_half_open_and_evaluate_bounds_once() {
    let result = compile_and_run(
        r#"
        fn bound() -> i64 {
            print_i64(99);
            6
        }
        fn main() -> i64 {
            let total = 0;
            for i in 0..bound() {
                total = total + i;
            }
            print_i64(total);
            0
        }
        "#,
        "for-range",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "99\n15\n");
}

#[test]
fn for_loop_variable_is_immutable() {
    let (dir, compile) = compile_source(
        "fn helper() -> i64 { 0 }\nfn main() -> i64 {\n    for i in 0..3 { i = i + 1; }\n    0\n}\n",
        "for-immutable",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("cannot assign to immutable loop variable `i`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn minimum_i64_literal_compiles_and_runs() {
    let result = compile_and_run(
        "fn main() -> i64 {\n    print_i64(-9223372036854775808);\n    0\n}\n",
        "minimum-i64",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "-9223372036854775808\n"
    );
}

#[test]
fn positive_literal_outside_i64_reports_source_location() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    9223372036854775808\n}\n",
        "positive-i64-overflow",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:5: integer literal is out of range for i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_type_errors_are_reported_before_codegen() {
    let (dir, compile) = compile_source("fn main() -> i64 {\n    true\n}\n", "return-type-error");
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:5: function result has type bool, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(!dir.join("program.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_checker_validates_function_argument_types() {
    let (dir, compile) = compile_source(
        "fn identity(value: i64) -> i64 { value }\nfn main() -> i64 {\n    identity(true)\n}\n",
        "argument-type-error",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:3:14: argument 1 to `identity` has type bool, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_checker_rejects_functions_with_a_fallthrough_path() {
    let (dir, compile) = compile_source(
        "fn maybe(x: bool) -> i64 {\n    if x { return 1; }\n}\nfn main() -> i64 { 0 }\n",
        "missing-return-path",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr
            .contains("test.apx:1:1: function `maybe` can reach its end without returning a value"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn entry_point_signature_is_required_and_must_be_exact() {
    for (source, test_name, expected) in [
        (
            "fn helper() -> i64 { 0 }\n",
            "missing-main",
            "program must define the required entry point `fn main() -> i64`",
        ),
        (
            "fn main(value: i64) -> i64 { value }\n",
            "main-with-parameter",
            "entry point must have signature `fn main() -> i64`",
        ),
        (
            "fn main() -> bool { true }\n",
            "main-wrong-return-type",
            "entry point must have signature `fn main() -> i64`",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn function_signatures_support_forward_calls_and_validate_call_arguments() {
    let result = compile_and_run(
        "fn main() -> i64 { later(41) }\nfn later(value: i64) -> i64 { value + 1 }\n",
        "forward-call",
    );
    assert_eq!(result.status.code(), Some(42));

    for (source, test_name, expected) in [
        (
            "fn main() -> i64 { missing() }\n",
            "unknown-function",
            "unknown function `missing`",
        ),
        (
            "fn add(a: i64, b: i64) -> i64 { a + b }\nfn main() -> i64 { add(1) }\n",
            "too-few-arguments",
            "function `add` expects 2 argument(s), got 1",
        ),
        (
            "fn add(a: i64) -> i64 { a }\nfn main() -> i64 { add(1, 2) }\n",
            "too-many-arguments",
            "function `add` expects 1 argument(s), got 2",
        ),
        (
            "fn main() -> i64 { print_i64(); 0 }\n",
            "builtin-wrong-argument-count",
            "function `print_i64` expects 1 argument(s), got 0",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn return_path_analysis_accepts_two_returning_branches() {
    let result = compile_and_run(
        r#"
        fn choose(flag: bool) -> i64 {
            if flag { return 11; } else { return 22; }
        }
        fn main() -> i64 {
            print_i64(choose(true));
            print_i64(choose(false));
            0
        }
        "#,
        "both-branches-return",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "11\n22\n");
}

#[test]
fn return_statements_must_match_the_declared_return_type() {
    for (source, test_name, expected) in [
        (
            "fn value() -> i64 { return; }\nfn main() -> i64 { 0 }\n",
            "bare-return-from-value-function",
            "`return;` is only valid in a function returning `unit`",
        ),
        (
            "fn value() -> i64 { return true; }\nfn main() -> i64 { 0 }\n",
            "wrong-explicit-return-type",
            "return value has type bool, expected i64",
        ),
        (
            "fn report() -> unit { 1 }\nfn main() -> i64 { 0 }\n",
            "value-from-unit-function",
            "function result has type i64, expected unit",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn parser_diagnostics_highlight_the_offending_source_token() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let x = ;\n    0\n}\n",
        "parser-location",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:13: expected expression, found `;`"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(
        stderr.contains(" 2 |     let x = ;"),
        "missing source line: {stderr}"
    );
    assert!(
        stderr.contains("   |             ^"),
        "missing caret: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unit_functions_can_be_called_and_return_implicitly_or_explicitly() {
    let result = compile_and_run(
        r#"
        fn report(value: i64) -> unit {
            print_i64(value);
        }
        fn maybe_report(should_report: bool) -> unit {
            if should_report {
                report(7);
                return;
            }
            report(8);
        }
        fn main() -> i64 {
            report(42);
            maybe_report(true);
            0
        }
        "#,
        "unit-functions",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n7\n");
}

#[test]
fn unit_function_results_cannot_be_used_as_values() {
    let (dir, compile) = compile_source(
        "fn report() -> unit { print_i64(1); }\nfn main() -> i64 { let result = report(); 0 }\n",
        "unit-as-value",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("cannot bind a value of type `unit`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unit_parameters_are_rejected() {
    let (dir, compile) = compile_source(
        "fn invalid(value: unit) -> i64 { 0 }\nfn main() -> i64 { 0 }\n",
        "unit-parameter",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("parameters cannot have type `unit`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn nested_aggregate_fields_respect_immutable_bindings() {
    for (source, diagnostic, name) in [
        (
            "struct Inner { value: i64 } struct Outer { inner: Inner } fn main() -> i64 { const outer = Outer { inner: Inner { value: 1 } }; outer.inner.value = 2; 0 }",
            "cannot assign through immutable binding `outer`",
            "const-nested-struct-field-assignment",
        ),
        (
            "struct Outer { values: [i64; 2] } fn main() -> i64 { const outer = Outer { values: [1, 2] }; outer.values[0] = 3; 0 }",
            "cannot assign through immutable binding `outer`",
            "const-struct-array-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn if_expressions_preserve_bool_and_string_result_types() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let flag = if true { false } else { true };
            let message = if flag { "wrong" } else { "selected" };
            println(message);
            0
        }
        "#,
        "if-expression-bool-string",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "selected\n");
}
