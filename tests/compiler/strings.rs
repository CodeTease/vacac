use crate::common::*;
use std::fs;

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
fn strings_can_be_concatenated() {
    let result = compile_and_run(
        r#"
        fn punctuation() -> str { "!" }
        fn greeting(name: str) -> str { "Hello, " + name + punctuation() }
        fn main() -> i64 {
            println(greeting("Apex"));
            println("" + "empty");
            println("tail" + "");
            println("hé" + "llo ✓");
            let repeated = "x";
            for i in 0..3 {
                repeated = repeated + "y";
            }
            println(repeated);
            0
        }
        "#,
        "string-concat",
    );
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "Hello, Apex!\nempty\ntail\nhéllo ✓\nxyyy\n"
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
fn strings_only_support_addition_and_equality() {
    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let s = \"a\" - \"b\";\n    0\n}\n",
        "string-subtract",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("test.apx:2:17: arithmetic operands has type str, expected i64"),
        "unexpected diagnostic: {stderr}"
    );
    let _ = fs::remove_dir_all(dir);

    let (dir, compile) = compile_source(
        "fn main() -> i64 {\n    let is_less = \"a\" < \"b\";\n    0\n}\n",
        "string-ordering",
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("comparison operands has type str, expected i64"),
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
