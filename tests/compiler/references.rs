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

    let result = compile_and_run(
        r#"
        fn select_if<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {
            if condition { first } else { second }
        }
        fn select_match<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {
            match condition { true => { first }, false => { second } }
        }
        fn main() -> i64 {
            const first = 40;
            const second = 42;
            *select_if(false, &first, &second) + *select_match(true, &first, &second) - 82
        }
        "#,
        "references-named-lifetime-control-flow-joins",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        "fn choose<'a>(shared: &'a i64, mutable: &'a mut i64) -> &'a i64 { shared } fn main() -> i64 { let value = 42; let other = 1; const mutable: &mut i64 = &mut other; *choose(&value, mutable) - 42 }",
        "references-named-lifetime-shared-origin-with-mutable-parameter",
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
            "fn choose<'a, 'b>(condition: bool, first: &'a i64, second: &'b i64) -> &'a i64 { if condition { first } else { second } } fn main() -> i64 { 0 }",
            "references-named-lifetime-mixed-if-origins",
            "not all returned reference paths originate from lifetime `'a`",
        ),
        (
            "fn choose<'a, 'b>(condition: bool, first: &'a i64, second: &'b i64) -> &'a i64 { match condition { true => { first }, false => { second } } } fn main() -> i64 { 0 }",
            "references-named-lifetime-mixed-match-origins",
            "not all returned reference paths originate from lifetime `'a`",
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
    assert_compile_error(
        "fn escape<'a>(input: &'a i64) -> &'a i64 { const local = 1; const alias = &local; alias } fn main() -> i64 { 0 }",
        "references-named-lifetime-local-alias-escape",
        "cannot return a reference that does not originate from a parameter",
    );
    assert_compile_error(
        "fn invalid<'a>(value: &'a mut i64) -> &'a i64 { value } fn main() -> i64 { 0 }",
        "references-named-lifetime-mutable-only-origin",
        "return lifetime `'a` must appear on a shared-reference parameter",
    );
    assert_compile_error(
        "fn wrong_parameter<'a>(shared: &'a i64, mutable: &'a mut i64) -> &'a i64 { mutable } fn main() -> i64 { 0 }",
        "references-named-lifetime-mutable-parameter-return",
        "not all returned reference paths originate from lifetime `'a`",
    );
}

#[test]
fn generic_functions_preserve_named_lifetimes_during_type_specialization() {
    let result = compile_and_run(
        r#"
        fn identity<'a, T>(value: &'a T) -> &'a T { value }
        fn choose<'a, T>(condition: bool, first: &'a T, second: &'a T) -> &'a T {
            if condition { first } else { second }
        }
        fn main() -> i64 {
            const first = 10;
            const second = 32;
            *identity(&first) + *identity::<i64>(&second)
                + *choose(false, &first, &second) - 74
        }
        "#,
        "references-generic-named-lifetimes",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn choose<'a, 'b, T>(first: &'a T, second: &'b T) -> &'a T { second } fn main() -> i64 { const first = 1; const second = 2; *choose(&first, &second) }",
        "references-generic-named-lifetime-invalid-return",
        "returned reference has lifetime `'b`, expected `'a`",
    );

    assert_compile_error(
        "fn identity<'a, T>(value: &'a T) -> &'a T { value } fn main() -> i64 { let value = 42; const reference = identity(&value); value = 43; *reference }",
        "references-generic-return-loan",
        "cannot modify `value` while it is borrowed",
    );
}

#[test]
fn generic_mutable_references_infer_types_without_weakening_borrow_rules() {
    let result = compile_and_run(
        r#"
        fn set<'a, T>(value: &'a mut T, replacement: T) -> unit {
            *value = replacement;
        }
        fn main() -> i64 {
            let inferred = 1;
            let explicit = 2;
            if true {
                const reference: &mut i64 = &mut inferred;
                set(reference, 20);
                *reference - 20
            } else { 1 };
            if true {
                const reference: &mut i64 = &mut explicit;
                set::<i64>(reference, 22);
                *reference - 22
            } else { 1 };
            inferred + explicit - 42
        }
        "#,
        "references-generic-mutable-inference",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn set<'a, T>(value: &'a mut T, replacement: T) -> unit { *value = replacement; } fn main() -> i64 { let value = 1; const reference: &i64 = &value; set(reference, 2); 0 }",
        "references-generic-mutable-rejects-shared-reference",
        "generic argument has type `&i64`, expected `&mut T`",
    );

    assert_compile_error(
        "fn invalid<'a, T>(value: &'a mut T) -> &'a T { value } fn main() -> i64 { let value = 1; const reference: &mut i64 = &mut value; *invalid(reference) }",
        "references-generic-mutable-cannot-authorize-shared-return",
        "return lifetime `'a` must appear on a shared-reference parameter",
    );

    assert_compile_error(
        "fn set<'a, T>(value: &'a mut T, replacement: T) -> unit { *value = replacement; } fn main() -> i64 { let value = 1; if true { const reference: &mut i64 = &mut value; set(reference, 2); value; } else { 0 } 0 }",
        "references-generic-mutable-loan-blocks-access",
        "cannot access `value` while it is mutably borrowed",
    );
}

