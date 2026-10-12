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
        crate::ast::Type::Array(element, _) | crate::ast::Type::Vector(element) => {
            resolve_type(element, module, span, symbols)?
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
            if !matches!(name.as_str(), "len" | "vec_new" | "vec_push")
                && !matches!(
                    name.as_str(),
                    "print_i64" | "print_f64" | "print" | "println" | "read_i64"
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
        Expr::Integer(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::String(..)
        | Expr::Variable(..) => {}
    }
    Ok(())
}
