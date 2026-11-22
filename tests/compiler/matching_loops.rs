use crate::common::*;
use std::fs;

#[test]
fn match_scrutinee_shadowing_a_struct_name_is_not_a_struct_literal() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        fn main() -> i64 {
            let Point = 1;
            match Point { 1 => { 0 }, _ => { 1 } }
        }
        "#,
        "match-shadowed-struct-name",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn boolean_and_integer_matches_produce_values() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let flag = false;
            let bool_result = match flag {
                true => { 10 },
                false => { 20 },
            };
            let number = -9223372036854775808;
            let int_result = match number {
                0 => { 1 },
                -9223372036854775808 => { 22 },
                _ => { 3 },
            };
            print_i64(bool_result + int_result);
            0
        }
        "#,
        "scalar-matches",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn scalar_matches_allow_wildcards_and_integer_fallback() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let flag = true;
            let bool_result = match flag { _ => { 17 } };
            let number = 99;
            let int_result = match number { -1 => { 0 }, _ => { 25 } };
            print_i64(bool_result + int_result);
            0
        }
        "#,
        "scalar-match-wildcards",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn match_scrutinees_accept_equality_bitwise_and_logical_operators() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let equality = match 1 == 1 { true => { 10 }, false => { 0 } };
            let inequality = match 1 != 2 { true => { 10 }, false => { 0 } };
            let bit_and = match 6 & 3 { 2 => { 10 }, _ => { 0 } };
            let bit_xor = match 6 ^ 3 { 5 => { 10 }, _ => { 0 } };
            let bit_or = match 6 | 3 { 7 => { 10 }, _ => { 0 } };
            let logical_and = match true && true { true => { 10 }, false => { 0 } };
            let logical_or = match false || true { true => { 10 }, false => { 0 } };
            print_i64(equality + inequality + bit_and + bit_xor + bit_or + logical_and + logical_or);
            0
        }
        "#,
        "match-scrutinee-operator-precedence",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "70\n");
}

#[test]
fn match_scrutinees_parse_with_full_binary_operator_precedence() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let a = 2;
            let b = 3;
            let mixed = match a + b * a == 8 && b < 4 {
                true => { 40 },
                false => { 0 },
            };
            let grouped = match (1 == 1) || (2 > 3) {
                true => { 2 },
                false => { 0 },
            };
            print_i64(mixed + grouped);
            0
        }
        "#,
        "match-scrutinee-mixed-precedence",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn unit_producing_matches_are_usable_as_statements() {
    let result = compile_and_run(
        r#"
        enum Color { Red, Green }
        struct Point { x: i64, y: i64 }
        fn report(value: i64) -> unit {
            match value {
                0 => { println("zero") },
                _ => { println("other") },
            }
        }
        fn main() -> i64 {
            let color = Color::Green;
            match color {
                Color::Red => { print_i64(1) },
                Color::Green => { print_i64(2) },
            };
            match 7 {
                0 => { print_i64(3) },
                _ => { print_i64(4) },
            };
            let point = Point { x: 5, y: 6 };
            match point {
                Point { x, y } => { print_i64(x + y) },
            };
            println("");
            report(0);
            report(9);
            0
        }
        "#,
        "unit-matches",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "2\n4\n11\n\nzero\nother\n"
    );
}

