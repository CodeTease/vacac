use crate::common::*;
use std::fs;
use std::process::Command;

fn module_project(
    name: &str,
    root: &str,
    files: &[(&str, &str)],
) -> (std::path::PathBuf, std::process::Output) {
    let directory =
        std::env::temp_dir().join(format!("vacac-traits-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let root_path = directory.join("main.vca");
    fs::write(&root_path, root).unwrap();
    for (path, source) in files {
        let file = directory.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(&root_path)
        .arg("-o")
        .arg(directory.join("program"))
        .output()
        .unwrap();
    (directory, output)
}

#[test]
fn traits_support_direct_calls_explicit_self_and_unit_returns() {
    let result = compile_and_run(
        r#"
trait Show { fn show(value: Self) -> str; }
trait Zero { fn zero() -> Self; }
trait Touch { fn touch(value: Self) -> unit; }

impl Show for i64 { fn show(value: Self) -> str { "integer" } }
impl Show for str { fn show(value: Self) -> str { value } }
impl Zero for i64 { fn zero() -> Self { 42 } }
impl Touch for bool { fn touch(value: bool) -> unit { println("touched"); } }

fn main() -> i64 {
    println(Show::show(1));
    println(Show::show("text"));
    Touch::touch(true);
    <i64 as Zero>::zero() - 42
}
"#,
        "traits-basic",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "integer\ntext\ntouched\n"
    );
}

#[test]
fn generic_functions_dispatch_on_declared_bounds() {
    let result = compile_and_run(
        r#"
trait Left { fn left(value: Self) -> i64; }
trait Right { fn right(value: Self) -> i64; }

impl Left for i64 { fn left(value: Self) -> i64 { value } }
impl Right for i64 { fn right(value: Self) -> i64 { value + 1 } }

fn combine<T: Left + Right>(value: T) -> i64 {
    Left::left(value) + Right::right(value)
}

fn main() -> i64 { combine(20) - 41 }
"#,
        "traits-multiple-bounds",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn marker_traits_can_constrain_generic_functions() {
    let result = compile_and_run(
        r#"
trait Marker {}
impl Marker for bool {}
fn require<T: Marker>(value: T) -> T { value }
fn main() -> i64 { if require(true) { 0 } else { 1 } }
"#,
        "traits-marker",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_impls_delegate_through_nested_bounds() {
    let result = compile_and_run(
        r#"
trait Show { fn show(value: Self) -> str; }
struct Box<T> { value: T }
impl Show for str { fn show(value: Self) -> str { value } }
impl<T: Show> Show for Box<T> {
    fn show(value: Self) -> str { Show::show(value.value) }
}
fn print_show<T: Show>(value: T) -> unit { println(Show::show(value)); }
fn main() -> i64 {
    print_show(Box<str> { value: "nested" });
    0
}
"#,
        "traits-generic-impl",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "nested\n");
}

#[test]
fn self_substitution_works_through_aggregate_function_parameters() {
    let result = compile_and_run(
        r#"
trait First { fn first(values: Vec<Self>) -> Self; }
impl First for i64 { fn first(values: Vec<i64>) -> i64 { values[0] } }
fn main() -> i64 {
    let values: Vec<i64> = vec_new();
    vec_push(values, 42);
    First::first(values) - 42
}
"#,
        "traits-nested-self",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_structs_and_enums_enforce_declared_trait_bounds() {
    let result = compile_and_run(
        r#"
trait Show { fn show(value: Self) -> str; }
struct Wrapper<T: Show> { value: T }
enum Wrapped<T: Show> { Value(T) }
impl Show for i64 { fn show(value: Self) -> str { "integer" } }
fn main() -> i64 {
    let wrapper = Wrapper<i64> { value: 42 };
    let wrapped: Wrapped<i64> = Wrapped::Value(wrapper.value);
    match wrapped {
        Wrapped::Value(value) => { value - 42 },
    }
}
"#,
        "traits-generic-type-bounds",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn separate_traits_can_reuse_function_names() {
    let result = compile_and_run(
        r#"
trait A { fn value(value: Self) -> i64; }
trait B { fn value(value: Self) -> i64; }
impl A for i64 { fn value(value: Self) -> i64 { value } }
impl B for i64 { fn value(value: Self) -> i64 { value + 1 } }
fn main() -> i64 { A::value(20) + B::value(20) - 41 }
"#,
        "traits-duplicate-method-names",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn trait_calls_work_inside_closures_and_generic_specializations() {
    let result = compile_and_run(
        r#"
trait AddOne { fn add_one(value: Self) -> i64; }
impl AddOne for i64 { fn add_one(value: Self) -> i64 { value + 1 } }
fn apply<T: AddOne>(value: T, function: fn(T) -> i64) -> i64 { function(value) }
fn main() -> i64 {
    let closure: fn(i64) -> i64 = fn(value: i64) -> i64 { AddOne::add_one(value) };
    apply(41, closure) - 42
}
"#,
        "traits-closures",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn explicit_self_works_with_generic_type_parameters() {
    let result = compile_and_run(
        r#"
trait Zero { fn zero() -> Self; }
impl Zero for i64 { fn zero() -> Self { 42 } }
fn zero<T: Zero>() -> T { <T as Zero>::zero() }
fn main() -> i64 { zero::<i64>() - 42 }
"#,
        "traits-generic-explicit-self",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn structural_generic_impl_targets_are_specialized() {
    let result = compile_and_run(
        r#"
trait Length { fn length(value: Self) -> i64; }
impl<T> Length for Vec<T> { fn length(value: Vec<T>) -> i64 { len(value) } }
trait Second { fn second(value: Self) -> i64; }
impl<T> Second for (T, i64) { fn second(value: (T, i64)) -> i64 { let (_, second) = value; second } }
fn main() -> i64 {
    let values: Vec<i64> = vec_new();
    vec_push(values, 42);
    Length::length(values) - 1 + Second::second((true, 42)) - 42
}
"#,
        "traits-structural-targets",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_selection_distinguishes_multiple_impls() {
    let result = compile_and_run(
        r#"
trait Label { fn label(value: Self) -> str; }
struct Left<T> { value: T }
struct Right<T> { value: T }
impl<T> Label for Left<T> { fn label(value: Self) -> str { "left" } }
impl<T> Label for Right<T> { fn label(value: Self) -> str { "right" } }
fn main() -> i64 {
    println(Label::label(Left<i64> { value: 1 }));
    println(Label::label(Right<i64> { value: 2 }));
    0
}
"#,
        "traits-multiple-generic-impls",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "left\nright\n");
}

#[test]
fn trait_selection_uses_processed_nested_generic_arguments() {
    let result = compile_and_run(
        r#"
struct Box<T> { value: T }
trait Unwrap { fn unwrap(value: Self) -> i64; }
impl Unwrap for Box<Box<i64>> {
    fn unwrap(value: Box<Box<i64>>) -> i64 { value.value.value }
}
fn main() -> i64 {
    Unwrap::unwrap(Box<Box<i64>> { value: Box<i64> { value: 42 } }) - 42
}
"#,
        "traits-nested-generic-selection",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn unused_generic_bodies_defer_missing_impls_until_instantiation() {
    let result = compile_and_run(
        r#"
trait Show { fn show(value: Self) -> str; }
impl Show for i64 { fn show(value: Self) -> str { "integer" } }
fn unused<T: Show>(value: T) -> str { Show::show(value) }
fn main() -> i64 { 0 }
"#,
        "traits-unused-generic",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_bodies_follow_instantiation_time_checking() {
    let result = compile_and_run(
        r#"
trait Show { fn show(value: Self) -> str; }
impl Show for i64 { fn show(value: Self) -> str { "integer" } }
fn unbounded<T>(value: T) -> str { Show::show(value) }
fn main() -> i64 { println(unbounded(42)); 0 }
"#,
        "traits-instantiation-checking",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "integer\n");
}

#[test]
fn public_declarations_cannot_expose_private_trait_bounds() {
    assert_compile_error(
        "trait Hidden {} pub fn require<T: Hidden>(value: T) -> T { value } fn main() -> i64 { 0 }",
        "traits-private-bound",
        "private trait `Hidden` cannot appear in a public trait bound",
    );
}

#[test]
fn generic_bounds_reject_unknown_and_duplicate_traits() {
    assert_compile_error(
        "fn require<T: Missing>(value: T) -> T { value } fn main() -> i64 { 0 }",
        "traits-unknown-bound",
        "unknown trait `Missing` in bound for `T`",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } fn require<T: Show + Show>(value: T) -> T { value } fn main() -> i64 { 0 }",
        "traits-duplicate-bound",
        "trait `Show` is listed more than once",
    );
}

#[test]
fn grammar_rejects_generic_traits_public_impls_and_method_generics() {
    assert_compile_error(
        "trait Convert<T> { fn convert(value: Self) -> T; } fn main() -> i64 { 0 }",
        "traits-generic-trait",
        "generic traits are not supported",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } pub impl Show for i64 { fn show(value: i64) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-public-impl",
        "`pub impl` is not supported",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show<U>(value: i64) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-method-generics",
        "impl functions cannot declare their own generic or lifetime parameters",
    );
}

#[test]
fn self_is_rejected_outside_trait_and_impl_signatures() {
    assert_compile_error(
        "fn main() -> i64 { let value: Self = 0; value }",
        "traits-self-outside",
        "`Self` is only available in trait and impl declarations",
    );
}

#[test]
fn trait_declarations_share_the_type_namespace() {
    assert_compile_error(
        "struct Show { value: i64 } trait Show { fn show(value: Self) -> i64; } fn main() -> i64 { 0 }",
        "traits-type-namespace",
        "trait `Show` is defined more than once",
    );
}

#[test]
fn unused_trait_and_impl_declarations_still_validate_their_types() {
    assert_compile_error(
        "trait Broken { fn broken(value: Missing) -> i64; } fn main() -> i64 { 0 }",
        "traits-invalid-trait-type",
        "unknown type `Missing`",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> i64; } struct Box<T> { value: T } impl Show for Box { fn show(value: Box<T>) -> i64 { 0 } } fn main() -> i64 { 0 }",
        "traits-invalid-target-type",
        "generic type `Box` requires explicit type arguments",
    );
}

#[test]
fn impl_target_types_are_validated_without_self_in_methods() {
    assert_compile_error(
        "trait Marker { fn marker() -> i64; } impl Marker for Vec<unit> { fn marker() -> i64 { 0 } } fn main() -> i64 { 0 }",
        "traits-invalid-nested-target-type",
        "type `unit` is not valid in this function type position",
    );

    let result = compile_and_run(
        r#"
trait Marker { fn marker() -> i64; }
trait Concrete { fn concrete() -> i64; }
impl<T> Marker for Vec<T> { fn marker() -> i64 { 0 } }
impl Concrete for Vec<i64> { fn concrete() -> i64 { 0 } }
fn main() -> i64 {
    <Vec<i64> as Marker>::marker() + <Vec<i64> as Concrete>::concrete()
}
"#,
        "traits-valid-nested-target-types",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_bounds_on_structs_are_checked_at_use_sites() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } struct Wrapper<T: Show> { value: T } fn main() -> i64 { let wrapper = Wrapper<bool> { value: true }; 0 }",
        "traits-struct-bound-failure",
        "type `bool` does not implement trait `Show`",
    );
}

#[test]
fn self_is_rejected_in_local_type_annotations_inside_impls() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: Self) -> str { let copy: Self = value; \"x\" } } fn main() -> i64 { 0 }",
        "traits-self-local",
        "`Self` is only available in trait and impl declarations",
    );
}

#[test]
fn duplicate_and_unknown_trait_declarations_are_rejected() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } trait Show { fn other(value: Self) -> str; } fn main() -> i64 { 0 }",
        "traits-duplicate-declaration",
        "trait `Show` is defined more than once",
    );
    assert_compile_error(
        "trait Empty {} impl Empty for bool {} fn main() -> i64 { Empty::missing(true); 0 }",
        "traits-unknown-function",
        "trait `Empty` has no function `missing`",
    );
}

#[test]
fn missing_implementations_report_the_instantiation_site() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: Self) -> str { \"int\" } } fn show_it<T: Show>(value: T) -> str { Show::show(value) } fn main() -> i64 { println(show_it(\"no\")); 0 }",
        "traits-missing-impl",
        "type `str` does not implement trait `Show`",
    );
}

#[test]
fn explicit_self_reports_a_missing_implementation() {
    assert_compile_error(
        "trait Zero { fn zero() -> Self; } impl Zero for i64 { fn zero() -> Self { 0 } } fn main() -> i64 { <bool as Zero>::zero(); 0 }",
        "traits-missing-explicit-impl",
        "type `bool` does not implement trait `Zero`",
    );
}

#[test]
fn implicit_self_requires_an_inferable_parameter() {
    assert_compile_error(
        "trait Zero { fn zero() -> Self; } impl Zero for i64 { fn zero() -> Self { 0 } } fn main() -> i64 { Zero::zero(); 0 }",
        "traits-ambiguous-self",
        "cannot infer `Self`",
    );
}

#[test]
fn impl_signatures_must_match_the_trait_exactly() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: bool) -> str { \"bad\" } } fn main() -> i64 { 0 }",
        "traits-signature-parameter",
        "parameter `value` of `Show::show` has type `bool`, expected `i64`",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> i64 { value } } fn main() -> i64 { 0 }",
        "traits-signature-return",
        "function `Show::show` returns `i64`, expected `str`",
    );
}

#[test]
fn impls_must_define_exactly_the_required_functions() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 {} fn main() -> i64 { 0 }",
        "traits-missing-function",
        "is missing required function `show`",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"x\" } fn extra(value: i64) -> i64 { value } } fn main() -> i64 { 0 }",
        "traits-extra-function",
        "function `extra` is not a member of trait `Show`",
    );
}

#[test]
fn duplicate_impls_and_generic_overlap_are_rejected() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"a\" } } impl Show for i64 { fn show(value: i64) -> str { \"b\" } } fn main() -> i64 { 0 }",
        "traits-duplicate-impl",
        "conflicting implementations of trait `Show`",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } struct Box<T> { value: T } impl<T> Show for Box<T> { fn show(value: Self) -> str { \"a\" } } impl Show for Box<i64> { fn show(value: Self) -> str { \"b\" } } fn main() -> i64 { 0 }",
        "traits-overlap-impl",
        "conflicting implementations of trait `Show`",
    );
}

#[test]
fn invalid_impl_targets_and_unknown_traits_are_rejected() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl<T> Show for T { fn show(value: T) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-blanket-impl",
        "impl target cannot be a bare type parameter",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } struct Box<T> { value: T } impl<T, U> Show for Box<T> { fn show(value: Self) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-unconstrained-impl",
        "impl type parameter `U` is not constrained",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Missing for i64 { fn show(value: i64) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-unknown-trait",
        "unknown trait `Missing`",
    );
}

#[test]
fn bound_failures_on_public_declarations_are_reported() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } fn print_show<T: Show>(value: T) -> str { Show::show(value) } fn main() -> i64 { println(print_show(true)); 0 }",
        "traits-bound-failure",
        "type `bool` does not implement trait `Show`",
    );
}

#[test]
fn public_traits_and_impls_work_across_modules() {
    let (directory, compile) = module_project(
        "public",
        "mod common;\nfn show_it<T: common::Show>(value: T) -> str { common::Show::show(value) }\nfn main() -> i64 { println(show_it(common::Counter { value: 42 })); 0 }\n",
        &[(
            "common.vca",
            "fn helper() -> str { \"counter\" }\npub trait Show { fn show(value: Self) -> str; }\npub struct Counter { value: i64 }\nimpl Show for Counter { fn show(value: Self) -> str { helper() } }\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "counter\n");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn private_traits_and_types_remain_private_across_modules() {
    let (directory, compile) = module_project(
        "private",
        "mod common;\nfn main() -> i64 { common::Show::show(1); 0 }\n",
        &[(
            "common.vca",
            "trait Show { fn show(value: Self) -> str; }\nimpl Show for i64 { fn show(value: i64) -> str { \"x\" } }\n",
        )],
    );
    assert!(!compile.status.success());
    assert!(String::from_utf8_lossy(&compile.stderr).contains("trait `common::Show` is private"));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn trait_calls_propagate_expected_types_to_arguments() {
    let result = compile_and_run(
        r#"
trait Unwrap { fn unwrap(value: Self) -> i64; }
trait Combine { fn combine(tag: i64, value: Self) -> i64; }
struct Box<T> { value: T }
impl Unwrap for Box<i64> { fn unwrap(value: Box<i64>) -> i64 { value.value } }
impl Combine for Box<i64> {
    fn combine(tag: i64, value: Box<i64>) -> i64 { tag + value.value }
}
fn main() -> i64 {
    let explicit = <Box<i64> as Unwrap>::unwrap(Box { value: 20 });
    let inferred = Combine::combine(1, Box { value: 21 });
    explicit + inferred - 42
}
"#,
        "traits-expected-arguments",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn trait_calls_normalize_typed_global_arguments() {
    let result = compile_and_run(
        r#"
struct Box<T> { value: T }
const VALUE: Box<i64> = Box<i64> { value: 42 };
trait Unwrap { fn unwrap(value: Self) -> i64; }
impl Unwrap for Box<i64> { fn unwrap(value: Box<i64>) -> i64 { value.value } }
fn main() -> i64 { Unwrap::unwrap(VALUE) - 42 }
"#,
        "traits-normalize-global-argument",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn trait_self_inference_compares_concrete_type_identities() {
    let result = compile_and_run(
        r#"
struct Box<T> { value: T }
const RIGHT: Box<i64> = Box<i64> { value: 22 };
trait Combine { fn combine(left: Self, right: Self) -> i64; }
impl Combine for Box<i64> {
    fn combine(left: Box<i64>, right: Box<i64>) -> i64 { left.value + right.value }
}
fn main() -> i64 { Combine::combine(Box<i64> { value: 20 }, RIGHT) - 42 }
"#,
        "traits-self-concrete-identity",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        "struct Box<T> { value: T } trait Combine { fn combine(left: Self, right: Self) -> i64; } impl Combine for Box<i64> { fn combine(left: Box<i64>, right: Box<i64>) -> i64 { 0 } } fn main() -> i64 { Combine::combine(Box<bool> { value: true }, Box<i64> { value: 1 }) }",
        "traits-self-conflicting-concrete-types",
        "trait `Self` is inferred as both",
    );
}

#[test]
fn explicit_self_errors_use_trait_function_names() {
    assert_compile_error(
        "trait Zero { fn zero() -> Self; } impl Zero for i64 { fn zero() -> Self { 0 } } fn main() -> i64 { <i64 as Zero>::zero(1); 0 }",
        "traits-explicit-arity",
        "trait function `Zero::zero` expects 0 argument(s), got 1",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"x\" } } fn main() -> i64 { println(<i64 as Show>::show(\"bad\")); 0 }",
        "traits-explicit-type",
        "argument 1 to `Show::show` has type `str`, expected `i64`",
    );
}

#[test]
fn trait_calls_infer_generic_enum_arguments_and_explicit_none() {
    let result = compile_and_run(
        r#"
trait Unwrap { fn unwrap(value: Self) -> i64; }
enum Maybe<T> { Some(T), None }
impl Unwrap for Maybe<i64> {
    fn unwrap(value: Maybe<i64>) -> i64 {
        match value {
            Maybe::Some(number) => { number },
            Maybe::None => { 0 },
        }
    }
}
fn main() -> i64 {
    let explicit = <Maybe<i64> as Unwrap>::unwrap(Maybe::Some(20));
    let inferred = Unwrap::unwrap(Maybe::Some(22));
    let none = Unwrap::unwrap(Maybe::<i64>::None);
    explicit + inferred + none - 42
}
"#,
        "traits-expected-enum",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn unrepresentable_trait_signatures_are_rejected() {
    assert_compile_error(
        "trait Borrow { fn borrow(value: &Self) -> &Self; } fn main() -> i64 { 0 }",
        "traits-reference-return",
        "trait functions cannot return references",
    );
    assert_compile_error(
        "trait Store { fn store(value: Vec<&Self>) -> i64; } fn main() -> i64 { 0 }",
        "traits-reference-aggregate",
        "references cannot be stored in arrays, vectors, tuples, or generic arguments",
    );
    assert_compile_error(
        "trait Store { fn store(value: Vec<unit>) -> i64; } fn main() -> i64 { 0 }",
        "traits-unit-aggregate",
        "type `unit` is not valid in this function type position",
    );
}

#[test]
fn self_cannot_be_a_generic_parameter_or_type_name() {
    assert_compile_error(
        "struct Box<Self> { value: i64 } fn main() -> i64 { 0 }",
        "traits-self-generic-parameter",
        "`Self` is reserved and cannot be a generic parameter",
    );
    assert_compile_error(
        "enum Self { Value } fn main() -> i64 { 0 }",
        "traits-self-type-name",
        "`Self` is reserved and cannot name a type",
    );
}

#[test]
fn generic_impl_bound_failures_name_the_nested_requirement() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } struct Box<T> { value: T } impl Show for i64 { fn show(value: i64) -> str { \"int\" } } impl<T: Show> Show for Box<T> { fn show(value: Self) -> str { Show::show(value.value) } } fn main() -> i64 { println(Show::show(Box<bool> { value: true })); 0 }",
        "traits-generic-impl-bound",
        "type `bool` does not implement trait `Show`",
    );
}

#[test]
fn duplicate_trait_members_and_impl_functions_are_rejected() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; fn show(other: Self) -> str; } fn main() -> i64 { 0 }",
        "traits-duplicate-function-declaration",
        "trait function `Show::show` is defined more than once",
    );
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"a\" } fn show(other: i64) -> str { \"b\" } } fn main() -> i64 { 0 }",
        "traits-duplicate-impl-function",
        "implementation of `Show` defines `show` more than once",
    );
}

#[test]
fn trait_names_cannot_collide_with_modules() {
    let (directory, compile) = module_project(
        "module-collision",
        "mod util;\ntrait util {}\nfn main() -> i64 { util::f(1) }\n",
        &[("util.vca", "pub fn f(value: i64) -> i64 { value }\n")],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr)
            .contains("trait `util` conflicts with a module of the same name")
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn module_functions_take_precedence_over_root_traits() {
    let (directory, compile) = module_project(
        "module-callable-before-trait",
        concat!(
            "mod child;\n",
            "pub trait Ops { fn value(value: Self) -> i64; }\n",
            "impl Ops for i64 { fn value(value: i64) -> i64 { value } }\n",
            "pub trait Show { fn show(value: Self) -> i64; }\n",
            "impl Show for i64 { fn show(value: i64) -> i64 { value } }\n",
            "fn main() -> i64 { child::module_call() - 42 + child::trait_call() - 41 + child::Ops::value(39) - 40 }\n",
        ),
        &[
            (
                "child.vca",
                "mod Ops;\npub fn module_call() -> i64 { Ops::value(41) }\npub fn trait_call() -> i64 { Show::show(41) }\n",
            ),
            (
                "child/Ops.vca",
                "pub fn value(value: i64) -> i64 { value + 1 }\n",
            ),
        ],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn non_trait_names_do_not_shadow_module_function_calls() {
    let (directory, compile) = module_project(
        "module-shadow",
        "mod util;\nstruct util { value: i64 }\nfn main() -> i64 { util::f(42) }\n",
        &[("util.vca", "pub fn f(value: i64) -> i64 { value }\n")],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(42));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn trait_diagnostics_keep_child_module_source_locations() {
    let (directory, compile) = module_project(
        "diagnostic",
        "mod common;\nfn main() -> i64 { 0 }\n",
        &[(
            "common.vca",
            "pub trait Show { fn show(value: Self) -> str; }\nimpl Show for bool { fn show(value: i64) -> str { \"x\" } }\n",
        )],
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("common.vca:2:"), "{stderr}");
    assert!(stderr.contains("expected `bool`"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn trait_diagnostics_have_structured_json_locations() {
    let (directory, result) = run_compiler_with_args(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"x\" } } fn show_it<T: Show>(value: T) -> str { Show::show(value) } fn main() -> i64 { println(show_it(true)); 0 }",
        "traits-json",
        &["--json"],
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(
        stderr.starts_with("{\"severity\":\"error\",\"file\":"),
        "{stderr}"
    );
    assert!(stderr.contains("\"line\":1"), "{stderr}");
    assert!(stderr.contains("\"column\":"), "{stderr}");
    assert!(
        stderr.contains("does not implement trait `Show`"),
        "{stderr}"
    );
    assert!(!stderr.contains('^'), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn trait_dispatch_is_erased_before_llvm_emission() {
    let (directory, result) = run_compiler_with_args(
        "trait Show { fn show(value: Self) -> str; } impl Show for i64 { fn show(value: i64) -> str { \"x\" } } fn main() -> i64 { println(Show::show(42)); 0 }",
        "traits-llvm",
        &["--emit-llvm", "program.ll", "--quiet"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(directory.join("program.ll")).unwrap();
    assert!(ir.contains("define"), "{ir}");
    assert!(ir.contains("call"), "{ir}");
    assert!(!ir.contains("vtable"), "{ir}");
    assert!(!ir.contains("trait.dispatch"), "{ir}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn malformed_traits_and_impls_recover_to_later_declarations() {
    let (directory, compile) = compile_source(
        "trait Broken { fn show(value: Self) -> str }\nfn broken( -> i64 { 0 }\nfn main() -> i64 { 0 }\n",
        "traits-parser-recovery",
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("expected `;`"), "{stderr}");
    assert_eq!(stderr.matches("vacac:").count(), 2, "{stderr}");
    assert!(stderr.contains("test.vca:1:"), "{stderr}");
    assert!(stderr.contains("test.vca:2:"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn default_bodies_are_inherited_and_can_be_overridden() {
    let result = compile_and_run(
        r#"
trait Twice {
    fn twice(value: Self) -> i64 { value + value }
}
impl Twice for i64 {}
impl Twice for bool {
    fn twice(value: bool) -> i64 { 100 }
}
fn main() -> i64 { Twice::twice(21) - 42 + Twice::twice(true) - 100 }
"#,
        "traits-default-inherited",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn inherited_default_bodies_specialize_stateful_closure_captures() {
    let result = compile_and_run(
        r#"
        trait Counted {
            fn make(seed: i64) -> fn() -> i64 {
                let count = seed;
                fn() -> i64 {
                    count = count + 1;
                    count
                }
            }
        }
        impl Counted for i64 {}
        impl Counted for bool {}
        fn main() -> i64 {
            const first = <i64 as Counted>::make(40);
            const second = <bool as Counted>::make(40);
            print_i64(first());
            print_i64(first());
            print_i64(second());
            first() - 43
        }
        "#,
        "traits-default-stateful-closures",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "41\n42\n41\n");
}

#[test]
fn default_bodies_call_sibling_functions_and_annotate_self() {
    let result = compile_and_run(
        r#"
trait Label {
    fn name(value: Self) -> str;
    fn shout(value: Self) -> str { <Self as Label>::name(value) + "!" }
    fn copied(value: Self) -> Self { let same: Self = value; same }
}
impl Label for i64 { fn name(value: Self) -> str { "num" } }
impl Label for str {
    fn name(value: Self) -> str { "text" }
    fn shout(value: Self) -> str { "loud" }
}
fn main() -> i64 {
    println(Label::shout(1));
    println(Label::shout("x"));
    println(Label::name(Label::copied(7)));
    0
}
"#,
        "traits-default-sibling",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "num!\nloud\nnum\n");
}

#[test]
fn default_bodies_specialize_for_generic_impls() {
    let result = compile_and_run(
        r#"
trait Content { fn content(value: Self) -> i64; }
trait Total { fn total(value: Self) -> i64 { <Self as Content>::content(value) + 1 } }
struct Box<T> { value: T }
impl Content for i64 { fn content(value: Self) -> i64 { value } }
impl<T: Content> Content for Box<T> { fn content(value: Self) -> i64 { value.value } }
impl<T: Content> Total for Box<T> {}
fn summarize<T: Total>(value: T) -> i64 { Total::total(value) }
fn main() -> i64 { summarize(Box<i64> { value: 41 }) - 42 }
"#,
        "traits-default-generic-impl",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn inherited_default_bodies_keep_the_trait_module_scope() {
    let (directory, compile) = module_project(
        "default-scope",
        concat!(
            "mod shapes;\n",
            "struct Square { side: i64 }\n",
            "impl shapes::Shape for Square { fn sides(value: Square) -> i64 { 4 } }\n",
            "fn main() -> i64 { shapes::Shape::total(Square { side: 0 }) - 5 }\n",
        ),
        &[(
            "shapes.vca",
            concat!(
                "fn adjust() -> i64 { 1 }\n",
                "pub trait Shape {\n",
                "    fn sides(value: Self) -> i64;\n",
                "    fn extra(value: Self) -> i64 { adjust() }\n",
                "    fn total(value: Self) -> i64 { <Self as Shape>::sides(value) + <Self as Shape>::extra(value) }\n",
                "}\n",
            ),
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn default_bodies_cannot_reach_private_items_from_other_modules() {
    let (directory, compile) = module_project(
        "default-private",
        "mod hidden;\ntrait Uses { fn uses(value: Self) -> i64 { hidden::secret() } }\nimpl Uses for i64 {}\nfn main() -> i64 { 0 }\n",
        &[("hidden.vca", "fn secret() -> i64 { 1 }\n")],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr).contains("function `hidden::secret` is private"),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn inherited_default_bodies_are_statically_dispatched() {
    let (directory, result) = run_compiler_with_args(
        "trait Doubled { fn doubled(value: Self) -> i64 { value + value } } impl Doubled for i64 {} fn main() -> i64 { Doubled::doubled(21) - 42 }",
        "traits-default-llvm",
        &["--emit-llvm", "program.ll", "--quiet"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(directory.join("program.ll")).unwrap();
    assert!(ir.contains("Doubled for i64::doubled"), "{ir}");
    assert!(!ir.contains("vtable"), "{ir}");
    assert!(!ir.contains("trait.dispatch"), "{ir}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn implementations_still_require_functions_without_default_bodies() {
    assert_compile_error(
        "trait Show { fn show(value: Self) -> str; fn describe(value: Self) -> str { \"d\" } } impl Show for i64 {} fn main() -> i64 { 0 }",
        "traits-default-missing-required",
        "missing required function `show`",
    );
}

#[test]
fn overriding_a_default_body_must_match_the_trait_signature() {
    assert_compile_error(
        "trait D { fn d(value: Self) -> i64 { 0 } } impl D for i64 { fn d(value: i64) -> str { \"x\" } } fn main() -> i64 { 0 }",
        "traits-default-override-signature",
        "function `D::d` returns `str`, expected `i64`",
    );
}

#[test]
fn inherited_default_body_errors_report_the_trait_source() {
    let (directory, compile) = compile_source(
        "trait Bad { fn bad(value: Self) -> i64 { value + true } }\nimpl Bad for i64 {}\nfn main() -> i64 { 0 }",
        "traits-default-body-error",
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        stderr.contains("operator operands have different types: i64 and bool"),
        "{stderr}"
    );
    assert!(stderr.contains("test.vca:1:"), "{stderr}");
    assert!(!stderr.contains("test.vca:2:"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}
