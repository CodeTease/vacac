use crate::common::*;

#[test]
fn fixed_arrays_support_literals_indexed_mutation_and_length_in_loops() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let values = [10, 20, 30];
            values[1] = 42;
            for i in 0..len(values) {
                print_i64(values[i]);
            }
            0
        }
        "#,
        "array-basics",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "10\n42\n30\n");
}

#[test]
fn fixed_arrays_support_annotations_and_function_parameters_and_returns() {
    let result = compile_and_run(
        r#"
        fn replace(values: [i64; 3]) -> [i64; 3] {
            values[0] = values[0] + 5;
            values
        }
        fn main() -> i64 {
            let original: [i64; 3] = [2, 4, 6];
            let changed = replace(original);
            print_i64(original[0]);
            print_i64(changed[0]);
            let flags: [bool; 2] = [true, false];
            let words: [str; 2] = ["apex", "array"];
            if flags[0] && words[1] == "array" { 0 } else { 1 }
        }
        "#,
        "array-functions-types",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "2\n7\n");
}

#[test]
fn nested_fixed_arrays_support_literals_mutation_functions_and_struct_fields() {
    let result = compile_and_run(
        r#"
        struct Grid { cells: [[i64; 2]; 2] }

        fn adjust(values: [[i64; 2]; 2]) -> [[i64; 2]; 2] {
            values[0][1] = values[0][1] + 10;
            values
        }

        fn main() -> i64 {
            let rows = [[1, 2], [3, 4]];
            let changed = adjust(rows);
            changed[1][0] = 8;
            for row in changed {
                print_i64(row[1]);
            }
            let grid = Grid { cells: changed };
            print_i64(grid.cells[0][0]);
            print_i64(grid.cells[0][1]);
            print_i64(grid.cells[1][0]);
            print_i64(len(grid.cells));
            0
        }
        "#,
        "nested-fixed-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "12\n4\n1\n12\n8\n2\n"
    );
}

#[test]
fn nested_zero_length_arrays_are_supported_as_elements() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let values: [[i64; 0]; 2] = [[], []];
            print_i64(len(values));
            print_i64(len(values[0]));
            0
        }
        "#,
        "nested-zero-length-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "2\n0\n");
}

#[test]
fn nested_empty_arrays_use_expected_types_in_calls_structs_and_returns() {
    let result = compile_and_run(
        r#"
        struct Grid { cells: [[i64; 0]; 2] }

        fn make(values: [[i64; 0]; 2]) -> [[i64; 0]; 2] {
            values
        }

        fn empty() -> [[i64; 0]; 2] {
            [[], []]
        }

        fn main() -> i64 {
            let grid = Grid { cells: make([[], []]) };
            let copy = empty();
            print_i64(len(grid.cells));
            print_i64(len(copy[0]));
            0
        }
        "#,
        "nested-empty-contexts",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "2\n0\n");
}

#[test]
fn arrays_of_structs_and_enums_support_value_operations_and_iteration() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        enum State { Idle, Number(i64) }

        fn adjust(points: [Point; 2]) -> [Point; 2] {
            points[0].x = points[0].x + 3;
            points[1] = Point { x: 9 };
            points
        }

        fn update(states: [State; 2]) -> [State; 2] {
            states[0] = State::Number(4);
            states
        }

        fn main() -> i64 {
            let points = adjust([Point { x: 2 }, Point { x: 5 }]);
            for point in points {
                print_i64(point.x);
            }

            let states: [State; 2] = update([State::Idle, State::Number(6)]);
            for state in states {
                print_i64(match state {
                    State::Idle => { 0 },
                    State::Number(value) => { value },
                });
            }
            0
        }
        "#,
        "aggregate-arrays-values-iteration",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "5\n9\n4\n6\n");
}

