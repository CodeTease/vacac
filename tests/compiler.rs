use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn compile_source(source: &str, test_name: &str) -> (std::path::PathBuf, std::process::Output) {
    let dir = std::env::temp_dir().join(format!("apexc-{test_name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("test.apx");
    let executable = dir.join("program");
    fs::write(&source_path, source).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_apexc"))
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    (dir, compile)
}

fn run_compiler_with_args(
    source: &str,
    test_name: &str,
    args: &[&str],
) -> (std::path::PathBuf, std::process::Output) {
    let dir = std::env::temp_dir().join(format!("apexc-{test_name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("test.apx");
    fs::write(&source_path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_apexc"))
        .arg(&source_path)
        .args(args)
        .current_dir(&dir)
        .output()
        .unwrap();
    (dir, output)
}

fn compile_and_run(source: &str, test_name: &str) -> std::process::Output {
    let (dir, compile) = compile_source(source, test_name);
    if !compile.status.success() {
        let error = String::from_utf8_lossy(&compile.stderr).into_owned();
        let _ = fs::remove_dir_all(dir);
        panic!("compiler failed:\n{error}");
    }
    let executable = dir.join("program");
    let run = Command::new(&executable).output().unwrap();
    let _ = fs::remove_dir_all(dir);
    run
}

fn compile_and_run_with_stdin(source: &str, test_name: &str, input: &[u8]) -> std::process::Output {
    let (dir, compile) = compile_source(source, test_name);
    if !compile.status.success() {
        let error = String::from_utf8_lossy(&compile.stderr).into_owned();
        let _ = fs::remove_dir_all(dir);
        panic!("compiler failed:\n{error}");
    }
    let executable = dir.join("program");
    let mut child = Command::new(&executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let run = child.wait_with_output().unwrap();
    let _ = fs::remove_dir_all(dir);
    run
}

fn assert_compile_error(source: &str, test_name: &str, diagnostic: &str) {
    let (dir, compile) = compile_source(source, test_name);
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success(), "compiler unexpectedly succeeded");
    assert!(
        stderr.contains(diagnostic),
        "expected diagnostic {diagnostic:?}, got:\n{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn help_and_version_are_available_without_a_source_file() {
    let help = Command::new(env!("CARGO_BIN_EXE_apexc"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("Usage:"));
    assert!(help.contains("--run"));
    assert!(help.contains("--json"));

    let version = Command::new(env!("CARGO_BIN_EXE_apexc"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("apexc {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn run_mode_runs_program_and_leaves_no_build_artifacts() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { print_i64(23); 37 }\n",
        "run-mode",
        &["--run"],
    );
    assert_eq!(result.status.code(), Some(37));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "23\n");
    assert!(!dir.join("a.out").exists());
    assert!(!dir.join("a.o").exists());
    assert!(!dir.join("program").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn quiet_suppresses_success_status_messages() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "quiet-mode",
        &["--check", "--quiet"],
    );
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn json_diagnostics_include_source_location_and_escaped_message() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 {\n    true\n}\n",
        "json-diagnostic",
        &["--json"],
    );
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.starts_with("{\"severity\":\"error\",\"file\":"),
        "{stderr}"
    );
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    assert!(stderr.contains("\"column\":5"), "{stderr}");
    assert!(stderr.contains("function result has type bool"), "{stderr}");
    assert!(!stderr.contains("^"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_mode_rejects_output_paths() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "run-output-conflict",
        &["--run", "-o", "program"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be used with --run"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn apex_main_result_becomes_process_exit_status() {
    let result = compile_and_run("fn main() -> i64 { 37 }\n", "main-exit-status");
    assert_eq!(result.status.code(), Some(37));
}

#[test]
fn check_mode_runs_frontend_without_creating_build_artifacts() {
    let (dir, result) =
        run_compiler_with_args("fn main() -> i64 { 0 }\n", "check-mode", &["--check"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("checked"));
    assert!(!dir.join("a.out").exists());
    assert!(!dir.join("a.o").exists());
    let _ = fs::remove_dir_all(dir);

    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { true }\n",
        "check-mode-type-error",
        &["--check"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("function result has type bool"));
    assert!(!dir.join("a.out").exists());
    assert!(!dir.join("a.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn emit_llvm_writes_a_verified_llvm_ir_module() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "emit-llvm",
        &["--emit-llvm", "module.ll"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(dir.join("module.ll")).unwrap();
    assert!(ir.contains("define i64 @apex_main()"), "{ir}");
    assert!(ir.contains("target triple"), "{ir}");
    assert!(!dir.join("a.out").exists());
    assert!(!dir.join("a.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn output_modes_reject_conflicting_arguments() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "output-mode-conflict",
        &["--check", "--emit-llvm", "module.ll"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("mutually exclusive"));
    let _ = fs::remove_dir_all(dir);

    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "output-mode-duplicate",
        &["--check", "--check"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("may be specified only once"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_i64_parses_signed_decimal_values_and_i64_minimum() {
    let result = compile_and_run_with_stdin(
        "fn main() -> i64 {\n    let value = read_i64();\n    print_i64(value);\n    0\n}\n",
        "read-i64-values",
        b" \t-9223372036854775808\n",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "-9223372036854775808\n"
    );
}

#[test]
fn read_i64_accepts_plus_sign_and_reads_consecutive_tokens() {
    let result = compile_and_run_with_stdin(
        "fn main() -> i64 {\n    print_i64(read_i64());\n    print_i64(read_i64());\n    print_i64(read_i64());\n    0\n}\n",
        "read-i64-consecutive",
        b" +42\n-7 \t9",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n-7\n9\n");
}

#[test]
fn read_i64_rejects_eof_invalid_tokens_and_overflow() {
    let source = "fn main() -> i64 { read_i64(); 0 }\n";
    for (input, name) in [
        (&b""[..], "read-i64-eof"),
        (&b"+\n"[..], "read-i64-sign-only"),
        (&b"12x\n"[..], "read-i64-invalid-token"),
        (&b"9223372036854775808\n"[..], "read-i64-positive-overflow"),
        (&b"-9223372036854775809\n"[..], "read-i64-negative-overflow"),
    ] {
        let result = compile_and_run_with_stdin(source, name, input);
        assert_eq!(result.status.code(), Some(1), "input: {input:?}");
    }
}

#[test]
fn division_and_remainder_trap_on_zero_and_signed_overflow() {
    for (expression, name) in [
        ("1 / 0", "divide-zero"),
        ("1 % 0", "remainder-zero"),
        ("-9223372036854775808 / -1", "divide-overflow"),
        ("-9223372036854775808 % -1", "remainder-overflow"),
    ] {
        let source = format!("fn main() -> i64 {{ {expression}; 0 }}\n");
        let result = compile_and_run(&source, name);
        assert_eq!(result.status.code(), Some(1), "expression: {expression}");
    }
}

#[test]
fn recursive_factorial_compiles_to_native_executable() {
    let result = compile_and_run(
        r#"
        fn factorial(n: i64) -> i64 {
            if n <= 1 { return 1; }
            return n * factorial(n - 1);
        }
        fn main() -> i64 {
            print_i64(factorial(10));
            0
        }
        "#,
        "factorial",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "3628800\n");
}

#[test]
fn mutation_loops_and_short_circuit_boolean_operators_work() {
    let result = compile_and_run(
        r#"
        fn should_not_run() -> bool {
            print_i64(99);
            true
        }
        fn main() -> i64 {
            let i = 0;
            let total = 0;
            while i < 6 {
                total = total + i;
                i = i + 1;
            }
            let truth: bool = total == 15 && !(total < 0);
            let shorted: bool = false && should_not_run();
            if truth && !shorted { print_i64(total); } else { print_i64(0); }
            0
        }
        "#,
        "loops",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "15\n");
}

#[test]
fn boolean_operators_short_circuit_or_and_evaluate_required_operands() {
    let result = compile_and_run(
        r#"
        fn mark(value: i64) -> bool {
            print_i64(value);
            true
        }
        fn main() -> i64 {
            let skipped = true || mark(99);
            let evaluated_or = false || mark(1);
            let evaluated_and = true && mark(2);
            if skipped && evaluated_or && evaluated_and { return 0; } else { return 1; }
        }
        "#,
        "boolean-short-circuit",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n2\n");
}

#[test]
fn operator_precedence_and_left_associativity_are_preserved() {
    let result = compile_and_run(
        "fn main() -> i64 {\n    if 2 + 3 * 4 == 14 && !(1 > 2) || false { return 0; } else { return 1; }\n}\n",
        "operator-precedence",
    );
    assert_eq!(result.status.code(), Some(0));

    let result = compile_and_run(
        "fn main() -> i64 { if 10 - 3 - 2 == 5 { return 0; } else { return 1; } }\n",
        "operator-associativity",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn if_expressions_produce_values_and_compose_with_other_expressions() {
    let result = compile_and_run(
        r#"
        fn choose(flag: bool, left: i64, right: i64) -> i64 {
            if flag { left } else { right }
        }
        fn main() -> i64 {
            let first = if 3 < 4 { choose(true, 19, 23) } else { 99 };
            let second = if false { 5 } else { if true { 7 } else { 11 } };
            print_i64(first + second);
            0
        }
        "#,
        "if-expression-values",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "26\n");
}

#[test]
fn if_expression_branches_support_scoped_statements() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let selected = if true {
                let local = 41;
                local + 1
            } else {
                let local = 8;
                local
            };
            print_i64(selected);
            0
        }
        "#,
        "if-expression-scopes",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn if_expressions_preserve_bool_and_string_result_types() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let flag = if true { false } else { true };
            let message = if flag { "wrong" } else { "selected" };
            println(message);
            0
        }
        "#,
        "if-expression-bool-string",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "selected\n");
}

#[test]
fn if_expression_requires_matching_non_unit_value_branches() {
    assert_compile_error(
        "fn main() -> i64 { let result = if true { 1 } else { false }; 0 }\n",
        "if-expression-branch-types",
        "`if` expression branches have different types: i64 and bool",
    );
    assert_compile_error(
        "fn main() -> i64 { let result = if true { print_i64(1) } else { print_i64(2) }; 0 }\n",
        "if-expression-unit-branches",
        "`if` expression branches must produce a value",
    );
}

#[test]
fn if_expression_requires_an_else_branch() {
    assert_compile_error(
        "fn main() -> i64 { let result = if true { 1 }; 0 }\n",
        "if-expression-missing-else",
        "expected `else` in an `if` expression",
    );
}

#[test]
fn if_expression_can_be_discarded_as_an_expression_statement() {
    let result = compile_and_run(
        "fn main() -> i64 { if true { print_i64(1); 2 } else { print_i64(3); 4 }; 0 }\n",
        "if-expression-statement",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n");
}

#[test]
fn lexical_scopes_allow_shadowing_without_mutating_outer_bindings() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let value = 10;
            if true {
                let value = 20;
                value = 21;
                print_i64(value);
            }
            print_i64(value);

            let index = 50;
            for index in 0..2 {
                print_i64(index);
            }
            print_i64(index);
            0
        }
        "#,
        "scope-shadowing",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "21\n10\n0\n1\n50\n"
    );
}

#[test]
fn bindings_cannot_be_redeclared_in_one_scope_or_escape_a_for_loop() {
    for (source, test_name, expected) in [
        (
            "fn main() -> i64 { let value = 1; let value = 2; 0 }\n",
            "duplicate-local",
            "`value` is already declared in this scope",
        ),
        (
            "fn main() -> i64 { for index in 0..1 { } index }\n",
            "for-index-out-of-scope",
            "undefined variable `index`",
        ),
        (
            "fn identity(value: i64, value: i64) -> i64 { value }\nfn main() -> i64 { 0 }\n",
            "duplicate-parameter",
            "duplicate parameter `value`",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn while_break_and_continue_change_loop_flow() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let i = 0;
            let total = 0;
            while i < 10 {
                i = i + 1;
                if i == 3 { continue; }
                if i == 7 { break; }
                total = total + i;
            }
            print_i64(total);
            0
        }
        "#,
        "while-break-continue",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "18\n");
}

#[test]
fn for_continue_runs_the_increment_and_break_targets_the_innermost_loop() {
    let result = compile_and_run(
        r#"
        fn main() -> i64 {
            let total = 0;
            for i in 0..5 {
                if i == 1 { continue; }
                for j in 0..3 {
                    if j == 1 { break; }
                    total = total + i;
                }
            }
            print_i64(total);
            0
        }
        "#,
        "for-break-continue",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "9\n");
}

#[test]
fn break_and_continue_are_rejected_outside_loops() {
    for (statement, diagnostic, test_name) in [
        (
            "break;",
            "`break` is only valid inside a loop",
            "break-outside-loop",
        ),
        (
            "continue;",
            "`continue` is only valid inside a loop",
            "continue-outside-loop",
        ),
    ] {
        let source = format!("fn main() -> i64 {{\n    {statement}\n    0\n}}\n");
        let (dir, compile) = compile_source(&source, test_name);
        let stderr = String::from_utf8_lossy(&compile.stderr);
        assert!(!compile.status.success());
        assert!(
            stderr.contains(diagnostic),
            "unexpected diagnostic: {stderr}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn for_ranges_are_half_open_and_evaluate_bounds_once() {
    let result = compile_and_run(
        r#"
        fn bound() -> i64 {
            print_i64(99);
            6
        }
        fn main() -> i64 {
            let total = 0;
            for i in 0..bound() {
                total = total + i;
            }
            print_i64(total);
            0
        }
        "#,
        "for-range",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "99\n15\n");
}

#[test]
fn for_loop_variable_is_immutable() {
    let (dir, compile) = compile_source(
        "fn helper() -> i64 { 0 }\nfn main() -> i64 {\n    for i in 0..3 { i = i + 1; }\n    0\n}\n",
        "for-immutable",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("cannot assign to immutable loop variable `i`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn minimum_i64_literal_compiles_and_runs() {
    let result = compile_and_run(
        "fn main() -> i64 {\n    print_i64(-9223372036854775808);\n    0\n}\n",
        "minimum-i64",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "-9223372036854775808\n"
    );
}

#[test]
fn positive_literal_outside_i64_reports_source_location() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    9223372036854775808\n}\n",
        "positive-i64-overflow",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:5: integer literal is out of range for i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_type_errors_are_reported_before_codegen() {
    let (dir, compile) = compile_source("fn main() -> i64 {\n    true\n}\n", "return-type-error");
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:5: function result has type bool, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(!dir.join("program.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_checker_validates_function_argument_types() {
    let (dir, compile) = compile_source(
        "fn identity(value: i64) -> i64 { value }\nfn main() -> i64 {\n    identity(true)\n}\n",
        "argument-type-error",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:3:14: argument 1 to `identity` has type bool, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn semantic_checker_rejects_functions_with_a_fallthrough_path() {
    let (dir, compile) = compile_source(
        "fn maybe(x: bool) -> i64 {\n    if x { return 1; }\n}\nfn main() -> i64 { 0 }\n",
        "missing-return-path",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr
            .contains("test.apx:1:1: function `maybe` can reach its end without returning a value"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn entry_point_signature_is_required_and_must_be_exact() {
    for (source, test_name, expected) in [
        (
            "fn helper() -> i64 { 0 }\n",
            "missing-main",
            "program must define the required entry point `fn main() -> i64`",
        ),
        (
            "fn main(value: i64) -> i64 { value }\n",
            "main-with-parameter",
            "entry point must have signature `fn main() -> i64`",
        ),
        (
            "fn main() -> bool { true }\n",
            "main-wrong-return-type",
            "entry point must have signature `fn main() -> i64`",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn function_signatures_support_forward_calls_and_validate_call_arguments() {
    let result = compile_and_run(
        "fn main() -> i64 { later(41) }\nfn later(value: i64) -> i64 { value + 1 }\n",
        "forward-call",
    );
    assert_eq!(result.status.code(), Some(42));

    for (source, test_name, expected) in [
        (
            "fn main() -> i64 { missing() }\n",
            "unknown-function",
            "unknown function `missing`",
        ),
        (
            "fn add(a: i64, b: i64) -> i64 { a + b }\nfn main() -> i64 { add(1) }\n",
            "too-few-arguments",
            "function `add` expects 2 argument(s), got 1",
        ),
        (
            "fn add(a: i64) -> i64 { a }\nfn main() -> i64 { add(1, 2) }\n",
            "too-many-arguments",
            "function `add` expects 1 argument(s), got 2",
        ),
        (
            "fn main() -> i64 { print_i64(); 0 }\n",
            "builtin-wrong-argument-count",
            "function `print_i64` expects 1 argument(s), got 0",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn return_path_analysis_accepts_two_returning_branches() {
    let result = compile_and_run(
        r#"
        fn choose(flag: bool) -> i64 {
            if flag { return 11; } else { return 22; }
        }
        fn main() -> i64 {
            print_i64(choose(true));
            print_i64(choose(false));
            0
        }
        "#,
        "both-branches-return",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "11\n22\n");
}

#[test]
fn return_statements_must_match_the_declared_return_type() {
    for (source, test_name, expected) in [
        (
            "fn value() -> i64 { return; }\nfn main() -> i64 { 0 }\n",
            "bare-return-from-value-function",
            "`return;` is only valid in a function returning `unit`",
        ),
        (
            "fn value() -> i64 { return true; }\nfn main() -> i64 { 0 }\n",
            "wrong-explicit-return-type",
            "return value has type bool, expected i64",
        ),
        (
            "fn report() -> unit { 1 }\nfn main() -> i64 { 0 }\n",
            "value-from-unit-function",
            "function result has type i64, expected unit",
        ),
    ] {
        assert_compile_error(source, test_name, expected);
    }
}

#[test]
fn parser_diagnostics_highlight_the_offending_source_token() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let x = ;\n    0\n}\n",
        "parser-location",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:13: expected expression, found `;`"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(
        stderr.contains(" 2 |     let x = ;"),
        "missing source line: {stderr}"
    );
    assert!(
        stderr.contains("   |             ^"),
        "missing caret: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unit_functions_can_be_called_and_return_implicitly_or_explicitly() {
    let result = compile_and_run(
        r#"
        fn report(value: i64) -> unit {
            print_i64(value);
        }
        fn maybe_report(should_report: bool) -> unit {
            if should_report {
                report(7);
                return;
            }
            report(8);
        }
        fn main() -> i64 {
            report(42);
            maybe_report(true);
            0
        }
        "#,
        "unit-functions",
    );
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n7\n");
}

#[test]
fn unit_function_results_cannot_be_used_as_values() {
    let (dir, compile) = compile_source(
        "fn report() -> unit { print_i64(1); }\nfn main() -> i64 { let result = report(); 0 }\n",
        "unit-as-value",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("cannot bind a value of type `unit`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unit_parameters_are_rejected() {
    let (dir, compile) = compile_source(
        "fn invalid(value: unit) -> i64 { 0 }\nfn main() -> i64 { 0 }\n",
        "unit-parameter",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("parameters cannot have type `unit`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn strings_can_be_bound_passed_returned_and_printed() {
    let result = compile_and_run(
        r#"
        fn greeting(formal: bool) -> str {
            if formal { return "Good day"; }
            "Hi"
        }
        fn shout(message: str) -> unit {
            print(message);
            println("!");
        }
        fn main() -> i64 {
            let name: str = "Apex";
            print(greeting(true));
            print(", ");
            println(name);
            let label = "first";
            label = "second";
            println(label);
            print("answer = ");
            print_i64(42);
            shout(greeting(false));
            println("");
            println("100% done");
            0
        }
        "#,
        "strings",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "Good day, Apex\nsecond\nanswer = 42\nHi!\n\n100% done\n"
    );
}

#[test]
fn string_escapes_and_utf8_are_preserved() {
    let result = compile_and_run(
        "fn main() -> i64 {\n    print(\"a\\tb\\n\\\"q\\\" \\\\ \\r\");\n    println(\"héllo ✓\");\n    0\n}\n",
        "string-escapes",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "a\tb\n\"q\" \\ \rhéllo ✓\n"
    );
}

#[test]
fn string_equality_compares_contents() {
    let result = compile_and_run(
        r#"
        fn pick(first: bool) -> str {
            if first { return "same"; }
            "other"
        }
        fn main() -> i64 {
            let a = pick(true);
            if a == "same" { println("equal"); }
            if a != pick(false) { println("different"); }
            if !(a == "sam") { println("prefix differs"); }
            0
        }
        "#,
        "string-equality",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "equal\ndifferent\nprefix differs\n"
    );
}

#[test]
fn unterminated_string_literal_reports_source_location() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let s = \"abc;\n    0\n}\n",
        "unterminated-string",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:13: unterminated string literal"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn invalid_string_escapes_are_rejected() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    println(\"bad \\q\");\n    0\n}\n",
        "bad-escape",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:18: unknown escape sequence `\\q`"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);

    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    println(\"a\\0b\");\n    0\n}\n",
        "nul-escape",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("NUL characters are not supported in string literals"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn strings_do_not_support_arithmetic_or_ordering() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let s = \"a\" + \"b\";\n    0\n}\n",
        "string-add",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:17: arithmetic operands has type str, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);

    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    print_i64(\"x\");\n    0\n}\n",
        "string-to-print-i64",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:15: argument 1 to `print_i64` has type str, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn string_builtin_names_are_reserved() {
    let (dir, compile) = compile_source(
        "fn println(s: str) -> unit { }\nfn main() -> i64 { 0 }\n",
        "reserved-println",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("`println` is a reserved compiler/runtime name"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

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
fn structs_support_construction_field_reads_and_value_passing() {
    let result = compile_and_run(
        r#"
        struct User {
            id: i64,
            active: bool,
            name: str,
        }
        fn rename(user: User) -> User {
            User { id: user.id, active: user.active, name: "Apex" }
        }
        fn main() -> i64 {
            let user = User { name: "Ada", active: true, id: 42 };
            let renamed = rename(user);
            print_i64(renamed.id);
            println(renamed.name);
            if renamed.active { 0 } else { 1 }
        }
        "#,
        "structs-value-passing",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\nApex\n");
}

#[test]
fn structs_support_return_values_and_if_expression_joins() {
    let result = compile_and_run(
        r#"
        struct Pair { left: i64, right: i64 }
        fn make_pair(value: i64) -> Pair {
            Pair { left: value, right: value + 1 }
        }
        fn main() -> i64 {
            let chosen = if true { make_pair(20) } else { make_pair(40) };
            chosen.left + chosen.right - 41
        }
        "#,
        "structs-return-if",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn structs_support_mutable_field_assignment() {
    let result = compile_and_run(
        r#"
        struct User { id: i64, active: bool, name: str }
        fn main() -> i64 {
            let user = User { id: 41, active: true, name: "Ada" };
            user.id = user.id + 1;
            user.active = false;
            user.name = "Grace";
            print_i64(user.id);
            println(user.name);
            if user.active { 1 } else { 0 }
        }
        "#,
        "struct-field-assignment",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\nGrace\n");
}

#[test]
fn structs_support_nested_struct_and_fixed_array_fields() {
    let result = compile_and_run(
        r#"
        struct Envelope { payload: Payload, samples: [i64; 3] }
        struct Payload { count: i64, enabled: bool, flags: [bool; 2] }

        fn update(envelope: Envelope) -> Envelope {
            envelope.payload.count = envelope.payload.count + 1;
            envelope.payload.flags[1] = false;
            envelope.samples[2] = envelope.samples[2] + 10;
            envelope
        }

        fn main() -> i64 {
            let original = Envelope {
                samples: [3, 5, 7],
                payload: Payload { count: 40, enabled: true, flags: [true, true] },
            };
            let updated = update(original);
            print_i64(updated.payload.count);
            print_i64(updated.samples[2]);
            if updated.payload.enabled && updated.payload.flags[0] && !updated.payload.flags[1] { 0 } else { 1 }
        }
        "#,
        "nested-struct-array-fields",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "41\n17\n");
}

#[test]
fn nested_aggregate_fields_respect_immutable_bindings() {
    for (source, diagnostic, name) in [
        (
            "struct Inner { value: i64 } struct Outer { inner: Inner } fn main() -> i64 { const outer = Outer { inner: Inner { value: 1 } }; outer.inner.value = 2; 0 }",
            "cannot assign through immutable binding `outer`",
            "const-nested-struct-field-assignment",
        ),
        (
            "struct Outer { values: [i64; 2] } fn main() -> i64 { const outer = Outer { values: [1, 2] }; outer.values[0] = 3; 0 }",
            "cannot assign through immutable binding `outer`",
            "const-struct-array-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn const_bindings_are_readable_but_cannot_be_mutated() {
    let result = compile_and_run(
        r#"
        struct Point { x: i64 }
        fn main() -> i64 {
            const point = Point { x: 42 };
            const answer: i64 = point.x;
            print_i64(answer);
            answer - 42
        }
        "#,
        "const-binding",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");

    for (source, diagnostic, name) in [
        (
            "fn main() -> i64 { const value = 1; value = 2; 0 }",
            "cannot assign to immutable binding `value`",
            "const-reassignment",
        ),
        (
            "fn main() -> i64 { const values = [1, 2]; values[0] = 3; 0 }",
            "cannot assign through immutable binding `values`",
            "const-array-assignment",
        ),
        (
            "struct Point { x: i64 } fn main() -> i64 { const point = Point { x: 1 }; point.x = 2; 0 }",
            "cannot assign through immutable binding `point`",
            "const-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn struct_field_assignment_checks_target_and_value_types() {
    for (source, diagnostic, name) in [
        (
            "struct Point { x: i64 } fn main() -> i64 { let point = Point { x: 1 }; point.x = true; 0 }",
            "assignment to field `x` has type bool, expected i64",
            "struct-field-assignment-type",
        ),
        (
            "struct Point { x: i64 } fn main() -> i64 { let point = Point { x: 1 }; point.y = 2; 0 }",
            "struct `Point` has no field `y`",
            "struct-field-assignment-unknown",
        ),
        (
            "fn main() -> i64 { let value = 1; value.x = 2; 0 }",
            "`value` is not a struct",
            "non-struct-field-assignment",
        ),
    ] {
        assert_compile_error(source, name, diagnostic);
    }
}

#[test]
fn structs_reject_duplicate_missing_unknown_and_wrongly_typed_fields() {
    for (source, expected, name) in [
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let p = Pair { left: 1, left: 2 }; 0 }",
            "struct field `left` is initialized more than once",
            "struct-duplicate-field-init",
        ),
        (
            "struct Pair { left: i64, right: i64 } fn main() -> i64 { let p = Pair { left: 1 }; 0 }",
            "struct `Pair` is missing field(s): right",
            "struct-missing-field",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { other: 1 }; 0 }",
            "struct `Pair` has no field `other`",
            "struct-unknown-field",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { left: true }; 0 }",
            "initializer for field `left` has type bool, expected i64",
            "struct-field-type",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { left: 1 }; p.missing }",
            "struct `Pair` has no field `missing`",
            "struct-unknown-member",
        ),
        (
            "struct Pair { left: i64 } fn main() -> i64 { let p = Pair { left: 1 }; let q = p; let bad = p == q; 0 }",
            "struct values cannot be compared",
            "struct-comparison",
        ),
    ] {
        assert_compile_error(source, name, expected);
    }
}

#[test]
fn structs_reject_duplicate_declarations_and_unsupported_fields() {
    assert_compile_error(
        "struct Item { id: i64 } struct Item { name: str } fn main() -> i64 { 0 }",
        "struct-duplicate-declaration",
        "type `Item` is defined more than once",
    );
    assert_compile_error(
        "enum Choice { First, Second } struct Item { choice: Choice } fn main() -> i64 { 0 }",
        "struct-enum-field",
        "struct fields cannot have enum type `Choice`",
    );
    assert_compile_error(
        "struct Item { value: unit } fn main() -> i64 { 0 }",
        "struct-unit-field",
        "struct fields cannot have type `unit`",
    );
    assert_compile_error(
        "struct Item { value: Missing } fn main() -> i64 { 0 }",
        "struct-unknown-field-type",
        "unknown type `Missing`",
    );
    assert_compile_error(
        "struct Node { next: Node } fn main() -> i64 { 0 }",
        "struct-recursive-field",
        "recursive by-value field `next`",
    );
    assert_compile_error(
        "struct First { second: Second } struct Second { first: First } fn main() -> i64 { 0 }",
        "struct-mutually-recursive-fields",
        "recursive by-value field `first`",
    );
    assert_compile_error(
        "struct Item { id: i64, id: bool } fn main() -> i64 { 0 }",
        "struct-duplicate-field-declaration",
        "struct field `id` is defined more than once",
    );
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
            "fn main() -> i64 { let values = [1, 2]; if values == values { 0 } else { 1 } }\n",
            "array-comparison",
            "array values cannot be compared",
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
        "enum Choice { Value([i64; 2]) } fn main() -> i64 { 0 }\n",
        "enum-array-payload",
        "enum payloads must have type `i64`, `bool`, or `str`",
    );
    assert_compile_error(
        "enum Choice { A } enum Choice { B } fn main() -> i64 { 0 }\n",
        "enum-duplicate-type",
        "enum `Choice` is defined more than once",
    );
}
