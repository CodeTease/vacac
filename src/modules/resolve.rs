use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Symbol {
    qualified: String,
    module: String,
    public: bool,
    kind: &'static str,
}

pub(super) fn resolve_names(program: &mut Program) -> Result<(), Diagnostic> {
    if let Some(declaration) = program
        .enums
        .iter()
        .find(|declaration| declaration.name == "Result" && declaration.span.file.is_some())
    {
        return Err(Diagnostic::at(
            declaration.span.into(),
            "`Result` is a reserved compiler-provided prelude enum",
        ));
    }
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
    for declaration in &program.globals {
        add_symbol(
            &mut symbols,
            &declaration.name,
            &declaration.module,
            declaration.is_public,
            "global",
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
    for declaration in &program.extern_functions {
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
    for declaration in &mut program.globals {
        let original_name = declaration.name.clone();
        declaration.name = qualify(&declaration.module, &original_name);
        if let Some(ty) = &mut declaration.ty {
            resolve_type(ty, &declaration.module, declaration.span, &symbols)?;
        }
        resolve_expr(
            &mut declaration.value,
            &declaration.module,
            &symbols,
            &struct_names,
            &enum_names,
        )?;
    }
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
    for function in &mut program.extern_functions {
        let original_name = function.name.clone();
        for param in &mut function.params {
            resolve_type(&mut param.ty, &function.module, param.span, &symbols)?;
        }
        resolve_type(
            &mut function.return_type,
            &function.module,
            function.span,
            &symbols,
        )?;
        function.name = qualify(&function.module, &original_name);
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
    if name == "Result" {
        return Ok("Result".into());
    }
    if name == "Result" {
        return Ok("Result".into());
    }
    let already_qualified = symbols.contains_key(name);
    let symbol = if already_qualified {
        symbols.get(name)
    } else if name.contains("::") {
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
        crate::ast::Type::Applied(name, arguments) => {
            *name = resolve_reference(name, module, span, symbols)?;
            for argument in arguments {
                resolve_type(argument, module, span, symbols)?;
            }
        }
        crate::ast::Type::Parameter(_) => {}
        crate::ast::Type::Array(element, _) | crate::ast::Type::Vector(element) => {
            resolve_type(element, module, span, symbols)?
        }
        crate::ast::Type::Reference(element, _)
        | crate::ast::Type::MutableReference(element, _)
        | crate::ast::Type::RawPointer(element, _) => resolve_type(element, module, span, symbols)?,
        crate::ast::Type::Tuple(elements) => {
            for element in elements {
                resolve_type(element, module, span, symbols)?;
            }
        }
        crate::ast::Type::Function(params, result) => {
            for param in params {
                resolve_type(param, module, span, symbols)?;
            }
            resolve_type(result, module, span, symbols)?;
        }
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
            Stmt::Let {
                pattern, ty, value, ..
            } => {
                if let Some(ty) = ty {
                    resolve_type(ty, module, span, symbols)?;
                }
                resolve_pattern(pattern, module, span, symbols)?;
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
    use crate::ast::Expr;
    match expr {
        Expr::Array(values, _) => {
            for value in values {
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::Tuple(values, _) => {
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
        Expr::Struct {
            name,
            type_arguments,
            fields,
            span,
        } => {
            *name = resolve_reference(name, module, *span, symbols)?;
            for argument in type_arguments {
                resolve_type(argument, module, *span, symbols)?;
            }
            for (_, value, _) in fields {
                resolve_expr(value, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::Lambda {
            params,
            return_type,
            body,
            span,
        } => {
            for param in params {
                resolve_type(&mut param.ty, module, param.span, symbols)?;
            }
            resolve_type(return_type, module, *span, symbols)?;
            resolve_block(body, module, *span, symbols, struct_names, enum_names)?;
        }
        Expr::CallValue { callee, args, .. } => {
            resolve_expr(callee, module, symbols, struct_names, enum_names)?;
            for arg in args {
                resolve_expr(arg, module, symbols, struct_names, enum_names)?;
            }
        }
        Expr::Binary { left, right, .. } => {
            resolve_expr(left, module, symbols, struct_names, enum_names)?;
            resolve_expr(right, module, symbols, struct_names, enum_names)?;
        }
        Expr::Call {
            name,
            type_arguments,
            args,
            span,
        } => {
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
                    type_arguments: std::mem::take(type_arguments),
                    variant_name,
                    payload: Some(payload),
                    span: *span,
                };
                return resolve_expr(expr, module, symbols, struct_names, enum_names);
            }
            if !matches!(name.as_str(), "len" | "vec_new" | "vec_push")
                && !matches!(
                    name.as_str(),
                    "print_i64" | "print_f32" | "print_f64" | "print" | "println" | "read_i64"
                )
            {
                *name = resolve_reference(name, module, *span, symbols)?;
            }
            for argument in type_arguments {
                resolve_type(argument, module, *span, symbols)?;
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
                resolve_pattern(&mut arm.pattern, module, arm.span, symbols)?;
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
        Expr::Try { value, .. } => {
            resolve_expr(value, module, symbols, struct_names, enum_names)?;
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
        Expr::Variable(name, span) if name.contains("::") => {
            if symbols
                .get(name)
                .is_some_and(|symbol| symbol.kind == "function")
            {
                *name = resolve_reference(name, module, *span, symbols)?;
                return Ok(());
            }
            if let Some((enum_name, variant_name)) = name.rsplit_once("::") {
                let resolved_enum = resolve_reference(enum_name, module, *span, symbols)?;
                if symbols
                    .get(&resolved_enum)
                    .is_some_and(|symbol| symbol.kind == "enum")
                {
                    *expr = Expr::Variant {
                        enum_name: resolved_enum,
                        type_arguments: Vec::new(),
                        variant_name: variant_name.to_owned(),
                        payload: None,
                        span: *span,
                    };
                    return resolve_expr(expr, module, symbols, struct_names, enum_names);
                }
            }
            if let Expr::Variable(name, span) = expr {
                *name = resolve_reference(name, module, *span, symbols)?;
            }
        }
        Expr::Integer(..)
        | Expr::Float32(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::String(..)
        | Expr::Variable(..) => {}
    }
    Ok(())
}

fn resolve_pattern(
    pattern: &mut crate::ast::MatchPattern,
    module: &str,
    span: crate::ast::Span,
    symbols: &HashMap<String, Symbol>,
) -> Result<(), Diagnostic> {
    use crate::ast::MatchPattern;
    match pattern {
        MatchPattern::Tuple(patterns) | MatchPattern::Array(patterns) => {
            for pattern in patterns {
                resolve_pattern(pattern, module, span, symbols)?;
            }
        }
        MatchPattern::Struct { name, fields, .. } => {
            *name = resolve_reference(name, module, span, symbols)?;
            for (_, pattern, field_span) in fields {
                resolve_pattern(pattern, module, *field_span, symbols)?;
            }
        }
        MatchPattern::Variant {
            enum_name,
            bindings,
            ..
        } => {
            *enum_name = resolve_reference(enum_name, module, span, symbols)?;
            if let Some(bindings) = bindings {
                for pattern in bindings {
                    resolve_pattern(pattern, module, span, symbols)?;
                }
            }
        }
        MatchPattern::Wildcard
        | MatchPattern::Binding(_)
        | MatchPattern::Bool(_)
        | MatchPattern::I64(_)
        | MatchPattern::F32(_)
        | MatchPattern::F64(_) => {}
    }
    Ok(())
}
