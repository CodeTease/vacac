use crate::common::*;

#[test]
fn enums_support_exhaustive_matching_and_primitive_payloads() {
    let result = compile_and_run(
        r#"
        enum MaybeInt { None, Some(i64) }
        enum Message { Stop, Text(str), Flag(bool) }

        fn describe(value: Message) -> i64 {
            match value {
                Message::Stop => { 0 },
                Message::Text(text) => { if text == "go" { 1 } else { 2 } },
                Message::Flag(flag) => { if flag { 3 } else { 4 } },
            }
        }

        fn get(value: MaybeInt) -> i64 {
            match value {
                MaybeInt::None => { -1 },
                MaybeInt::Some(number) => { number },
            }
        }

        fn main() -> i64 {
            let value: MaybeInt = MaybeInt::Some(37);
            print_i64(get(value));
            let text = Message::Text("go");
            print_i64(describe(text));
            print_i64(describe(Message::Flag(true)));
            describe(Message::Stop)
        }
        "#,
        "enum-match-basics",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "37\n1\n3\n");
}

#[test]
fn enums_support_multiple_primitive_payloads_and_independent_match_bindings() {
    let result = compile_and_run(
        r#"
        enum Event {
            Empty,
            Record(i64, bool, str, i64),
            Mixed(str, i64, str, bool),
        }

        fn total(event: Event) -> i64 {
            match event {
                Event::Empty => { 0 },
                Event::Record(id, active, name, extra) => {
                    println(name);
                    id + extra + if active { 1 } else { 0 }
                },
                Event::Mixed(_, number, label, flag) => {
                    println(label);
                    number + if flag { 10 } else { 0 }
                },
            }
        }

        fn main() -> i64 {
            print_i64(total(Event::Record(20, true, "record", 3)));
            print_i64(total(Event::Mixed("ignored", 5, "mixed", true)));
            total(Event::Empty)
        }
        "#,
        "enum-multiple-payloads",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "record\n24\nmixed\n15\n"
    );
}

#[test]
fn enum_payloads_support_arrays_structs_and_nested_enums() {
    let result = compile_and_run(
        r#"
        enum MaybeInt { None, Some(i64) }

        struct Pair { left: i64, right: bool }

        enum Message {
            Empty,
            Bundle([i64; 2], Pair, MaybeInt),
            Matrix([[bool; 2]; 2]),
        }

        fn total(message: Message) -> i64 {
            match message {
                Message::Empty => { 0 },
                Message::Bundle(values, pair, maybe) => {
                    let nested = match maybe {
                        MaybeInt::None => { 0 },
                        MaybeInt::Some(number) => { number },
                    };
                    values[0] + values[1] + pair.left + if pair.right { nested } else { 0 }
                },
                Message::Matrix(matrix) => {
                    if matrix[0][0] && matrix[1][1] { 99 } else { 1 }
                },
            }
        }

        fn main() -> i64 {
            let first = Message::Bundle(
                [3, 4],
                Pair { left: 5, right: true },
                MaybeInt::Some(6),
            );
            let same = Message::Bundle(
                [3, 4],
                Pair { left: 5, right: true },
                MaybeInt::Some(6),
            );
            if first != same { return 1; }
            let changed = Message::Bundle(
                [3, 4],
                Pair { left: 5, right: false },
                MaybeInt::Some(6),
            );
            if first == changed { return 2; }
            print_i64(total(first));
            print_i64(total(Message::Matrix([[true, false], [false, true]])));
            total(Message::Empty)
        }
        "#,
        "enum-aggregate-payloads",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "18\n99\n");
}

