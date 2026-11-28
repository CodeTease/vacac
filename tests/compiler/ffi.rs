use std::fs;
use std::process::Command;

fn build_native_inputs(
    directory: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let object_source = directory.join("object.c");
    let object = directory.join("object.o");
    fs::write(
        &object_source,
        "#include <stdint.h>\n#include <stddef.h>\nint64_t object_value(void) { return 19; }\nint64_t add_i64(int64_t a, int64_t b) { return a + b; }\nfloat add_f32(float a, float b) { return a + b; }\ndouble add_f64(double a, double b) { return a + b; }\nvoid accept_i64(int64_t value) { (void)value; }\nconst char *native_label(void) { return \"vaca\"; }\nconst char *native_echo(const char *text) { return text; }\nint64_t native_fill(const char *label, int64_t *out) {\n    if (label == NULL || out == NULL) return 0;\n    if (label[0] != 'v' || label[1] != 'a') return 0;\n    *out = 42;\n    return 1;\n}\nint64_t *native_null(void) { return 0; }\nconst char *native_null_string(void) { return 0; }\nvoid native_write_label(const char **out) { if (out != NULL) *out = \"from-c\"; }\n",
    )
    .unwrap();
    let result = Command::new("cc")
        .arg("-c")
        .arg(&object_source)
        .arg("-o")
        .arg(&object)
        .status()
        .unwrap();
    assert!(result.success());

    let library_source = directory.join("library.c");
    let library_object = directory.join("library.o");
    let library = directory.join("libanswer.a");
    fs::write(
        &library_source,
        "#include <stdint.h>\nint64_t library_value(void) { return 23; }\n",
    )
    .unwrap();
    let result = Command::new("cc")
        .arg("-c")
        .arg(&library_source)
        .arg("-o")
        .arg(&library_object)
        .status()
        .unwrap();
    assert!(result.success());
    let result = Command::new("ar")
        .arg("rcs")
        .arg(&library)
        .arg(&library_object)
        .status()
        .unwrap();
    assert!(result.success());
    let second_source = directory.join("second.c");
    let second_object = directory.join("second.o");
    fs::write(
        &second_source,
        "int second_object_value(void) { return 0; }\n",
    )
    .unwrap();
    let result = Command::new("cc")
        .arg("-c")
        .arg(&second_source)
        .arg("-o")
        .arg(&second_object)
        .status()
        .unwrap();
    assert!(result.success());
    (object, second_object, library)
}

