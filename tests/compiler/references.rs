use crate::common::*;

#[test]
fn immutable_local_references_can_be_passed_and_dereferenced() {
    let result = compile_and_run(
        r#"
        fn read<T>(value: &T) -> T { *value }

        fn main() -> i64 {
            const answer: i64 = 42;
            const reference: &i64 = &answer;
            read(reference) - *reference
        }
        "#,
        "references-local-readonly",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn immutable_aggregate_values_can_be_borrowed() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }
        fn sum(point: &Point) -> i64 { (*point).x + (*point).y }
        fn main() -> i64 {
            const point = Point { x: 19, y: 23 };
            sum(&point) - 42
        }
        "#,
        "references-struct-readonly",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn shared_references_borrow_mutable_local_bindings() {
    let result = compile_and_run(
        r#"
        fn read(value: &i64) -> i64 { *value }
        fn main() -> i64 {
            let answer = 42;
            const first: &i64 = &answer;
            const second: &i64 = &answer;
            read(first) + read(second) - 84
        }
        "#,
        "shared-reference-mutable-local",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        "fn main() -> i64 { let answer = 42; if true { const reference: &i64 = &answer; *reference - 42 } else { 0 }; answer = 43; answer - 43 }",
        "shared-reference-lexical-release",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn main() -> i64 { let answer = 1; const reference: &i64 = &answer; answer = 2; 0 }",
        "shared-reference-blocks-write",
        "cannot modify `answer` while it is borrowed",
    );
    assert_compile_error(
        "fn main() -> i64 { let answer = 1; const reference: &i64 = &answer; const mutable: &mut i64 = &mut answer; 0 }",
        "shared-reference-blocks-mutable-borrow",
        "`answer` is already mutably borrowed",
    );
}

#[test]
fn references_reject_unsafe_or_unsupported_uses() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let value = 1; const reference: &i64 = &value; value = 2; 0 }",
            "references-shared-loan-write",
            "cannot modify `value` while it is borrowed",
        ),
        (
            "fn main() -> i64 { const value = 1; let reference = &(value + 1); 0 }",
            "references-rvalue",
            "borrowing currently requires a named local binding",
        ),
        (
            "fn main() -> i64 { const value = 1; const reference: &i64 = &value; *reference = 2; 0 }",
            "references-write-through",
            "assignment target must start with a variable",
        ),
        (
            "fn escape() -> &i64 { const value = 1; &value } fn main() -> i64 { 0 }",
            "references-return",
            "functions cannot return references",
        ),
        (
            "fn main() -> i64 { const value = 1; ( &value, 2 ); 0 }",
            "references-tuple-storage",
            "references cannot be stored in tuples",
        ),
        (
            "fn main() -> i64 { const value = 1; let values: [&i64; 1] = [&value]; 0 }",
            "references-array-storage",
            "references cannot be stored in arrays or vectors",
        ),
        (
            "const REFERENCE: &i64 = 1; fn main() -> i64 { 0 }",
            "references-global-storage",
            "global initializer type cannot be `unit`, a vector, or a reference",
        ),
        (
            "fn main() -> i64 { const value = 1; if true { &value } else { &value }; 0 }",
            "references-if-escape",
            "references cannot escape through `if` expressions",
        ),
        (
            "fn main() -> i64 { const values: Vec<i64> = vec_new(); let reference = &values; 0 }",
            "references-vector-alias",
            "references to values containing vectors are not supported",
        ),
        (
            "fn main() -> i64 { const value = 1; const reference = &value; let closure = fn() -> i64 { *reference }; 0 }",
            "references-closure-capture",
            "closures cannot capture references",
        ),
        (
            "fn reassign(reference: &i64, other: &i64) -> i64 { reference = other; 0 } fn main() -> i64 { const value = 1; reassign(&value, &value) }",
            "references-parameter-reassignment",
            "cannot assign to immutable binding `reference`",
        ),
        (
            "struct Holder { reference: &i64 } fn main() -> i64 { 0 }",
            "references-struct-storage",
            "references cannot be stored in structs",
        ),
        (
            "enum Holder { Reference(&i64) } fn main() -> i64 { 0 }",
            "references-enum-storage",
            "references cannot be stored in enums",
        ),
        (
            "fn main() -> i64 { const value = 1; const reference: &unit = &value; 0 }",
            "references-unit-target",
            "references cannot target `unit`",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn references_cannot_be_returned_through_function_values_or_externs() {
    assert_compile_error(
        "fn main() -> i64 { let identity: fn(&i64) -> &i64 = fn(value: &i64) -> &i64 { value }; 0 }",
        "references-function-type-return",
        "function types cannot return references",
    );
    assert_compile_error(
        "extern \"C\" fn borrow(value: i64) -> &i64; fn main() -> i64 { 0 }",
        "references-ffi-return",
        "C extern return types support only `i64`, `f32`, `f64`, and `unit`",
    );
}
