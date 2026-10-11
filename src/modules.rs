use crate::ast::Program;
use crate::diagnostic::Diagnostic;
use crate::{lexer, parser};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) fn load(root: &Path) -> Result<Program, Diagnostic> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        Diagnostic::new(format!("cannot read source file: {error}"))
            .with_file(root.display().to_string())
    })?;
    let mut loader = Loader {
        active: HashSet::new(),
        loaded: HashMap::new(),
        program: Program {
            enums: Vec::new(),
            structs: Vec::new(),
            functions: Vec::new(),
            modules: Vec::new(),
            source_files: Vec::new(),
            source_texts: Vec::new(),
            functions_are_qualified: false,
        },
    };
    loader.visit(&root, String::new())?;
    loader.program.modules.clear();
    resolve_names(&mut loader.program)?;
    loader.program.functions_are_qualified = true;
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
        self.program.functions.append(&mut parsed.functions);
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
    let flat = directory.join(format!("{child_module}.apx"));
    if flat.is_file() {
        flat
    } else {
        let fallback = directory.join(child_module).join("mod.apx");
        if fallback.is_file() { fallback } else { flat }
    }
}

#[derive(Clone)]
struct Symbol {
    qualified: String,
    module: String,
    public: bool,
    kind: &'static str,
}

fn resolve_names(program: &mut Program) -> Result<(), Diagnostic> {
    let mut symbols = HashMap::<String, Symbol>::new();
    for declaration in &program.enums {
        add_symbol(
            &mut symbols,
            &declaration.name,
            &declaration.module,
            declaration.is_public,
            "enum",
            declaration.span,
        )?;
    }
    for declaration in &program.structs {
        if let Some(previous) = symbols.get(&qualify(&declaration.module, &declaration.name)) {
            if previous.kind == "enum" {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("type `{}` is defined more than once", declaration.name),
                ));
            }
        }
    }
    for declaration in &program.structs {
        add_symbol(
            &mut symbols,
            &declaration.name,
            &declaration.module,
            declaration.is_public,
            "type",
            declaration.span,
        )?;
    }
    for declaration in &program.functions {
        add_symbol(
            &mut symbols,
            &declaration.name,
            &declaration.module,
            declaration.is_public,
            "function",
            declaration.span,
        )?;
    }

    for declaration in &mut program.enums {
        declaration.name = qualify(&declaration.module, &declaration.name);
        for variant in &mut declaration.variants {
            for ty in &mut variant.payload {
                resolve_type(ty, &declaration.module, variant.span, &symbols)?;
            }
        }
    }
    for declaration in &mut program.structs {
        declaration.name = qualify(&declaration.module, &declaration.name);
        for field in &mut declaration.fields {
            resolve_type(&mut field.ty, &declaration.module, field.span, &symbols)?;
        }
    }
    let struct_names = program
        .structs
        .iter()
        .map(|declaration| {
            declaration
                .name
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<HashSet<_>>();
    let enum_names = program
        .enums
        .iter()
        .map(|declaration| {
            declaration
                .name
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<HashSet<_>>();
    for function in &mut program.functions {
        let original_name = function.name.clone();
        let is_main = function.module.is_empty() && original_name == "main";
        if original_name == "main" && !is_main {
            return Err(Diagnostic::at(
                function.span.into(),
                "`main` may only be defined in the root source file",
            ));
        }
        for param in &mut function.params {
            resolve_type(&mut param.ty, &function.module, param.span, &symbols)?;
        }
        resolve_type(
            &mut function.return_type,
            &function.module,
            function.span,
            &symbols,
        )?;
        function.name = if is_main {
            "main".into()
        } else {
            qualify(&function.module, &original_name)
        };
        resolve_block(
            &mut function.body,
            &function.module,
            function.span,
            &symbols,
            &struct_names,
            &enum_names,
        )?;
    }
    Ok(())
}

fn add_symbol(
    symbols: &mut HashMap<String, Symbol>,
    name: &str,
    module: &str,
    public: bool,
    kind: &'static str,
    span: crate::ast::Span,
) -> Result<(), Diagnostic> {
    let qualified = qualify(module, name);
    if symbols.contains_key(&qualified) {
        return Err(Diagnostic::at(
            span.into(),
            format!("{kind} `{qualified}` is defined more than once"),
        ));
    }
    symbols.insert(
        qualified.clone(),
        Symbol {
            qualified,
            module: module.into(),
            public,
            kind,
        },
    );
    Ok(())
}

fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.into()
    } else {
        format!("{module}::{name}")
    }
}

fn resolve_reference(
    name: &str,
    module: &str,
    span: crate::ast::Span,
    symbols: &HashMap<String, Symbol>,
) -> Result<String, Diagnostic> {
    let symbol = if name.contains("::") {
        symbols.get(name).or_else(|| {
            let local = qualify(module, name);
            symbols.get(&local)
        })
    } else {
        let local = qualify(module, name);
        if let Some(symbol) = symbols.get(&local) {
            Some(symbol)
        } else {
            None
        }
    };
    let Some(symbol) = symbol else {
        return Ok(if name.contains("::") {
            name.into()
        } else {
            qualify(module, name)
        });
    };
    if symbol.module != module && !symbol.public {
        return Err(Diagnostic::at(
            span.into(),
            format!("{} `{}` is private", symbol.kind, name),
        ));
    }
    Ok(symbol.qualified.clone())
}

fn resolve_type(
    ty: &mut crate::ast::Type,
    module: &str,
    span: crate::ast::Span,
    symbols: &HashMap<String, Symbol>,
) -> Result<(), Diagnostic> {
    match ty {
        crate::ast::Type::Named(name) => *name = resolve_reference(name, module, span, symbols)?,
        crate::ast::Type::Array(element, _) => resolve_type(element, module, span, symbols)?,
        _ => {}
    }
    Ok(())
}

fn resolve_block(
    block: &mut crate::ast::Block,
    module: &str,
    span: crate::ast::Span,
    symbols: &HashMap<String, Symbol>,
    struct_names: &HashSet<String>,
    enum_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    use crate::ast::Stmt;
    for statement in &mut block.statements {
        match statement {
            Stmt::Let { ty, value, .. } => {
                if let Some(ty) = ty {
                    resolve_type(ty, module, span, symbols)?;
                }
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
            Stmt::Assign { value, .. } | Stmt::Expr(value) => {
                resolve_expr(value, module, symbols, struct_names, enum_names)?
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                resolve_expr(condition, module, symbols, struct_names, enum_names)?;
                resolve_block(then_block, module, span, symbols, struct_names, enum_names)?;
                if let Some(block) = else_block {
                    resolve_block(block, module, span, symbols, struct_names, enum_names)?;
                }
            }
            Stmt::While { condition, body } => {
                resolve_expr(condition, module, symbols, struct_names, enum_names)?;
                resolve_block(body, module, span, symbols, struct_names, enum_names)?;
            }
            Stmt::For {
                start, end, body, ..
            } => {
                resolve_expr(start, module, symbols, struct_names, enum_names)?;
                resolve_expr(end, module, symbols, struct_names, enum_names)?;
                resolve_block(body, module, span, symbols, struct_names, enum_names)?;
            }
            Stmt::ForArray { array, body, .. } => {
                resolve_expr(array, module, symbols, struct_names, enum_names)?;
                resolve_block(body, module, span, symbols, struct_names, enum_names)?;
            }
            Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    resolve_expr(value, module, symbols, struct_names, enum_names)?;
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::AssignPlace { target, value, .. } => {
                resolve_expr(target, module, symbols, struct_names, enum_names)?;
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
        }
    }
    if let Some(tail) = &mut block.tail {
        resolve_expr(tail, module, symbols, struct_names, enum_names)?;
    }
    Ok(())
}

fn resolve_expr(
    expr: &mut crate::ast::Expr,
    module: &str,
    symbols: &HashMap<String, Symbol>,
    struct_names: &HashSet<String>,
    enum_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    use crate::ast::{Expr, MatchPattern};
    match expr {
        Expr::Array(values, _) => {
            for value in values {
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::Index { array, index, .. } => {
            resolve_expr(array, module, symbols, struct_names, enum_names)?;
            resolve_expr(index, module, symbols, struct_names, enum_names)?;
        }
        Expr::Field { value, .. } | Expr::Unary { value, .. } => {
            resolve_expr(value, module, symbols, struct_names, enum_names)?
        }
        Expr::Struct { name, fields, span } => {
            *name = resolve_reference(name, module, *span, symbols)?;
            for (_, value, _) in fields {
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::Binary { left, right, .. } => {
            resolve_expr(left, module, symbols, struct_names, enum_names)?;
            resolve_expr(right, module, symbols, struct_names, enum_names)?;
        }
        Expr::Call { name, args, span } => {
            let enum_constructor = if let Some((enum_name, variant_name)) = name.rsplit_once("::") {
                let resolved_enum = resolve_reference(enum_name, module, *span, symbols)?;
                let is_enum = symbols
                    .get(&resolved_enum)
                    .is_some_and(|symbol| symbol.kind == "enum")
                    && enum_names.contains(enum_name.rsplit("::").next().unwrap_or(enum_name));
                is_enum.then_some((resolved_enum, variant_name.to_owned()))
            } else {
                None
            };
            if let Some((enum_name, variant_name)) = enum_constructor {
                if args.is_empty() {
                    return Err(Diagnostic::at(
                        (*span).into(),
                        "enum variant payload must contain at least one value",
                    ));
                }
                let payload = std::mem::take(args);
                *expr = Expr::Variant {
                    enum_name,
                    variant_name,
                    payload: Some(payload),
                    span: *span,
                };
                return resolve_expr(expr, module, symbols, struct_names, enum_names);
            }
            if name != "len"
                && !matches!(
                    name.as_str(),
                    "print_i64" | "print" | "println" | "read_i64"
                )
            {
                *name = resolve_reference(name, module, *span, symbols)?;
            }
            for arg in args {
                resolve_expr(arg, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::If {
            condition,
            then_block,
            else_block,
            span,
        } => {
            resolve_expr(condition, module, symbols, struct_names, enum_names)?;
            resolve_block(then_block, module, *span, symbols, struct_names, enum_names)?;
            resolve_block(else_block, module, *span, symbols, struct_names, enum_names)?;
        }
        Expr::Match { value, arms, span } => {
            resolve_expr(value, module, symbols, struct_names, enum_names)?;
            for arm in arms {
                if let MatchPattern::Variant { enum_name, .. } = &mut arm.pattern {
                    *enum_name = resolve_reference(enum_name, module, arm.span, symbols)?;
                }
                resolve_block(
                    &mut arm.body,
                    module,
                    *span,
                    symbols,
                    struct_names,
                    enum_names,
                )?;
            }
        }
        Expr::Loop { body, span } => {
            resolve_block(body, module, *span, symbols, struct_names, enum_names)?
        }
        Expr::Variant {
            enum_name,
            payload,
            span,
            ..
        } => {
            *enum_name = resolve_reference(enum_name, module, *span, symbols)?;
            if let Some(payload) = payload {
                for value in payload {
                    resolve_expr(value, module, symbols, struct_names, enum_names)?;
                }
            }
        }
        Expr::Integer(..) | Expr::Bool(..) | Expr::String(..) | Expr::Variable(..) => {}
    }
    Ok(())
}
