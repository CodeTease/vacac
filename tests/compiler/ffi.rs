use std::fs;
use std::process::Command;

fn build_native_inputs(
    directory: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let object_source = directory.join("object.c");
    let object = directory.join("object.o");
    fs::write(
        &object_source,
        "#include <stdint.h>\nint64_t object_value(void) { return 19; }\nint64_t add_i64(int64_t a, int64_t b) { return a + b; }\nfloat add_f32(float a, float b) { return a + b; }\ndouble add_f64(double a, double b) { return a + b; }\nvoid accept_i64(int64_t value) { (void)value; }\n",
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
            "extern \"C\" fn bad() -> str; fn main() -> i64 { 0 }",
            "C extern return types support only",
            "ffi-str-return",
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
