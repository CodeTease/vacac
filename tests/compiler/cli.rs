use crate::common::*;
use std::fs;
use std::process::Command;

#[test]
fn help_and_version_are_available_without_a_source_file() {
    let help = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("Usage:"));
    assert!(help.contains("vacac <source.vca>"));
    assert!(help.contains("--run"));
    assert!(help.contains("--json"));

    let version = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("vacac {}", env!("CARGO_PKG_VERSION"))
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
    let source_path = dir.join("test.vca").to_string_lossy().into_owned();
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
fn resolve_and_generics_diagnostics_include_file_and_excerpt() {
    let source = "fn broken<T>(x: Unknown) -> i64 { 0 }\nfn main() -> i64 { 0 }\n";
    let (dir, result) = run_compiler_with_args(source, "generics-source-json", &["--json"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    let escaped_path = dir
        .join("test.vca")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    assert!(
        stderr.contains(&format!(
            "\"file\":\"{escaped_path}\",\"line\":1,\"column\":14"
        )),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);

    let (dir, result) = run_compiler_with_args(source, "generics-source-human", &["--check"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    let path = dir.join("test.vca");
    assert!(
        stderr.contains(&format!("{}:1:14: unknown type", path.display())),
        "{stderr}"
    );
    assert!(stderr.contains("fn broken<T>(x: Unknown)"), "{stderr}");
    assert!(stderr.contains("^"), "{stderr}");
    let _ = fs::remove_dir_all(dir);

    let (dir, result) = run_compiler_with_args(
        "fn dup() -> i64 { 0 }\nfn dup() -> i64 { 1 }\nfn main() -> i64 { 0 }\n",
        "resolve-source-json",
        &["--json"],
    );
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("\"file\":") && stderr.contains("\"line\":2"),
        "{stderr}"
    );
    assert!(!stderr.contains("\"file\":null"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn json_diagnostics_use_null_for_unavailable_source_locations() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 { 0 }\n",
        "json-no-location",
        &["--json", "--unknown"],
    );
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stderr).trim(),
        r#"{"severity":"error","file":null,"line":null,"column":null,"end_column":null,"message":"unknown argument: --unknown (try --help)"}"#
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
    assert!(ir.contains("define i64 @vaca_main()"), "{ir}");
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

fn cli_test_dir(test_name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vacac-{test_name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn options_may_appear_before_the_source_path() {
    let dir = cli_test_dir("options-before-source");
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 0 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["--quiet", "--check", "test.vca"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(!dir.join("a.out").exists());
    assert!(!dir.join("a.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn option_values_may_appear_before_the_source_path() {
    let dir = cli_test_dir("option-value-before-source");
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 7 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["-o", "program", "test.vca"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let run = Command::new(dir.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(7));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn double_dash_allows_a_source_path_that_begins_with_a_dash() {
    let dir = cli_test_dir("dash-source");
    fs::write(dir.join("-program.vca"), "fn main() -> i64 { 0 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["--check", "--", "-program.vca"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dash_prefixed_output_paths_are_passed_safely_to_the_linker() {
    let dir = cli_test_dir("dash-output");
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 5 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "-o", "-E"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let executable = dir.join("-E");
    assert!(executable.exists());
    assert!(dir.join("-E.o").exists());
    let run = Command::new(&executable)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(5));
    let _ = fs::remove_dir_all(dir);
}
#[test]
fn output_paths_that_match_the_source_file_are_rejected() {
    for (label, args) in [
        ("executable", vec!["test.vca", "-o", "test.vca"]),
        ("object", vec!["--emit-obj", "test.vca", "test.vca"]),
        ("llvm", vec!["--emit-llvm", "test.vca", "test.vca"]),
    ] {
        let dir = cli_test_dir(&format!("collision-source-{label}"));
        let source = "fn main() -> i64 { 0 }\n";
        fs::write(dir.join("test.vca"), source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
            .current_dir(&dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            !result.status.success(),
            "{label}: compiler unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("would overwrite the source file"),
            "{label}: {stderr}"
        );
        assert_eq!(
            fs::read_to_string(dir.join("test.vca")).unwrap(),
            source,
            "{label}: source file was modified"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn output_paths_that_match_the_derived_object_are_rejected() {
    let dir = cli_test_dir("collision-derived-object");
    let source = "fn main() -> i64 { 0 }\n";
    fs::write(dir.join("test.vca"), source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "-o", "test.vca"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("would overwrite the source file"));
    assert_eq!(fs::read_to_string(dir.join("test.vca")).unwrap(), source);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn executable_and_object_outputs_must_differ() {
    let dir = cli_test_dir("collision-object-executable");
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 0 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "-o", "program.o"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("is the same as the executable output path")
    );
    assert!(!dir.join("program.o").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn output_paths_that_match_link_object_inputs_are_rejected() {
    let dir = cli_test_dir("collision-link-object");
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 0 }\n").unwrap();
    let object = dir.join("support.o");
    let object_bytes = b"not a real object file";
    fs::write(&object, object_bytes).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "--link-object", "support.o", "-o", "support"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("would overwrite the linker input"));
    assert_eq!(fs::read(&object).unwrap(), object_bytes);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dotted_output_paths_still_detect_source_collisions() {
    let dir = cli_test_dir("collision-dotted-path");
    let source = "fn main() -> i64 { 0 }\n";
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::write(dir.join("test.vca"), source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "-o", "nested/../test.vca"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("would overwrite the source file"));
    assert_eq!(fs::read_to_string(dir.join("test.vca")).unwrap(), source);
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn hard_linked_output_paths_cannot_overwrite_the_source_file() {
    let dir = cli_test_dir("collision-hard-link");
    let source = "fn main() -> i64 { 0 }\n";
    fs::write(dir.join("test.vca"), source).unwrap();
    fs::hard_link(dir.join("test.vca"), dir.join("alias.vca")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["test.vca", "-o", "alias.vca"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("would overwrite the source file"));
    assert_eq!(fs::read_to_string(dir.join("test.vca")).unwrap(), source);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn outputs_cannot_overwrite_a_module_source_file() {
    let dir = cli_test_dir("collision-module-source");
    let module_source = "pub fn value() -> i64 { 7 }\n";
    fs::write(
        dir.join("main.vca"),
        "mod child;\nfn main() -> i64 { child::value() }\n",
    )
    .unwrap();
    fs::write(dir.join("child.vca"), module_source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["main.vca", "-o", "child.vca"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("would overwrite the source file"),
        "{stderr}"
    );
    assert!(stderr.contains("child.vca"), "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.join("child.vca")).unwrap(),
        module_source
    );
    let _ = fs::remove_dir_all(dir);
}
