mod codegen;
mod lexer;
mod parser;
mod sema;

use std::path::{Path, PathBuf};
use std::process::Command;

fn usage() {
    eprintln!(
        "Usage: apexc <source.apx> [-o <executable>] [--emit-obj <object.o>]\n       apexc <source.apx> --check\n       apexc <source.apx> --emit-llvm <module.ll>"
    );
}

enum OutputMode {
    Executable {
        output: PathBuf,
        object: Option<PathBuf>,
    },
    Check,
    EmitLlvm(PathBuf),
}

fn parse_args() -> Result<(PathBuf, OutputMode), String> {
    let mut args = std::env::args_os().skip(1);
    let Some(source) = args.next().map(PathBuf::from) else {
        usage();
        return Err("missing source file".into());
    };

    let mut output = PathBuf::from("a.out");
    let mut object = None;
    let mut output_was_set = false;
    let mut object_was_set = false;
    let mut mode = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
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
            Some("--check") => {
                if mode.is_some() {
                    return Err(
                        "output mode options may be specified only once; --check and --emit-llvm are mutually exclusive"
                            .into(),
                    );
                }
                mode = Some(OutputMode::Check);
            }
            Some("--emit-llvm") => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--emit-llvm requires a path")?;
                if mode.is_some() {
                    return Err(
                        "output mode options may be specified only once; --check and --emit-llvm are mutually exclusive"
                            .into(),
                    );
                }
                mode = Some(OutputMode::EmitLlvm(path));
            }
            _ => {
                usage();
                return Err(format!("unknown argument: {}", arg.to_string_lossy()));
            }
        }
    }
    let mode = match mode {
        Some(OutputMode::Check) => {
            if output_was_set || object_was_set {
                return Err("-o and --emit-obj cannot be used with --check".into());
            }
            OutputMode::Check
        }
        Some(OutputMode::EmitLlvm(path)) => {
            if output_was_set || object_was_set {
                return Err("-o and --emit-obj cannot be used with --emit-llvm".into());
            }
            OutputMode::EmitLlvm(path)
        }
        None => OutputMode::Executable { output, object },
        Some(OutputMode::Executable { .. }) => unreachable!(),
    };
    Ok((source, mode))
}

fn object_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("o");
    path
}

fn run() -> Result<(), String> {
    let (source_path, mode) = parse_args()?;
    let source = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("cannot read {}: {e}", source_path.display()))?;

    let tokens =
        lexer::lex(&source).map_err(|e| format_source_diagnostic(&source_path, &source, &e))?;
    let program = parser::Parser::new(tokens)
        .parse_program()
        .map_err(|e| format_source_diagnostic(&source_path, &source, &e))?;
    sema::check(&program).map_err(|e| format_source_diagnostic(&source_path, &source, &e))?;

    match mode {
        OutputMode::Check => {
            codegen::check(&program).map_err(|e| {
                format_backend_diagnostic(&source_path, &source, "code generation failed", &e)
            })?;
            println!("checked {}", source_path.display());
        }
        OutputMode::EmitLlvm(path) => {
            codegen::emit_llvm(&program, &path).map_err(|e| {
                format_backend_diagnostic(&source_path, &source, "LLVM IR emission failed", &e)
            })?;
            println!("wrote {}", path.display());
        }
        OutputMode::Executable { output, object } => {
            let object = object.unwrap_or_else(|| object_path(&output));
            codegen::emit_object(&program, &object).map_err(|e| {
                format_backend_diagnostic(&source_path, &source, "code generation failed", &e)
            })?;

            let status = Command::new("cc")
                .arg(&object)
                .arg("-o")
                .arg(&output)
                .status()
                .map_err(|e| format!("failed to run cc: {e}"))?;
            if !status.success() {
                return Err(format!("linker exited with status {status}"));
            }
            println!("built {}", output.display());
        }
    }
    Ok(())
}

fn format_backend_diagnostic(path: &Path, source: &str, context: &str, error: &str) -> String {
    if error.starts_with("line ") {
        format_source_diagnostic(path, source, error)
    } else {
        format!("{context}: {error}")
    }
}

fn format_source_diagnostic(path: &Path, source: &str, error: &str) -> String {
    let Some(location) = error.strip_prefix("line ") else {
        return format!("{}: {error}", path.display());
    };
    let Some((line, rest)) = location.split_once(':') else {
        return format!("{}: {error}", path.display());
    };
    let Some((column, location_tail)) = rest.split_once(':') else {
        return format!("{}: {error}", path.display());
    };
    let (Ok(line), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>()) else {
        return format!("{}: {error}", path.display());
    };
    let (end_column, message) = if let Some((end_column, message)) = location_tail.split_once(": ")
    {
        if let Ok(end_column) = end_column.parse::<usize>() {
            (end_column, message)
        } else {
            return format!("{}: {error}", path.display());
        }
    } else if let Some(message) = location_tail.strip_prefix(' ') {
        (column.saturating_add(1), message)
    } else {
        return format!("{}: {error}", path.display());
    };

    let mut diagnostic = format!("{}:{line}:{column}: {message}", path.display());
    if let Some(source_line) = source.lines().nth(line.saturating_sub(1)) {
        let gutter_width = line.to_string().len();
        let marker = format!(
            "{}{}",
            " ".repeat(column.saturating_sub(1)),
            "^".repeat(end_column.saturating_sub(column).max(1))
        );
        diagnostic.push_str(&format!(
            "\n {line:>gutter_width$} | {source_line}\n {:>gutter_width$} | {marker}",
            ""
        ));
    }
    diagnostic
}

fn main() {
    if let Err(error) = run() {
        eprintln!("apexc: {error}");
        std::process::exit(1);
    }
}