#[test]
fn named_lifetime_provenance_unions_same_lifetime_parameters() {
    let result = compile_and_run(
        r#"
        fn choose<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {
            if condition { first } else { second }
        }
        fn forward<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {
            const alias = choose(condition, first, second);
            alias
        }
        fn main() -> i64 {
            const first = 10;
            const second = 32;
            *forward(false, &first, &second) - 32
        }
        "#,
        "references-named-lifetime-same-lifetime-origins",
    );
    assert_eq!(result.status.code(), Some(0));

    for (place, name) in [("first", "first"), ("second", "second")] {
        let source = format!(
            "fn choose<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {{ if condition {{ first }} else {{ second }} }} fn main() -> i64 {{ let first = 10; let second = 32; const selected = choose(true, &first, &second); {place} = 42; *selected }}"
        );
        assert_compile_error(
            &source,
            &format!("references-call-return-loans-{name}"),
            &format!("cannot modify `{name}` while it is borrowed"),
        );
    }
}

#[test]
fn match_returned_references_keep_loans_for_every_possible_origin() {
    let result = compile_and_run(
        r#"
        fn select<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {
            match condition {
                true => { first },
                false => { second },
            }
        }
        fn main() -> i64 {
            const first = 20;
            const second = 22;
            *select(false, &first, &second) - 22
        }
        "#,
        "references-match-return-loan-readable",
    );
    assert_eq!(result.status.code(), Some(0));

    for (place, name) in [("first", "first"), ("second", "second")] {
        let source = format!(
            "fn select<'a>(condition: bool, first: &'a i64, second: &'a i64) -> &'a i64 {{ match condition {{ true => {{ first }}, false => {{ second }} }} }} fn main() -> i64 {{ let first = 20; let second = 22; const selected = select(true, &first, &second); {place} = 42; *selected }}"
        );
        assert_compile_error(
            &source,
            &format!("references-match-return-loans-{name}"),
            &format!("cannot modify `{name}` while it is borrowed"),
        );
    }
}

