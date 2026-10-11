use crate::common::*;

#[test]
fn vaca_main_result_becomes_process_exit_status() {
    let result = compile_and_run("fn main() -> i64 { 37 }\n", "main-exit-status");
    assert_eq!(result.status.code(), Some(37));
}

#[test]
fn read_i64_parses_signed_decimal_values_and_i64_minimum() {
    let result = compile_and_run_with_stdin(
        "fn main() -> i64 {\n    let value = read_i64();\n    print_i64(value);\n    0\n}\n",
        "read-i64-values",
        b" \t-9223372036854775808\n",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "-9223372036854775808\n"
    );
}

#[test]
fn read_i64_accepts_plus_sign_and_reads_consecutive_tokens() {
    let result = compile_and_run_with_stdin(
        "fn main() -> i64 {\n    print_i64(read_i64());\n    print_i64(read_i64());\n    print_i64(read_i64());\n    0\n}\n",
        "read-i64-consecutive",
        b" +42\n-7 \t9",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n-7\n9\n");
}

#[test]
fn read_i64_rejects_eof_invalid_tokens_and_overflow() {
    let source = "fn main() -> i64 { read_i64(); 0 }\n";
    for (input, name) in [
        (&b""[..], "read-i64-eof"),
        (&b"+\n"[..], "read-i64-sign-only"),
        (&b"12x\n"[..], "read-i64-invalid-token"),
        (&b"9223372036854775808\n"[..], "read-i64-positive-overflow"),
        (&b"-9223372036854775809\n"[..], "read-i64-negative-overflow"),
    ] {
        let result = compile_and_run_with_stdin(source, name, input);
        assert_eq!(result.status.code(), Some(1), "input: {input:?}");
    }
}

#[test]
fn division_and_remainder_trap_on_zero_and_signed_overflow() {
    for (expression, name) in [
        ("1 / 0", "divide-zero"),
        ("1 % 0", "remainder-zero"),
        ("-9223372036854775808 / -1", "divide-overflow"),
        ("-9223372036854775808 % -1", "remainder-overflow"),
    ] {
        let source = format!("fn main() -> i64 {{ {expression}; 0 }}\n");
        let result = compile_and_run(&source, name);
        assert_eq!(result.status.code(), Some(1), "expression: {expression}");
    }
}

#[test]
fn signed_i64_addition_subtraction_and_multiplication_trap_on_overflow() {
    for (expression, name) in [
        ("9223372036854775807 + 1", "add-positive-overflow"),
        ("-9223372036854775808 + -1", "add-negative-overflow"),
        ("-9223372036854775808 - 1", "subtract-negative-overflow"),
        ("9223372036854775807 - -1", "subtract-positive-overflow"),
        ("9223372036854775807 * 2", "multiply-positive-overflow"),
        ("-9223372036854775808 * -1", "multiply-negative-overflow"),
    ] {
        let source = format!("fn main() -> i64 {{ {expression}; 0 }}\n");
        let result = compile_and_run(&source, name);
        assert_eq!(result.status.code(), Some(1), "expression: {expression}");
    }
}

#[test]
fn negating_a_runtime_i64_minimum_traps_but_in_range_arithmetic_works() {
    let result = compile_and_run(
        "fn main() -> i64 { let minimum = -9223372036854775808; -minimum }\n",
        "negate-runtime-minimum",
    );
    assert_eq!(result.status.code(), Some(1));

    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            print_i64(9223372036854775807 + 0);
            print_i64(-9223372036854775808 - 0);
            print_i64(3037000499 * 3037000499);
            print_i64(-(-42));
            0
        }
        "#,
        "checked-arithmetic-in-range",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "9223372036854775807\n-9223372036854775808\n9223372030926249001\n42\n"
    );
}
