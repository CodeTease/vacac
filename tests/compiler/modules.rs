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
fn parser_reports_syntax_errors_from_root_and_nested_modules_together() {
    let (directory, compile) = module_project(
        "parser-errors-multiple-modules",
        "mod broken;\nfn main() -> i64 { let value = ; 0 }\n",
        &[("broken.vca", "pub fn value( -> i64 { 0 }\n")],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(stderr.contains("main.vca:2:"), "{stderr}");
    assert!(stderr.contains("broken.vca:1:"), "{stderr}");
    assert!(stderr.matches("vacac:").count() == 2, "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn parser_keeps_reporting_after_a_missing_module_file() {
    let (directory, compile) = module_project(
        "parser-errors-and-missing-module",
        "mod missing;\nmod broken;\nfn main() -> i64 { let value = ; 0 }\n",
        &[("broken.vca", "pub fn value( -> i64 { 0 }\n")],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(stderr.contains("main.vca:3:"), "{stderr}");
    assert!(stderr.contains("broken.vca:1:"), "{stderr}");
    assert!(stderr.contains("missing.vca"), "{stderr}");
    assert_eq!(stderr.matches("vacac:").count(), 3, "{stderr}");
    let _ = fs::remove_dir_all(directory);
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
fn module_struct_patterns_resolve_qualified_names_and_visibility() {
    let (directory, compile) = module_project(
        "module-struct-pattern",
        "mod geometry;\nfn main() -> i64 { let geometry::Point { x, .. } = geometry::make(); x - 42 }\n",
        &[(
            "geometry.vca",
            "pub struct Point { x: i64, y: bool }\npub fn make() -> Point { Point { x: 42, y: true } }\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);

    let (directory, compile) = module_project(
        "private-struct-pattern",
        "mod geometry;\nfn main() -> i64 { let geometry::Point { x, .. } = geometry::make(); x }\n",
        &[(
            "geometry.vca",
            "struct Point { x: i64, y: bool }\npub fn make() -> Point { Point { x: 42, y: true } }\n",
        )],
    );
    assert!(!compile.status.success());
    assert!(String::from_utf8_lossy(&compile.stderr).contains("type `geometry::Point` is private"));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn public_enum_patterns_resolve_from_child_modules() {
    let (directory, compile) = module_project(
        "module-enum-pattern",
        "mod events;\nfn main() -> i64 { events::classify(events::Kind::Value(42)) - 42 }\n",
        &[(
            "events.vca",
            "pub enum Kind { Value(i64), Empty }\npub fn classify(value: Kind) -> i64 { match value { Kind::Value(number) => { number }, Kind::Empty => { 0 } } }\n",
        )],
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
fn module_functions_can_use_the_print_f64_builtin() {
    let (directory, compile) = module_project(
        "module-print-f64",
        "mod math;\nfn main() -> i64 { math::report(1.25); 0 }\n",
        &[(
            "math.vca",
            "pub fn report(value: f64) -> unit { print_f64(value); }\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1.25\n");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn module_functions_can_use_the_print_f32_builtin() {
    let (directory, compile) = module_project(
        "module-print-f32",
        "mod math;\nfn main() -> i64 { math::report(1.25f32); 0 }\n",
        &[(
            "math.vca",
            "pub fn report(value: f32) -> unit { print_f32(value); }\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1.25\n");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn public_generic_functions_and_structs_work_across_modules() {
    let (directory, compile) = module_project(
        "generic-module",
        "mod common;\nfn main() -> i64 { let wrapped = common::Box<i64> { value: common::identity(42) }; wrapped.value - 42 }\n",
        &[(
            "common.vca",
            "pub struct Box<T> { value: T }\npub fn identity<T>(value: T) -> T { value }\n",
        )],
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
fn public_extern_declarations_resolve_across_modules() {
    let directory =
        std::env::temp_dir().join(format!("vacac-module-extern-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let root = directory.join("main.vca");
    fs::write(
        &root,
        "mod native;\nfn main() -> i64 { native::labs(-42) - 42 }\n",
    )
    .unwrap();
    fs::write(
        directory.join("native.vca"),
        "pub extern \"C\" fn labs(value: i64) -> i64;\n",
    )
    .unwrap();
    let executable = directory.join("program");
    let compile = Command::new(env!("CARGO_BIN_EXE_vacac"))
        .arg(root)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(executable).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn public_module_functions_can_be_used_as_function_values() {
    let (directory, compile) = module_project(
        "module-function-values",
        "mod math;\nfn main() -> i64 { let f: fn(i64) -> i64 = math::increment; f(41) - 42 }\n",
        &[(
            "math.vca",
            "pub fn increment(value: i64) -> i64 { value + 1 }\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);

    let (directory, compile) = module_project(
        "private-module-function-value",
        "mod math;\nfn main() -> i64 { let f: fn(i64) -> i64 = math::increment; f(41) - 42 }\n",
        &[(
            "math.vca",
            "fn increment(value: i64) -> i64 { value + 1 }\n",
        )],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr).contains("function `math::increment` is private")
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn public_globals_work_across_modules_and_private_globals_stay_private() {
    let (directory, compile) = module_project(
        "module-globals",
        "mod values;\nfn main() -> i64 { values::COUNTER = 43; values::ANSWER + values::COUNTER - 85 }\n",
        &[(
            "values.vca",
            "pub const ANSWER: i64 = 42;\npub const NEXT: i64 = ANSWER + 1;\npub let COUNTER: i64 = 42;\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    let _ = fs::remove_dir_all(directory);

    let (directory, compile) = module_project(
        "private-global",
        "mod values;\nfn main() -> i64 { values::SECRET }\n",
        &[("values.vca", "const SECRET: i64 = 42;\n")],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr).contains("global `values::SECRET` is private"),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn private_generic_module_declarations_remain_private() {
    let (directory, compile) = module_project(
        "private-generic-module",
        "mod common;\nfn main() -> i64 { common::identity(42) }\n",
        &[("common.vca", "fn identity<T>(value: T) -> T { value }\n")],
    );
    assert!(!compile.status.success());
    assert!(
        String::from_utf8_lossy(&compile.stderr).contains("function `common::identity` is private")
    );
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
