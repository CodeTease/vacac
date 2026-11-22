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
    let module_names = program
        .modules
        .iter()
        .map(|module| module.name.as_str())
        .collect::<HashSet<_>>();
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
        if let Some(previous) = symbols.get(&qualify(&declaration.module, &declaration.name))
            && previous.kind == "enum"
        {
            return Err(Diagnostic::at(
                declaration.span.into(),
                format!("type `{}` is defined more than once", declaration.name),
            ));
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
    for declaration in &program.traits {
        let qualified = qualify(&declaration.module, &declaration.name);
        if module_names.contains(qualified.as_str()) {
            return Err(Diagnostic::at(
                declaration.span.into(),
                format!(
                    "trait `{}` conflicts with a module of the same name",
                    qualified
                ),
            ));
        }
        add_symbol(
            &mut symbols,
            &declaration.name,
            &declaration.module,
            declaration.is_public,
            "trait",
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
        resolve_generic_bounds(
            &mut declaration.generic_params,
            &declaration.module,
            &symbols,
        )?;
        for variant in &mut declaration.variants {
            for ty in &mut variant.payload {
                resolve_type(ty, &declaration.module, variant.span, &symbols)?;
            }
        }
    }
    for declaration in &mut program.structs {
        declaration.name = qualify(&declaration.module, &declaration.name);
        resolve_generic_bounds(
            &mut declaration.generic_params,
            &declaration.module,
            &symbols,
        )?;
        for field in &mut declaration.fields {
            resolve_type(&mut field.ty, &declaration.module, field.span, &symbols)?;
        }
    }
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
    for declaration in &mut program.traits {
        declaration.name = qualify(&declaration.module, &declaration.name);
        let mut seen = HashSet::new();
        for function in &mut declaration.functions {
            if !seen.insert(function.name.clone()) {
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!(
                        "trait function `{}::{}` is defined more than once",
                        declaration.name, function.name
                    ),
                ));
            }
            resolve_function_signature(
                &mut function.params,
                &mut function.return_type,
                &declaration.module,
                function.span,
                &symbols,
            )?;
            if let Some(body) = &mut function.body {
                let mut resolver = Resolver::new(&declaration.module, &symbols, &enum_names);
                resolver.push_scope();
                for param in &function.params {
                    resolver.bind(&param.name);
                }
                let result = resolver.resolve_block(body, function.span);
                resolver.pop_scope();
                result?;
            }
        }
    }
    for declaration in &mut program.globals {
        let original_name = declaration.name.clone();
        declaration.name = qualify(&declaration.module, &original_name);
        if let Some(ty) = &mut declaration.ty {
            resolve_type(ty, &declaration.module, declaration.span, &symbols)?;
        }
        let mut resolver = Resolver::new(&declaration.module, &symbols, &enum_names);
        resolver.resolve_expr(&mut declaration.value)?;
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
        resolve_generic_bounds(&mut function.generic_params, &function.module, &symbols)?;
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
        let mut resolver = Resolver::new(&function.module, &symbols, &enum_names);
        resolver.push_scope();
        for param in &function.params {
            resolver.bind(&param.name);
        }
        resolver.resolve_block(&mut function.body, function.span)?;
        resolver.pop_scope();
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
    for declaration in &mut program.impls {
        declaration.trait_name = resolve_reference(
            &declaration.trait_name,
            &declaration.module,
            declaration.trait_span,
            &symbols,
        )?;
        resolve_generic_bounds(
            &mut declaration.generic_params,
            &declaration.module,
            &symbols,
        )?;
        resolve_type(
            &mut declaration.target,
            &declaration.module,
            declaration.target_span,
            &symbols,
        )?;
        for function in &mut declaration.functions {
            resolve_function_signature(
                &mut function.params,
                &mut function.return_type,
                &declaration.module,
                function.span,
                &symbols,
            )?;
            let mut resolver = Resolver::new(&declaration.module, &symbols, &enum_names);
            resolver.push_scope();
            for param in &function.params {
                resolver.bind(&param.name);
            }
            resolver.resolve_block(&mut function.body, function.span)?;
            resolver.pop_scope();
        }
    }
    Ok(())
}

fn resolve_generic_bounds(
    parameters: &mut [GenericParam],
    module: &str,
    symbols: &HashMap<String, Symbol>,
) -> Result<(), Diagnostic> {
    for parameter in parameters {
        for bound in &mut parameter.bounds {
            bound.name = resolve_reference(&bound.name, module, bound.span, symbols)?;
        }
    }
    Ok(())
}

