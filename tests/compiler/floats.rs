use crate::common::*;

#[test]
fn f64_literals_bindings_functions_arithmetic_and_printing_work() {
    let result = compile_and_run(
        r#"
        fn combine(left: f64, right: f64) -> f64 {
            (left + right) * 2.0 - right / 2.0
        }

        fn passthrough(value: f64) -> f64 {
            return value;
        }

        fn main() -> i64 {
            let first = 1.25;
            first = first + 0.0;
            let second: f64 = 2e0;
            let result: f64 = combine(first, second);
            print_f64(passthrough(result));
            print_f64(first - second);
            print_f64(first * second);
            print_f64(first / second);
            print_f64(first % second);
            print_f64(-0.0);
            0
        }
        "#,
        "f64-runtime",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "5.5\n-0.75\n2.5\n0.625\n1.25\n-0\n"
    );
}

#[test]
fn f32_suffixed_literals_bindings_functions_arithmetic_and_printing_work() {
    let result = compile_and_run(
        r#"
        fn combine(left: f32, right: f32) -> f32 {
            (left + right) * 2.0f32 - right / 2.0f32
        }

        fn passthrough(value: f32) -> f32 {
            return value;
        }

        fn main() -> i64 {
            let first = 1.25f32;
            first = first + 0.0f32;
            let second: f32 = 2e0f32;
            let result: f32 = combine(first, second);
            print_f32(passthrough(result));
            print_f32(first - second);
            print_f32(first * second);
            print_f32(first / second);
            print_f32(first % second);
            print_f32(-0.0f32);
            0
        }
        "#,
        "f32-runtime",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "5.5\n-0.75\n2.5\n0.625\n1.25\n-0\n"
    );
}

#[test]
fn f32_rounding_comparisons_and_ieee_nan_semantics_work() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let rounded: f32 = 16777217f32;
            print_f32(rounded);
            let nan = 0f32 / 0f32;
            if rounded == 16777216f32 && rounded != 16777218f32
                && 1f32 < 2f32 && 2f32 >= 2f32
                && nan != nan && !(nan < 1f32) {
                return 0;
            }
            1
        }
        "#,
        "f32-comparisons",
    );
    assert!(result.status.success());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "16777216\n");
}

#[test]
fn f32_rejects_implicit_conversions_and_unsupported_operators() {
    assert_compile_error(
        "fn main() -> i64 { let value: f32 = 1; 0 }",
        "f32-no-int-coercion",
        "initializer for `value` has type i64, expected f32",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1f32 + 1.0; 0 }",
        "f32-no-mixed-arithmetic",
        "operator operands have different types: f32 and f64",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1f32 & 1f32; 0 }",
        "f32-no-bitwise",
        "bitwise operands has type f32, expected i64",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1f32 && 2f32; 0 }",
        "f32-no-logical",
        "logical operands has type f32, expected bool",
    );
}

#[test]
fn f32_arrays_vectors_struct_fields_and_enum_payloads_work() {
    let result = compile_and_run(
        r#"
        struct Readings { samples: [f32; 2], latest: f32 }
        enum Reading { Narrow(f32) }

        fn update(values: [f32; 2]) -> [f32; 2] {
            values[1] = values[1] + 1f32;
            values
        }

        fn update_readings(data: Readings) -> Readings {
            data.latest = data.latest + 0.5f32;
            data
        }

        fn payload(value: Reading) -> f32 {
            match value {
                Reading::Narrow(number) => { number },
            }
        }

        fn main() -> i64 {
            let values = update([1.5f32, 2.5f32]);
            let data = update_readings(Readings { samples: values, latest: values[0] });
            let vector: Vec<f32> = vec_new();
            vec_push(vector, data.latest);
            vec_push(vector, payload(Reading::Narrow(data.samples[1])));
            vector[0] = vector[0] + vector[1];
            for value in vector { print_f32(value); }
            let selected = match vector[0] {
                5.5f32 => { 40 },
                _ => { 0 },
            };
            print_i64(selected);
            0
        }
        "#,
        "f32-aggregate-runtime",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "5.5\n3.5\n40\n");
}

#[test]
fn f32_match_patterns_use_ieee_equality_and_require_wildcards() {
    let result = compile_and_run(
        r#"
        fn classify(value: f32) -> i64 {
            match value {
                1f32 => { 10 },
                -0.0f32 => { 20 },
                _ => { 30 },
            }
        }
        fn main() -> i64 {
            let nan = 0f32 / 0f32;
            print_i64(classify(1f32));
            print_i64(classify(0f32));
            print_i64(classify(nan));
            0
        }
        "#,
        "f32-match-ieee",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "10\n20\n30\n");
}

#[test]
fn f32_match_patterns_reject_mismatches_duplicates_and_missing_wildcards() {
    assert_compile_error(
        "fn main() -> i64 { let value: f32 = 1f32; match value { 1.0 => { 0 }, _ => { 1 } } }",
        "f32-match-pattern-width",
        "match pattern type does not match f32 value",
    );
    assert_compile_error(
        "fn main() -> i64 { match 1f32 { 1f32 => { 0 } } }",
        "f32-match-non-exhaustive",
        "non-exhaustive match on `f32`; add a wildcard (`_`) arm",
    );
    assert_compile_error(
        "fn main() -> i64 { match 0f32 { 0f32 => { 0 }, -0f32 => { 1 }, _ => { 2 } } }",
        "f32-match-signed-zero-duplicate",
        "duplicate match arm for `-0`",
    );
}

