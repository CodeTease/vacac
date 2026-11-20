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
fn pointer_dereference_assignment_evaluates_the_place_once() {
    let result = compile_and_run(
        r#"
        fn side_effect(v: &i64) -> i64 {
            print_i64(777);
            0
        }
        fn main() -> i64 {
            let x = 10;
            let arr: [*mut i64; 1] = [&raw mut x];
            let dummy = 0;
            *arr[side_effect(&dummy)] = 42;
            x - 42
        }
        "#,
        "pointer-deref-assignment-single-evaluation",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "777\n");
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
            "fn main() -> i64 { let value = 1; const reference: &i64 = &value; const pointer: *mut i64 = &raw mut value; 0 }",
            "raw-pointer-shared-borrow",
            "cannot take a mutable raw pointer to `value` while it is borrowed",
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
    assert_compile_error(
        "fn main() -> i64 { let value = 1; if true { const first: &mut i64 = &mut value; const second: &mut i64 = &mut value; } else {} 0 }",
        "mutable-reference-double-borrow",
        "`value` is already mutably borrowed",
    );
}

#[test]
fn mutable_references_reject_alias_copies_and_overlapping_call_arguments() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let value = 1; const reference: &mut i64 = &mut value; const alias = reference; 0 }",
            "mutable-reference-copy-alias",
            "mutable references cannot be copied into another binding",
        ),
        (
            "fn update(first: &mut i64, second: &mut i64) -> unit { *first = 1; *second = 2; } fn main() -> i64 { let value = 0; const reference: &mut i64 = &mut value; update(reference, reference); 0 }",
            "mutable-reference-duplicate-call-argument",
            "overlapping mutable references cannot be passed to the same call",
        ),
        (
            "fn main() -> i64 { let update: fn(&mut i64, &mut i64) -> unit = fn(first: &mut i64, second: &mut i64) -> unit { *first = 1; *second = 2; }; let value = 0; const reference: &mut i64 = &mut value; update(reference, reference); 0 }",
            "mutable-reference-duplicate-indirect-call-argument",
            "overlapping mutable references cannot be passed to the same call",
        ),
        (
            "fn update(first: &mut i64, second: &mut i64) -> unit { *first = 1; *second = 2; } fn forward(value: &mut i64) -> unit { update(value, value); } fn main() -> i64 { let value = 0; const reference: &mut i64 = &mut value; forward(reference); 0 }",
            "mutable-reference-forwarded-duplicate-arguments",
            "overlapping mutable references cannot be passed to the same call",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn mutable_reference_calls_allow_disjoint_origins_and_reuse_afterward() {
    let result = compile_and_run(
        r#"
        fn update_both(first: &mut i64, second: &mut i64) -> unit {
            *first = 20;
            *second = 22;
        }

        fn main() -> i64 {
            let first = 0;
            let second = 0;
            const first_ref: &mut i64 = &mut first;
            const second_ref: &mut i64 = &mut second;
            update_both(first_ref, second_ref);
            *first_ref = *first_ref + 1;
            *second_ref = *second_ref + 1;
            *first_ref + *second_ref - 44
        }
        "#,
        "mutable-reference-disjoint-call-and-reuse",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn mutable_borrows_are_attached_to_lexical_bindings_not_shadowed_names() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 40;
            if true {
                const reference: &mut i64 = &mut value;
                if true {
                    let value = 1;
                    value - 1
                } else {
                    0
                };
                *reference = 42;
                0
            } else {
                0
            };
            value - 42
        }
        "#,
        "mutable-reference-shadowed-binding",
    );
    assert_eq!(result.status.code(), Some(0));
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

#[test]
fn closures_cannot_take_the_raw_address_of_enclosing_locals() {
    for (source, name) in [
        (
            "fn main() -> i64 { let x = 42; let f: fn() -> *const i64 = fn() -> *const i64 { &raw const x }; 0 }",
            "raw-pointer-closure-const-capture",
        ),
        (
            "fn main() -> i64 { let x = 42; let f: fn() -> *mut i64 = fn() -> *mut i64 { &raw mut x }; 0 }",
            "raw-pointer-closure-mut-capture",
        ),
    ] {
        assert_compile_error(
            source,
            name,
            "closures cannot take the raw address of enclosing local `x`",
        );
    }
}

#[test]
fn closures_can_take_raw_addresses_of_globals_and_their_own_locals() {
    let result = compile_and_run(
        r#"
        const GLOBAL: i64 = 42;
        fn main() -> i64 {
            let closure: fn() -> i64 = fn() -> i64 {
                let local = 40;
                let pointer = &raw const local;
                *pointer + *(&raw const GLOBAL) - 82
            };
            closure()
        }
        "#,
        "raw-pointer-closure-allowed-roots",
    );
    assert_eq!(result.status.code(), Some(0));
}
