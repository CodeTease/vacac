use crate::common::*;
use std::fs;

#[test]
fn vectors_grow_alias_index_iterate_and_snapshot_length() {
    let result = compile_and_run(
        r#"
        fn append_twice(values: Vec<i64>) -> Vec<i64> {
            vec_push(values, 3);
            vec_push(values, 4);
            values
        }

        fn main() -> i64 {
            let values: Vec<i64> = vec_new();
            let alias = values;
            vec_push(values, 10);
            vec_push(values, 20);
            let returned = append_twice(alias);
            returned[1] = 42;
            for value in returned {
                print_i64(value);
                vec_push(values, 99);
            }
            print_i64(len(values));
            print_i64(values[2]);
            0
        }
        "#,
        "vector-growth-alias-iteration",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "10\n42\n3\n4\n8\n3\n"
    );
}

#[test]
fn vectors_support_strings_structs_and_nested_vectors() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        struct Holder { points: Vec<Point> }

        fn main() -> i64 {
            let words: Vec<str> = vec_new();
            vec_push(words, "Vaca");
            vec_push(words, "Vec");
            print(words[0]);
            print(words[1]);

            let points: Vec<Point> = vec_new();
            vec_push(points, Point { x: 7 });
            let holder = Holder { points: points };
            print_i64(holder.points[0].x);
            holder.points[0] = Point { x: 9 };
            print_i64(holder.points[0].x);

            let nested: Vec<Vec<i64>> = vec_new();
            let child: Vec<i64> = vec_new();
            vec_push(child, 55);
            vec_push(nested, child);
            print_i64(nested[0][0]);
            0
        }
        "#,
        "vector-aggregates-nested",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "VacaVec7\n9\n55\n");
}

#[test]
fn vectors_allow_mutation_through_const_handles_but_not_rebinding() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            const values: Vec<i64> = vec_new();
            vec_push(values, 8);
            values[0] = 11;
            print_i64(values[0]);
            0
        }
        "#,
        "vector-shallow-const",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "11\n");
}

#[test]
fn vector_identity_empty_values_and_fixed_array_elements_work() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let values: Vec<[i64; 2]> = vec_new();
            let empty: Vec<[i64; 0]> = vec_new();
            vec_push(values, [3, 4]);
            vec_push(empty, []);
            let alias = values;
            let distinct: Vec<[i64; 2]> = vec_new();
            print_i64(values[0][1]);
            print_i64(len(empty));
            print_i64(len(empty[0]));
            if values == alias && values != distinct { 0 } else { 1 }
        }
        "#,
        "vector-identity-array-elements",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "4\n1\n0\n");
}

