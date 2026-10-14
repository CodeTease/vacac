use crate::common::*;

#[test]
fn raw_pointers_are_typed_and_can_be_dereferenced() {
    let result = compile_and_run(
        r#"
        struct PointerBox { pointer: *const i64 }

        fn read(pointer: *const i64) -> i64 { *pointer }
        fn identity(pointer: *const i64) -> *const i64 { pointer }

        fn main() -> i64 {
            let value = 41;
            let pointer: *const i64 = identity(&raw const value);
            let pointers = [pointer];
            let boxed = PointerBox { pointer: pointers[0] };
            read(boxed.pointer) + 1 - 42
        }
        "#,
        "raw-pointer-read",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn mutable_raw_pointers_write_through_const_bindings() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 7;
            const pointer: *mut i64 = &raw mut value;
            *pointer = 42;
            value - 42
        }
        "#,
        "raw-pointer-write",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn raw_pointer_mutability_and_unsupported_escapes_are_checked() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let value = 1; const pointer: *const i64 = &raw const value; *pointer = 2; 0 }",
            "raw-pointer-const-write",
            "cannot assign through an immutable pointer",
        ),
        (
            "fn main() -> i64 { const value = 1; const pointer: *mut i64 = &raw mut value; 0 }",
            "raw-pointer-immutable-address",
            "cannot take a mutable raw pointer to an immutable binding",
        ),
        (
            "fn main() -> i64 { let value = 1; const pointer: *mut i64 = &raw mut value; let other: *const i64 = pointer; 0 }",
            "raw-pointer-no-coercion",
            "initializer for `other` has type *mut i64, expected *const i64",
        ),
        (
            "fn main() -> i64 { let value = 1; const pointer = &raw const value; pointer == pointer; 0 }",
            "raw-pointer-comparison",
            "references and raw pointers cannot be compared",
        ),
        (
            "fn main() -> i64 { let value = 1; let pointers = [&raw const value]; pointers == pointers; 0 }",
            "raw-pointer-aggregate-comparison",
            "values containing raw pointers cannot be compared",
        ),
        (
            "const VALUE = 0; const POINTER: *const i64 = &raw const VALUE; fn main() -> i64 { 0 }",
            "raw-pointer-global",
            "global initializers cannot contain raw pointers",
        ),
        (
            "struct PointerHolder { pointer: *const i64 } const HOLDER = PointerHolder { pointer: &raw const ZERO }; const ZERO = 0; fn main() -> i64 { 0 }",
            "raw-pointer-nested-global",
            "global initializers cannot contain raw pointers",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn mutable_references_write_through_and_reserve_the_source_to_scope_end() {
    let result = compile_and_run(
        r#"
        fn set(value: &mut i64) -> unit { *value = 42; }
        fn main() -> i64 {
            let mut_value = 40;
            if true {
                const reference: &mut i64 = &mut mut_value;
                set(reference);
                *reference = *reference + 1;
                0
            } else {
                0
            }
            mut_value - 43
        }
        "#,
        "mutable-reference-write",
    );
    assert_eq!(result.status.code(), Some(0));

    let sibling_scope = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 9;
            if true { const reference: &mut i64 = &mut value; *reference = 1; 0 } else { 0 };
            value - 1
        }
        "#,
        "mutable-reference-sibling-scope",
    );
    assert_eq!(sibling_scope.status.code(), Some(0));

    assert_compile_error(
        "fn main() -> i64 { let value = 1; if true { const reference: &mut i64 = &mut value; value = 2; } else {} 0 }",
        "mutable-reference-blocks-write",
        "cannot access `value` while it is mutably borrowed",
    );
    assert_compile_error(
        "fn main() -> i64 { let value = 1; if true { const reference: &mut i64 = &mut value; let copy = value; } else {} 0 }",
        "mutable-reference-blocks-read",
        "cannot access `value` while it is mutably borrowed",
    );
}

#[test]
fn mutable_reference_rules_reject_unsafe_lifetimes_and_immutable_origins() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { const value = 1; const reference: &mut i64 = &mut value; 0 }",
            "mutable-reference-immutable-origin",
            "mutable borrowing requires a mutable local binding",
        ),
        (
            "fn main() -> i64 { let value = 1; let reference: &mut i64 = &mut value; 0 }",
            "mutable-reference-binding-mutability",
            "reference bindings must be immutable",
        ),
        (
            "fn main() -> i64 { let value = 1; const reference: &mut i64 = &mut value; (reference, 2); 0 }",
            "mutable-reference-tuple-escape",
            "references cannot be stored in tuples",
        ),
        (
            "fn set(value: &mut i64) -> unit { *value = 1; } fn main() -> i64 { let value = 0; set(&mut value); 0 }",
            "mutable-reference-direct-call",
            "mutable borrows must initialize a local reference binding",
        ),
        (
            "fn escape(value: &mut i64) -> &mut i64 { value } fn main() -> i64 { 0 }",
            "mutable-reference-return",
            "functions cannot return references",
        ),
        (
            "fn main() -> i64 { let value = 1; const reference: &mut i64 = &mut value; let closure = fn() -> i64 { *reference }; 0 }",
            "mutable-reference-capture",
            "closures cannot capture references",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}
