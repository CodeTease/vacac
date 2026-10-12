use crate::common::*;

#[test]
fn tuples_support_types_values_nesting_arrays_and_value_passing() {
    let result = compile_and_run(
        r#"
        fn flip(value: (i64, bool)) -> (bool, i64) {
            let (number, flag) = value;
            (flag, number)
        }
        fn main() -> i64 {
            let pair: (i64, bool) = (42, true);
            if pair != (42, true) { return 1; }
            let flipped = flip(pair);
            let nested = ((flipped, "tuple"), (1.5f32, false));
            if nested != ((flipped, "tuple"), (1.5f32, false)) { return 2; }
            let (((flag, number), _), (fraction, _)) = nested;
            let pairs: [(i64, bool); 2] = [(1, false), (number, flag)];
            let [(first, _), (second, enabled)] = pairs;
            print_i64(first + second);
            if enabled && fraction == 1.5f32 { 0 } else { 1 }
        }
        "#,
        "tuples-roundtrip",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "43\n");
}

#[test]
fn scalar_and_enum_match_bindings_are_catch_all_patterns() {
    let result = compile_and_run(
        r#"
        enum Choice { Number(i64), Empty }
        fn main() -> i64 {
            let value = match 42 { number => { number } };
            let choice = match Choice::Number(value) { entire => { entire } };
            match choice {
                Choice::Number(number) => { number - value },
                Choice::Empty => { 1 },
            }
        }
        "#,
        "match-binding-catchall",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn tuples_arrays_and_structs_can_be_destructured_in_match_arms() {
    let result = compile_and_run(
        r#"
        struct User { id: i64, active: bool, score: i64 }
        enum Maybe { Value((i64, bool)), Empty }
        enum Nested { Inner(i64), Other }
        enum Outer { Wrap(Nested), Done }
        fn main() -> i64 {
            let user = User { id: 40, active: true, score: 2 };
            let User { id: user_id, .. } = user;
            let score = match user { User { id, active, .. } => { if active { id + 2 } else { 0 } } };
            let array = [score, 10, 99];
            let [array_head, _, array_tail] = array;
            let total = match array { [head, _, tail] => { head + tail } };
            let pair = match (total, true) { (number, enabled) => { if enabled { number } else { 0 } } };
            let result = match Maybe::Value((pair, true)) {
                Maybe::Value((value, enabled)) => { if enabled { value } else { 0 } },
                Maybe::Empty => { 0 },
            };
            let nested_enum = match Outer::Wrap(Nested::Inner(result)) {
                Outer::Wrap(_nested) => { 0 },
                Outer::Done => { 1 },
            };
            nested_enum + result - user_id + 40 - array_head - array_tail
        }
        "#,
        "pattern-match-aggregates",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn tuples_support_generic_function_inference() {
    let result = compile_and_run(
        r#"
        fn identity<T>(value: T) -> T { value }
        fn first<T>(value: (T, bool)) -> T {
            let (item, _) = value;
            item
        }
        fn main() -> i64 {
            let pair = identity((40, true));
            let (number, enabled) = pair;
            let one: (i64,) = (number,);
            let (only,) = one;
            if enabled { first((only + 2, false)) } else { 0 }
        }
        "#,
        "tuple-generic-inference",
    );
    assert_eq!(result.status.code(), Some(42));
}

#[test]
fn generic_struct_patterns_are_specialized_before_destructuring() {
    let result = compile_and_run(
        r#"
        struct Box<T> { value: T, ignored: bool }
        fn main() -> i64 {
            let Box { value, .. } = Box<i64> { value: 42, ignored: false };
            value
        }
        "#,
        "generic-struct-pattern",
    );
    assert_eq!(result.status.code(), Some(42));
}

#[test]
fn destructuring_patterns_reject_wrong_shapes_and_duplicate_bindings() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let (left, right) = (1, 2, 3); 0 }",
            "tuple-pattern-arity",
            "tuple pattern has 2 element(s), expected 3",
        ),
        (
            "fn main() -> i64 { let [first, _] = [1, 2, 3]; 0 }",
            "array-pattern-arity",
            "array pattern has 2 element(s), expected 3",
        ),
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let Pair { left } = Pair { left: 1, right: 2 }; 0 }",
            "struct-pattern-rest-required",
            "add `..` to ignore remaining fields",
        ),
        (
            "fn main() -> i64 { let (same, same) = (1, 2); 0 }",
            "tuple-pattern-duplicate-binding",
            "`same` is already declared in this scope",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}
