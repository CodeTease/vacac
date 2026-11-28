use crate::common::*;
use std::fs;

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
fn raw_pointers_compare_by_address_and_support_typed_null() {
    let result = compile_and_run(
        r#"
        struct Holder { pointer: *const i64 }

        fn same(left: *const i64, right: *const i64) -> bool { left == right }
        fn identity<T>(value: T) -> T { value }

        fn main() -> i64 {
            let value = 41;
            const pointer: *const i64 = &raw const value;
            const alias = pointer;
            const null_pointer = null::<*const i64>();
            if !(pointer == alias) || pointer != alias { return 1; }
            if pointer == null_pointer || null_pointer != null::<*const i64>() { return 2; }
            if null::<*mut i64>() != null::<*mut i64>() { return 3; }
            if !same(pointer, alias) { return 4; }

            const holder = Holder { pointer: null_pointer };
            if holder.pointer != null::<*const i64>() { return 5; }
            const pointers = [null::<*const i64>()];
            if pointers[0] != null::<*const i64>() { return 6; }
            const generic = identity(null::<*mut i64>());
            if generic != null::<*mut i64>() { return 7; }
            0
        }
        "#,
        "raw-pointer-equality-and-null",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn typed_null_requires_a_raw_pointer_type() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { null::<i64>(); 0 }",
            "null-non-pointer",
            "null requires a raw pointer type",
        ),
        (
            "fn main() -> i64 { null::<str>(); 0 }",
            "null-string",
            "null requires a raw pointer type",
        ),
        (
            "fn main() -> i64 { null::<&i64>(); 0 }",
            "null-reference",
            "null requires a raw pointer type",
        ),
        (
            "const POINTER: *const i64 = null::<*const i64>(); fn main() -> i64 { 0 }",
            "null-global",
            "global initializers cannot contain raw pointers",
        ),
        (
            "fn main() -> i64 { const pointer = null::<*const i64>>(); 0 }",
            "null-extra-greater",
            "unexpected `>` after the null pointer type",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn raw_pointer_equality_emits_direct_pointer_comparisons() {
    let (directory, emitted) = run_compiler_with_args(
        r#"
        fn main() -> i64 {
            let value = 1;
            const pointer = &raw const value;
            if pointer == null::<*const i64>() { 0 } else { 0 }
        }
        "#,
        "raw-pointer-equality-ir",
        &["--emit-llvm", "program.ll", "--quiet"],
    );
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let ir = fs::read_to_string(directory.join("program.ll")).unwrap();
    let _ = std::fs::remove_dir_all(directory);
    assert!(
        ir.contains("icmp eq ptr") && ir.contains("ptr null"),
        "pointer equality must use direct pointer comparison:\n{ir}"
    );
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
fn raw_addresses_target_aggregate_and_vector_subplaces() {
    let result = compile_and_run(
        r#"
        struct Row { cells: [i64; 2] }
        struct Point { x: i64, y: i64 }
        const ORIGIN: Point = Point { x: 1, y: 2 };
        let GLOBAL: i64 = 1;

        fn main() -> i64 {
            let point = Point { x: 3, y: 4 };
            const y_pointer: *const i64 = &raw const point.y;
            if *y_pointer != 4 { return 1; }

            let values = [5, 6];
            const second: *mut i64 = &raw mut values[1];
            *second = 7;
            if values[1] != 7 { return 2; }

            let rows: Vec<Row> = vec_new();
            vec_push(rows, Row { cells: [8, 9] });
            const cell: *mut i64 = &raw mut rows[0].cells[1];
            *cell = 42;
            if rows[0].cells[1] != 42 { return 3; }

            const const_rows: Vec<Row> = rows;
            const nested: *mut i64 = &raw mut const_rows[0].cells[0];
            *nested = 11;
            if rows[0].cells[0] != 11 { return 4; }

            if *(&raw const ORIGIN.y) != 2 { return 5; }
            const global_pointer: *mut i64 = &raw mut GLOBAL;
            *global_pointer = 42;
            if GLOBAL != 42 { return 6; }
            0
        }
        "#,
        "raw-pointer-subplaces",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn raw_subplace_addresses_evaluate_indexes_once_and_track_vector_growth() {
    let result = compile_and_run(
        r#"
        struct Row { cells: [i64; 1] }
        let PICKS: i64 = 0;

        fn pick() -> i64 {
            PICKS = PICKS + 1;
            0
        }

        fn grow(rows: Vec<Row>) -> i64 {
            let index = 0;
            while index < 8 {
                vec_push(rows, Row { cells: [100 + index] });
                index = index + 1;
            }
            0
        }

        fn grow_outer(outer: Vec<Vec<i64>>) -> i64 {
            let index = 0;
            while index < 8 {
                let extra: Vec<i64> = vec_new();
                vec_push(extra, 50 + index);
                vec_push(outer, extra);
                index = index + 1;
            }
            0
        }

        fn main() -> i64 {
            let rows: Vec<Row> = vec_new();
            vec_push(rows, Row { cells: [1] });
            const pointer = &raw mut rows[pick()].cells[0];
            *pointer = 42;
            if PICKS != 1 { return 1; }
            if rows[0].cells[0] != 42 { return 2; }

            const moved = &raw mut rows[grow(rows)].cells[0];
            *moved = 43;
            if rows[0].cells[0] != 43 { return 3; }
            if len(rows) != 9 { return 4; }

            let outer: Vec<Vec<i64>> = vec_new();
            let inner: Vec<i64> = vec_new();
            vec_push(inner, 1);
            vec_push(outer, inner);
            const nested = &raw mut outer[0][grow_outer(outer)];
            *nested = 5;
            if inner[0] != 5 { return 5; }
            if len(outer) != 9 { return 6; }
            if outer[0][0] != 5 { return 7; }
            0
        }
        "#,
        "raw-pointer-subplace-growth",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn raw_subplace_mutability_and_place_rules_are_checked() {
    for (source, name, diagnostic) in [
        (
            "struct P { x: i64 } fn main() -> i64 { const p = P { x: 1 }; const q: *mut i64 = &raw mut p.x; 0 }",
            "raw-pointer-immutable-field",
            "cannot take a mutable raw pointer to an immutable binding",
        ),
        (
            "fn main() -> i64 { const q: *const i64 = &raw const (1 + 2); 0 }",
            "raw-pointer-temporary",
            "raw address-taking requires a named local or global variable",
        ),
        (
            "fn main() -> i64 { let value = 1; const r: *const i64 = &raw const value; const q: *mut i64 = &raw mut *r; 0 }",
            "raw-pointer-deref-root",
            "raw address-taking requires a named local or global variable",
        ),
        (
            "fn main() -> i64 { let value = 1; const r: &i64 = &value; const q: *const i64 = &raw const r; 0 }",
            "raw-pointer-reference-target",
            "raw pointers cannot target safe references",
        ),
        (
            "fn main() -> i64 { let value = 1; const r: &i64 = &value; const q: *mut i64 = &raw mut value; 0 }",
            "raw-subplace-shared-borrow",
            "cannot take a mutable raw pointer to `value` while it is borrowed",
        ),
        (
            "fn main() -> i64 { let value = 1; if true { const r: &mut i64 = &mut value; const q: *const i64 = &raw const value; } else {} 0 }",
            "raw-pointer-mutable-borrow",
            "cannot access `value` while it is mutably borrowed",
        ),
        (
            "struct P { x: i64 } fn main() -> i64 { let p = P { x: 1 }; const r: &i64 = &p.x; const q: *mut i64 = &raw mut p.x; 0 }",
            "raw-pointer-overlapping-subplace",
            "cannot take a mutable raw pointer to `p` while it is borrowed",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn raw_subplace_addresses_allow_known_disjoint_loans() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }

        fn main() -> i64 {
            let point = Point { x: 1, y: 2 };
            if true {
                const x_reference: &i64 = &point.x;
                const y_pointer: *mut i64 = &raw mut point.y;
                *y_pointer = 42;
                if *x_reference != 1 { return 1; }
            } else {
                0
            }
            point.y - 42
        }
        "#,
        "raw-pointer-disjoint-subplace-loans",
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
            "functions returning references must specify a named return lifetime",
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
