use std::fs;
use std::process::Command;

fn module_project(
    name: &str,
    root: &str,
    files: &[(&str, &str)],
) -> (std::path::PathBuf, std::process::Output) {
    let directory =
        std::env::temp_dir().join(format!("vacac-modules-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let root_path = directory.join("main.vca");
    fs::write(&root_path, root).unwrap();
    for (path, source) in files {
        let file = directory.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(&root_path)
        .arg("-o")
        .arg(directory.join("program"))
        .output()
        .unwrap();
    (directory, output)
}

#[test]
fn nested_modules_resolve_qualified_functions_types_and_enums() {
    let (directory, compile) = module_project(
        "nested-success",
        "mod geometry;\nfn main() -> i64 { geometry::distance() - 7 }\n",
        &[
            (
                "geometry.vca",
                "mod point;\npub fn distance() -> i64 {\n let p = point::Point { x: 3, y: 4 };\n let c = point::Color::Blue;\n match c { point::Color::Blue => { p.x + p.y }, point::Color::Red => { 0 } }\n}\n",
            ),
            (
                "geometry/point.vca",
                "pub struct Point { x: i64, y: i64 }\npub enum Color { Red, Blue }\n",
            ),
        ],
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
fn directory_modules_load_mod_vca_sources() {
    let (directory, compile) = module_project(
        "directory-module",
        "mod math;\nfn main() -> i64 { math::value() - 42 }\n",
        &[("math/mod.vca", "pub fn value() -> i64 { 42 }\n")],
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
fn private_module_function_cannot_be_called_from_root() {
    let (directory, compile) = module_project(
        "private-function",
        "mod math;\nfn main() -> i64 { math::hidden() }\n",
        &[("math.vca", "fn hidden() -> i64 { 1 }\n")],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr).contains("function `math::hidden` is private")
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn missing_child_module_reports_the_expected_path() {
    let (directory, compile) = module_project(
        "missing-child",
        "mod absent;\nfn main() -> i64 { 0 }\n",
        &[],
    );
    assert!(!compile.status.success());
    assert!(String::from_utf8_lossy(&compile.stderr).contains("absent.vca"));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn diagnostics_from_child_modules_include_the_child_source_path() {
    let (directory, compile) = module_project(
        "child-diagnostic",
        "mod math;\nfn main() -> i64 { math::value() }\n",
        &[("math.vca", "pub fn value() -> i64 { missing }\n")],
    );
    assert!(!compile.status.success());
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("math.vca:1:"), "{stderr}");
    assert!(stderr.contains("undefined variable `missing`"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}
