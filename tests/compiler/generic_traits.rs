use super::common::*;
use std::fs;

#[test]
fn generic_traits_dispatch_on_trait_arguments() {
    let result = compile_and_run(
        r#"
trait Choose<T> {
    fn pick(value: Self, input: T) -> T;
}
impl<A> Choose<A> for i64 {
    fn pick(value: i64, input: A) -> A { input }
}
fn main() -> i64 {
    let first = <i64 as Choose<i64>>::pick(0, 20);
    let second = <i64 as Choose<bool>>::pick(0, true);
    if second { first - 20 } else { 1 }
}
"#,
        "generic-traits-dispatch",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_impl_parameters_flow_through_trait_arguments() {
    let result = compile_and_run(
        r#"
trait Pair<L, R> {
    fn left(value: Self) -> L;
    fn right(value: Self) -> R;
}
struct Both<A, B> { first: A, second: B }
impl<A, B> Pair<B, A> for Both<A, B> {
    fn left(value: Both<A, B>) -> B { value.second }
    fn right(value: Both<A, B>) -> A { value.first }
}
fn main() -> i64 {
    let both = Both<i64, bool> { first: 40, second: true };
    <Both<i64, bool> as Pair<bool, i64>>::right(both) - 40
}
"#,
        "generic-traits-swapped-arguments",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn default_bodies_specialize_trait_and_function_arguments() {
    let result = compile_and_run(
        r#"
trait Select<T> {
    fn select<U>(value: Self, input: T, fallback: U) -> U;
    fn relay<U>(value: Self, input: T, fallback: U) -> U {
        <Self as Select<T>>::select::<U>(value, input, fallback)
    }
}
impl<A> Select<A> for i64 {
    fn select<B>(value: i64, input: A, fallback: B) -> B {
        if true { fallback } else { fallback }
    }
}
fn main() -> i64 {
    let inherited = <i64 as Select<bool>>::relay::<i64>(0, true, 42);
    inherited - 42
}
"#,
        "generic-traits-default-bodies",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn inherited_defaults_rename_colliding_function_parameters() {
    let result = compile_and_run(
        r#"
trait Mapper<T> {
    fn map<U>(value: Self, input: T, fallback: U) -> U { fallback }
}
impl<U> Mapper<U> for i64 {}
fn main() -> i64 {
    <i64 as Mapper<bool>>::map::<i64>(0, true, 42) - 42
}
"#,
        "generic-traits-default-renaming",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn unqualified_calls_infer_self_trait_and_function_arguments() {
    let result = compile_and_run(
        r#"
trait Select<T> {
    fn select<U>(value: Self, input: T, fallback: U) -> U;
}
impl<A> Select<A> for i64 {
    fn select<B>(value: i64, input: A, fallback: B) -> B { fallback }
}
fn main() -> i64 {
    let inferred = Select::select(0, true, 42);
    let function_explicit = Select::select::<i64>(0, true, 42);
    let trait_explicit = <i64 as Select<bool>>::select(0, true, 42);
    inferred + function_explicit + trait_explicit - 126
}
"#,
        "generic-traits-inference",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn inferred_trait_arguments_provide_contextual_argument_types() {
    let result = compile_and_run(
        r#"
trait Take<T> {
    fn take(value: Self, input: T) -> i64;
}
impl Take<Vec<i64>> for i64 {
    fn take(value: i64, input: Vec<i64>) -> i64 { len(input) }
}
trait Tag<T> {
    fn tag(value: Self) -> i64;
}
struct Box<T> { value: T }
impl<T> Tag<T> for Box<T> {
    fn tag(value: Box<T>) -> i64 { value.value }
}
fn main() -> i64 {
    let empty: Vec<i64> = vec_new();
    let taken = <i64 as Take<Vec<i64>>>::take(0, empty);
    let tagged = <Box<i64> as Tag<i64>>::tag(Box<i64> { value: 42 });
    taken + tagged - 42
}
"#,
        "generic-traits-contextual-arguments",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn generic_trait_bounds_are_checked_at_concrete_uses() {
    let result = compile_and_run(
        r#"
trait Copyable {
    fn copy(value: Self) -> Self;
}
trait Container<T: Copyable> {
    fn get(value: Self) -> T;
    fn twice(value: Self) -> T {
        Copyable::copy(<Self as Container<T>>::get(value))
    }
}
impl Copyable for i64 {
    fn copy(value: i64) -> i64 { value }
}
struct Box<T> { value: T }
impl<T: Copyable> Container<T> for Box<T> {
    fn get(value: Box<T>) -> T { value.value }
}
fn main() -> i64 {
    let first = <Box<i64> as Container<i64>>::get(Box<i64> { value: 20 });
    let second = <Box<i64> as Container<i64>>::twice(Box<i64> { value: 22 });
    first + second - 42
}
"#,
        "generic-traits-bounds",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        r#"
trait Copyable { fn copy(value: Self) -> Self; }
trait Container<T: Copyable> { fn get(value: Self) -> T; }
struct Box<T> { value: T }
impl<T> Container<T> for Box<T> { fn get(value: Box<T>) -> T { value.value } }
fn main() -> i64 { <Box<bool> as Container<bool>>::get(Box<bool> { value: true }); 0 }
"#,
        "generic-traits-trait-parameter-bound",
        "type `bool` does not implement trait `Copyable`",
    );
}

#[test]
fn generic_trait_coherence_uses_the_full_impl_head() {
    let result = compile_and_run(
        r#"
trait Tag<T> { fn tag(value: Self) -> i64; }
impl Tag<bool> for i64 { fn tag(value: i64) -> i64 { 20 } }
impl Tag<i64> for i64 { fn tag(value: i64) -> i64 { 22 } }
fn main() -> i64 {
    <i64 as Tag<bool>>::tag(0) + <i64 as Tag<i64>>::tag(0) - 42
}
"#,
        "generic-traits-coherence-distinct-arguments",
    );
    assert_eq!(result.status.code(), Some(0));

    assert_compile_error(
        r#"
struct Box<T> { value: T }
trait Tag<T> { fn tag(value: Self) -> i64; }
impl<T> Tag<T> for Box<T> { fn tag(value: Box<T>) -> i64 { 0 } }
impl Tag<i64> for Box<i64> { fn tag(value: Box<i64>) -> i64 { 1 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-coherence-overlap",
        "conflicting implementations of trait `Tag`",
    );

    assert_compile_error(
        r#"
trait Tag<T> { fn tag(value: Self) -> i64; }
impl<T> Tag<T> for i64 { fn tag(value: i64) -> i64 { 0 } }
impl Tag<bool> for i64 { fn tag(value: i64) -> i64 { 1 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-coherence-argument-overlap",
        "conflicting implementations of trait `Tag`",
    );
}

#[test]
fn generic_trait_signatures_and_bounds_must_match() {
    assert_compile_error(
        r#"
trait Map<T> { fn map<U>(value: Self, fallback: U) -> U; }
impl<T> Map<T> for i64 { fn map(value: i64) -> i64 { 0 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-method-arity",
        "generic parameter(s), but the trait declares 1",
    );
    assert_compile_error(
        r#"
trait Convert<T> { fn convert(value: Self) -> i64; }
trait Map<T> { fn map<U: Convert<T>>(value: Self) -> i64; }
impl<T> Map<T> for i64 { fn map<U>(value: i64) -> i64 { 0 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-method-bounds",
        "bounds on `Map::map` do not match the trait declaration",
    );
    assert_compile_error(
        r#"
trait Map<T> { fn map(value: Self, input: T) -> i64; }
impl Map<i64> for i64 { fn map(value: i64, input: bool) -> i64 { 0 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-method-types",
        "has type `bool`, expected `i64`",
    );
}

#[test]
fn generic_trait_arguments_require_and_report_their_arity() {
    assert_compile_error(
        "trait Tag<T> { fn tag(value: Self) -> i64; } impl Tag for i64 { fn tag(value: i64) -> i64 { 0 } } fn main() -> i64 { 0 }",
        "generic-traits-impl-arity",
        "trait `Tag` expects 1 type argument(s), got 0",
    );
    assert_compile_error(
        "trait Tag<T> { fn tag(value: Self) -> i64; } fn require<T: Tag>(value: T) -> i64 { 0 } fn main() -> i64 { 0 }",
        "generic-traits-bound-arity",
        "trait `Tag` expects 1 type argument(s), got 0",
    );
    assert_compile_error(
        "trait Pick { fn pick<U>(value: Self, fallback: U) -> U; } impl Pick for i64 { fn pick<U>(value: i64, fallback: U) -> U { fallback } } fn main() -> i64 { <i64 as Pick>::pick::<i64, bool>(0, 42) }",
        "generic-traits-function-arity",
        "trait function `Pick::pick` expects 1 type argument(s), got 2",
    );
}

#[test]
fn generic_trait_inference_reports_missing_and_conflicting_arguments() {
    assert_compile_error(
        "trait Zero<T> { fn zero() -> Self; } impl Zero<i64> for i64 { fn zero() -> i64 { 0 } } fn main() -> i64 { Zero::zero(); 0 }",
        "generic-traits-infer-self",
        "cannot infer `Self` for `Zero::zero`",
    );
    assert_compile_error(
        "trait Tag<T> { fn tag(value: Self) -> i64; } impl Tag<i64> for i64 { fn tag(value: i64) -> i64 { 0 } } fn main() -> i64 { Tag::tag(0); 0 }",
        "generic-traits-infer-trait-argument",
        "cannot infer trait type argument `T` for `Tag::tag`",
    );
    assert_compile_error(
        "trait Tag<T> { fn tag(value: Self, first: T, second: T) -> i64; } impl Tag<i64> for i64 { fn tag(value: i64, first: i64, second: i64) -> i64 { 0 } } fn main() -> i64 { Tag::tag(0, 1, true) }",
        "generic-traits-infer-conflict",
        "generic type parameter `T` is inferred as both `i64` and `bool`",
    );
    assert_compile_error(
        "trait Pick { fn pick<U>(value: Self) -> U; } impl Pick for i64 { fn pick<U>(value: i64) -> U { loop {} } } fn main() -> i64 { <i64 as Pick>::pick(0); 0 }",
        "generic-traits-infer-function-argument",
        "cannot infer generic type parameter `U` for `Pick::pick`",
    );
}

#[test]
fn parameterized_bounds_support_forward_references_and_self() {
    let result = compile_and_run(
        r#"
trait Convert<T> { fn convert(value: Self) -> T; }
struct Source { value: i64 }
impl Convert<i64> for Source { fn convert(value: Source) -> i64 { value.value } }
fn forward<T, U: Convert<T>>(value: U) -> i64 {
    let converted: T = <U as Convert<T>>::convert(value);
    42
}

trait Wrapper<T> { fn value(value: Self) -> i64; }
struct Box<T> { value: T }
impl Wrapper<i64> for Box<i64> { fn value(value: Box<i64>) -> i64 { value.value } }
trait Run {
    fn run<U: Wrapper<Self>>(value: Self, input: U) -> i64;
}
impl Run for i64 {
    fn run<U: Wrapper<i64>>(value: i64, input: U) -> i64 { <U as Wrapper<i64>>::value(input) }
}
fn main() -> i64 {
    forward::<i64, Source>(Source { value: 42 }) - 42
        + <i64 as Run>::run::<Box<i64>>(0, Box<i64> { value: 42 }) - 42
}
"#,
        "generic-traits-forward-bounds",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn unconstrained_impl_parameters_and_positional_bound_mismatches_are_rejected() {
    assert_compile_error(
        "trait Choose<T> { fn choose(value: Self) -> i64; } impl<A, B> Choose<A> for i64 { fn choose(value: i64) -> i64 { 0 } } fn main() -> i64 { 0 }",
        "generic-traits-unconstrained-impl-parameter",
        "impl type parameter `B` is not constrained by the impl target or trait arguments",
    );
    assert_compile_error(
        r#"
trait Left { fn left(value: Self) -> i64; }
trait Right { fn right(value: Self) -> i64; }
impl Left for i64 { fn left(value: i64) -> i64 { 0 } }
impl Right for i64 { fn right(value: i64) -> i64 { 0 } }
trait Pair { fn pair<U: Left, V: Right>(value: Self) -> i64; }
impl Pair for i64 { fn pair<U: Right, V: Left>(value: i64) -> i64 { 0 } }
fn main() -> i64 { 0 }
"#,
        "generic-traits-positional-bounds",
        "bounds on `Pair::pair` do not match the trait declaration",
    );
}

#[test]
fn duplicate_trait_arguments_in_bounds_are_distinguished() {
    let result = compile_and_run(
        r#"
trait Convert<T> { fn convert(value: Self) -> i64; }
impl Convert<bool> for i64 { fn convert(value: i64) -> i64 { 0 } }
impl Convert<i64> for i64 { fn convert(value: i64) -> i64 { 0 } }
fn both<T: Convert<bool> + Convert<i64>>(value: T) -> i64 { 0 }
fn main() -> i64 { both(0) }
"#,
        "generic-traits-bound-arguments",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_compile_error(
        "trait Convert<T> { fn convert(value: Self) -> i64; } fn both<T: Convert<i64> + Convert<i64>>(value: T) -> i64 { 0 } fn main() -> i64 { 0 }",
        "generic-traits-duplicate-bound-arguments",
        "trait `Convert` is listed more than once",
    );
}

#[test]
fn generic_trait_argument_shorthand_is_rejected() {
    assert_compile_error(
        "trait Tag<T> { fn tag(value: Self) -> i64; } impl Tag<i64> for i64 { fn tag(value: i64) -> i64 { 0 } } fn main() -> i64 { Tag::<i64>::tag(0) }",
        "generic-traits-shorthand",
        "is not supported; write `<Self as Tag<...>>::tag(...)`",
    );
}

#[test]
fn public_bounds_reject_private_generic_traits() {
    assert_compile_error(
        "trait Hidden<T> {} pub fn require<T: Hidden<i64>>(value: T) -> T { value } fn main() -> i64 { 0 }",
        "generic-traits-private-bound",
        "private trait `Hidden` cannot appear in a public trait bound",
    );
}

#[test]
fn generic_trait_dispatch_is_erased_before_llvm_emission() {
    let (directory, result) = run_compiler_with_args(
        "trait Tag<T> { fn tag(value: Self) -> i64; } impl Tag<bool> for i64 { fn tag(value: i64) -> i64 { 1 } } impl Tag<i64> for i64 { fn tag(value: i64) -> i64 { 2 } } fn main() -> i64 { <i64 as Tag<bool>>::tag(0) + <i64 as Tag<i64>>::tag(0) - 3 }",
        "generic-traits-llvm",
        &["--emit-llvm", "program.ll", "--quiet"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(directory.join("program.ll")).unwrap();
    assert!(ir.contains("Tag<bool> for i64::tag"), "{ir}");
    assert!(ir.contains("Tag<i64> for i64::tag"), "{ir}");
    assert!(!ir.contains("vtable"), "{ir}");
    assert!(!ir.contains("trait.dispatch"), "{ir}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn public_generic_traits_work_across_modules() {
    let directory = std::env::temp_dir().join(format!(
        "vacac-generic-traits-modules-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("main.vca"),
        "mod lib;\nfn main() -> i64 { <lib::Box<i64> as lib::Tag<i64>>::tag(lib::Box<i64> { value: 42 }) - 42 }\n",
    )
    .unwrap();
    fs::write(
        directory.join("lib.vca"),
        "pub trait Tag<T> { fn tag(value: Self) -> T; }\npub struct Box<T> { value: T }\nimpl<T> Tag<T> for Box<T> { fn tag(value: Box<T>) -> T { value.value } }\n",
    )
    .unwrap();
    let compile = std::process::Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(directory.join("main.vca"))
        .arg("-o")
        .arg(directory.join("program"))
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = std::process::Command::new(directory.join("program"))
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn generic_trait_diagnostics_keep_module_locations() {
    let directory = std::env::temp_dir().join(format!(
        "vacac-generic-traits-diagnostics-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("main.vca"),
        "mod common;\nfn main() -> i64 { 0 }\n",
    )
    .unwrap();
    fs::write(
        directory.join("common.vca"),
        "pub trait Tag<T> { fn tag(value: Self) -> T; }\nimpl Tag<i64> for i64 { fn tag(value: i64) -> bool { true } }\n",
    )
    .unwrap();
    let compile = std::process::Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(directory.join("main.vca"))
        .arg("--check")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("\"file\":"), "{stderr}");
    assert!(stderr.contains("common.vca"), "{stderr}");
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    assert!(
        stderr.contains("returns `bool`, expected `i64`"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn malformed_generic_traits_recover_to_later_declarations() {
    let (directory, compile) = compile_source(
        "trait Broken<T { fn broken(value: Self) -> T; }\nfn main() -> i64 { 0 }\n",
        "generic-traits-parser-recovery",
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        stderr.contains("expected `>` after generic type argument"),
        "{stderr}"
    );
    assert_eq!(stderr.matches("vacac:").count(), 1, "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn generic_traits_share_the_specialization_limit() {
    fn source(count: usize) -> String {
        let mut source = String::from(
            "trait Mark { fn mark<U>(value: Self) -> i64; }\nimpl Mark for i64 { fn mark<U>(value: i64) -> i64 { 0 } }\nfn main() -> i64 {\n",
        );
        for index in 0..count {
            source.push_str(&format!("    <i64 as Mark>::mark::<[i64; {index}]>(0);\n"));
        }
        source.push_str("    0\n}\n");
        source
    }

    let result = compile_and_run(&source(512), "generic-traits-limit-ok");
    assert_eq!(result.status.code(), Some(0));
    assert_compile_error(
        &source(513),
        "generic-traits-limit-rejected",
        "generic specialization exceeded the limit of 512",
    );
}
