use crate::ast::{EnumDecl, EnumVariant, Program, Span, Type};
use crate::diagnostic::Diagnostic;
use crate::{lexer, parser};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(super) fn load(root: &Path) -> Result<Program, Diagnostic> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        Diagnostic::new(format!("cannot read source file: {error}"))
            .with_file(root.display().to_string())
    })?;
    let result_span = Span {
        line: 1,
        column: 1,
        end_column: 1,
        file: None,
    };
    let mut loader = Loader {
        active: HashSet::new(),
        loaded: HashMap::new(),
        program: Program {
            enums: vec![EnumDecl {
                name: "Result".into(),
                generic_params: vec!["T".into(), "E".into()],
                variants: vec![
                    EnumVariant {
                        name: "Ok".into(),
                        payload: vec![Type::Parameter("T".into())],
                        span: result_span,
                    },
                    EnumVariant {
                        name: "Err".into(),
                        payload: vec![Type::Parameter("E".into())],
                        span: result_span,
                    },
                ],
                span: result_span,
                is_public: true,
                module: String::new(),
            }],
            structs: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            extern_functions: Vec::new(),
            modules: Vec::new(),
            source_files: Vec::new(),
            source_texts: Vec::new(),
            functions_are_qualified: false,
            lambda_captures: HashMap::new(),
        },
    };
    loader.visit(&root, String::new())?;
    Ok(loader.program)
}

struct Loader {
    active: HashSet<PathBuf>,
    loaded: HashMap<PathBuf, String>,
    program: Program,
}

impl Loader {
    fn visit(&mut self, path: &Path, module: String) -> Result<(), Diagnostic> {
        let path = std::fs::canonicalize(path).map_err(|error| {
            Diagnostic::new(format!("cannot load module source: {error}"))
                .with_file(path.display().to_string())
        })?;
        if self.active.contains(&path) {
            return Err(Diagnostic::new(format!(
                "cyclic module declaration involving `{}`",
                path.display()
            ))
            .with_file(path.display().to_string()));
        }
        if let Some(first_module) = self.loaded.get(&path) {
            return Err(Diagnostic::new(format!(
                "source file is declared more than once as module `{first_module}` and `{module}`"
            ))
            .with_file(path.display().to_string()));
        }
        self.active.insert(path.clone());
        self.loaded.insert(path.clone(), module.clone());

        let source = std::fs::read_to_string(&path).map_err(|error| {
            Diagnostic::new(format!("cannot read source file: {error}"))
                .with_file(path.display().to_string())
        })?;
        let source_id = self.program.source_files.len();
        let filename = path.display().to_string();
        let tokens = lexer::lex_named(&source, Some(source_id))
            .map_err(|error| error.with_source_id(source_id, filename.clone(), &source))?;
        let mut parsed = parser::Parser::new_in_module(tokens, module.clone())
            .parse_program()
            .map_err(|error| error.with_source_id(source_id, filename.clone(), &source))?;
        self.program.source_files.push(filename);
        self.program.source_texts.push(source);

        let declarations = std::mem::take(&mut parsed.modules);
        self.program.enums.append(&mut parsed.enums);
        self.program.structs.append(&mut parsed.structs);
        self.program.globals.append(&mut parsed.globals);
        self.program.functions.append(&mut parsed.functions);
        self.program
            .extern_functions
            .append(&mut parsed.extern_functions);
        for declaration in declarations {
            if declaration.name == "" || declaration.name.contains("::") {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    "module name must be a single identifier",
                ));
            }
            let child_module = if module.is_empty() {
                declaration.name.clone()
            } else {
                format!("{module}::{}", declaration.name)
            };
            let child = module_file_path(&path, &module, &declaration.name);
            self.visit(&child, child_module)?;
        }
        self.active.remove(&path);
        Ok(())
    }
}

fn module_file_path(parent: &Path, current_module: &str, child_module: &str) -> PathBuf {
    let containing_directory = parent.parent().unwrap_or_else(|| Path::new("."));
    let current_name = current_module.rsplit("::").next().unwrap_or_default();
    let directory = if !current_name.is_empty()
        && parent.file_stem().and_then(|stem| stem.to_str()) == Some(current_name)
    {
        containing_directory.join(current_name)
    } else {
        containing_directory.to_path_buf()
    };
    let flat = directory.join(format!("{child_module}.vca"));
    if flat.is_file() {
        flat
    } else {
        let fallback = directory.join(child_module).join("mod.vca");
        if fallback.is_file() { fallback } else { flat }
    }
}