#[test]
fn unit_matches_do_not_build_a_result_phi() {
    let (dir, result) = run_compiler_with_args(
        r#"
        enum Flag { On, Off }
        fn noop() -> unit { return; }
        fn main() -> i64 {
            let flag = Flag::On;
            match flag {
                Flag::On => { noop() },
                Flag::Off => { noop() },
            };
            match 1 {
                1 => { noop() },
                _ => { noop() },
            };
            0
        }
        "#,
        "unit-match-phi",
        &["--emit-llvm", "unit-match.ll", "--quiet"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(dir.join("unit-match.ll")).unwrap();
    assert!(!ir.contains("match.value"), "{ir}");
    assert!(!ir.contains(" phi "), "{ir}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn match_arms_cannot_mix_unit_and_value_results() {
    assert_compile_error(
        "fn main() -> i64 { match 1 { 1 => { print_i64(1) }, _ => { 2 } }; 0 }\n",
        "match-unit-mixed-arms",
        "match arm result has type i64, expected unit",
    );
    assert_compile_error(
        "fn main() -> i64 { let x = match 1 { _ => { print_i64(1) } }; 0 }\n",
        "match-unit-bound",
        "cannot bind a value of type `unit`",
    );
}

#[test]
fn value_producing_loop_returns_from_break_and_supports_continue() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let index = 0;
            let answer = loop {
                index = index + 1;
                if index < 4 {
                    continue;
                } else {
                    break index * 10 + 2;
                }
            };
            print_i64(answer);
            0
        }
        "#,
        "value-loop-break",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn nested_value_loops_use_the_innermost_break_target() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let outer = loop {
                let inner = loop { break 40; };
                break inner + 2;
            };
            print_i64(outer);
            0
        }
        "#,
        "nested-value-loops",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn statement_loop_break_does_not_exit_enclosing_value_loop() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let answer = loop {
                let count = 0;
                while true {
                    count = count + 1;
                    break;
                }
                break count + 41;
            };
            print_i64(answer);
            0
        }
        "#,
        "statement-loop-inside-value-loop",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn boolean_match_must_be_exhaustive_and_reject_duplicate_arms() {
    assert_compile_error(
        "fn main() -> i64 { match true { true => { 1 } } }\n",
        "bool-match-non-exhaustive",
        "non-exhaustive match on `bool`; missing value: false",
    );
    assert_compile_error(
        "fn main() -> i64 { match true { true => { 1 }, true => { 2 }, false => { 3 } } }\n",
        "bool-match-duplicate",
        "duplicate match arm for `true`",
    );
}

#[test]
fn scalar_match_rejects_arms_after_a_binding_or_wildcard() {
    assert_compile_error(
        "fn main() -> i64 { match 5 { x => { x }, _ => { 2 } } }\n",
        "integer-match-binding-before-wildcard",
        "unreachable match arm after wildcard",
    );
    assert_compile_error(
        "fn main() -> i64 { match 5 { _ => { 1 }, _ => { 2 } } }\n",
        "integer-match-duplicate-wildcard",
        "unreachable match arm after wildcard",
    );
}

#[test]
fn integer_match_requires_a_wildcard_and_scalar_patterns_must_match_value_type() {
    assert_compile_error(
        "fn main() -> i64 { match 1 { 1 => { 1 } } }\n",
        "integer-match-non-exhaustive",
        "non-exhaustive match on `i64`; add a wildcard (`_`) arm",
    );
    assert_compile_error(
        "fn main() -> i64 { match true { 1 => { 1 }, _ => { 2 } } }\n",
        "bool-match-pattern-type",
        "match pattern type does not match bool value",
    );
}

#[test]
fn unreachable_statements_after_return_are_rejected_during_type_checking() {
    assert_compile_error(
        r#"
        fn main() -> i64 {
            let x = loop {
                return 0;
                break 1;
            };
            x
        }
        "#,
        "unreachable-statement-after-return",
        "unreachable statement after `return`",
    );
}

#[test]
fn value_loop_breaks_must_be_typed_and_bare_breaks_stay_statement_loop_only() {
    assert_compile_error(
        "fn main() -> i64 { let result = loop { break 1; break true; }; result }\n",
        "loop-break-type-mismatch",
        "loop break value has type bool, expected i64",
    );
    assert_compile_error(
        "fn main() -> i64 { let result = loop { break; }; 0 }\n",
        "loop-break-value-required",
        "a value-producing loop requires `break` with a value",
    );
    assert_compile_error(
        "fn main() -> i64 { while true { break 1; } 0 }\n",
        "statement-loop-break-value",
        "`break` with a value is only valid inside a value-producing `loop`",
    );
    assert_compile_error(
        "fn main() -> i64 { let result = loop { print_i64(1); }; 0 }\n",
        "loop-break-missing",
        "value-producing loop must contain a `break` with a value",
    );
}
