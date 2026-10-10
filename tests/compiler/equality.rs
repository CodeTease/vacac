use crate::common::*;

#[test]
fn aggregate_equality_compares_nested_arrays_structs_enums_and_strings() {
    let result = compile_and_run(
        r#"
        enum Choice { Empty, Number(i64), Label(str), Flag(bool, str) }

        struct Leaf {
            label: str,
            choice: Choice,
        }

        struct Group {
            members: [Leaf; 2],
            matrix: [[i64; 2]; 2],
            labels: [str; 2],
        }

        fn main() -> i64 {
            let left = Group {
                members: [
                    Leaf { label: "first", choice: Choice::Number(12) },
                    Leaf { label: "second", choice: Choice::Flag(true, "ready") },
                ],
                matrix: [[1, 2], [3, 4]],
                labels: ["alpha", "beta"],
            };
            let right = Group {
                members: [
                    Leaf { label: "first", choice: Choice::Number(12) },
                    Leaf { label: "second", choice: Choice::Flag(true, "ready") },
                ],
                matrix: [[1, 2], [3, 4]],
                labels: ["alpha", "beta"],
            };

            if left != right { return 1; }
            right.members[1].choice = Choice::Flag(true, "not ready");
            if left == right { return 2; }
            right.members[1].choice = Choice::Flag(true, "ready");
            right.matrix[1][1] = 5;
            if left == right { return 3; }
            right.matrix[1][1] = 4;
            right.labels[0] = "changed";
            if left == right { return 4; }
            0
        }
        "#,
        "aggregate-equality-nested",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn enum_equality_checks_variant_before_payload_and_compares_all_payload_kinds() {
    let result = compile_and_run(
        r#"
        enum Choice { Empty, Number(i64), Label(str), Flag(bool, str) }

        fn main() -> i64 {
            if Choice::Empty != Choice::Empty { return 1; }
            if Choice::Number(42) != Choice::Number(42) { return 2; }
            if Choice::Number(42) == Choice::Number(43) { return 3; }
            if Choice::Label("same") != Choice::Label("same") { return 4; }
            if Choice::Label("left") == Choice::Label("right") { return 5; }
            if Choice::Flag(true, "ok") != Choice::Flag(true, "ok") { return 6; }
            if Choice::Flag(false, "ok") == Choice::Flag(true, "ok") { return 7; }
            if Choice::Number(42) == Choice::Label("different variant") { return 8; }
            if Choice::Label("different variant") == Choice::Number(42) { return 9; }
            0
        }
        "#,
        "enum-equality-payloads",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn array_equality_handles_nested_and_zero_length_arrays() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let left = [["one", "two"], ["three", "four"]];
            let right = [["one", "two"], ["three", "four"]];
            if left != right { return 1; }
            let empty: [i64; 0] = [];
            let also_empty: [i64; 0] = [];
            if empty != also_empty { return 2; }
            0
        }
        "#,
        "array-equality-zero-length",
    );
    assert_eq!(result.status.code(), Some(0));
}