#[test]
fn print_f32_is_a_reserved_builtin_name() {
    assert_compile_error(
        "fn print_f32(value: f32) -> unit { } fn main() -> i64 { 0 }",
        "f32-reserved-name",
        "`print_f32` is a reserved compiler/runtime name",
    );
}

#[test]
fn malformed_and_out_of_range_f32_literals_report_lexer_diagnostics() {
    assert_compile_error(
        "fn main() -> i64 { let value = 1e+f32; 0 }",
        "f32-malformed-exponent",
        "floating-point exponent must contain at least one digit",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1e39f32; 0 }",
        "f32-overflow-literal",
        "f32 literal is out of range",
    );
}

#[test]
fn f64_if_expressions_comparisons_and_ieee_nan_semantics_work() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = if 1.5 < 2.0 && 2.0 >= 2.0 { 4.0 } else { 7.0 };
            let nan = 0.0 / 0.0;
            if value == 4.0 && value != 3.0 && nan != nan && !(nan < 1.0) {
                return 0;
            }
            1
        }
        "#,
        "f64-comparisons",
    );
    assert!(result.status.success());
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn f64_rejects_implicit_conversions_and_unsupported_operators() {
    assert_compile_error(
        "fn main() -> i64 { let value: f64 = 1; 0 }",
        "f64-no-int-coercion",
        "initializer for `value` has type i64, expected f64",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1.0 + 1; 0 }",
        "f64-no-mixed-arithmetic",
        "operator operands have different types: f64 and i64",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1.0 & 1.0; 0 }",
        "f64-no-bitwise",
        "bitwise operands has type f64, expected i64",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1.0 && 2.0; 0 }",
        "f64-no-logical",
        "logical operands has type f64, expected bool",
    );
}

#[test]
fn f64_arrays_vectors_struct_fields_and_enum_payloads_work() {
    let result = compile_and_run(
        r#"
        struct Readings { samples: [f64; 2], latest: f64 }
        enum Reading { Wide(f64) }

        fn update(values: [f64; 2]) -> [f64; 2] {
            values[1] = values[1] + 1.0;
            values
        }

        fn update_readings(data: Readings) -> Readings {
            data.latest = data.latest + 0.5;
            data
        }

        fn payload(value: Reading) -> f64 {
            match value {
                Reading::Wide(number) => { number },
            }
        }

        fn main() -> i64 {
            let values = update([1.5, 2.5]);
            let data = update_readings(Readings { samples: values, latest: values[0] });
            let vector: Vec<f64> = vec_new();
            vec_push(vector, data.latest);
            vec_push(vector, payload(Reading::Wide(data.samples[1])));
            vector[0] = vector[0] + vector[1];
            for value in vector { print_f64(value); }
            let selected = match vector[0] {
                5.5 => { 40 },
                _ => { 0 },
            };
            print_i64(selected);
            0
        }
        "#,
        "f64-aggregate-runtime",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "5.5\n3.5\n40\n");
}

#[test]
fn f64_match_patterns_use_ieee_equality_and_require_wildcards() {
    let result = compile_and_run(
        r#"
        fn classify(value: f64) -> i64 {
            match value {
                1.0 => { 10 },
                -0.0 => { 20 },
                _ => { 30 },
            }
        }
        fn main() -> i64 {
            let nan = 0.0 / 0.0;
            print_i64(classify(1.0));
            print_i64(classify(0.0));
            print_i64(classify(nan));
            0
        }
        "#,
        "f64-match-ieee",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "10\n20\n30\n");
}

#[test]
fn f64_match_patterns_reject_mismatches_duplicates_and_missing_wildcards() {
    assert_compile_error(
        "fn main() -> i64 { let value: f64 = 1.0; match value { 1f32 => { 0 }, _ => { 1 } } }",
        "f64-match-pattern-width",
        "match pattern type does not match f64 value",
    );
    assert_compile_error(
        "fn main() -> i64 { match 1.0 { 1.0 => { 0 } } }",
        "f64-match-non-exhaustive",
        "non-exhaustive match on `f64`; add a wildcard (`_`) arm",
    );
    assert_compile_error(
        "fn main() -> i64 { match 0.0 { 0.0 => { 0 }, -0.0 => { 1 }, _ => { 2 } } }",
        "f64-match-signed-zero-duplicate",
        "duplicate match arm for `-0`",
    );
}

#[test]
fn print_f64_is_a_reserved_builtin_name() {
    assert_compile_error(
        "fn print_f64(value: f64) -> unit { } fn main() -> i64 { 0 }",
        "f64-reserved-name",
        "`print_f64` is a reserved compiler/runtime name",
    );
}

#[test]
fn malformed_and_out_of_range_f64_literals_report_lexer_diagnostics() {
    assert_compile_error(
        "fn main() -> i64 { let value = 1e+; 0 }",
        "f64-malformed-exponent",
        "floating-point exponent must contain at least one digit",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1e9999; 0 }",
        "f64-overflow-literal",
        "f64 literal is out of range",
    );
}
