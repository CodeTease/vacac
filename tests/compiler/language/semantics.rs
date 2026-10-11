use crate::common::*;
use std::fs;

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
        stderr.contains("test.vca:2:5: integer literal is out of range for i64"),
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
        stderr.contains("test.vca:2:5: function result has type bool, expected i64"),
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
        stderr.contains("test.vca:3:14: argument 1 to `identity` has type bool, expected i64"),
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
            .contains("test.vca:1:1: function `maybe` can reach its end without returning a value"),
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