#[test]
fn named_lifetime_signature_parameters_are_scoped_and_well_formed() {
    for (source, name, diagnostic) in [
        (
            "fn invalid<'a, 'a>(value: &'a i64) -> i64 { 0 } fn main() -> i64 { 0 }",
            "references-duplicate-lifetime-parameter",
            "lifetime parameter `'a` is defined more than once",
        ),
        (
            "fn invalid<'a, a>(value: &'a i64) -> i64 { 0 } fn main() -> i64 { 0 }",
            "references-type-lifetime-name-collision",
            "`a` cannot be both a type and lifetime parameter",
        ),
        (
            "fn invalid<'a>(value: &i64) -> i64 { 0 } fn main() -> i64 { 0 }",
            "references-named-lifetime-required-in-generic-signature",
            "references in lifetime-generic function signatures must name a lifetime",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn returned_reference_calls_propagate_shared_loans_through_aliases() {
    let source = "fn identity<'a>(value: &'a i64) -> &'a i64 { value } fn relay<'a>(value: &'a i64) -> &'a i64 { const alias = identity(value); alias } fn main() -> i64 { let answer = 42; const reference = relay(&answer); answer = 43; *reference }";
    assert_compile_error(
        source,
        "references-call-return-loan-through-alias",
        "cannot modify `answer` while it is borrowed",
    );
    assert_compile_error(
        "fn identity<'a>(value: &'a i64) -> &'a i64 { value } fn main() -> i64 { let answer = 42; const reference = identity(&answer); const mutable: &mut i64 = &mut answer; *reference }",
        "references-call-return-loan-blocks-mutable-borrow",
        "`answer` is already mutably borrowed",
    );

    let result = compile_and_run(
        "fn identity<'a>(value: &'a i64) -> &'a i64 { value } fn main() -> i64 { let answer = 42; const reference = identity(&answer); *reference - 42 }",
        "references-call-return-loan-readable",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn shared_references_can_be_reborrowed_through_direct_dereferences() {
    let result = compile_and_run(
        r#"
        fn read(value: &i64) -> i64 { *value }
        fn reborrow<'a>(value: &'a i64) -> &'a i64 {
            const alias: &'a i64 = &*value;
            alias
        }
        fn main() -> i64 {
            let answer = 42;
            const original: &i64 = &answer;
            const alias: &i64 = &*original;
            read(alias) + *reborrow(original) - 84
        }
        "#,
        "references-shared-direct-reborrow",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn escape<'a>(input: &'a i64) -> &'a i64 { let local = 1; const reference: &i64 = &local; &*reference } fn main() -> i64 { 0 }",
        "references-reborrow-local-lifetime-escape",
        "cannot return a reference that does not originate from a parameter",
    );
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let answer = 42;
            const mutable: &mut i64 = &mut answer;
            const shared: &i64 = &*mutable;
            *shared - 42
        }
        "#,
        "references-shared-mutable-reborrow",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn main() -> i64 { let answer = 42; const mutable: &mut i64 = &mut answer; const shared: &i64 = &*mutable; *mutable = 1; 0 }",
        "references-shared-reborrow-blocks-parent-write",
        "cannot modify `answer` while it is borrowed",
    );
    assert_compile_error(
        "fn main() -> i64 { let answer = 42; const shared: &i64 = &answer; const mutable: &mut i64 = &mut *shared; 0 }",
        "references-mutable-through-shared-reborrow",
        "cannot mutably borrow through a shared reference",
    );
}

