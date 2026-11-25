use crate::common::*;
use std::fs;

#[test]
fn nested_patterns_dispatch_across_enum_struct_and_array_fields() {
    let result = compile_and_run(
        r#"
        enum Reading { Sample(bool, i64), Missing }
        struct Packet { reading: Reading, flags: [bool; 2] }

        fn classify(packet: Packet) -> i64 {
            match packet {
                Packet { reading: Reading::Sample(true, 0), flags: [true, _] } => { 1 },
                Packet { reading: Reading::Sample(true, n), .. } => { n },
                Packet { reading: Reading::Sample(false, _), .. } => { 2 },
                Packet { reading: Reading::Missing, .. } => { 3 },
            }
        }

        fn main() -> i64 {
            print_i64(classify(Packet { reading: Reading::Sample(true, 0), flags: [true, false] }));
            print_i64(classify(Packet { reading: Reading::Sample(true, 42), flags: [false, false] }));
            print_i64(classify(Packet { reading: Reading::Sample(false, 7), flags: [true, true] }));
            print_i64(classify(Packet { reading: Reading::Missing, flags: [false, true] }));
            0
        }
        "#,
        "nested-fields",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n42\n2\n3\n");
}

#[test]
fn nested_patterns_inside_tuples_and_arrays_match_in_arm_order() {
    let result = compile_and_run(
        r#"
        enum Value { Number(i64), Empty }

        fn pair(value: (Value, bool)) -> i64 {
            match value {
                (Value::Number(0), _) => { 100 },
                (Value::Number(number), true) => { number },
                (Value::Number(_), false) => { 2 },
                (Value::Empty, _) => { 3 },
            }
        }

        fn array(values: [Value; 2]) -> i64 {
            match values {
                [Value::Number(number), Value::Empty] => { number },
                [Value::Empty, Value::Number(number)] => { number + 10 },
                [Value::Empty, Value::Empty] => { 20 },
                [Value::Number(_), Value::Number(_)] => { 30 },
            }
        }

        fn main() -> i64 {
            print_i64(pair((Value::Number(0), true)));
            print_i64(pair((Value::Number(5), true)));
            print_i64(pair((Value::Number(5), false)));
            print_i64(array([Value::Number(4), Value::Empty]));
            print_i64(array([Value::Empty, Value::Number(1)]));
            print_i64(array([Value::Number(9), Value::Number(9)]));
            0
        }
        "#,
        "nested-tuples-arrays",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "100\n5\n2\n4\n11\n30\n"
    );
}

#[test]
fn partially_overlapping_arms_stay_reachable_until_fully_shadowed() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }

        fn classify(point: Point) -> i64 {
            match point {
                Point { x: 0, y: 0 } => { 0 },
                Point { x: 0, .. } => { point.y },
                Point { .. } => { -1 },
            }
        }

        fn main() -> i64 {
            print_i64(classify(Point { x: 0, y: 0 }));
            print_i64(classify(Point { x: 0, y: 5 }));
            print_i64(classify(Point { x: 1, y: 5 }));
            0
        }
        "#,
        "nested-overlap",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "0\n5\n-1\n");
}

#[test]
fn exhaustiveness_requires_every_combination_not_each_field_separately() {
    assert_compile_error(
        r#"
        fn main() -> i64 {
            match (true, false) {
                (true, _) => { 1 },
                (_, true) => { 2 },
            }
        }
        "#,
        "nested-product-non-exhaustive",
        "non-exhaustive match on `(bool, bool)`",
    );

    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            match (false, false) {
                (true, _) => { 1 },
                (_, true) => { 2 },
                (false, false) => { 0 },
            }
        }
        "#,
        "nested-product-exhaustive",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn exhaustiveness_names_the_missing_nested_case() {
    assert_compile_error(
        r#"
        enum Reading { Sample(bool, i64), Missing }
        fn classify(value: Reading) -> i64 {
            match value {
                Reading::Sample(true, _) => { 1 },
                Reading::Missing => { 2 },
            }
        }
        fn main() -> i64 { 0 }
        "#,
        "nested-missing-case",
        "missing case: Reading::Sample(false, _)",
    );
}

#[test]
fn unreachable_arms_are_rejected_after_the_union_of_prior_arms() {
    assert_compile_error(
        r#"
        enum Reading { Sample(bool, i64), Missing }
        fn classify(value: Reading) -> i64 {
            match value {
                Reading::Sample(true, _) => { 1 },
                Reading::Sample(false, _) => { 2 },
                Reading::Sample(_, _) => { 3 },
                Reading::Missing => { 4 },
            }
        }
        fn main() -> i64 { 0 }
        "#,
        "nested-unreachable-arm",
        "unreachable match arm",
    );
}

