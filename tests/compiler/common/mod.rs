use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) fn compile_source(
    source: &str,
    test_name: &str,
) -> (std::path::PathBuf, std::process::Output) {
    let dir = std::env::temp_dir().join(format!("vacac-{test_name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("test.vca");
    let executable = dir.join("program");
    fs::write(&source_path, source).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    (dir, compile)
}

pub(crate) fn run_compiler_with_args(
    source: &str,
    test_name: &str,
    args: &[&str],
) -> (std::path::PathBuf, std::process::Output) {
    let dir = std::env::temp_dir().join(format!("vacac-{test_name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("test.vca");
    fs::write(&source_path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(&source_path)
        .args(args)
        .current_dir(&dir)
        .output()
        .unwrap();
    (dir, output)
}

pub(crate) fn compile_and_run(source: &str, test_name: &str) -> std::process::Output {
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

pub(crate) fn compile_and_run_with_stdin(
    source: &str,
    test_name: &str,
    input: &[u8],
) -> std::process::Output {
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

pub(crate) fn assert_compile_error(source: &str, test_name: &str, diagnostic: &str) {
    let (dir, compile) = compile_source(source, test_name);
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success(), "compiler unexpectedly succeeded");
    assert!(
        stderr.contains(diagnostic),
        "expected diagnostic {diagnostic:?}, got:\n{stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}