#[test]
fn mutable_reborrows_nest_and_keep_parent_loans_lexical() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 0;
            if true {
                const parent: &mut i64 = &mut value;
                const child: &mut i64 = &mut *parent;
                *child = 42;
                0
            } else {
                0
            };
            value - 42
        }
        "#,
        "references-mutable-reborrow-lexical-release",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }

        fn main() -> i64 {
            let pair = Pair { left: 0, right: 0 };
            const whole: &mut Pair = &mut pair;
            const left: &mut i64 = &mut (*whole).left;
            (*whole).right = 40;
            *left = 2;
            (*whole).right + *left - 42
        }
        "#,
        "references-mutable-reborrow-projected-places",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let first = 0;
            let second = 0;
            if true {
                const first_ref: &mut i64 = &mut first;
                const second_ref: &mut i64 = &mut second;
                const selected = if false {
                    const child: &mut i64 = &mut *first_ref;
                    child
                } else {
                    const child: &mut i64 = &mut *second_ref;
                    child
                };
                *selected = 42;
                0
            } else {
                0
            };
            if first != 0 { return 1; }
            if second != 42 { return 2; }
            if true {
                const first_ref: &mut i64 = &mut first;
                const second_ref: &mut i64 = &mut second;
                const selected = if true {
                    const child: &mut i64 = &mut *first_ref;
                    child
                } else {
                    const child: &mut i64 = &mut *second_ref;
                    child
                };
                *selected = 12;
                0
            } else {
                0
            };
            if first != 12 { return 3; }
            if second != 42 { return 4; }
            first + second - 54
        }
        "#,
        "references-mutable-reborrow-branch-join",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let value = 0; const parent: &mut i64 = &mut value; const child: &mut i64 = &mut *parent; *parent = 1; *child }",
            "references-mutable-reborrow-parent-write",
            "cannot access `value` while it is mutably borrowed",
        ),
        (
            "fn main() -> i64 { let value = 0; const parent: &mut i64 = &mut value; const first: &mut i64 = &mut *parent; const second: &mut i64 = &mut *parent; 0 }",
            "references-mutable-reborrow-sibling",
            "`value` is already mutably borrowed",
        ),
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let pair = Pair { left: 0, right: 0 }; const whole: &mut Pair = &mut pair; const left: &mut i64 = &mut (*whole).left; *whole = Pair { left: 1, right: 2 }; *left }",
            "references-mutable-reborrow-parent-whole-write",
            "cannot access `pair` while it is mutably borrowed",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn named_lifetimes_allow_mutable_reference_returns() {
    let result = compile_and_run(
        r#"
        fn identity<'a, T>(value: &'a mut T) -> &'a mut T { value }
        fn forward<'a>(value: &'a mut i64) -> &'a mut i64 { identity(value) }
        fn choose<'a>(condition: bool, first: &'a mut i64, second: &'a mut i64) -> &'a mut i64 {
            if condition { first } else { second }
        }

        fn main() -> i64 {
            let value = 0;
            if true {
                const parent: &mut i64 = &mut value;
                const child = forward(parent);
                *child = 42;
                0
            } else {
                0
            };
            value - 42
        }
        "#,
        "references-mutable-named-lifetime-return",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn select<'a>(condition: bool, first: &'a mut i64, second: &'a mut i64) -> &'a mut i64 {
            match condition {
                true => { first },
                false => { second },
            }
        }

        fn main() -> i64 {
            let first = 0;
            let second = 0;
            if true {
                const first_ref: &mut i64 = &mut first;
                const second_ref: &mut i64 = &mut second;
                const selected = select(false, first_ref, second_ref);
                *selected = 42;
                0
            } else {
                0
            };
            if first != 0 { return 1; }
            if second != 42 { return 2; }
            if true {
                const first_ref: &mut i64 = &mut first;
                const second_ref: &mut i64 = &mut second;
                const selected = select(true, first_ref, second_ref);
                *selected = 12;
                0
            } else {
                0
            };
            if first != 12 { return 3; }
            if second != 42 { return 4; }
            first + second - 54
        }
        "#,
        "references-mutable-named-lifetime-match",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name, diagnostic) in [
        (
            "fn escape<'a>(input: &'a mut i64) -> &'a mut i64 { let local = 1; const reference: &mut i64 = &mut local; reference } fn main() -> i64 { 0 }",
            "references-mutable-return-local-escape",
            "cannot return a reference that does not originate from a parameter",
        ),
        (
            "fn choose<'a, 'b>(first: &'a mut i64, second: &'b mut i64) -> &'a mut i64 { second } fn main() -> i64 { 0 }",
            "references-mutable-return-wrong-lifetime",
            "returned reference has lifetime `'b`, expected `'a`",
        ),
        (
            "fn choose<'a, 'b>(condition: bool, first: &'a mut i64, second: &'b mut i64) -> &'a mut i64 { if condition { first } else { second } } fn main() -> i64 { 0 }",
            "references-mutable-return-mixed-lifetimes",
            "not all returned reference paths originate from lifetime `'a`",
        ),
        (
            "fn invalid<'a>(value: &'a i64) -> &'a mut i64 { value } fn main() -> i64 { 0 }",
            "references-mutable-return-shared-origin",
            "return lifetime `'a` must appear on a mutable-reference parameter",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn mutable_reborrow_and_return_loans_cover_call_arguments() {
    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }
        fn set_both(first: &mut i64, second: &mut i64) -> unit {
            *first = 20;
            *second = 22;
        }

        fn main() -> i64 {
            let pair = Pair { left: 0, right: 0 };
            const whole: &mut Pair = &mut pair;
            set_both(&mut (*whole).left, &mut (*whole).right);
            (*whole).left + (*whole).right - 42
        }
        "#,
        "references-reborrow-disjoint-call-arguments",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn set_both(first: &mut i64, second: &mut i64) -> unit { *first = 1; *second = 2; } fn main() -> i64 { let values = [0, 0]; const parent: &mut [i64; 2] = &mut values; set_both(&mut (*parent)[0], &mut (*parent)[0]); 0 }",
        "references-reborrow-overlapping-call-arguments",
        "overlapping mutable references cannot be passed to the same call",
    );
    assert_compile_error(
        "fn observe(value: &mut i64, other: i64) -> i64 { *value + other } fn main() -> i64 { let value = 0; const parent: &mut i64 = &mut value; observe(&mut *parent, loop { *parent = 9; break 0; }) }",
        "references-reborrow-later-call-argument",
        "cannot access `value` while it is mutably borrowed",
    );

    let result = compile_and_run(
        r#"
        fn identity<'a>(value: &'a mut i64) -> &'a mut i64 { value }

        fn main() -> i64 {
            let value = 0;
            const parent: &mut i64 = &mut value;
            const child = identity(parent);
            *child = 42;
            0
        }
        "#,
        "references-call-return-blocks-parent",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "fn identity<'a>(value: &'a mut i64) -> &'a mut i64 { value } fn main() -> i64 { let value = 0; const parent: &mut i64 = &mut value; const child = identity(parent); *parent = 1; *child }",
        "references-call-return-parent-write",
        "cannot access `value` while it is mutably borrowed",
    );
}

