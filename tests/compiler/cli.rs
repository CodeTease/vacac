use crate::common::*;
use std::fs;
use std::process::Command;

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
        r#"json: "diagnostic\path"#,
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
    assert!(stderr.contains("\"end_column\":9"), "{stderr}");
    assert!(stderr.contains("function result has type bool"), "{stderr}");
    let source_path = dir.join("test.apx").to_string_lossy().into_owned();
    let escaped_path = source_path.replace('\\', "\\\\").replace('"', "\\\"");
    assert!(
        stderr.contains(&format!("\"file\":\"{escaped_path}\"")),
        "{stderr}"
    );
    assert!(!stderr.contains("^"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn json_diagnostics_include_lexer_spans() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 {\n    println(\"bad \\q\");\n    0\n}\n",
        "json-lexer-span",
        &["--json"],
    );
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    assert!(stderr.contains("\"column\":18"), "{stderr}");
    assert!(stderr.contains("\"end_column\":20"), "{stderr}");
    assert!(stderr.contains("unknown escape sequence"), "{stderr}");
    assert!(
        !stderr.contains("\\n"),
        "unexpected multiline JSON: {stderr}"
    );
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
