mod aggregate_validation;
mod ast;
mod codegen;
mod diagnostic;
mod lexer;
mod modules;
mod parser;
mod sema;

use diagnostic::{Diagnostic, DiagnosticSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const HELP: &str = "vacac — compile Vaca source files

Usage:
  vacac <source.vca> [-o <executable>] [--emit-obj <object.o>]
  vacac <source.vca> --run
  vacac <source.vca> --check
  vacac <source.vca> --emit-llvm <module.ll>

Options:
  -o <path>            Set executable output path (default: a.out)
  --emit-obj <path>    Set object-file output path
  --link-object <path> Add an object file to the linker (repeatable)
  --link-search <dir>  Add a native library search directory (repeatable)
  --link-lib <name>    Link a native library (repeatable; e.g. m)
  --run                Build and run the program without keeping build artifacts
  --check              Type-check and verify without writing build artifacts
  --emit-llvm <path>   Write verified LLVM IR to a file
  --json               Output diagnostics as JSON on errors
  -q, --quiet          Suppress successful compiler status messages
  -h, --help           Show this help
  -V, --version        Show version information
";

enum OutputMode {
    Executable {
        output: PathBuf,
        object: Option<PathBuf>,
    },
    Run,
    Check,
    EmitLlvm(PathBuf),
}

struct Options {
    source: PathBuf,
    mode: OutputMode,
    quiet: bool,
    link_objects: Vec<PathBuf>,
    link_search: Vec<PathBuf>,
    link_libs: Vec<OsString>,
}

enum ParseArgs {
    Help,
    Version,
    Options(Options),
}

fn parse_args() -> Result<ParseArgs, String> {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Err("missing source file (try --help)".into());
    };
    if first == "--help" || first == "-h" {
        return Ok(ParseArgs::Help);
    }
    if first == "--version" || first == "-V" {
        return Ok(ParseArgs::Version);
    }
    let source = PathBuf::from(first);

    let mut output = PathBuf::from("a.out");
    let mut object = None;
    let mut output_was_set = false;
    let mut object_was_set = false;
    let mut link_objects = Vec::new();
    let mut link_search = Vec::new();
    let mut link_libs = Vec::new();
    let mut mode: Option<OutputMode> = None;
    let mut quiet = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h") | Some("--help") => return Ok(ParseArgs::Help),
            Some("-V") | Some("--version") => return Ok(ParseArgs::Version),
            Some("-o") => {
                output = args.next().map(PathBuf::from).ok_or("-o requires a path")?;
                output_was_set = true;
            }
            Some("--emit-obj") => {
                if object_was_set {
                    return Err("--emit-obj may be specified only once".into());
                }
                object = Some(
                    args.next()
                        .map(PathBuf::from)
                        .ok_or("--emit-obj requires a path")?,
                );
                object_was_set = true;
            }
            Some("--link-object") => link_objects.push(
                args.next()
                    .map(PathBuf::from)
                    .ok_or("--link-object requires a path")?,
            ),
            Some("--link-search") => link_search.push(
                args.next()
                    .map(PathBuf::from)
                    .ok_or("--link-search requires a directory")?,
            ),
            Some("--link-lib") => {
                link_libs.push(args.next().ok_or("--link-lib requires a library name")?)
            }
            Some("--check") => set_mode(&mut mode, OutputMode::Check)?,
            Some("--run") => set_mode(&mut mode, OutputMode::Run)?,
            Some("--emit-llvm") => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--emit-llvm requires a path")?;
                set_mode(&mut mode, OutputMode::EmitLlvm(path))?;
            }
            Some("--quiet") | Some("-q") => quiet = true,
            Some("--json") => {}
            _ => {
                return Err(format!(
                    "unknown argument: {} (try --help)",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    let mode = match mode {
        Some(OutputMode::Check) => {
            if !link_objects.is_empty() || !link_search.is_empty() || !link_libs.is_empty() {
                return Err("linker inputs cannot be used with --check".into());
            }
            if output_was_set || object_was_set {
                return Err("-o and --emit-obj cannot be used with --check".into());
            }
            OutputMode::Check
        }
        Some(OutputMode::EmitLlvm(path)) => {
            if !link_objects.is_empty() || !link_search.is_empty() || !link_libs.is_empty() {
                return Err("linker inputs cannot be used with --emit-llvm".into());
            }
            if output_was_set || object_was_set {
                return Err("-o and --emit-obj cannot be used with --emit-llvm".into());
            }
            OutputMode::EmitLlvm(path)
        }
        Some(OutputMode::Run) => {
            if output_was_set || object_was_set {
                return Err("-o and --emit-obj cannot be used with --run".into());
            }
            OutputMode::Run
        }
        None => OutputMode::Executable { output, object },
        Some(OutputMode::Executable { .. }) => unreachable!(),
    };
    Ok(ParseArgs::Options(Options {
        source,
        mode,
        quiet,
        link_objects,
        link_search,
        link_libs,
    }))
}

fn set_mode(mode: &mut Option<OutputMode>, new_mode: OutputMode) -> Result<(), String> {
    if mode.is_some() {
        return Err(
            "output mode options may be specified only once; --check, --run, and --emit-llvm are mutually exclusive"
                .into(),
        );
    }
    *mode = Some(new_mode);
    Ok(())
}

fn object_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("o");
    path
}

fn run() -> Result<i32, DiagnosticSet> {
    let options = match parse_args()? {
        ParseArgs::Help => {
            print!("{HELP}");
            return Ok(0);
        }
        ParseArgs::Version => {
            println!("vacac {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        ParseArgs::Options(options) => options,
    };
    let Options {
        source: source_path,
        mode,
        quiet,
        link_objects,
        link_search,
        link_libs,
        ..
    } = options;
    let mut program = modules::load(&source_path)?;
    let file = source_path.display().to_string();
    sema::check(&mut program).map_err(|e| {
        if let Some(source_id) = e.source_id {
            e.with_source_id(
                source_id,
                program.source_files[source_id].clone(),
                &program.source_texts[source_id],
            )
        } else {
            e.with_file(file.clone())
        }
    })?;

    match mode {
        OutputMode::Check => {
            codegen::check(&program).map_err(|e| {
                backend_diagnostic(&program, &source_path, "code generation failed", e)
            })?;
            if !quiet {
                println!("checked {}", source_path.display());
            }
        }
        OutputMode::EmitLlvm(path) => {
            codegen::emit_llvm(&program, &path).map_err(|e| {
                backend_diagnostic(&program, &source_path, "LLVM IR emission failed", e)
            })?;
            if !quiet {
                println!("wrote {}", path.display());
            }
        }
        OutputMode::Executable { output, object } => {
            let object = object.unwrap_or_else(|| object_path(&output));
            codegen::emit_object(&program, &object).map_err(|e| {
                backend_diagnostic(&program, &source_path, "code generation failed", e)
            })?;
            let status = link_object(&object, &output, &link_objects, &link_search, &link_libs)?;
            if !status.success() {
                return Err(Diagnostic::new(format!("linker exited with status {status}")).into());
            }
            if !quiet {
                println!("built {}", output.display());
            }
        }
        OutputMode::Run => {
            let temp = TempBuildDir::new()?;
            let executable = temp.path().join("program");
            let object = temp.path().join("program.o");
            codegen::emit_object(&program, &object).map_err(|e| {
                backend_diagnostic(&program, &source_path, "code generation failed", e)
            })?;
            let status = link_object(
                &object,
                &executable,
                &link_objects,
                &link_search,
                &link_libs,
            )?;
            if !status.success() {
                return Err(Diagnostic::new(format!("linker exited with status {status}")).into());
            }
            let status = Command::new(&executable)
                .status()
                .map_err(|e| Diagnostic::new(format!("failed to run program: {e}")))?;
            return Ok(status.code().unwrap_or(1));
        }
    }
    Ok(0)
}

fn link_object(
    object: &Path,
    output: &Path,
    link_objects: &[PathBuf],
    link_search: &[PathBuf],
    link_libs: &[OsString],
) -> Result<std::process::ExitStatus, Diagnostic> {
    let mut command = Command::new("cc");
    command.arg(object).args(link_objects).arg("-o").arg(output);
    for path in link_search {
        command.arg("-L").arg(path);
    }
    for library in link_libs {
        let mut argument = OsString::from("-l");
        argument.push(library);
        command.arg(argument);
    }
    command
        .arg("-lm")
        .status()
        .map_err(|e| Diagnostic::new(format!("failed to run cc: {e}")))
}

struct TempBuildDir(PathBuf);

impl TempBuildDir {
    fn new() -> Result<Self, Diagnostic> {
        let base = std::env::temp_dir();
        for attempt in 0..100 {
            let path = base.join(format!("vacac-run-{}-{attempt}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(Diagnostic::new(format!(
                        "cannot create temporary build directory: {error}"
                    )));
                }
            }
        }
        Err(Diagnostic::new(
            "cannot create unique temporary build directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBuildDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn backend_diagnostic(
    program: &parser::Program,
    root_path: &Path,
    context: &str,
    mut error: Diagnostic,
) -> Diagnostic {
    if let Some(source_id) = error
        .span
        .and_then(|span| span.file)
        .filter(|source_id| *source_id < program.source_files.len())
    {
        error = error.with_source_id(
            source_id,
            program.source_files[source_id].clone(),
            &program.source_texts[source_id],
        );
    } else if error.span.is_some() && !program.source_files.is_empty() {
        error = error.with_source_id(0, program.source_files[0].clone(), &program.source_texts[0]);
    } else if error.span.is_none() {
        error.file = Some(root_path.display().to_string());
        error.message = format!("{context}: {}", error.message);
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_diagnostics_keep_the_originating_module_file_and_excerpt() {
        let program = parser::Program {
            enums: Vec::new(),
            structs: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            extern_functions: Vec::new(),
            modules: Vec::new(),
            source_files: vec!["root.vca".into(), "nested/child.vca".into()],
            source_texts: vec![
                "fn main() -> i64 { 0 }\n".into(),
                "pub fn value() { missing }\n".into(),
            ],
            functions_are_qualified: true,
            lambda_captures: std::collections::HashMap::new(),
        };
        let diagnostic = Diagnostic::at(
            diagnostic::SourceSpan {
                line: 1,
                column: 18,
                end_column: 25,
                file: Some(1),
            },
            "undefined variable `missing`",
        );

        let diagnostic = backend_diagnostic(
            &program,
            Path::new("root.vca"),
            "code generation failed",
            diagnostic,
        );

        assert_eq!(diagnostic.file.as_deref(), Some("nested/child.vca"));
        assert_eq!(
            diagnostic.source_line.as_deref(),
            Some("pub fn value() { missing }")
        );
        assert!(diagnostic.render_human().contains("nested/child.vca:1:18"));
    }
}

fn main() -> ExitCode {
    let json_diagnostics = std::env::args_os().any(|arg| arg == "--json");
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(errors) => {
            if json_diagnostics {
                for error in errors.diagnostics {
                    eprintln!("{}", error.render_json());
                }
            } else {
                for error in errors.diagnostics {
                    eprintln!("vacac: {}", error.render_human());
                }
            }
            ExitCode::from(1)
        }
    }
}