#[test]
fn borrow_overlap_distinguishes_parameter_roots_and_known_places() {
    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }
        fn take(first: &mut i64, second: &mut i64) -> unit {
            *first = 1;
            *second = 2;
        }
        fn nested(pair: &mut Pair) -> i64 {
            if true {
                const left: &mut i64 = &mut (*pair).left;
                const right: &mut i64 = &mut (*pair).right;
                take(left, right);
                0
            } else { 0 };
            if (*pair).left != 1 { return 1; }
            if (*pair).right != 2 { return 2; }
            0
        }
        fn main() -> i64 {
            let pair = Pair { left: 0, right: 0 };
            if true {
                const reference: &mut Pair = &mut pair;
                nested(reference)
            } else { 0 }
        }
        "#,
        "references-parameter-disjoint-fields",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn take(first: &mut i64, second: &mut i64) -> unit {
            *first = 1;
            *second = 2;
        }
        fn nested(parameter: &mut i64) -> i64 {
            let local = 0;
            if true {
                const reference: &mut i64 = &mut local;
                take(parameter, reference);
                0
            } else { 0 };
            local - 2
        }
        fn main() -> i64 {
            let value = 0;
            let result = if true {
                const reference: &mut i64 = &mut value;
                nested(reference)
            } else { 0 };
            if result != 0 { return 1; }
            value - 1
        }
        "#,
        "references-parameter-and-local-disjoint",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn outer(value: &mut i64) -> i64 {
            let identity: fn(&mut i64) -> i64 = fn(param: &mut i64) -> i64 { *param };
            identity(value)
        }
        fn main() -> i64 {
            let value = 42;
            if true {
                const reference: &mut i64 = &mut value;
                outer(reference) - 42
            } else { 0 }
        }
        "#,
        "references-parameter-callable-identity",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn pick(condition: bool, parent: &mut i64) -> i64 {
            const selected = if condition {
                const child: &mut i64 = &mut *parent;
                child
            } else {
                const child: &mut i64 = &mut *parent;
                child
            };
            *selected = 42;
            *selected - 42
        }
        fn main() -> i64 {
            let value = 0;
            if true {
                const reference: &mut i64 = &mut value;
                pick(true, reference)
            } else { 0 }
        }
        "#,
        "references-alternative-origins-share-one-binding-loan",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        fn read(value: &i64) -> i64 { *value }
        fn main() -> i64 {
            let value = 42;
            const parent: &mut i64 = &mut value;
            const child: &i64 = &*parent;
            read(child) - 42
        }
        "#,
        "references-by-value-child-reference",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        r#"
        struct Pair { left: i64, right: i64 }
        fn take(first: &mut i64, second: &mut i64) -> unit { *first = 1; *second = 2; }
        fn nested(pair: &mut Pair) -> i64 {
            if true {
                const left: &mut i64 = &mut (*pair).left;
                const other: &mut i64 = &mut (*pair).left;
                take(left, other);
                0
            } else { 0 }
        }
        fn main() -> i64 { 0 }
        "#,
        "references-parameter-duplicate-field",
        "`reference parameter` is already mutably borrowed",
    );
    assert_compile_error(
        r#"
        fn set(value: &mut i64) -> unit { *value = 1; }
        fn main() -> i64 {
            let value = 0;
            const parent: &mut i64 = &mut value;
            const shared: &i64 = &*parent;
            set(parent);
            0
        }
        "#,
        "references-by-value-mutable-argument-blocks-shared",
        "cannot modify `value` while it is borrowed",
    );
}

