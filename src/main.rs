mod ast;
mod codegen;
mod lexer;
mod parser;
mod sema;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const HELP: &str = "apexc — compile Apex source files

Usage:
  apexc <source.apx> [-o <executable>] [--emit-obj <object.o>]
  apexc <source.apx> --run
  apexc <source.apx> --check
  apexc <source.apx> --emit-llvm <module.ll>

Options:
  -o <path>            Set executable output path (default: a.out)
  --emit-obj <path>    Set object-file output path
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

fn run() -> Result<i32, String> {
    let options = match parse_args()? {
        ParseArgs::Help => {
            print!("{HELP}");
            return Ok(0);
        }
        ParseArgs::Version => {
            println!("apexc {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        ParseArgs::Options(options) => options,
    };
    let Options {
        source: source_path,
        mode,
        quiet,
        ..
    } = options;
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
            if !quiet {
                println!("checked {}", source_path.display());
            }
        }
        OutputMode::EmitLlvm(path) => {
            codegen::emit_llvm(&program, &path).map_err(|e| {
                format_backend_diagnostic(&source_path, &source, "LLVM IR emission failed", &e)
            })?;
            if !quiet {
                println!("wrote {}", path.display());
            }
        }
        OutputMode::Executable { output, object } => {
            let object = object.unwrap_or_else(|| object_path(&output));
            codegen::emit_object(&program, &object).map_err(|e| {
                format_backend_diagnostic(&source_path, &source, "code generation failed", &e)
            })?;
            let status = link_object(&object, &output)?;
            if !status.success() {
                return Err(format!("linker exited with status {status}"));
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
                format_backend_diagnostic(&source_path, &source, "code generation failed", &e)
            })?;
            let status = link_object(&object, &executable)?;
            if !status.success() {
                return Err(format!("linker exited with status {status}"));
            }
            let status = Command::new(&executable)
                .status()
                .map_err(|e| format!("failed to run program: {e}"))?;
            return Ok(status.code().unwrap_or(1));
        }
    }
    Ok(0)
}

fn link_object(object: &Path, output: &Path) -> Result<std::process::ExitStatus, String> {
    Command::new("cc")
        .arg(object)
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|e| format!("failed to run cc: {e}"))
}

struct TempBuildDir(PathBuf);

impl TempBuildDir {
    fn new() -> Result<Self, String> {
        let base = std::env::temp_dir();
        for attempt in 0..100 {
            let path = base.join(format!("apexc-run-{}-{attempt}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("cannot create temporary build directory: {error}"));
                }
            }
        }
        Err("cannot create unique temporary build directory".into())
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

fn main() -> ExitCode {
    let json_diagnostics = std::env::args_os().any(|arg| arg == "--json");
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            if json_diagnostics {
                eprintln!("{}", diagnostic_json(&error));
            } else {
                eprintln!("apexc: {error}");
            }
            ExitCode::from(1)
        }
    }
}

fn diagnostic_json(error: &str) -> String {
    let (file, line, column, message) = parse_diagnostic(error);
    format!(
        "{{\"severity\":\"error\",\"file\":{},\"line\":{},\"column\":{},\"message\":{}}}",
        file.map_or_else(|| "null".to_string(), json_string),
        line.map_or("null".to_string(), |n| n.to_string()),
        column.map_or("null".to_string(), |n| n.to_string()),
        json_string(message)
    )
}

fn parse_diagnostic(error: &str) -> (Option<&str>, Option<usize>, Option<usize>, &str) {
    let mut message = error;
    if let Some(rest) = message.strip_prefix("apexc: ") {
        message = rest;
    }
    let Some((location, rest)) = message.split_once(": ") else {
        return (None, None, None, message);
    };
    let mut parts = location.rsplitn(3, ':');
    let column_part = parts.next().unwrap_or_default();
    let line_part = parts.next().unwrap_or_default();
    let file = parts.next();
    if let (Some(file), Ok(line), Ok(column)) = (
        file,
        line_part.parse::<usize>(),
        column_part.parse::<usize>(),
    ) {
        return (
            Some(file),
            Some(line),
            Some(column),
            rest.split_once('\n').map_or(rest, |(message, _)| message),
        );
    }
    (None, None, None, message)
}

fn json_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 2);
    output.push('"');
    for character in input.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}