#[test]
fn vectors_support_enum_elements_and_recursive_struct_layouts() {
    let result = compile_and_run(
        r#"
        enum Status { Ready, Done }
        struct Node { children: Vec<Node>, value: i64 }

        fn main() -> i64 {
            let statuses: Vec<Status> = vec_new();
            vec_push(statuses, Status::Ready);
            if statuses[0] == Status::Ready { print_i64(1); }

            let nodes: Vec<Node> = vec_new();
            let children: Vec<Node> = vec_new();
            vec_push(nodes, Node { children: children, value: 17 });
            print_i64(nodes[0].value);
            print_i64(len(nodes[0].children));
            0
        }
        "#,
        "vector-enums-recursive-structs",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n17\n0\n");
}

#[test]
fn vectors_store_and_call_function_values_including_capturing_closures() {
    let result = compile_and_run(
        r#"
        fn increment(value: i64) -> i64 { value + 1 }

        fn main() -> i64 {
            let amount = 2;
            let captured: fn(i64) -> i64 = fn(value: i64) -> i64 { value + amount };
            let functions: Vec<fn(i64) -> i64> = vec_new();
            vec_push(functions, increment);
            vec_push(functions, captured);
            vec_push(functions, captured);
            vec_push(functions, increment);
            vec_push(functions, captured);
            print_i64(if functions[1] == functions[2] { 1 } else { 0 });
            functions[0] = captured;
            for function in functions {
                print_i64(function(40));
            }
            if len(functions) == 5 && functions[0](40) == 42 && functions[4](40) == 42 {
                0
            } else {
                2
            }
        }
        "#,
        "vector-function-values-captures",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "1\n42\n42\n42\n41\n42\n"
    );
}

#[test]
fn vectors_reject_function_elements_with_incompatible_signatures() {
    assert_compile_error(
        "fn main() -> i64 { let functions: Vec<fn(i64) -> i64> = vec_new(); vec_push(functions, fn(value: bool) -> bool { value }); 0 }",
        "vector-function-value-signature-mismatch",
        "vector element has type fn(bool) -> bool, expected fn(i64) -> i64",
    );
}

#[test]
fn vector_element_assignment_survives_growth_during_the_rhs_and_evaluates_indexes_once() {
    let source = r#"
        struct Item { value: i64 }

        let picks = 0;

        fn pick() -> i64 {
            picks = picks + 1;
            0
        }

        fn grow(values: Vec<i64>, result: i64) -> i64 {
            let index = 0;
            while index < 8 {
                vec_push(values, 100 + index);
                index = index + 1;
            }
            result
        }

        fn grow_items(items: Vec<Item>, result: i64) -> i64 {
            let index = 0;
            while index < 8 {
                vec_push(items, Item { value: index });
                index = index + 1;
            }
            result
        }

        fn grow_nested(outer: Vec<Vec<i64>>, inner: Vec<i64>, result: i64) -> i64 {
            let index = 0;
            while index < 8 {
                vec_push(outer, inner);
                vec_push(inner, index);
                index = index + 1;
            }
            result
        }

        fn main() -> i64 {
            let values: Vec<i64> = vec_new();
            vec_push(values, 1);
            vec_push(values, 2);
            values[pick()] = grow(values, 7);
            print_i64(values[0]);
            print_i64(len(values));
            print_i64(picks);

            let items: Vec<Item> = vec_new();
            vec_push(items, Item { value: 1 });
            items[0].value = grow_items(items, 9);
            print_i64(items[0].value);
            print_i64(len(items));

            let outer: Vec<Vec<i64>> = vec_new();
            let inner: Vec<i64> = vec_new();
            vec_push(inner, 1);
            vec_push(outer, inner);
            outer[0][0] = grow_nested(outer, inner, 11);
            print_i64(outer[0][0]);
            print_i64(inner[0]);
            print_i64(len(outer));
            print_i64(len(inner));
            outer[0][0] - 11
        }
        "#;
    let result = compile_and_run(source, "vector-assignment-after-rhs-growth");
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "7\n10\n1\n9\n9\n11\n11\n9\n9\n"
    );

    let (directory, emitted) = run_compiler_with_args(
        source,
        "vector-assignment-after-rhs-growth-ir",
        &["--emit-llvm", "program.ll", "--quiet"],
    );
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let ir = fs::read_to_string(directory.join("program.ll")).unwrap();
    let _ = fs::remove_dir_all(directory);
    let lines: Vec<&str> = ir.lines().collect();
    let call = lines
        .iter()
        .position(|line| line.contains("call i64 @grow("))
        .unwrap_or_else(|| panic!("missing `grow` call:\n{ir}"));
    let result_name = lines[call].split('=').next().unwrap().trim();
    let store = lines
        .iter()
        .position(|line| line.contains(&format!("store i64 {result_name}, ptr ")))
        .unwrap_or_else(|| panic!("missing vector element store:\n{ir}"));
    let address = lines[store]
        .split(", ptr ")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap();
    let address_definition = lines
        .iter()
        .position(|line| {
            line.trim_start()
                .starts_with(&format!("{address} = getelementptr"))
        })
        .unwrap_or_else(|| panic!("missing element address computation:\n{ir}"));
    let data = lines[address_definition]
        .split(", ptr ")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap();
    let data_definition = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&format!("{data} = load ptr")))
        .unwrap_or_else(|| panic!("missing vector data load:\n{ir}"));
    assert!(
        call < data_definition
            && data_definition < address_definition
            && address_definition < store,
        "element store must reload vector data after the rhs call:\n{ir}"
    );
}

#[test]
fn nested_vector_index_evaluation_keeps_the_child_handle_addressable() {
    let result = compile_and_run(
        r#"
        fn grow_outer(outer: Vec<Vec<i64>>) -> i64 {
            let index = 0;
            while index < 8 {
                let extra: Vec<i64> = vec_new();
                vec_push(extra, 50 + index);
                vec_push(outer, extra);
                index = index + 1;
            }
            0
        }

        fn main() -> i64 {
            let outer: Vec<Vec<i64>> = vec_new();
            let inner: Vec<i64> = vec_new();
            vec_push(inner, 1);
            vec_push(outer, inner);
            outer[0][grow_outer(outer)] = 5;
            print_i64(inner[0]);
            print_i64(outer[0][0]);
            print_i64(outer[0][grow_outer(outer)]);
            print_i64(len(outer));
            len(outer) - 17
        }
        "#,
        "vector-nested-index-growth",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "5\n5\n5\n17\n");
}

#[test]
fn vector_indexing_is_bounds_checked() {
    for (index, test_name) in [("0", "vector-index-high"), ("-1", "vector-index-negative")] {
        let source =
            format!("fn main() -> i64 {{ let values: Vec<i64> = vec_new(); values[{index}]; 0 }}");
        let result = compile_and_run(&source, test_name);
        assert_eq!(result.status.code(), Some(1), "index {index}");
    }
    let result = compile_and_run(
        "fn main() -> i64 { let values: Vec<i64> = vec_new(); values[-1] = 2; 0 }",
        "vector-index-assignment-bounds",
    );
    assert_eq!(result.status.code(), Some(1));
}

#[test]
fn invalid_vector_types_and_intrinsics_are_rejected() {
    assert_compile_error(
        "fn main() -> i64 { let values = vec_new(); 0 }",
        "vector-needs-annotation",
        "requires a `Vec<T>` type annotation",
    );
    assert_compile_error(
        "fn main() -> i64 { let values: Vec<unit> = vec_new(); 0 }",
        "vector-unit-element",
        "array elements cannot have type `unit`",
    );
    assert_compile_error(
        "fn main() -> i64 { vec_push(1, 2); 0 }",
        "vector-push-non-vector",
        "argument 1 to `vec_push` must be a vector",
    );
    assert_compile_error(
        "fn main() -> i64 { const values: Vec<i64> = vec_new(); values = vec_new(); 0 }",
        "vector-const-rebind",
        "cannot assign to immutable binding `values`",
    );
}
