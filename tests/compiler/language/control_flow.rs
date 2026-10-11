use crate::common::*;
use std::fs;

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
