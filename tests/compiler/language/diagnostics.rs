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
