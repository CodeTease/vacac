use crate::common::*;

#[test]
fn bitwise_operators_and_signed_shifts_work_on_i64() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let and_result = 12 & 10;
            let or_result = 12 | 10;
            let xor_result = 12 ^ 10;
            let left_result = 3 << 4;
            let right_result = -16 >> 2;
            print_i64(and_result + or_result + xor_result + left_result + right_result);
            print_i64((1 << 63) >> 63);
            0
        }
        "#,
        "bitwise-operators",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "72\n-1\n");
}

#[test]
fn bitwise_operator_precedence_is_distinct_and_ordered() {
    let result = compile_and_run(
        "fn main() -> i64 { print_i64(1 | 2 ^ 3 & 1 << 2 + 1); 0 }\n",
        "bitwise-precedence",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "3\n");
}

#[test]
fn shifts_reject_negative_or_out_of_range_counts_at_runtime() {
    for (source, name) in [
        ("fn main() -> i64 { 1 << -1 }\n", "negative-shift-count"),
        ("fn main() -> i64 { 1 >> 64 }\n", "large-shift-count"),
    ] {
        let result = compile_and_run(source, name);
        assert_eq!(result.status.code(), Some(1), "{name}");
    }
}

#[test]
fn bitwise_operators_and_shifts_require_i64_operands() {
    assert_compile_error(
        "fn main() -> i64 { true & false; 0 }\n",
        "boolean-bitwise-operands",
        "bitwise operands has type bool, expected i64",
    );
    assert_compile_error(
        "fn main() -> i64 { true << false; 0 }\n",
        "boolean-shift-operand",
        "shift operands has type bool, expected i64",
    );
}
