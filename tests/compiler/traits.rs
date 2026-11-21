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
        "mod common;\nfn main() -> i64 { println(common::Show::show(common::Counter { value: 42 })); 0 }\n",
        &[(
            "common.vca",
            "pub trait Show { fn show(value: Self) -> str; }\npub struct Counter { value: i64 }\nimpl Show for Counter { fn show(value: Self) -> str { \"counter\" } }\n",
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
fn malformed_traits_and_impls_recover_to_later_declarations() {
    let (directory, compile) = compile_source(
        "trait Broken { fn show(value: Self) -> str }\nimpl Show for i64 { fn show(value: i64) -> str { \"x\" } }\nfn main() -> i64 { 0 }\n",
        "traits-parser-recovery",
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("expected `;`"), "{stderr}");
    assert!(stderr.matches("vacac:").count() >= 1, "{stderr}");
    let _ = fs::remove_dir_all(directory);
}
