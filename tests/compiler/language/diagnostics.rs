use crate::common::*;
use std::fs;

#[test]
fn parser_diagnostics_highlight_the_offending_source_token() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let x = ;\n    0\n}\n",
        "parser-location",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.vca:2:13: expected expression, found `;`"),
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
fn parser_recovers_after_bad_top_level_declarations_and_reports_each_error() {
    let (dir, compile) = compile_source(
        "fn first() -> i64 { let value = ; 0 }\nfn second( -> i64 { 0 }\nfn main() -> i64 { 0 }\n",
        "parser-multiple-errors",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(stderr.contains("test.vca:1:"), "{stderr}");
    assert!(stderr.contains("test.vca:2:"), "{stderr}");
    assert!(
        stderr.contains("expected expression, found `;`"),
        "{stderr}"
    );
    assert!(stderr.matches("vacac:").count() == 2, "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn parser_multiple_errors_are_emitted_as_individual_json_diagnostics() {
    let (dir, result) = run_compiler_with_args(
        "fn first() -> i64 { let value = ; 0 }\nfn main( -> i64 { 0 }\n",
        "parser-multiple-json-errors",
        &["--json"],
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    assert!(
        stderr
            .lines()
            .all(|line| line.starts_with("{\"severity\":\"error\"")),
        "{stderr}"
    );
    assert!(stderr.contains("\"line\":1"), "{stderr}");
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn parser_recovers_from_pub_module_error_and_reports_a_malformed_declaration_at_eof() {
    let (dir, result) = run_compiler_with_args(
        "pub mod forbidden;\nfn main( -> i64 { 0",
        "parser-pub-and-eof-errors",
        &["--json"],
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    assert!(stderr.contains("\"line\":1"), "{stderr}");
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    assert!(stderr.contains("`pub mod` is not supported"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn lexer_columns_and_underlines_count_unicode_scalar_values() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    println(\"λ\\q\");\n    0\n}\n",
        "unicode-diagnostic-column",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.vca:2:15: unknown escape sequence"),
        "{stderr}"
    );
    assert!(stderr.contains("    println(\"λ\\q\");"), "{stderr}");
    assert!(
        stderr.contains(&format!("   | {}^^", " ".repeat(14))),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}