#[test]
fn zero_length_struct_and_enum_arrays_are_supported() {
    let result = compile_and_run(
        r#"
        struct Item { value: i64 }
        enum State { Ready, Waiting }
        fn main() -> i64 {
            let items: [Item; 0] = [];
            let states: [State; 0] = [];
            print_i64(len(items));
            print_i64(len(states));
            for item in items { print_i64(item.value); }
            for state in states { match state { State::Ready => { 0 }, State::Waiting => { 1 } }; }
            0
        }
        "#,
        "zero-length-aggregate-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "0\n0\n");
}

#[test]
fn aggregate_arrays_reject_mixed_unknown_and_unit_elements() {
    for (source, name, diagnostic) in [
        (
            "struct A { value: i64 } struct B { value: i64 } fn main() -> i64 { let values = [A { value: 1 }, B { value: 2 }]; 0 }\n",
            "aggregate-array-mixed-structs",
            "array element has type B, expected A",
        ),
        (
            "enum A { Ready } enum B { Ready } fn main() -> i64 { let values = [A::Ready, B::Ready]; 0 }\n",
            "aggregate-array-mixed-enums",
            "array element has type B, expected A",
        ),
        (
            "fn main() -> i64 { let values: [Missing; 2] = []; 0 }\n",
            "aggregate-array-unknown-type",
            "unknown type `Missing`",
        ),
        (
            "fn main() -> i64 { let values: [unit; 1] = []; 0 }\n",
            "aggregate-array-unit-type",
            "array elements cannot have type `unit`",
        ),
        (
            "struct Item { value: i64 } fn main() -> i64 { const items = [Item { value: 1 }]; items[0].value = 2; 0 }\n",
            "aggregate-array-immutable-field",
            "cannot assign through immutable binding `items`",
        ),
        (
            "enum State { Ready } fn main() -> i64 { const states = [State::Ready]; states[0] = State::Ready; 0 }\n",
            "aggregate-array-immutable-element",
            "cannot assign through immutable binding `states`",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn arrays_can_be_iterated_by_immutable_value_with_break_and_continue() {
    let result = compile_and_run(
        r#"
        fn values() -> [i64; 5] {
            print_i64(99);
            [2, 4, 6, 8, 10]
        }
        fn main() -> i64 {
            let total = 0;
            for value in values() {
                if value == 4 { continue; }
                if value == 10 { break; }
                total = total + value;
            }
            print_i64(total);
            0
        }
        "#,
        "array-iteration-control-flow",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "99\n16\n");
}

#[test]
fn arrays_can_be_iterated_for_all_primitive_element_types_and_empty_arrays() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let flags = [true, false];
            let words = ["Apex", "rocks"];
            for flag in flags { if flag { print("T"); } else { print("F"); } }
            for word in words { print(word); }
            let empty: [i64; 0] = [];
            for number in empty { print_i64(number); }
            0
        }
        "#,
        "array-iteration-types",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "TFApexrocks");
}

#[test]
fn array_iteration_rejects_non_arrays_and_mutation_of_the_element_binding() {
    assert_compile_error(
        "fn main() -> i64 { for value in 3 { print_i64(value); } 0 }",
        "array-iteration-non-array",
        "array iteration requires an array",
    );
    assert_compile_error(
        "fn main() -> i64 { for value in [1, 2] { value = 3; } 0 }",
        "array-iteration-immutable",
        "cannot assign to immutable loop variable `value`",
    );
    assert_compile_error(
        "fn main() -> i64 { for value in [1, 2] { } value }",
        "array-iteration-binding-scope",
        "undefined variable `value`",
    );
}

#[test]
fn zero_length_arrays_support_annotations_parameters_and_length() {
    let result = compile_and_run(
        r#"
        fn size(values: [i64; 0]) -> i64 {
            len(values)
        }
        fn main() -> i64 {
            let values: [i64; 0] = [];
            let flags: [bool; 0] = [];
            print_i64(size(values));
            print_i64(len(flags));
            0
        }
        "#,
        "zero-length-arrays",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "0\n0\n");
}

#[test]
fn indexing_a_zero_length_array_exits_with_status_one() {
    let result = compile_and_run(
        "fn main() -> i64 { let values: [i64; 0] = []; values[0]; 0 }\n",
        "zero-length-array-bounds",
    );
    assert_eq!(result.status.code(), Some(1));
}

#[test]
fn fixed_array_out_of_bounds_and_negative_indexes_exit_with_status_one() {
    for (index, test_name) in [("2", "array-index-high"), ("-1", "array-index-negative")] {
        let source = format!("fn main() -> i64 {{ let a = [4, 5]; a[{index}] }}\n");
        let result = compile_and_run(&source, test_name);
        assert_eq!(result.status.code(), Some(1), "index {index}");
    }
    let source = "fn main() -> i64 { let a = [4, 5]; a[-1] = 9; 0 }\n";
    let result = compile_and_run(source, "array-assignment-negative-index");
    assert_eq!(result.status.code(), Some(1));
}

#[test]
fn arrays_reject_mixed_or_empty_literals_bad_indexes_and_wrong_annotations() {
    for (source, name, diagnostic) in [
        (
            "fn main() -> i64 { let values = [1, true]; 0 }\n",
            "array-mixed-elements",
            "array element has type bool, expected i64",
        ),
        (
            "fn main() -> i64 { let values = []; 0 }\n",
            "array-empty-literal",
            "empty array literals require an array type annotation",
        ),
        (
            "fn main() -> i64 { let values = [1, 2]; values[true]; 0 }\n",
            "array-bool-index",
            "array index has type bool, expected i64",
        ),
        (
            "fn main() -> i64 { let values: [i64; 3] = [1, 2]; 0 }\n",
            "array-annotation-length",
            "initializer for `values` has type [i64; 2], expected [i64; 3]",
        ),
        (
            "fn main() -> i64 { let values = [1, 2]; values[0] = true; 0 }\n",
            "array-assignment-type",
            "array element assignment to `values` has type bool, expected i64",
        ),
        (
            "fn main() -> i64 { let value = 5; value[0]; 0 }\n",
            "index-non-array",
            "indexing requires an array",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn len_requires_exactly_one_array_argument() {
    assert_compile_error(
        "fn main() -> i64 { len(1); 0 }\n",
        "len-scalar",
        "argument 1 to `len` must be an array",
    );
    assert_compile_error(
        "fn main() -> i64 { len(); 0 }\n",
        "len-no-args",
        "function `len` expects 1 argument(s), got 0",
    );
}
