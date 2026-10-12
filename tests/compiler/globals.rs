use crate::common::{assert_compile_error, compile_and_run};

#[test]
fn global_constants_and_mutable_variables_work() {
    let output = compile_and_run(
        r#"
const BASE: i64 = 40 + 2;
const NEXT = BASE + 1;
let counter: i64 = NEXT;

fn main() -> i64 {
    counter = counter + 1;
    counter - 44
}
"#,
        "global-basic",
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn global_mutable_aggregates_can_be_updated_and_local_bindings_shadow_globals() {
    let output = compile_and_run(
        r#"
struct Stats { values: [i64; 2], total: i64 }
let stats = Stats { values: [1, 2], total: 3 };
const total: i64 = 99;

fn main() -> i64 {
    stats.values[0] = 40;
    stats.total = stats.values[0] + stats.values[1];
    let total = stats.total;
    total - 42
}
"#,
        "global-mutable-aggregate",
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn constant_aggregate_and_float_initializers_are_static_values() {
    let output = compile_and_run(
        r#"
struct Point { x: i64, y: f32 }
enum Color { Red, Rgb(i64, i64, i64) }
const ORIGIN = Point { x: 0, y: 0.0f32 };
const COLORS: [Color; 2] = [Color::Red, Color::Rgb(1, 2, 3)];
const PAIR: (i64, bool) = (21 * 2, true);
const GREETING: str = "hello" + " world";

fn main() -> i64 {
    println(GREETING);
    let point = ORIGIN;
    let (pair_num, _) = PAIR;
    match COLORS[1] {
        Color::Rgb(a, b, c) => { point.x + a + b + c + 36 - pair_num },
        Color::Red => { 1 },
    }
}
"#,
        "global-aggregate",
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello world\n");
}

#[test]
fn global_constants_reject_runtime_initializers_and_mutable_dependencies() {
    assert_compile_error(
        "fn value() -> i64 { 1 }\nconst VALUE: i64 = value();\nfn main() -> i64 { VALUE }\n",
        "global-runtime-init",
        "global initializers must be compile-time constant expressions",
    );
    assert_compile_error(
        "let source: i64 = 1;\nconst COPY: i64 = source;\nfn main() -> i64 { COPY }\n",
        "global-mutable-dependency",
        "global initializers may reference only immutable globals",
    );
    assert_compile_error(
        "const FIRST: i64 = SECOND;\nconst SECOND: i64 = FIRST;\nfn main() -> i64 { FIRST }\n",
        "global-cycle",
        "global initializer cycle",
    );
}

#[test]
fn global_mutability_is_enforced_for_scalar_and_aggregate_places() {
    assert_compile_error(
        "const VALUE: i64 = 1;\nfn main() -> i64 { VALUE = 2; 0 }\n",
        "global-immutable-assign",
        "cannot assign to immutable global `VALUE`",
    );
    assert_compile_error(
        "struct Point { x: i64 }\nconst POINT = Point { x: 1 };\nfn main() -> i64 { POINT.x = 2; 0 }\n",
        "global-immutable-place",
        "cannot assign through immutable global `POINT`",
    );
}

#[test]
fn global_types_are_checked_and_inferred() {
    assert_compile_error(
        "const VALUE: bool = 1;\nfn main() -> i64 { 0 }\n",
        "global-type-mismatch",
        "initializer for global `VALUE` has type i64, expected bool",
    );
    assert_compile_error(
        "const VALUE = [];\nfn main() -> i64 { 0 }\n",
        "global-empty-array",
        "empty array literals require an array type annotation",
    );
    assert_compile_error(
        "const VALUE: unit = 1;\nfn main() -> i64 { 0 }\n",
        "global-unit",
        "global initializer type cannot be `unit`",
    );
}

#[test]
fn global_constant_evaluator_handles_minimum_integer_and_rejects_constant_overflow() {
    let output = compile_and_run(
        "const MINIMUM: i64 = -9223372036854775808;\nfn main() -> i64 { MINIMUM + 9223372036854775807 + 1 }\n",
        "global-i64-minimum",
    );
    assert_eq!(output.status.code(), Some(0));
    assert_compile_error(
        "const VALUE: i64 = 9223372036854775807 + 1;\nfn main() -> i64 { 0 }\n",
        "global-overflow",
        "integer overflow in constant expression",
    );
}
