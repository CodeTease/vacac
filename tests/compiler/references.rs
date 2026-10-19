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
fn named_lifetimes_allow_parameter_derived_reference_returns() {
    let result = compile_and_run(
        r#"
        fn identity<'a, T>(value: &'a T) -> &'a T { return value; }
        fn forward<'a>(value: &'a i64) -> &'a i64 { return identity(value); }

        fn main() -> i64 {
            const answer: i64 = 42;
            const reference: &i64 = forward(&answer);
            *reference - 42
        }
        "#,
        "references-named-lifetime-return",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name, diagnostic) in [
        (
            "fn identity<'a>(value: &'a i64) -> &'a i64 { value } fn escape<'a>(input: &'a i64) -> &'a i64 { const local = 1; &local } fn main() -> i64 { 0 }",
            "references-named-lifetime-local-escape",
            "cannot return a reference that does not originate from a parameter",
        ),
        (
            "fn choose<'a, 'b>(first: &'a i64, second: &'b i64) -> &'a i64 { second } fn main() -> i64 { 0 }",
            "references-named-lifetime-wrong-origin",
            "returned reference has lifetime `'b`, expected `'a`",
        ),
        (
            "fn escape<'a>(input: &'a i64) -> &'a i64 { const local = 1; &local } fn main() -> i64 { 0 }",
            "references-named-lifetime-local-shadow-return",
            "cannot return a reference that does not originate from a parameter",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }

    assert_compile_error(
        "fn choose<'a, 'b>(first: &'a i64, second: &'b i64) -> &'a i64 { const alias = second; alias } fn main() -> i64 { 0 }",
        "references-named-lifetime-local-alias-origin",
        "returned reference has lifetime `'b`, expected `'a`",
    );
    assert_compile_error(
        "fn invalid<'a>(value: &'b i64) -> i64 { 0 } fn main() -> i64 { 0 }",
        "references-named-lifetime-undeclared",
        "undeclared lifetime `'b`",
    );
    assert_compile_error(
        "fn missing_origin<'a>(value: &'a i64, unused: i64) -> &'a i64 { const local = 1; &local } fn main() -> i64 { 0 }",
        "references-named-lifetime-missing-origin",
        "cannot return a reference that does not originate from a parameter",
    );
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
fn references_borrow_struct_fields_and_track_projection_overlap() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }

        fn main() -> i64 {
            let point = Point { x: 19, y: 20 };
            const first: &i64 = &point.x;
            const second: &i64 = &point.x;
            const other: &mut i64 = &mut point.y;
            *other = 23;
            *first + *second + *other - 61
        }
        "#,
        "shared-reference-struct-field-projections",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }

        fn set_both(first: &mut i64, second: &mut i64) -> unit {
            *first = 20;
            *second = 22;
        }

        fn main() -> i64 {
            let point = Point { x: 20, y: 21 };
            const field: &mut i64 = &mut point.x;
            const other: &mut i64 = &mut point.y;
            set_both(field, other);
            *field + *other - 42
        }
        "#,
        "mutable-reference-struct-field-projections",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name, diagnostic) in [
        (
            "struct Point { x: i64, y: i64 } fn main() -> i64 { let point = Point { x: 1, y: 2 }; const field: &i64 = &point.x; point.x = 3; 0 }",
            "shared-reference-struct-field-blocks-write",
            "cannot modify `point` while it is borrowed",
        ),
        (
            "struct Point { x: i64, y: i64 } fn main() -> i64 { let point = Point { x: 1, y: 2 }; const field: &i64 = &point.x; point = Point { x: 3, y: 4 }; 0 }",
            "shared-reference-struct-field-blocks-whole-write",
            "cannot modify `point` while it is borrowed",
        ),
        (
            "struct Point { x: i64, y: i64 } fn main() -> i64 { let point = Point { x: 1, y: 2 }; const field: &mut i64 = &mut point.x; const other: &mut i64 = &mut point.x; 0 }",
            "mutable-reference-same-struct-field-overlap",
            "`point` is already mutably borrowed",
        ),
        (
            "struct Point { x: i64, y: i64 } fn main() -> i64 { let point = Point { x: 1, y: 2 }; const field: &mut i64 = &mut point.x; point.x; 0 }",
            "mutable-reference-struct-field-blocks-read",
            "cannot access `point` while it is mutably borrowed",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn references_borrow_fixed_array_elements_and_track_index_overlap() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let values = [10, 20];
            const first: &i64 = &values[0];
            values[1] = 21;
            *first + values[1] - 31
        }
        "#,
        "shared-reference-array-element-projections",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn set_both(first: &mut i64, second: &mut i64) -> unit {
            *first = 11;
            *second = 21;
        }

        fn main() -> i64 {
            let values = [10, 20];
            const first: &mut i64 = &mut values[0];
            const second: &mut i64 = &mut values[1];
            set_both(first, second);
            *first + *second - 32
        }
        "#,
        "mutable-reference-array-element-projections",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let values = [1, 2]; const first: &i64 = &values[0]; values[0] = 3; 0 }",
            "shared-reference-array-element-blocks-write",
            "cannot modify `values` while it is borrowed",
        ),
        (
            "fn main() -> i64 { let values = [1, 2]; const first: &i64 = &values[0]; values = [3, 4]; 0 }",
            "shared-reference-array-element-blocks-whole-write",
            "cannot modify `values` while it is borrowed",
        ),
        (
            "fn main() -> i64 { let values = [1, 2]; let index = 0; const first: &mut i64 = &mut values[index]; const other: &mut i64 = &mut values[1]; 0 }",
            "mutable-reference-unknown-array-index-overlap",
            "`values` is already mutably borrowed",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
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
            "functions returning references must specify a named return lifetime",
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
            "struct Buffer { values: Vec<i64> } fn main() -> i64 { let buffer = Buffer { values: vec_new() }; let reference = &buffer.values; 0 }",
            "references-vector-field-alias",
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
