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
fn module_constants_parse_in_if_and_while_headers() {
    let (directory, compile) = module_project(
        "module-const-if-header",
        "mod m;\nfn main() -> i64 {\n    if m::ENABLED { 0 } else { 1 }\n}\n",
        &[("m.vca", "pub const ENABLED: bool = true;\n")],
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
        "module-const-while-header",
        "mod m;\nfn main() -> i64 {\n    while m::ENABLED { return 0; }\n    1\n}\n",
        &[("m.vca", "pub const ENABLED: bool = true;\n")],
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
fn module_constants_parse_in_for_range_headers() {
    let (directory, compile) = module_project(
        "module-const-for-header",
        concat!(
            "mod m;\n",
            "fn main() -> i64 {\n",
            "    let total = 0;\n",
            "    for i in 0..m::LIMIT { total = total + i; }\n",
            "    total - 3\n",
            "}\n",
        ),
        &[("m.vca", "pub const LIMIT: i64 = 3;\n")],
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
fn child_module_declarations_are_not_shadowed_by_root_declarations() {
    let (directory, compile) = module_project(
        "root-shadowing-public",
        "mod child;\npub let x: i64 = 1;\nfn main() -> i64 {\n    let c = child::update();\n    print_i64(x);\n    print_i64(c);\n    0\n}\n",
        &[(
            "child.vca",
            "let x: i64 = 100;\npub fn update() -> i64 {\n    x = 42;\n    x\n}\n",
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n42\n");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn child_module_declarations_resolve_before_private_root_declarations() {
    let (directory, compile) = module_project(
        "root-shadowing-private",
        "mod child;\nconst value: i64 = 1;\nfn helper() -> i64 { 1 }\nfn main() -> i64 { child::run() - 42 }\n",
        &[(
            "child.vca",
            "const value: i64 = 42;\nfn helper() -> i64 { value }\npub fn run() -> i64 { helper() }\n",
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
fn local_function_value_shadows_a_private_root_function_in_a_child_module() {
    let (directory, compile) = module_project(
        "local-function-value-shadow",
        "mod child;\nfn cb() -> i64 { 99 }\nfn main() -> i64 {\n    child::run(fn() -> i64 { 42 }) - 42\n}\n",
        &[(
            "child.vca",
            "pub fn run(cb: fn() -> i64) -> i64 {\n    cb()\n}\n",
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

#[test]
fn imports_alias_functions_globals_types_traits_and_enums() {
    let (directory, compile) = module_project(
        "imports-aliasing",
        concat!(
            "mod math;\n",
            "use math as m;\n",
            "use math::ANSWER;\n",
            "use math::Counter;\n",
            "use math::Color as Hue;\n",
            "use math::Label as Labelled;\n",
            "use math::increment as bump;\n",
            "use math::COUNTER;\n",
            "const NEXT: i64 = ANSWER + 1;\n",
            "fn describe<T: Labelled>(value: T) -> i64 { Labelled::label(value) }\n",
            "fn main() -> i64 {\n",
            "    let counter = Counter { value: bump(ANSWER) };\n",
            "    COUNTER = NEXT;\n",
            "    let hue = Hue::Green;\n",
            "    let extra = match hue {\n",
            "        Hue::Red => { 0 },\n",
            "        Hue::Green => { 1 },\n",
            "    };\n",
            "    print_i64(m::increment(ANSWER));\n",
            "    describe(counter.value) + extra + COUNTER - 86\n",
            "}\n",
        ),
        &[(
            "math.vca",
            concat!(
                "pub const ANSWER: i64 = 41;\n",
                "pub let COUNTER: i64 = 0;\n",
                "pub struct Counter { value: i64 }\n",
                "pub enum Color { Red, Green }\n",
                "pub trait Label { fn label(value: Self) -> i64; }\n",
                "impl Label for i64 { fn label(value: Self) -> i64 { value + 1 } }\n",
                "pub fn increment(value: i64) -> i64 { value + 1 }\n",
            ),
        )],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(directory.join("program")).output().unwrap();
    assert_eq!(run.status.code(), Some(0));
    // `bump(41)` is 42, `describe(42)` is 43, `extra` is 1, `COUNTER` is 42.
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn directly_imported_enum_variants_work_in_values_and_patterns() {
    let (directory, compile) = module_project(
        "imported-variants",
        concat!(
            "mod shapes;\n",
            "use shapes::Shape;\n",
            "use shapes::Shape::Circle;\n",
            "use shapes::Shape::Square;\n",
            "use shapes::size;\n",
            "fn main() -> i64 {\n",
            "    let round = Circle(3);\n",
            "    let flat: Shape = Square;\n",
            "    let radius = match round {\n",
            "        Circle(value) => { value },\n",
            "        Square => { 0 },\n",
            "    };\n",
            "    let square_size = match flat {\n",
            "        Circle(_) => { 1 },\n",
            "        Square => { size(flat) },\n",
            "    };\n",
            "    radius + square_size - 7\n",
            "}\n",
        ),
        &[(
            "shapes.vca",
            concat!(
                "pub enum Shape { Circle(i64), Square }\n",
                "pub fn size(shape: Shape) -> i64 {\n",
                "    match shape { Shape::Circle(radius) => { radius }, Shape::Square => { 4 } }\n",
                "}\n",
            ),
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
fn direct_variant_import_keeps_its_enum_when_the_module_reuses_the_name() {
    let (directory, compile) = module_project(
        "imported-variant-shadowed-enum",
        concat!(
            "mod app;\n",
            "pub enum Shape { Circle(i64), Square }\n",
            "fn main() -> i64 { app::run() - 3 }\n",
        ),
        &[(
            "app.vca",
            concat!(
                "use Shape::Circle;\n",
                "pub enum Shape { Other }\n",
                "pub fn run() -> i64 {\n",
                "    let round = Circle(3);\n",
                "    match round { Circle(value) => { value }, _ => { 0 } }\n",
                "}\n",
            ),
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
fn import_alias_shadows_a_root_declaration_of_the_same_name() {
    let (directory, compile) = module_project(
        "import-alias-shadows-root",
        concat!(
            "mod app;\n",
            "mod other;\n",
            "pub enum Shape { Root }\n",
            "fn main() -> i64 { app::run() - 1 }\n",
        ),
        &[
            (
                "app.vca",
                concat!(
                    "use other::Shape as Shape;\n",
                    "pub fn run() -> i64 { let value: Shape = Shape::Mine; 1 }\n",
                ),
            ),
            ("other.vca", "pub enum Shape { Mine }\n"),
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
fn explicit_variant_type_arguments_resolve_imported_types() {
    let (directory, compile) = module_project(
        "variant-imported-type-argument",
        concat!(
            "mod pack;\n",
            "use pack::Pack;\n",
            "enum Wrapper<T> { Value(T) }\n",
            "fn main() -> i64 {\n",
            "    let value = Wrapper::<Pack>::Value(Pack { n: 7 });\n",
            "    match value { Wrapper::Value(inner) => { inner.n } } - 7\n",
            "}\n",
        ),
        &[("pack.vca", "pub struct Pack { n: i64 }\n")],
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
fn shorthand_struct_field_patterns_are_not_promoted_to_variants() {
    let (directory, compile) = module_project(
        "shorthand-field-pattern",
        concat!(
            "use Shape::Circle;\n",
            "enum Shape { Circle, Square }\n",
            "struct Point { Circle: i64 }\n",
            "fn main() -> i64 {\n",
            "    let point = Point { Circle: 7 };\n",
            "    match point { Point { Circle } => { Circle } } - 7\n",
            "}\n",
        ),
        &[],
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
fn imported_functions_and_globals_work_as_values() {
    let (directory, compile) = module_project(
        "imported-values",
        concat!(
            "mod math;\n",
            "use math::increment;\n",
            "use math::STEP;\n",
            "fn main() -> i64 {\n",
            "    let callable: fn(i64) -> i64 = increment;\n",
            "    callable(STEP) - 42\n",
            "}\n",
        ),
        &[(
            "math.vca",
            "pub const STEP: i64 = 41;\npub fn increment(value: i64) -> i64 { value + 1 }\n",
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
fn imports_are_private_to_the_declaring_module() {
    let (directory, compile) = module_project(
        "import-scope",
        "mod values;\nmod reader;\nfn main() -> i64 { reader::read() - 42 }\n",
        &[
            ("values.vca", "pub const ANSWER: i64 = 42;\n"),
            (
                "reader.vca",
                "use values::ANSWER;\npub fn read() -> i64 { ANSWER }\n",
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

    let (directory, compile) = module_project(
        "import-scope-leak",
        "mod values;\nuse values::ANSWER;\nmod reader;\nfn main() -> i64 { ANSWER + reader::read() - 84 }\n",
        &[
            ("values.vca", "pub const ANSWER: i64 = 42;\n"),
            (
                "reader.vca",
                "use values::ANSWER;\npub fn read() -> i64 { ANSWER }\n",
            ),
        ],
    );
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let _ = fs::remove_dir_all(directory);

    let (directory, compile) = module_project(
        "import-scope-root-only",
        "mod values;\nmod reader;\nuse values::ANSWER;\nfn main() -> i64 { reader::read() }\n",
        &[
            ("values.vca", "pub const ANSWER: i64 = 42;\n"),
            ("reader.vca", "pub fn read() -> i64 { ANSWER }\n"),
        ],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success(), "{}", stderr);
    assert!(stderr.contains("undefined variable `ANSWER`"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn import_collisions_with_local_declarations_and_modules_are_rejected() {
    for (name, root, expected) in [
        (
            "import-duplicate",
            "mod math;\nuse math::value;\nuse math::other as value;\nfn main() -> i64 { value() }\n",
            "import name `value` is imported more than once",
        ),
        (
            "import-local-declaration",
            "mod math;\nuse math::value;\nfn value() -> i64 { 3 }\nfn main() -> i64 { value() }\n",
            "import name `value` conflicts with function `value` declared in this module",
        ),
        (
            "import-module-name",
            "mod math;\nuse math::value as math;\nfn main() -> i64 { 0 }\n",
            "import name `math` conflicts with module `math`",
        ),
        (
            "import-reserved",
            "mod math;\nuse math::value as Result;\nfn main() -> i64 { 0 }\n",
            "`Result` is a reserved compiler-provided prelude enum",
        ),
    ] {
        let (directory, compile) = module_project(
            name,
            root,
            &[(
                "math.vca",
                "pub fn value() -> i64 { 1 }\npub fn other() -> i64 { 2 }\n",
            )],
        );
        let stderr = String::from_utf8_lossy(&compile.stderr);
        assert!(!compile.status.success(), "{name} compiled: {stderr}");
        assert!(stderr.contains(expected), "{name}: {stderr}");
        let _ = fs::remove_dir_all(directory);
    }
}

#[test]
fn private_and_unknown_import_targets_are_rejected() {
    for (name, root, module, expected) in [
        (
            "import-private-function",
            "mod math;\nuse math::hidden;\nfn main() -> i64 { 0 }\n",
            "fn hidden() -> i64 { 1 }\n",
            "function `math::hidden` is private",
        ),
        (
            "import-private-type",
            "mod math;\nuse math::Secret;\nfn main() -> i64 { 0 }\n",
            "struct Secret { value: i64 }\n",
            "type `math::Secret` is private",
        ),
        (
            "import-private-variant",
            "mod math;\nuse math::Kind::Red;\nfn main() -> i64 { 0 }\n",
            "enum Kind { Red, Blue }\n",
            "enum `math::Kind` is private",
        ),
        (
            "import-unknown",
            "mod math;\nuse math::missing;\nfn main() -> i64 { 0 }\n",
            "pub fn value() -> i64 { 1 }\n",
            "cannot import `math::missing`: no module or declaration has that path",
        ),
    ] {
        let (directory, compile) = module_project(name, root, &[("math.vca", module)]);
        let stderr = String::from_utf8_lossy(&compile.stderr);
        assert!(!compile.status.success(), "{name} compiled: {stderr}");
        assert!(stderr.contains(expected), "{name}: {stderr}");
        let _ = fs::remove_dir_all(directory);
    }
}

#[test]
fn import_syntax_errors_recover_across_root_and_modules() {
    let (directory, compile) = module_project(
        "import-recovery",
        "mod m;\nuse m::;\nuse broken\nfn main() -> i64 { 0 }\n",
        &[("m.vca", "use ;\npub fn value() -> i64 { 1 }\n")],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("expected name after `::` in import"),
        "{stderr}"
    );
    assert!(stderr.contains("expected `;` after import"), "{stderr}");
    assert!(stderr.contains("expected imported name"), "{stderr}");
    assert_eq!(stderr.matches("vacac:").count(), 3, "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn imported_module_alias_errors_name_the_missing_member() {
    let (directory, compile) = module_project(
        "import-missing-member",
        "mod math;\nuse math as m;\nfn main() -> i64 { let value: m::Missing = 0; 0 }\n",
        &[("math.vca", "pub struct Point { x: i64 }\n")],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success());
    assert!(
        stderr.contains("module `math` has no member `Missing`"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn module_alias_targets_are_not_reinterpreted_as_other_aliases() {
    let (directory, compile) = module_project(
        "import-alias-target-not-reexpanded",
        concat!(
            "mod app;\n",
            "mod a;\n",
            "mod q;\n",
            "fn main() -> i64 { app::run() }\n",
        ),
        &[
            (
                "app.vca",
                concat!(
                    "use q::r as a;\n",
                    "use a::b as o;\n",
                    "pub fn run() -> i64 { o::missing() }\n",
                ),
            ),
            ("a.vca", "mod b;\n"),
            ("a/b.vca", "pub fn make() -> i64 { 42 }\n"),
            ("q.vca", "mod r;\n"),
            ("q/r.vca", "pub fn make() -> i64 { 0 }\n"),
        ],
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(!compile.status.success(), "{stderr}");
    assert!(
        stderr.contains("module `a::b` has no member `missing`"),
        "{stderr}"
    );
    assert!(!stderr.contains("module `q::r`"), "{stderr}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn import_paths_resolve_relative_to_the_declaring_module() {
    let (directory, compile) = module_project(
        "import-relative-path",
        "mod outer;\nuse outer::nested::value;\nfn main() -> i64 { outer::run() + value() - 84 }\n",
        &[
            (
                "outer.vca",
                "mod nested;\nuse nested::value;\npub fn run() -> i64 { value() }\n",
            ),
            ("outer/nested.vca", "pub fn value() -> i64 { 42 }\n"),
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