#[test]
fn enum_match_supports_wildcards_and_ignored_payloads() {
    let result = compile_and_run(
        r#"
        enum Message { Stop, Number(i64), Text(str) }

        fn classify(value: Message) -> i64 {
            match value {
                Message::Number(_) => { 1 },
                _ => { 2 },
            }
        }

        fn main() -> i64 {
            print_i64(classify(Message::Number(99)));
            print_i64(classify(Message::Text("ignored")));
            classify(Message::Stop) - 2
        }
        "#,
        "enum-match-wildcard-ignore",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n2\n");

    let result = compile_and_run(
        "enum Choice { Value(i64), Empty } fn main() -> i64 { match Choice::Value(4) { Choice::Value(value) => { value }, Choice::Empty => { 0 } } }\n",
        "enum-match-binding-still-works",
    );
    assert_eq!(result.status.code(), Some(4));
}

#[test]
fn enum_match_wildcard_makes_match_exhaustive_and_must_be_final_and_unique() {
    for (source, name, diagnostic) in [
        (
            "enum Choice { A, B } fn main() -> i64 { match Choice::A { Choice::A => { 0 }, _ => { 1 }, Choice::B => { 2 } } }\n",
            "enum-match-after-wildcard",
            "unreachable match arm after wildcard",
        ),
        (
            "enum Choice { A, B } fn main() -> i64 { match Choice::A { _ => { 0 }, _ => { 1 } } }\n",
            "enum-match-duplicate-wildcard",
            "duplicate wildcard match arm",
        ),
        (
            "enum Choice { A, B } fn main() -> i64 { match Choice::A { Choice::A => { 0 }, Choice::B => { 1 }, _ => { 2 } } }\n",
            "enum-match-unreachable-wildcard",
            "unreachable wildcard match arm after all variants are covered",
        ),
        (
            "enum Choice { Empty } fn main() -> i64 { match Choice::Empty { Choice::Empty(_) => { 0 } } }\n",
            "enum-match-ignore-absent-payload",
            "has no payload to bind or ignore",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn enums_reject_non_exhaustive_duplicate_and_invalid_payload_matches() {
    for (source, test_name, diagnostic) in [
        (
            "enum Choice { Left, Right } fn main() -> i64 { match Choice::Left { Choice::Left => { 0 } } }\n",
            "enum-match-non-exhaustive",
            "non-exhaustive match on `Choice`",
        ),
        (
            "enum Choice { Left, Right } fn main() -> i64 { match Choice::Left { Choice::Left => { 0 }, Choice::Left => { 1 }, Choice::Right => { 2 } } }\n",
            "enum-match-duplicate",
            "duplicate match arm",
        ),
        (
            "enum Choice { Some(i64), None } fn main() -> i64 { match Choice::None { Choice::Some(value) => { value }, Choice::None(unexpected) => { unexpected } } }\n",
            "enum-match-payload-binding",
            "match arm `Choice::None` has no payload to bind",
        ),
        (
            "enum Choice { Some(i64), None } fn main() -> i64 { Choice::Some(true); 0 }\n",
            "enum-payload-type",
            "enum payload has type bool, expected i64",
        ),
        (
            "enum Choice { Some(i64), None } fn main() -> i64 { match Choice::None { Choice::Some => { 1 }, Choice::None => { 0 } } }\n",
            "enum-match-missing-payload-binding",
            "must bind its payload",
        ),
        (
            "enum Choice { Some(i64, bool), None } fn main() -> i64 { Choice::Some(1); 0 }\n",
            "enum-payload-arity",
            "expects 2 payload value(s), got 1",
        ),
        (
            "enum Choice { Some(i64, bool), None } fn main() -> i64 { Choice::Some(1, 2); 0 }\n",
            "enum-payload-type-in-tuple",
            "enum payload has type i64, expected bool",
        ),
        (
            "enum Choice { Some(i64, bool), None } fn main() -> i64 { match Choice::Some(1, true) { Choice::Some(value) => { value }, Choice::None => { 0 } } }\n",
            "enum-match-payload-arity",
            "expects 2 payload binding(s), got 1",
        ),
        (
            "enum Choice { Some(i64, bool), None } fn main() -> i64 { match Choice::Some(1, true) { Choice::Some(value, value) => { value }, Choice::None => { 0 } } }\n",
            "enum-match-duplicate-payload-binding",
            "`value` is already declared in this scope",
        ),
    ] {
        assert_compile_error(source, test_name, diagnostic);
    }
}

#[test]
fn enums_reject_duplicate_declarations_and_unsupported_payloads() {
    assert_compile_error(
        "enum Choice { A, A } fn main() -> i64 { 0 }\n",
        "enum-duplicate-variant",
        "enum variant `A` is defined more than once",
    );
    assert_compile_error(
        "enum Choice { Value(unit) } fn main() -> i64 { 0 }\n",
        "enum-unit-payload",
        "enum payloads cannot have type `unit`",
    );
    assert_compile_error(
        "enum Link { Next(Link) } fn main() -> i64 { 0 }\n",
        "enum-recursive-payload",
        "recursive by-value payload",
    );
    assert_compile_error(
        "enum Link { Next(Node) } struct Node { link: Link } fn main() -> i64 { 0 }\n",
        "enum-struct-recursive-payload",
        "recursive by-value field `link`",
    );
    assert_compile_error(
        "enum Choice { A } enum Choice { B } fn main() -> i64 { 0 }\n",
        "enum-duplicate-type",
        "enum `Choice` is defined more than once",
    );
}
