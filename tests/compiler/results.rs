use crate::common::*;

#[test]
fn generic_enums_specialize_payloads_and_match_bindings() {
    let result = compile_and_run(
        r#"
        enum Maybe<T> { Some(T), None }
        fn identity<T>(value: T) -> T { value }
        fn main() -> i64 {
            let value: Maybe<i64> = Maybe::Some(42);
            match value {
                Maybe::Some(number) => { identity(number) - 42 },
                Maybe::None => { 1 },
            }
        }
        "#,
        "generic-enum-specialization",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn result_question_mark_returns_errors_and_unwraps_successes() {
    let result = compile_and_run(
        r#"
        enum Fault { Bad }

        fn source() -> Result<i64, Fault> { Result::Ok(41) }
        fn fail() -> Result<i64, Fault> { Result::Err(Fault::Bad) }
        fn conditional(flag: bool) -> Result<i64, Fault> {
            Result::Ok(if flag { source()? } else { 42 })
        }
        fn compute() -> Result<i64, Fault> { Result::Ok(source()? + 1) }
        fn forward_error() -> Result<i64, Fault> { fail()?; Result::Ok(0) }

        fn main() -> i64 {
            let success = match compute() {
                Result::Ok(value) => { value - 42 },
                Result::Err(_) => { 1 },
            };
            let nested = match conditional(true) {
                Result::Ok(value) => { value - 41 },
                Result::Err(_) => { 1 },
            };
            let propagated = match forward_error() {
                Result::Ok(_) => { 1 },
                Result::Err(_) => { 0 },
            };
            success + nested + propagated
        }
        "#,
        "result-question-propagation",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn expected_result_types_flow_into_if_and_match_constructors() {
    let result = compile_and_run(
        r#"
        enum Fault { Bad }
        fn choose(flag: bool) -> Result<i64, Fault> {
            if flag { Result::Ok(42) } else { Result::Err(Fault::Bad) }
        }
        fn main() -> i64 {
            match choose(true) {
                Result::Ok(value) => { value - 42 },
                Result::Err(_) => { 1 },
            }
        }
        "#,
        "result-expected-constructors",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn question_mark_rejects_non_results_and_incompatible_returns() {
    assert_compile_error(
        "fn main() -> i64 { 42? }",
        "try-non-result",
        "postfix `?` requires a `Result<T, E>` value",
    );
    assert_compile_error(
        "enum Fault { Bad } fn source() -> Result<i64, Fault> { Result::Ok(1) } fn main() -> i64 { source()? }",
        "try-non-result-return",
        "postfix `?` can only be used in a function returning `Result<T, E>`",
    );
    assert_compile_error(
        "enum A { Bad } enum B { Worse } fn source() -> Result<i64, A> { Result::Err(A::Bad) } fn caller() -> Result<i64, B> { source()? } fn main() -> i64 { 0 }",
        "try-error-type-mismatch",
        "error propagation has type A, expected B",
    );
}

#[test]
fn result_identity_is_reserved_and_arbitrary_ok_err_enums_are_not_propagatable() {
    assert_compile_error(
        "enum Result<T, E> { Ok(T), Err(E) } fn main() -> i64 { 0 }",
        "result-reserved-name",
        "`Result` is a reserved compiler-provided prelude enum",
    );
    assert_compile_error(
        "enum Lookalike { Ok(i64), Err(i64) } fn make() -> Lookalike { Lookalike::Ok(1) } fn use_it() -> i64 { make()? } fn main() -> i64 { 0 }",
        "try-lookalike-enum",
        "postfix `?` requires the built-in `Result<T, E>` type",
    );
}
