use crate::common::*;

#[test]
fn generic_function_type_inference_reads_global_constants() {
    let result = compile_and_run(
        r#"
const VALUE = 42;
fn identity<T>(value: T) -> T { value }
fn main() -> i64 { identity::<i64>(VALUE) - 42 }
"#,
        "generics-global-inference",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_functions_infer_types_and_specialize_independently() {
    let result = compile_and_run(
        r#"
        fn identity<T>(value: T) -> T { value }
        fn main() -> i64 {
            let integer = identity(42);
            let text = identity::<str>("generic");
            println(text);
            integer - 42
        }
        "#,
        "generics-functions-inference",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "generic\n");
}

#[test]
fn generic_structs_can_be_constructed_nested_and_passed_to_functions() {
    let result = compile_and_run(
        r#"
        struct Box<T> { value: T }
        struct Pair<L, R> { left: L, right: R }
        fn unbox<T>(value: Box<T>) -> T { value.value }
        fn main() -> i64 {
            let nested = Pair<i64, Box<str>> {
                left: 42,
                right: Box<str> { value: "nested" },
            };
            println(unbox(nested.right));
            nested.left - 42
        }
        "#,
        "generics-structs-nested",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "nested\n");
}

#[test]
fn generic_functions_can_construct_and_return_specialized_structs() {
    let result = compile_and_run(
        r#"
        struct Box<T> { value: T }
        fn make<T>(value: T) -> Box<T> {
            Box<T> { value: value }
        }
        fn main() -> i64 {
            let boxed = make(42);
            boxed.value - 42
        }
        "#,
        "generics-return-struct",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_structs_can_be_used_in_enum_payloads() {
    let result = compile_and_run(
        r#"
        struct Box<T> { value: T }
        enum Wrapped { Value(Box<i64>) }
        fn main() -> i64 {
            match Wrapped::Value(Box<i64> { value: 42 }) {
                Wrapped::Value(boxed) => { boxed.value - 42 },
            }
        }
        "#,
        "generics-enum-payload",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_inference_uses_enum_payload_bindings() {
    let result = compile_and_run(
        r#"
        enum Maybe { Some(i64), None }
        fn identity<T>(value: T) -> T { value }
        fn main() -> i64 {
            match Maybe::Some(42) {
                Maybe::Some(value) => { identity(value) - 42 },
                Maybe::None => { 1 },
            }
        }
        "#,
        "generics-match-payload-binding",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_functions_can_call_themselves_after_specialization() {
    let result = compile_and_run(
        r#"
        fn countdown<T>(value: T) -> T {
            if false { countdown::<T>(value) } else { value }
        }
        fn main() -> i64 { countdown(42) - 42 }
        "#,
        "generics-recursion",
    );
    assert_eq!(result.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&result.stdout).is_empty());
}

#[test]
fn generic_functions_infer_types_inside_arrays_vectors_and_nested_calls() {
    let result = compile_and_run(
        r#"
        fn first<T>(values: Vec<T>) -> T { values[0] }
        fn head<T>(values: [T; 2]) -> T { values[0] }
        fn identity<T>(value: T) -> T { value }
        fn main() -> i64 {
            let values: Vec<i64> = vec_new();
            vec_push(values, 42);
            first(values) - head([identity(42), 0])
        }
        "#,
        "generics-vectors-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_inference_requires_all_type_parameters_to_be_determined() {
    assert_compile_error(
        "fn make<T>() -> i64 { 1 } fn main() -> i64 { make() }",
        "generics-inference-missing",
        "cannot infer generic type parameter `T`",
    );
}

#[test]
fn generic_type_arguments_are_checked() {
    assert_compile_error(
        "fn identity<T>(value: T) -> T { value } fn main() -> i64 { identity::<bool>(1) }",
        "generics-argument-mismatch",
        "explicit type argument `bool` conflicts with inferred type `i64`",
    );
}

#[test]
fn generic_structs_reject_missing_or_extra_type_arguments() {
    assert_compile_error(
        "struct Box<T> { value: T } fn main() -> i64 { let value: Box = Box { value: 1 }; value.value - 1 }",
        "generics-struct-missing-arguments",
        "generic struct `Box` requires explicit type arguments",
    );
    assert_compile_error(
        "struct Box<T> { value: T } fn main() -> i64 { let value = Box<i64, bool> { value: 1 }; 0 }",
        "generics-struct-extra-arguments",
        "generic struct `Box` expects 1 type argument(s), got 2",
    );
}

#[test]
fn generic_struct_templates_are_validated_even_when_unused() {
    assert_compile_error(
        "struct Broken<T> { value: Missing } fn main() -> i64 { 0 }",
        "generics-invalid-unused-struct",
        "unknown type `Missing`",
    );
    assert_compile_error(
        "fn broken<T>(value: U) -> i64 { 0 } fn main() -> i64 { 0 }",
        "generics-invalid-unused-function",
        "unknown type `U`",
    );
}

#[test]
fn expanding_generic_recursion_is_stopped_by_the_specialization_limit() {
    assert_compile_error(
        "fn grow<T>(value: T) -> i64 { let next: Vec<T> = vec_new(); grow::<Vec<T>>(next) } fn main() -> i64 { grow(1) }",
        "generics-specialization-limit",
        "generic specialization exceeded the limit of 512",
    );
}
