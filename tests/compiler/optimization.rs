use crate::common::*;
use std::fs;
use std::process::Command;

const LEVELS: [(&str, &str); 4] = [("O0", "-O0"), ("O1", "-O1"), ("O2", "-O2"), ("O3", "-O3")];

#[test]
fn optimization_flags_work_in_run_and_executable_modes() {
    let source = "fn main() -> i64 {\n    print_i64(40 + 2);\n    7\n}\n";
    for (name, level) in LEVELS {
        let (dir, result) = run_compiler_with_args(
            source,
            &format!("optimization-run-{name}"),
            &["--run", level],
        );
        assert_eq!(
            result.status.code(),
            Some(7),
            "{level}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n", "{level}");
        let _ = fs::remove_dir_all(dir);

        let (dir, compile) = run_compiler_with_args(
            source,
            &format!("optimization-executable-{name}"),
            &[level, "-o", "program"],
        );
        assert!(
            compile.status.success(),
            "{level}: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = Command::new(dir.join("program")).output().unwrap();
        assert_eq!(run.status.code(), Some(7), "{level}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n", "{level}");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn optimization_flags_control_the_emitted_llvm_pipeline() {
    let source = "fn main() -> i64 {\n    let value = 40;\n    value + 2\n}\n";
    for (name, level) in [("default", None)]
        .into_iter()
        .chain(LEVELS.map(|(name, level)| (name, Some(level))))
    {
        let mut args = vec!["--emit-llvm", "program.ll", "--quiet"];
        if let Some(level) = level {
            args.push(level);
        }
        let (dir, result) =
            run_compiler_with_args(source, &format!("optimization-ir-{name}"), &args);
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let ir = fs::read_to_string(dir.join("program.ll")).unwrap();
        assert!(ir.contains("target triple"), "{name}: {ir}");
        assert!(ir.contains("target datalayout"), "{name}: {ir}");
        let main_start = ir.find("@vaca_main(").unwrap();
        let main_end = ir[main_start..].find("\n}").unwrap();
        let main = &ir[main_start..main_start + main_end];
        if matches!(name, "O1" | "O2" | "O3") {
            assert!(main.contains("ret i64 42"), "{name}: {main}");
            assert!(!main.contains("alloca i64"), "{name}: {main}");
        } else {
            assert!(main.contains("alloca i64"), "{name}: {main}");
        }
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn optimization_flags_may_appear_before_the_source_path() {
    let dir = std::env::temp_dir().join(format!(
        "vacac-optimization-before-source-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("test.vca"), "fn main() -> i64 { 9 }\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .current_dir(&dir)
        .args(["-O2", "test.vca", "--run"])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn invalid_duplicate_and_check_optimization_flags_are_rejected() {
    let source = "fn main() -> i64 { 0 }\n";
    for (name, args, diagnostic) in [
        ("unsupported", vec!["-O4"], "unsupported optimization level"),
        ("bare", vec!["-O"], "unsupported optimization level"),
        (
            "duplicate",
            vec!["-O2", "-O3"],
            "optimization level may be specified only once",
        ),
        (
            "check",
            vec!["--check", "-O0"],
            "optimization levels cannot be used with --check",
        ),
    ] {
        let (dir, result) =
            run_compiler_with_args(source, &format!("optimization-invalid-{name}"), &args);
        assert!(!result.status.success(), "{name}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains(diagnostic), "{name}: {stderr}");
        let _ = fs::remove_dir_all(dir);
    }

    let (dir, result) =
        run_compiler_with_args(source, "optimization-invalid-json", &["--json", "-O4"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.starts_with("{\"severity\":\"error\""), "{stderr}");
    assert!(
        stderr.contains("unsupported optimization level"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn vaca_functions_with_c_library_names_keep_their_side_effects() {
    let source = "fn labs(value: i64) -> i64 {\n    print_i64(value);\n    7\n}\nfn main() -> i64 {\n    labs(-4);\n    0\n}\n";
    for (name, level) in LEVELS {
        let (dir, result) = run_compiler_with_args(
            source,
            &format!("optimization-library-name-{name}"),
            &["--run", level],
        );
        assert!(
            result.status.success(),
            "{level}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "-4\n", "{level}");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn checked_arithmetic_and_ieee_values_survive_o3() {
    let (dir, result) = run_compiler_with_args(
        r#"
        fn main() -> i64 {
            print_i64(9223372036854775807 + 0);
            print_i64(-9223372036854775808 - 0);
            print_i64(3037000499 * 3037000499);
            print_f32(1.25f32 + 2.0f32);
            if 0.0 == -0.0 { print_i64(1); }
            if 0.0 / 0.0 == 0.0 / 0.0 { print_i64(9); } else { print_i64(2); }
            0
        }
        "#,
        "optimization-o3-semantics",
        &["--run", "-O3"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "9223372036854775807\n-9223372036854775808\n9223372030926249001\n3.25\n1\n2\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn runtime_traps_survive_o3() {
    for (name, expression) in [
        ("overflow", "9223372036854775807 + 1"),
        ("division", "1 / 0"),
        ("shift", "1 << 64"),
        ("bounds", "[1, 2][2]"),
    ] {
        let source = format!("fn main() -> i64 {{ print_i64(42); {expression} }}\n");
        let (dir, result) = run_compiler_with_args(
            &source,
            &format!("optimization-o3-trap-{name}"),
            &["--run", "-O3"],
        );
        assert_eq!(
            result.status.code(),
            Some(1),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n", "{name}");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn c_ffi_calls_survive_o3() {
    let (dir, result) = run_compiler_with_args(
        r#"
        extern "C" fn labs(value: i64) -> i64;
        extern "C" fn atol(value: str) -> i64;

        fn main() -> i64 {
            print_i64(labs(-42));
            print_i64(atol("42"));
            0
        }
        "#,
        "optimization-o3-ffi",
        &["--run", "-O3"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n42\n");
    let _ = fs::remove_dir_all(dir);
}