#[test]
fn generic_enums_specialize_nested_patterns_and_arm_bindings_infer() {
    let result = compile_and_run(
        r#"
        enum Choice<T> { A(T), B }
        enum Outer<T> { Wrap(Choice<T>), Empty }
        struct Holder { choice: Choice<i64> }

        fn unwrap_or<T>(value: Outer<T>, fallback: T) -> T {
            match value {
                Outer::Wrap(Choice::A(inner)) => { inner },
                Outer::Wrap(Choice::B) => { fallback },
                Outer::Empty => { fallback },
            }
        }

        fn from_holder(holder: Holder) -> i64 {
            match holder {
                Holder { choice: Choice::A(number) } => { number },
                Holder { choice: Choice::B } => { 0 },
            }
        }

        fn identity<T>(value: T) -> T { value }

        fn main() -> i64 {
            let direct = unwrap_or(Outer::Wrap(Choice::A(40)), 7);
            let inferred = identity(match Outer::Wrap(Choice::A(2)) {
                Outer::Wrap(Choice::A(inner)) => { inner },
                _ => { 0 },
            });
            let nested = from_holder(Holder { choice: Choice::A(3) });
            print_i64(direct);
            print_i64(inferred);
            print_i64(nested);
            direct + inferred + nested - 45
        }
        "#,
        "nested-generic",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "40\n2\n3\n");
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn match_scrutinee_is_evaluated_once_even_with_nested_tests() {
    let result = compile_and_run(
        r#"
        let calls: i64 = 0;

        fn sample() -> (i64, bool) {
            calls = calls + 1;
            (7, false)
        }

        fn main() -> i64 {
            let label = match sample() {
                (0, _) => { 1 },
                (_, true) => { 2 },
                (7, false) => { 3 },
                _ => { 4 },
            };
            print_i64(calls);
            label - 3
        }
        "#,
        "nested-scrutinee-once",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n");
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn nested_float_patterns_use_ieee_equality_and_need_a_catch_all() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let pair: (f64, bool) = (0.0, true);
            match pair {
                (-0.0, true) => { 40 },
                (_, true) => { 1 },
                (_, false) => { 2 },
            } + 2
        }
        "#,
        "nested-float-zero",
    );
    assert_eq!(result.status.code(), Some(42));
}

#[test]
fn unit_payloads_and_unit_results_nest_without_a_result_phi() {
    let (dir, result) = run_compiler_with_args(
        r#"
        enum Signal { Ping(unit), Stop }
        struct Frame { signal: Signal, ready: bool }

        fn report(frame: Frame) -> unit {
            match frame {
                Frame { signal: Signal::Ping(value), ready: true } => {
                    let copy: unit = value;
                    copy = ();
                    println("ping")
                },
                Frame { signal: Signal::Stop, .. } => { println("stop") },
                Frame { ready: false, .. } => { println("idle") },
            }
        }

        fn main() -> i64 {
            report(Frame { signal: Signal::Ping(()), ready: true });
            report(Frame { signal: Signal::Stop, ready: true });
            report(Frame { signal: Signal::Stop, ready: false });
            0
        }
        "#,
        "nested-unit-phi",
        &["--emit-llvm", "nested-unit.ll", "--quiet"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(dir.join("nested-unit.ll")).unwrap();
    assert!(!ir.contains("match.value"), "{ir}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn nested_arms_with_question_mark_flow_only_from_normal_exits() {
    let result = compile_and_run(
        r#"
        fn positive(value: i64) -> Result<i64, i64> {
            if value > 0 { Result::Ok(value) } else { Result::Err(-value) }
        }

        fn classify(pair: (bool, i64)) -> Result<i64, i64> {
            match pair {
                (true, value) => {
                    let checked = positive(value)?;
                    Result::Ok(checked)
                },
                (false, _) => { Result::Ok(2) },
            }
        }

        fn main() -> i64 {
            let ok = match classify((true, 40)) {
                Result::Ok(value) => { value },
                Result::Err(_) => { 0 },
            };
            let err = match classify((true, -5)) {
                Result::Ok(_) => { 0 },
                Result::Err(value) => { value },
            };
            print_i64(ok);
            print_i64(err);
            ok + err - 45
        }
        "#,
        "nested-question-mark",
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "40\n5\n");
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn irrefutable_bindings_still_reject_nested_variant_patterns() {
    assert_compile_error(
        "enum Choice { A(i64), B } fn main() -> i64 { let Choice::A(value) = Choice::A(1); value }\n",
        "nested-let-variant",
        "enum variant pattern is not valid in this pattern position",
    );
}

#[test]
fn duplicate_lookalike_nested_arms_report_a_duplicate() {
    assert_compile_error(
        r#"
        fn main() -> i64 {
            match (1, true) {
                (1, true) => { 1 },
                (1, true) => { 2 },
                _ => { 3 },
            }
        }
        "#,
        "nested-duplicate-arm",
        "duplicate match arm",
    );
}
