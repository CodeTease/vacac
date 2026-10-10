use crate::common::*;

#[test]
fn structs_support_construction_field_reads_and_value_passing() {
    let result = compile_and_run(
        r#"
        struct User {
            id: i64,
            active: bool,
            name: str,
        }
        fn rename(user: User) -> User {
            User { id: user.id, active: user.active, name: "Apex" }
        }
        fn main() -> i64 {
            let user = User { name: "Ada", active: true, id: 42 };
            let renamed = rename(user);
            print_i64(renamed.id);
            println(renamed.name);
            if renamed.active { 0 } else { 1 }
        }
        "#,
        "structs-value-passing",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\nApex\n");
}

#[test]
fn structs_support_return_values_and_if_expression_joins() {
    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }
        fn make_pair(value: i64) -> Pair {
            Pair { left: value, right: value + 1 }
        }
        fn main() -> i64 {
            let chosen = if true { make_pair(20) } else { make_pair(40) };
            chosen.left + chosen.right - 41
        }
        "#,
        "structs-return-if",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn structs_support_enum_fields_construction_assignment_and_matching() {
    let result = compile_and_run(
        r#"
        enum Status { Ready, Failed(i64) }
        struct Job { id: i64, status: Status }

        fn fail(job: Job, code: i64) -> Job {
            job.status = Status::Failed(code);
            job
        }

        fn result(job: Job) -> i64 {
            match job.status {
                Status::Ready => { 0 },
                Status::Failed(code) => { code },
            }
        }

        fn main() -> i64 {
            let ready = Job { status: Status::Ready, id: 17 };
            let failed = fail(ready, 23);
            print_i64(failed.id);
            result(failed) - 23
        }
        "#,
        "struct-enum-fields",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "17\n");
}

#[test]
fn structs_support_mutable_field_assignment() {
    let result = compile_and_run(
        r#"
        struct User { id: i64, active: bool, name: str }
        fn main() -> i64 {
            let user = User { id: 41, active: true, name: "Ada" };
            user.id = user.id + 1;
            user.active = false;
            user.name = "Grace";
            print_i64(user.id);
            println(user.name);
            if user.active { 1 } else { 0 }
        }
        "#,
        "struct-field-assignment",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\nGrace\n");
}

#[test]
fn const_bindings_are_readable_but_cannot_be_mutated() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        fn main() -> i64 {
            const point = Point { x: 42 };
            const answer: i64 = point.x;
            print_i64(answer);
            answer - 42
        }
        "#,
        "const-binding",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");

    for (source, diagnostic, name) in [
        (
            "fn main() -> i64 { const value = 1; value = 2; 0 }",
            "cannot assign to immutable binding `value`",
            "const-reassignment",
        ),
        (
            "fn main() -> i64 { const values = [1, 2]; values[0] = 3; 0 }",
            "cannot assign through immutable binding `values`",
            "const-array-assignment",
        ),
        (
            "struct Point { x: i64 } fn main() -> i64 { const point = Point { x: 1 }; point.x = 2; 0 }",
            "cannot assign through immutable binding `point`",
            "const-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn struct_field_assignment_checks_target_and_value_types() {
    for (source, diagnostic, name) in [
        (
            "struct Point { x: i64 } fn main() -> i64 { let point = Point { x: 1 }; point.x = true; 0 }",
            "assignment to field `x` has type bool, expected i64",
            "struct-field-assignment-type",
        ),
        (
            "struct Point { x: i64 } fn main() -> i64 { let point = Point { x: 1 }; point.y = 2; 0 }",
            "struct `Point` has no field `y`",
            "struct-field-assignment-unknown",
        ),
        (
            "fn main() -> i64 { let value = 1; value.x = 2; 0 }",
            "`value` is not a struct",
            "non-struct-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn structs_reject_duplicate_missing_unknown_and_wrongly_typed_fields() {
    for (source, expected, name) in [
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let p = Pair { left: 1, left: 2 }; 0 }",
            "struct field `left` is initialized more than once",
            "struct-duplicate-field-init",
        ),
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let p = Pair { left: 1 }; 0 }",
            "struct `Pair` is missing field(s): right",
            "struct-missing-field",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { other: 1 }; 0 }",
            "struct `Pair` has no field `other`",
            "struct-unknown-field",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { left: true }; 0 }",
            "initializer for field `left` has type bool, expected i64",
            "struct-field-type",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { left: 1 }; p.missing }",
            "struct `Pair` has no field `missing`",
            "struct-unknown-member",
        ),
    ] {
        assert_compile_error(source, name, expected);
    }
}

#[test]
fn structs_reject_duplicate_declarations_and_invalid_fields() {
    assert_compile_error(
        "struct Item { id: i64 } struct Item { name: str } fn main() -> i64 { 0 }",
        "struct-duplicate-declaration",
        "type `Item` is defined more than once",
    );
    assert_compile_error(
        "struct Item { value: unit } fn main() -> i64 { 0 }",
        "struct-unit-field",
        "struct fields cannot have type `unit`",
    );
    assert_compile_error(
        "struct Item { value: Missing } fn main() -> i64 { 0 }",
        "struct-unknown-field-type",
        "unknown type `Missing`",
    );
    assert_compile_error(
        "struct Node { next: Node } fn main() -> i64 { 0 }",
        "struct-recursive-field",
        "recursive by-value field `next`",
    );
    assert_compile_error(
        "struct First { second: Second } struct Second { first: First } fn main() -> i64 { 0 }",
        "struct-mutually-recursive-fields",
        "recursive by-value field `first`",
    );
    assert_compile_error(
        "struct Node { children: [Node; 1] } fn main() -> i64 { 0 }",
        "struct-recursive-array-field",
        "recursive by-value field `children`",
    );
    assert_compile_error(
        "struct Item { id: i64, id: bool } fn main() -> i64 { 0 }",
        "struct-duplicate-field-declaration",
        "struct field `id` is defined more than once",
    );
}

#[test]
fn structs_support_nested_struct_and_fixed_array_fields() {
    let result = compile_and_run(
        r#"
        struct Envelope { payload: Payload, samples: [i64; 3] }
        struct Payload { count: i64, enabled: bool, flags: [bool; 2] }

        fn update(envelope: Envelope) -> Envelope {
            envelope.payload.count = envelope.payload.count + 1;
            envelope.payload.flags[1] = false;
            envelope.samples[2] = envelope.samples[2] + 10;
            envelope
        }

        fn main() -> i64 {
            let original = Envelope {
                samples: [3, 5, 7],
                payload: Payload { count: 40, enabled: true, flags: [true, true] },
            };
            let updated = update(original);
            print_i64(updated.payload.count);
            print_i64(updated.samples[2]);
            if updated.payload.enabled && updated.payload.flags[0] && !updated.payload.flags[1] { 0 } else { 1 }
        }
        "#,
        "nested-struct-array-fields",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "41\n17\n");
}

#[test]
fn structs_can_contain_arrays_of_structs_and_enums() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        enum State { Ready, Waiting }
        struct Board { points: [Point; 2], states: [State; 2] }

        fn make_board() -> Board {
            Board {
                points: [Point { x: 3 }, Point { x: 7 }],
                states: [State::Waiting, State::Ready],
            }
        }

        fn main() -> i64 {
            let board = make_board();
            board.points[1].x = board.points[1].x + 1;
            print_i64(board.points[0].x + board.points[1].x);
            print_i64(match board.states[0] {
                State::Ready => { 0 },
                State::Waiting => { 1 },
            });
            0
        }
        "#,
        "struct-fields-aggregate-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "11\n1\n");
}
