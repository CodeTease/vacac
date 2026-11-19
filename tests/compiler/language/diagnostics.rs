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
fn parser_recovers_between_bad_statements_inside_a_function() {
    let (dir, compile) = compile_source(
        "fn helper() -> i64 {\n    let first = ;\n    let second = ;\n    0\n}\nfn main() -> i64 { 0 }\n",
        "parser-statement-recovery",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert_eq!(
        stderr.matches("expected expression, found `;`").count(),
        2,
        "{stderr}"
    );
    assert_eq!(stderr.matches("vacac:").count(), 2, "{stderr}");
    assert!(stderr.contains("test.vca:2:"), "{stderr}");
    assert!(stderr.contains("test.vca:3:"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn parser_recovers_after_a_bad_match_arm_without_consuming_the_match_close() {
    let (dir, compile) = compile_source(
        "fn helper() -> i64 { match true { false => { 0 }, true => { 1, 2 => { 2 } } } }\nfn main() -> i64 { 0 }\n",
        "parser-match-arm-recovery",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert_eq!(stderr.matches("vacac:").count(), 1, "{stderr}");
    assert!(stderr.contains("expected `;` or `}`"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn parser_recovers_inside_nested_blocks_and_preserves_later_statements() {
    let (dir, compile) = compile_source(
        "fn helper() -> i64 { if true { let broken = ; } else { 0 } 1 }\nfn main() -> i64 { 0 }\n",
        "parser-nested-block-recovery",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert_eq!(stderr.matches("vacac:").count(), 1, "{stderr}");
    assert!(
        stderr.contains("expected expression, found `;`"),
        "{stderr}"
    );
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

#[test]
fn unexpected_non_ascii_characters_report_the_complete_scalar_value() {
    let (dir, compile) =
        compile_source("fn main() -> i64 { λ }\n", "unexpected-non-ascii-character");
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.vca:1:20: unexpected character `λ`"),
        "{stderr}"
    );
    assert!(
        !stderr.contains('Î'),
        "reported a Latin-1 rendering of the leading UTF-8 byte: {stderr}"
    );
    assert!(stderr.contains(" 1 | fn main() -> i64 { λ }"), "{stderr}");
    assert!(
        stderr.contains(&format!("   | {}^", " ".repeat(19))),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unexpected_non_ascii_character_columns_ignore_preceding_multibyte_text() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    println(\"héllo\"); €\n    0\n}\n",
        "unexpected-non-ascii-column",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.vca:2:23: unexpected character `€`"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("   | {}^", " ".repeat(22))),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn control_characters_are_escaped_in_unexpected_character_diagnostics() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n \u{7} 0\n}\n",
        "unexpected-control-character",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    let message = stderr.lines().next().unwrap_or_default();
    assert!(
        message.contains("test.vca:2:2: unexpected character `\\u{7}`"),
        "{stderr}"
    );
    assert!(
        !message.contains('\u{7}'),
        "leaked a raw control character into the diagnostic: {message:?}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn multibyte_escape_characters_get_a_two_column_span() {
    let (dir, result) = run_compiler_with_args(
        "fn main() -> i64 {\n    println(\"a\\€a\");\n    0\n}\n",
        "multibyte-escape-span",
        &["--json"],
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(stderr.contains("\"line\":2"), "{stderr}");
    assert!(stderr.contains("\"column\":15"), "{stderr}");
    assert!(stderr.contains("\"end_column\":17"), "{stderr}");
    assert!(
        stderr.contains("unknown escape sequence `\\\\€`"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}