fn resolve_function_signature(
    params: &mut [Param],
    return_type: &mut Type,
    module: &str,
    span: Span,
    symbols: &HashMap<String, Symbol>,
) -> Result<(), Diagnostic> {
    for param in params {
        resolve_type(&mut param.ty, module, param.span, symbols)?;
    }
    resolve_type(return_type, module, span, symbols)
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
    let Some(symbol) = find_symbol(name, module, symbols) else {
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

fn find_symbol<'a>(
    name: &str,
    module: &str,
    symbols: &'a HashMap<String, Symbol>,
) -> Option<&'a Symbol> {
    if name.contains("::") {
        symbols.get(name).or_else(|| {
            let local = qualify(module, name);
            symbols.get(&local)
        })
    } else {
        let local = qualify(module, name);
        symbols.get(&local).or_else(|| symbols.get(name))
    }
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

struct Resolver<'a> {
    module: &'a str,
    symbols: &'a HashMap<String, Symbol>,
    enum_names: &'a HashSet<String>,
    scopes: Vec<HashSet<String>>,
}

impl<'a> Resolver<'a> {
    fn new(
        module: &'a str,
        symbols: &'a HashMap<String, Symbol>,
        enum_names: &'a HashSet<String>,
    ) -> Self {
        Self {
            module,
            symbols,
            enum_names,
            scopes: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned());
        }
    }

    fn bind_pattern(&mut self, pattern: &crate::ast::MatchPattern) {
        use crate::ast::MatchPattern;
        match pattern {
            MatchPattern::Binding(name) => self.bind(name),
            MatchPattern::Tuple(patterns) | MatchPattern::Array(patterns) => {
                for pattern in patterns {
                    self.bind_pattern(pattern);
                }
            }
            MatchPattern::Struct { fields, .. } => {
                for (_, pattern, _) in fields {
                    self.bind_pattern(pattern);
                }
            }
            MatchPattern::Variant { bindings, .. } => {
                if let Some(bindings) = bindings {
                    for pattern in bindings {
                        self.bind_pattern(pattern);
                    }
                }
            }
            MatchPattern::Wildcard
            | MatchPattern::Bool(_)
            | MatchPattern::I64(_)
            | MatchPattern::F32(_)
            | MatchPattern::F64(_) => {}
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn resolve_block(
        &mut self,
        block: &mut crate::ast::Block,
        span: crate::ast::Span,
    ) -> Result<(), Diagnostic> {
        use crate::ast::Stmt;
        self.push_scope();
        let result = (|| -> Result<(), Diagnostic> {
            for statement in &mut block.statements {
                match statement {
                    Stmt::Let {
                        pattern, ty, value, ..
                    } => {
                        if let Some(ty) = ty {
                            resolve_type(ty, self.module, span, self.symbols)?;
                        }
                        resolve_pattern(pattern, self.module, span, self.symbols)?;
                        self.resolve_expr(value)?;
                        self.bind_pattern(pattern);
                    }
                    Stmt::Assign { value, .. } | Stmt::Expr(value) => self.resolve_expr(value)?,
                    Stmt::If {
                        condition,
                        then_block,
                        else_block,
                    } => {
                        self.resolve_expr(condition)?;
                        self.resolve_block(then_block, span)?;
                        if let Some(block) = else_block {
                            self.resolve_block(block, span)?;
                        }
                    }
                    Stmt::While { condition, body } => {
                        self.resolve_expr(condition)?;
                        self.resolve_block(body, span)?;
                    }
                    Stmt::For {
                        name,
                        start,
                        end,
                        body,
                    } => {
                        self.resolve_expr(start)?;
                        self.resolve_expr(end)?;
                        self.push_scope();
                        self.bind(name);
                        let result = self.resolve_block(body, span);
                        self.pop_scope();
                        result?;
                    }
                    Stmt::ForArray {
                        name, array, body, ..
                    } => {
                        self.resolve_expr(array)?;
                        self.push_scope();
                        self.bind(name);
                        let result = self.resolve_block(body, span);
                        self.pop_scope();
                        result?;
                    }
                    Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                        if let Some(value) = value {
                            self.resolve_expr(value)?;
                        }
                    }
                    Stmt::Continue { .. } => {}
                    Stmt::AssignPlace { target, value, .. } => {
                        self.resolve_expr(target)?;
                        self.resolve_expr(value)?;
                    }
                }
            }
            if let Some(tail) = &mut block.tail {
                self.resolve_expr(tail)?;
            }
            Ok(())
        })();
        self.pop_scope();
        result
    }

    fn resolve_expr(&mut self, expr: &mut crate::ast::Expr) -> Result<(), Diagnostic> {
        use crate::ast::Expr;
        match expr {
            Expr::Array(values, _) | Expr::Tuple(values, _) => {
                for value in values {
                    self.resolve_expr(value)?;
                }
            }
            Expr::Index { array, index, .. } => {
                self.resolve_expr(array)?;
                self.resolve_expr(index)?;
            }
            Expr::Field { value, .. } | Expr::Unary { value, .. } => self.resolve_expr(value)?,
            Expr::Struct {
                name,
                type_arguments,
                fields,
                span,
            } => {
                *name = resolve_reference(name, self.module, *span, self.symbols)?;
                for argument in type_arguments {
                    resolve_type(argument, self.module, *span, self.symbols)?;
                }
                for (_, value, _) in fields {
                    self.resolve_expr(value)?;
                }
            }
            Expr::Lambda {
                params,
                return_type,
                body,
                span,
            } => {
                for param in params.iter_mut() {
                    resolve_type(&mut param.ty, self.module, param.span, self.symbols)?;
                }
                resolve_type(return_type, self.module, *span, self.symbols)?;
                self.push_scope();
                for param in params.iter() {
                    self.bind(&param.name);
                }
                let result = self.resolve_block(body, *span);
                self.pop_scope();
                result?;
            }
            Expr::TraitCall {
                trait_name,
                self_type,
                args,
                span,
                ..
            } => {
                *trait_name = resolve_reference(trait_name, self.module, *span, self.symbols)?;
                if let Some(self_type) = self_type {
                    resolve_type(self_type, self.module, *span, self.symbols)?;
                }
                for arg in args {
                    self.resolve_expr(arg)?;
                }
            }
            Expr::CallValue { callee, args, .. } => {
                self.resolve_expr(callee)?;
                for arg in args {
                    self.resolve_expr(arg)?;
                }
            }
            Expr::Binary { left, right, .. } => {
                self.resolve_expr(left)?;
                self.resolve_expr(right)?;
            }
            Expr::Call {
                name,
                type_arguments,
                args,
                span,
            } => {
                if let Some((trait_name, function_name)) = name.rsplit_once("::") {
                    let is_callable = find_symbol(name, self.module, self.symbols)
                        .is_some_and(|symbol| symbol.kind == "function");
                    let is_trait = !is_callable
                        && find_symbol(trait_name, self.module, self.symbols)
                            .is_some_and(|symbol| symbol.kind == "trait");
                    if is_trait {
                        let resolved_trait =
                            resolve_reference(trait_name, self.module, *span, self.symbols)?;
                        if !type_arguments.is_empty() {
                            return Err(Diagnostic::at(
                                (*span).into(),
                                "trait calls do not accept turbofish arguments; use `<Type as Trait>::function(...)` to choose `Self`",
                            ));
                        }
                        let payload = std::mem::take(args);
                        *expr = Expr::TraitCall {
                            trait_name: resolved_trait,
                            self_type: None,
                            function_name: function_name.to_owned(),
                            args: payload,
                            span: *span,
                        };
                        return self.resolve_expr(expr);
                    }
                }
                let enum_constructor =
                    if let Some((enum_name, variant_name)) = name.rsplit_once("::") {
                        let resolved_enum =
                            resolve_reference(enum_name, self.module, *span, self.symbols)?;
                        let is_enum = self
                            .symbols
                            .get(&resolved_enum)
                            .is_some_and(|symbol| symbol.kind == "enum")
                            && self
                                .enum_names
                                .contains(enum_name.rsplit("::").next().unwrap_or(enum_name));
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
                    return self.resolve_expr(expr);
                }
                if !self.is_local(name)
                    && !matches!(name.as_str(), "len" | "vec_new" | "vec_push")
                    && !matches!(
                        name.as_str(),
                        "print_i64" | "print_f32" | "print_f64" | "print" | "println" | "read_i64"
                    )
                {
                    *name = resolve_reference(name, self.module, *span, self.symbols)?;
                }
                for argument in type_arguments {
                    resolve_type(argument, self.module, *span, self.symbols)?;
                }
                for arg in args {
                    self.resolve_expr(arg)?;
                }
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                self.resolve_expr(condition)?;
                self.resolve_block(then_block, *span)?;
                self.resolve_block(else_block, *span)?;
            }
            Expr::Match { value, arms, span } => {
                self.resolve_expr(value)?;
                for arm in arms {
                    resolve_pattern(&mut arm.pattern, self.module, arm.span, self.symbols)?;
                    self.push_scope();
                    self.bind_pattern(&arm.pattern);
                    let result = self.resolve_block(&mut arm.body, *span);
                    self.pop_scope();
                    result?;
                }
            }
            Expr::Loop { body, span } => self.resolve_block(body, *span)?,
            Expr::Try { value, .. } => self.resolve_expr(value)?,
            Expr::Variant {
                enum_name,
                payload,
                span,
                ..
            } => {
                *enum_name = resolve_reference(enum_name, self.module, *span, self.symbols)?;
                if let Some(payload) = payload {
                    for value in payload {
                        self.resolve_expr(value)?;
                    }
                }
            }
            Expr::Variable(name, span) if name.contains("::") => {
                if self
                    .symbols
                    .get(name)
                    .is_some_and(|symbol| symbol.kind == "function")
                {
                    *name = resolve_reference(name, self.module, *span, self.symbols)?;
                    return Ok(());
                }
                if let Some((enum_name, variant_name)) = name.rsplit_once("::") {
                    let resolved_enum =
                        resolve_reference(enum_name, self.module, *span, self.symbols)?;
                    if self
                        .symbols
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
                        return self.resolve_expr(expr);
                    }
                }
                if let Expr::Variable(name, span) = expr {
                    *name = resolve_reference(name, self.module, *span, self.symbols)?;
                }
            }
            Expr::Integer(..)
            | Expr::Float32(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::String(..)
            | Expr::Unit(..)
            | Expr::Variable(..) => {}
        }
        Ok(())
    }
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