fn compile_with_native_inputs(
    directory: &std::path::Path,
    source: &str,
    mode: &str,
    objects: &[std::path::PathBuf],
    libraries: &[&str],
) -> std::process::Output {
    let source_path = directory.join("main.vca");
    fs::write(&source_path, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_vacac"));
    command.arg(&source_path).arg(mode).current_dir(directory);
    for object in objects {
        command.arg("--link-object").arg(object);
    }
    for _ in 0..2 {
        command.arg("--link-search").arg(directory);
    }
    for library in libraries {
        command.arg("--link-lib").arg(library);
    }
    if mode != "--run" {
        command.arg("-o").arg(directory.join("program"));
    }
    command.output().unwrap()
}

const NATIVE_FFI_SOURCE: &str = r#"
extern "C" fn object_value() -> i64;
extern "C" fn library_value() -> i64;

fn main() -> i64 {
    object_value() + library_value() - 42
}
"#;

#[test]
fn links_extern_calls_with_objects_search_paths_and_libraries() {
    let directory = std::env::temp_dir().join(format!("vacac-ffi-link-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let (object, second_object, _) = build_native_inputs(&directory);

    let compile = compile_with_native_inputs(
        &directory,
        NATIVE_FFI_SOURCE,
        "--quiet",
        &[object, second_object],
        &["answer", "answer"],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn run_mode_accepts_native_linker_inputs() {
    let directory = std::env::temp_dir().join(format!("vacac-ffi-run-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let (object, second_object, _) = build_native_inputs(&directory);

    let run = compile_with_native_inputs(
        &directory,
        NATIVE_FFI_SOURCE,
        "--run",
        &[object, second_object],
        &["answer", "answer"],
    );
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(0));

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn supports_all_declared_scalar_c_abi_types() {
    let directory = std::env::temp_dir().join(format!("vacac-ffi-scalars-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let (object, second_object, _) = build_native_inputs(&directory);
    let result = compile_with_native_inputs(
        &directory,
        r#"
extern "C" fn add_i64(left: i64, right: i64) -> i64;
extern "C" fn add_f32(left: f32, right: f32) -> f32;
extern "C" fn add_f64(left: f64, right: f64) -> f64;
extern "C" fn accept_i64(value: i64) -> unit;

fn main() -> i64 {
    accept_i64(add_i64(1, 2));
    if add_f32(1.25f32, 2.0f32) == 3.25f32 && add_f64(1.5, 2.5) == 4.0 {
        0
    } else {
        1
    }
}
"#,
        "--quiet",
        &[object, second_object],
        &["answer"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.code(), Some(0));
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn supports_c_strings_and_typed_raw_pointers() {
    let directory = std::env::temp_dir().join(format!("vacac-ffi-strings-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let (object, second_object, _) = build_native_inputs(&directory);
    let result = compile_with_native_inputs(
        &directory,
        r#"
extern "C" fn native_label() -> str;
extern "C" fn native_echo(text: str) -> str;
extern "C" fn native_fill(label: str, out: *mut i64) -> i64;
extern "C" fn native_null() -> *mut i64;
extern "C" fn native_write_label(out: *mut str) -> unit;

fn main() -> i64 {
    const label = native_label();
    if label != "vaca" { return 1; }
    if native_echo(label) != label { return 2; }

    let value = 0;
    const out = &raw mut value;
    if native_fill(label, out) != 1 { return 3; }
    if value != 42 { return 4; }

    const empty = native_null();
    if empty != null::<*mut i64>() { return 5; }

    let writable = "initial";
    const slot = &raw mut writable;
    native_write_label(slot);
    if writable != "from-c" { return 6; }
    if native_null() == null::<*mut i64>() { 0 } else { 7 }
}
"#,
        "--quiet",
        &[object, second_object],
        &["answer"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn null_c_string_results_trap_instead_of_becoming_vaca_strings() {
    let directory = std::env::temp_dir().join(format!("vacac-ffi-null-str-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let (object, _, _) = build_native_inputs(&directory);

    for (source, name) in [
        (
            r#"
extern "C" fn native_null_string() -> str;
fn main() -> i64 { const value = native_null_string(); print(value); 0 }
"#,
            "ffi-null-string-direct",
        ),
        (
            r#"
extern "C" fn native_null_string() -> str;
fn main() -> i64 {
    const function: fn() -> str = native_null_string;
    const value = function();
    print(value);
    0
}
"#,
            "ffi-null-string-function-value",
        ),
    ] {
        let result = compile_with_native_inputs(
            &directory,
            source,
            "--quiet",
            std::slice::from_ref(&object),
            &["answer"],
        );
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let run = Command::new(directory.join("program")).output().unwrap();
        assert_eq!(run.status.code(), Some(1), "{name}");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "",
            "{name} must trap before using the string"
        );
    }
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn generic_types_in_pointer_extern_signatures_are_normalized() {
    let (directory, result) = crate::common::run_compiler_with_args(
        r#"
        struct Box<T> { value: T }
        extern "C" fn accept_box(value: *mut Box<i64>) -> unit;
        fn main() -> i64 { 0 }
        "#,
        "ffi-generic-pointer-check",
        &["--check"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn calls_a_libc_function() {
    let result = crate::common::compile_and_run(
        "extern \"C\" fn labs(value: i64) -> i64; fn main() -> i64 { labs(-42) - 42 }",
        "ffi-libc-labs",
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn rejects_unsupported_extern_types_and_duplicate_c_symbols() {
    for (source, expected, name) in [
        (
            "extern \"Rust\" fn bad(value: i64) -> i64; fn main() -> i64 { 0 }",
            "the ABI string `\"C\"`",
            "ffi-unsupported-abi",
        ),
        (
            "extern \"C\" fn bad(value: bool) -> i64; fn main() -> i64 { 0 }",
            "C extern parameters support only",
            "ffi-bool-parameter",
        ),
        (
            "extern \"C\" fn bad() -> bool; fn main() -> i64 { 0 }",
            "C extern return types support only",
            "ffi-bool-return",
        ),
        (
            "extern \"C\" fn first() -> i64; extern \"C\" fn first() -> i64; fn main() -> i64 { 0 }",
            "defined more than once",
            "ffi-duplicate-symbol",
        ),
        (
            "extern \"C\" fn printf() -> i64; fn main() -> i64 { 0 }",
            "reserved compiler/runtime symbol",
            "ffi-reserved-symbol",
        ),
    ] {
        let (directory, result) = crate::common::run_compiler_with_args(source, name, &["--check"]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "expected {expected:?}, got {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let _ = fs::remove_dir_all(directory);
    }
}

#[test]
fn linker_inputs_are_rejected_for_non_linking_modes() {
    let (directory, result) = crate::common::run_compiler_with_args(
        "fn main() -> i64 { 0 }",
        "ffi-link-check-rejected",
        &["--check", "--link-lib", "m"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be used with --check"));
    let _ = fs::remove_dir_all(directory);

    let (directory, result) = crate::common::run_compiler_with_args(
        "fn main() -> i64 { 0 }",
        "ffi-link-llvm-rejected",
        &["--emit-llvm", "out.ll", "--link-lib", "m"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be used with --emit-llvm"));
    let _ = fs::remove_dir_all(directory);
}