#[test]
fn reborrow_index_selectors_require_i64_and_dynamic_indices_stay_usable() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let values = [10, 20];
            const reference: &[i64; 2] = &values;
            let index = 1;
            const child: &i64 = &(*reference)[index];
            *child - 20
        }
        "#,
        "references-dynamic-reborrow-index",
    );
    assert_eq!(result.status.code(), Some(0));

    for (source, name) in [
        (
            "fn main() -> i64 { let values = [10, 20]; const reference: &[i64; 2] = &values; (*reference)[true] }",
            "references-direct-reborrow-index-type",
        ),
        (
            "fn main() -> i64 { let values = [[10, 20], [30, 40]]; const reference: &[[i64; 2]; 2] = &values; (*reference)[0][true] }",
            "references-nested-reborrow-index-type",
        ),
        (
            "fn main() -> i64 { let values = [10, 20]; const reference: &mut [i64; 2] = &mut values; const selected: &mut i64 = &mut (*reference)[true]; 0 }",
            "references-borrowed-reborrow-index-type",
        ),
    ] {
        assert_compile_error(source, name, "array index has type bool, expected i64");
    }
}

#[test]
fn scalar_block_results_can_dereference_local_references() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let result = if true {
                let local = 42;
                const reference: &i64 = &local;
                *reference
            } else {
                0
            };
            result - 42
        }
        "#,
        "references-branch-local-scalar-deref",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        enum Number { One(i64) }
        fn main() -> i64 {
            let result = match Number::One(42) {
                Number::One(value) => {
                    const reference: &i64 = &value;
                    *reference
                },
            };
            result - 42
        }
        "#,
        "references-match-binding-scalar-deref",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        r#"
        fn main() -> i64 {
            let other = 0;
            const selected = if true {
                let local = 42;
                const reference: &i64 = &local;
                reference
            } else {
                const reference: &i64 = &other;
                reference
            };
            *selected - 42
        }
        "#,
        "references-branch-local-reference-escape",
        "a reference to a block-local binding cannot escape its scope",
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
fn shared_borrows_span_sibling_call_arguments() {
    assert_compile_error(
        r#"
        fn mutate(value: &mut i64) -> i64 { *value = 999; 1 }
        fn observe(reference: &i64, computed: i64) -> i64 { *reference + computed }
        fn main() -> i64 {
            let answer = 10;
            observe(&answer, loop {
                const mutable: &mut i64 = &mut answer;
                break mutate(mutable);
            })
        }
        "#,
        "references-sibling-argument-mutable-borrow",
        "`answer` is already mutably borrowed",
    );
    assert_compile_error(
        "fn observe(reference: &i64, other: i64) -> i64 { 0 } fn main() -> i64 { let answer = 10; observe(&answer, loop { answer = 999; break 0; }) }",
        "references-sibling-argument-write",
        "cannot modify `answer` while it is borrowed",
    );
    assert_compile_error(
        "fn main() -> i64 { let answer = 10; let forward: fn(&i64, i64) -> i64 = fn(reference: &i64, other: i64) -> i64 { *reference + other }; forward(&answer, loop { const mutable: &mut i64 = &mut answer; break 0; }) }",
        "references-sibling-argument-function-value",
        "`answer` is already mutably borrowed",
    );

    let result = compile_and_run(
        r#"
        fn read(value: &i64, other: i64) -> i64 { *value + other }

        fn main() -> i64 {
            let answer = 10;
            read(&answer, 0);
            answer = 32;
            answer - 32
        }
        "#,
        "references-sibling-argument-loan-release",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        r#"
        struct Point { x: i64, y: i64 }
        fn read(value: &i64, other: i64) -> i64 { *value + other }

        fn main() -> i64 {
            let point = Point { x: 20, y: 22 };
            read(&point.x, loop {
                const mutable: &mut i64 = &mut point.y;
                break *mutable;
            }) - 42
        }
        "#,
        "references-sibling-argument-disjoint-fields",
    );
    assert_eq!(result.status.code(), Some(0));
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
            "fn main() -> i64 { const value = 1; match true { true => { &value }, false => { &value } }; 0 }",
            "references-match-escape",
            "references cannot escape through `match` expressions",
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
    assert_compile_error(
        "fn identity<'a>(value: &'a i64) -> &'a i64 { value } fn main() -> i64 { let function = identity; 0 }",
        "references-function-value-inferred-shared-return",
        "functions returning references cannot be used as function values",
    );
    assert_compile_error(
        "fn identity<'a>(value: &'a mut i64) -> &'a mut i64 { value } fn main() -> i64 { let function = identity; 0 }",
        "references-function-value-inferred-mutable-return",
        "functions returning references cannot be used as function values",
    );
}
