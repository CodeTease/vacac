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

/// One `use` declaration, keyed in `Scope::imports` by declaring module and
/// the local name it introduces.
#[derive(Clone)]
struct Import {
    /// Fully qualified item or module the local name stands for.
    target: String,
    kind: &'static str,
    /// `Some((enum, variant))` when the import names an enum variant directly.
    variant: Option<(String, String)>,
}

/// Name lookup state for one program: declarations, module names, and the
/// imports each module has brought into scope.
struct Scope<'a> {
    symbols: &'a HashMap<String, Symbol>,
    module_names: &'a HashSet<String>,
    imports: &'a HashMap<String, HashMap<String, Import>>,
}

impl<'a> Scope<'a> {
    fn import(&self, module: &str, name: &str) -> Option<&'a Import> {
        self.imports
            .get(module)
            .and_then(|imports| imports.get(name))
    }

    /// The enum and variant named by a direct variant import.
    fn variant_import(&self, module: &str, name: &str) -> Option<(&'a str, &'a str)> {
        self.import(module, name)
            .and_then(|import| import.variant.as_ref())
            .map(|(enum_name, variant)| (enum_name.as_str(), variant.as_str()))
    }

    /// Rewrite a leading imported name to its fully qualified target.
    ///
    /// Names that already resolve to a declaration or module are left alone, so
    /// applying this to an already-canonical name is a no-op.
    fn canonical(
        &self,
        module: &str,
        name: &str,
        span: crate::ast::Span,
    ) -> Result<Option<String>, Diagnostic> {
        let (head, rest) = match name.split_once("::") {
            Some((head, rest)) => (head, Some(rest)),
            None => (name, None),
        };
        let exact = self.symbols.contains_key(name) || self.module_names.contains(name);
        // A qualified name that already names a declaration is canonical, so
        // leave it alone. A qualified name that only names a module is a valid
        // prefix, not a declaration: let it proceed so an import alias used as
        // the prefix is rewritten to its target. But if an import already maps
        // to this exact module path, the name is a canonical import target and
        // must not be reinterpreted as another alias. The enum-constructor probe
        // treats a module prefix as "not an enum" and falls through to
        // function-reference resolution. A bare name must prefer the current
        // module's import alias over a root declaration of the same name.
        let is_import_target = self
            .imports
            .get(module)
            .is_some_and(|imports| imports.values().any(|import| import.target == name));
        if exact && rest.is_some() && (!self.module_names.contains(name) || is_import_target) {
            return Ok(None);
        }
        let Some(import) = self.import(module, head) else {
            return Ok(None);
        };
        if let Some((enum_name, variant)) = &import.variant {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "`{head}` names enum variant `{enum_name}::{variant}`, not a module or type"
                ),
            ));
        }
        Ok(Some(match rest {
            Some(rest) => format!("{}::{rest}", import.target),
            None => import.target.clone(),
        }))
    }

    fn find(&self, name: &str, module: &str) -> Option<&'a Symbol> {
        if name.contains("::") {
            self.symbols.get(name).or_else(|| {
                let local = qualify(module, name);
                self.symbols.get(&local)
            })
        } else {
            let local = qualify(module, name);
            self.symbols.get(&local).or_else(|| self.symbols.get(name))
        }
    }
}

fn collect_imports(
    program: &Program,
    symbols: &HashMap<String, Symbol>,
    module_names: &HashSet<String>,
) -> Result<HashMap<String, HashMap<String, Import>>, Diagnostic> {
    let enum_variants = program
        .enums
        .iter()
        .map(|declaration| {
            (
                qualify(&declaration.module, &declaration.name),
                declaration
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut imports: HashMap<String, HashMap<String, Import>> = HashMap::new();
    for declaration in &program.uses {
        let full = declaration.path.join("::");
        let alias = declaration
            .alias
            .clone()
            .unwrap_or_else(|| declaration.path.last().cloned().unwrap_or_default());
        let span = declaration.span;
        if let Some(reason) = reserved_import_reason(&alias) {
            return Err(Diagnostic::at(
                span.into(),
                format!("cannot import `{full}` as `{alias}`: {reason}"),
            ));
        }
        // Import paths are looked up like any other name: relative to the
        // declaring module first, then from the root.
        let mut candidates = Vec::new();
        if !declaration.module.is_empty() {
            candidates.push(qualify(&declaration.module, &full));
        }
        candidates.push(full.clone());
        let mut resolved: Option<(String, &'static str, Option<(String, String)>)> = None;
        for candidate in candidates {
            if let Some(symbol) = symbols.get(&candidate) {
                check_import_visibility(symbol, &declaration.module, &full, span)?;
                resolved = Some((candidate, symbol.kind, None));
                break;
            }
            if module_names.contains(&candidate) {
                resolved = Some((candidate, "module", None));
                break;
            }
            let variant = match candidate.rsplit_once("::") {
                Some((enum_path, variant_name))
                    if enum_variants.get(enum_path).is_some_and(|variants| {
                        variants.iter().any(|name| name == variant_name)
                    }) =>
                {
                    Some((enum_path.to_owned(), variant_name.to_owned()))
                }
                _ => None,
            };
            if let Some((enum_path, variant_name)) = variant {
                if let Some(symbol) = symbols.get(&enum_path) {
                    check_import_visibility(symbol, &declaration.module, &enum_path, span)?;
                }
                resolved = Some((candidate, "variant", Some((enum_path, variant_name))));
                break;
            }
        }
        let Some((target, kind, variant)) = resolved else {
            return Err(Diagnostic::at(
                span.into(),
                format!("cannot import `{full}`: no module or declaration has that path"),
            ));
        };
        let local = qualify(&declaration.module, &alias);
        if let Some(symbol) = symbols.get(&local) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "import name `{alias}` conflicts with {} `{local}` declared in this module",
                    symbol.kind
                ),
            ));
        }
        if module_names.contains(&local) {
            return Err(Diagnostic::at(
                span.into(),
                format!("import name `{alias}` conflicts with module `{local}`"),
            ));
        }
        let module_imports = imports.entry(declaration.module.clone()).or_default();
        if module_imports.contains_key(&alias) {
            return Err(Diagnostic::at(
                span.into(),
                format!("import name `{alias}` is imported more than once"),
            ));
        }
        module_imports.insert(
            alias,
            Import {
                target,
                kind,
                variant,
            },
        );
    }
    Ok(imports)
}

/// Reject an import target that is private to another module.
fn check_import_visibility(
    symbol: &Symbol,
    module: &str,
    imported: &str,
    span: crate::ast::Span,
) -> Result<(), Diagnostic> {
    if symbol.module == module || symbol.public {
        return Ok(());
    }
    Err(Diagnostic::at(
        span.into(),
        format!("{} `{imported}` is private", symbol.kind),
    ))
}

/// Import names that cannot be bound because the compiler already provides them.
fn reserved_import_reason(name: &str) -> Option<&'static str> {
    if name == "Result" {
        return Some("`Result` is a reserved compiler-provided prelude enum");
    }
    is_builtin(name).then_some("the name is a compiler built-in")
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "print_i64"
            | "print_f32"
            | "print_f64"
            | "print"
            | "println"
            | "read_i64"
            | "len"
            | "vec_new"
            | "vec_push"
    )
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
        .map(|module| module.name.clone())
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

    let imports = collect_imports(program, &symbols, &module_names)?;
    let scope = Scope {
        symbols: &symbols,
        module_names: &module_names,
        imports: &imports,
    };

    for declaration in &mut program.enums {
        declaration.name = qualify(&declaration.module, &declaration.name);
        resolve_generic_bounds(&mut declaration.generic_params, &declaration.module, &scope)?;
        for variant in &mut declaration.variants {
            for ty in &mut variant.payload {
                resolve_type(ty, &declaration.module, variant.span, &scope)?;
            }
        }
    }
    for declaration in &mut program.structs {
        declaration.name = qualify(&declaration.module, &declaration.name);
        resolve_generic_bounds(&mut declaration.generic_params, &declaration.module, &scope)?;
        for field in &mut declaration.fields {
            resolve_type(&mut field.ty, &declaration.module, field.span, &scope)?;
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
                &scope,
            )?;
            if let Some(body) = &mut function.body {
                let mut resolver = Resolver::new(&declaration.module, &scope, &enum_names);
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
            resolve_type(ty, &declaration.module, declaration.span, &scope)?;
        }
        let mut resolver = Resolver::new(&declaration.module, &scope, &enum_names);
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
            resolve_type(&mut param.ty, &function.module, param.span, &scope)?;
        }
        resolve_generic_bounds(&mut function.generic_params, &function.module, &scope)?;
        resolve_type(
            &mut function.return_type,
            &function.module,
            function.span,
            &scope,
        )?;
        function.name = if is_main {
            "main".into()
        } else {
            qualify(&function.module, &original_name)
        };
        let mut resolver = Resolver::new(&function.module, &scope, &enum_names);
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
            resolve_type(&mut param.ty, &function.module, param.span, &scope)?;
        }
        resolve_type(
            &mut function.return_type,
            &function.module,
            function.span,
            &scope,
        )?;
        function.name = qualify(&function.module, &original_name);
    }
    for declaration in &mut program.impls {
        declaration.trait_name = resolve_reference(
            &declaration.trait_name,
            &declaration.module,
            declaration.trait_span,
            &scope,
        )?;
        resolve_generic_bounds(&mut declaration.generic_params, &declaration.module, &scope)?;
        resolve_type(
            &mut declaration.target,
            &declaration.module,
            declaration.target_span,
            &scope,
        )?;
        for function in &mut declaration.functions {
            resolve_function_signature(
                &mut function.params,
                &mut function.return_type,
                &declaration.module,
                function.span,
                &scope,
            )?;
            let mut resolver = Resolver::new(&declaration.module, &scope, &enum_names);
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
    scope: &Scope<'_>,
) -> Result<(), Diagnostic> {
    for parameter in parameters {
        for bound in &mut parameter.bounds {
            bound.name = resolve_reference(&bound.name, module, bound.span, scope)?;
        }
    }
    Ok(())
}

fn resolve_function_signature(
    params: &mut [Param],
    return_type: &mut Type,
    module: &str,
    span: Span,
    scope: &Scope<'_>,
) -> Result<(), Diagnostic> {
    for param in params {
        resolve_type(&mut param.ty, module, param.span, scope)?;
    }
    resolve_type(return_type, module, span, scope)
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
    scope: &Scope<'_>,
) -> Result<String, Diagnostic> {
    if name == "Result" {
        return Ok("Result".into());
    }
    if let Some((enum_name, variant)) = scope.variant_import(module, name) {
        return Err(Diagnostic::at(
            span.into(),
            format!("`{name}` names enum variant `{enum_name}::{variant}` and cannot be used here"),
        ));
    }
    let canonical = scope.canonical(module, name, span)?;
    let resolved = canonical.as_deref().unwrap_or(name);
    let Some(symbol) = scope.find(resolved, module) else {
        if canonical.is_some() {
            let (head, rest) = match name.split_once("::") {
                Some((head, rest)) => (head, Some(rest)),
                None => (name, None),
            };
            if let Some(import) = scope.import(module, head) {
                if let Some(member) = rest {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("module `{}` has no member `{member}`", import.target),
                    ));
                }
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "`{name}` names {} `{}`, which is not valid here",
                        import.kind, import.target
                    ),
                ));
            }
        }
        return Ok(if resolved.contains("::") {
            resolved.into()
        } else {
            qualify(module, resolved)
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

/// Resolve a name that `Scope::canonical` already rewrote to an import target.
///
/// The target is looked up exactly so it is never reinterpreted as another
/// import alias.
fn resolve_canonical_reference(
    original: &str,
    canonical: &str,
    module: &str,
    span: crate::ast::Span,
    scope: &Scope<'_>,
) -> Result<String, Diagnostic> {
    let Some(symbol) = scope.symbols.get(canonical) else {
        let (head, rest) = match original.split_once("::") {
            Some((head, rest)) => (head, Some(rest)),
            None => (original, None),
        };
        if let Some(import) = scope.import(module, head) {
            if let Some(member) = rest {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("module `{}` has no member `{member}`", import.target),
                ));
            }
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "`{original}` names {} `{}`, which is not valid here",
                    import.kind, import.target
                ),
            ));
        }
        return Ok(if canonical.contains("::") {
            canonical.into()
        } else {
            qualify(module, canonical)
        });
    };
    if symbol.module != module && !symbol.public {
        return Err(Diagnostic::at(
            span.into(),
            format!("{} `{original}` is private", symbol.kind),
        ));
    }
    Ok(root_value_identity(
        &symbol.qualified,
        &symbol.module,
        symbol.kind,
    ))
}

/// Give an imported root value declaration an identity that cannot be mistaken
/// for a local binding.
///
/// Root declarations keep bare names, so an alias that targets one would
/// otherwise collapse to that bare name and be captured by a same-named local
/// in sema, codegen, and generic specialization. A leading `::` marks the root
/// namespace explicitly; only functions, extern functions, and globals are
/// values that can collide with locals (types and traits are not locals).
fn root_value_identity(qualified: &str, owner_module: &str, kind: &str) -> String {
    if owner_module.is_empty() && matches!(kind, "function" | "global") {
        format!("::{qualified}")
    } else {
        qualified.to_owned()
    }
}

fn resolve_type(
    ty: &mut crate::ast::Type,
    module: &str,
    span: crate::ast::Span,
    scope: &Scope<'_>,
) -> Result<(), Diagnostic> {
    match ty {
        crate::ast::Type::Named(name) => *name = resolve_reference(name, module, span, scope)?,
        crate::ast::Type::Applied(name, arguments) => {
            *name = resolve_reference(name, module, span, scope)?;
            for argument in arguments {
                resolve_type(argument, module, span, scope)?;
            }
        }
        crate::ast::Type::Parameter(_) => {}
        crate::ast::Type::Array(element, _) | crate::ast::Type::Vector(element) => {
            resolve_type(element, module, span, scope)?
        }
        crate::ast::Type::Reference(element, _)
        | crate::ast::Type::MutableReference(element, _)
        | crate::ast::Type::RawPointer(element, _) => resolve_type(element, module, span, scope)?,
        crate::ast::Type::Tuple(elements) => {
            for element in elements {
                resolve_type(element, module, span, scope)?;
            }
        }
        crate::ast::Type::Function(params, result) => {
            for param in params {
                resolve_type(param, module, span, scope)?;
            }
            resolve_type(result, module, span, scope)?;
        }
        _ => {}
    }
    Ok(())
}

struct Resolver<'a> {
    module: &'a str,
    scope: &'a Scope<'a>,
    enum_names: &'a HashSet<String>,
    scopes: Vec<HashSet<String>>,
}

impl<'a> Resolver<'a> {
    fn new(module: &'a str, scope: &'a Scope<'a>, enum_names: &'a HashSet<String>) -> Self {
        Self {
            module,
            scope,
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
            MatchPattern::ImportedVariant { bindings, .. } => {
                for pattern in bindings {
                    self.bind_pattern(pattern);
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
                            resolve_type(ty, self.module, span, self.scope)?;
                        }
                        resolve_pattern(pattern, self.module, span, self.scope, false)?;
                        self.resolve_expr(value)?;
                        self.bind_pattern(pattern);
                    }
                    Stmt::Assign { name, value, span } => {
                        if !self.is_local(name) {
                            if let Some((enum_name, variant_name)) =
                                self.scope.variant_import(self.module, name)
                            {
                                return Err(Diagnostic::at(
                                    (*span).into(),
                                    format!(
                                        "cannot assign to `{name}`: it names enum variant `{enum_name}::{variant_name}`"
                                    ),
                                ));
                            }
                            if let Some(canonical) =
                                self.scope.canonical(self.module, name, *span)?
                            {
                                *name = match self.scope.symbols.get(&canonical) {
                                    Some(symbol) if symbol.kind == "global" => root_value_identity(
                                        &symbol.qualified,
                                        &symbol.module,
                                        symbol.kind,
                                    ),
                                    _ => canonical,
                                };
                            }
                        }
                        self.resolve_expr(value)?;
                    }
                    Stmt::Expr(value) => self.resolve_expr(value)?,
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
                *name = resolve_reference(name, self.module, *span, self.scope)?;
                for argument in type_arguments {
                    resolve_type(argument, self.module, *span, self.scope)?;
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
                    resolve_type(&mut param.ty, self.module, param.span, self.scope)?;
                }
                resolve_type(return_type, self.module, *span, self.scope)?;
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
                *trait_name = resolve_reference(trait_name, self.module, *span, self.scope)?;
                if let Some(self_type) = self_type {
                    resolve_type(self_type, self.module, *span, self.scope)?;
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
                let original_name = name.clone();
                let mut canonical = None;
                // Locality is decided on the written name: an import alias is
                // not a local even if its target shares a local's name.
                let name_is_local = self.is_local(name);
                if !name_is_local {
                    if let Some((enum_name, variant_name)) =
                        self.scope.variant_import(self.module, name)
                    {
                        let (enum_name, variant_name) =
                            (enum_name.to_owned(), variant_name.to_owned());
                        if args.is_empty() {
                            return Err(Diagnostic::at(
                                (*span).into(),
                                "enum variant payload must contain at least one value",
                            ));
                        }
                        let mut payload = std::mem::take(args);
                        let mut type_arguments = std::mem::take(type_arguments);
                        for argument in &mut type_arguments {
                            resolve_type(argument, self.module, *span, self.scope)?;
                        }
                        for value in &mut payload {
                            self.resolve_expr(value)?;
                        }
                        *expr = Expr::Variant {
                            enum_name,
                            type_arguments,
                            variant_name,
                            payload: Some(payload),
                            span: *span,
                        };
                        return Ok(());
                    }
                    if !is_builtin(name) {
                        canonical = self.scope.canonical(self.module, name, *span)?;
                        if let Some(canonical) = &canonical {
                            *name = canonical.clone();
                        }
                    }
                }
                if let Some((trait_name, function_name)) = name.rsplit_once("::") {
                    let is_callable = self
                        .scope
                        .find(name, self.module)
                        .is_some_and(|symbol| symbol.kind == "function");
                    let is_trait = !is_callable
                        && self
                            .scope
                            .find(trait_name, self.module)
                            .is_some_and(|symbol| symbol.kind == "trait");
                    if is_trait {
                        let resolved_trait =
                            resolve_reference(trait_name, self.module, *span, self.scope)?;
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
                        // The prefix may resolve to a module (a valid call
                        // prefix, not an enum). Tolerate that and fall through to
                        // function-reference resolution instead of rejecting it.
                        let resolved_enum =
                            resolve_reference(enum_name, self.module, *span, self.scope).ok();
                        let is_enum = resolved_enum.as_ref().is_some_and(|resolved| {
                            self.scope
                                .symbols
                                .get(resolved)
                                .is_some_and(|symbol| symbol.kind == "enum")
                        }) && self
                            .enum_names
                            .contains(enum_name.rsplit("::").next().unwrap_or(enum_name));
                        match resolved_enum {
                            Some(resolved) if is_enum => Some((resolved, variant_name.to_owned())),
                            _ => None,
                        }
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
                if !name_is_local && !is_builtin(&original_name) {
                    *name = match &canonical {
                        Some(canonical) => resolve_canonical_reference(
                            &original_name,
                            canonical,
                            self.module,
                            *span,
                            self.scope,
                        )?,
                        None => resolve_reference(name, self.module, *span, self.scope)?,
                    };
                }
                for argument in type_arguments {
                    resolve_type(argument, self.module, *span, self.scope)?;
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
                    resolve_pattern(&mut arm.pattern, self.module, arm.span, self.scope, true)?;
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
                type_arguments,
                payload,
                span,
                ..
            } => {
                *enum_name = resolve_reference(enum_name, self.module, *span, self.scope)?;
                for argument in type_arguments {
                    resolve_type(argument, self.module, *span, self.scope)?;
                }
                if let Some(payload) = payload {
                    for value in payload {
                        self.resolve_expr(value)?;
                    }
                }
            }
            Expr::Variable(name, span) => {
                if self.is_local(name) {
                    return Ok(());
                }
                if let Some((enum_name, variant_name)) =
                    self.scope.variant_import(self.module, name)
                {
                    let (enum_name, variant_name) = (enum_name.to_owned(), variant_name.to_owned());
                    *expr = Expr::Variant {
                        enum_name,
                        type_arguments: Vec::new(),
                        variant_name,
                        payload: None,
                        span: *span,
                    };
                    return Ok(());
                }
                let canonical = self.scope.canonical(self.module, name, *span)?;
                if let Some(canonical) = &canonical {
                    *name = canonical.clone();
                }
                if !name.contains("::") {
                    // A bare name that came from an import alias targets a root
                    // declaration; give it a root-qualified identity so a
                    // same-named local cannot capture it. Other bare names are
                    // locals or same-module globals and stay as written.
                    if canonical.is_some()
                        && let Some(symbol) = self.scope.symbols.get(name.as_str())
                        && matches!(symbol.kind, "function" | "global")
                    {
                        *name = root_value_identity(&symbol.qualified, &symbol.module, symbol.kind);
                    }
                    return Ok(());
                }
                // Resolve a complete value path before probing its prefix as an enum.
                if self
                    .scope
                    .symbols
                    .get(name)
                    .is_some_and(|symbol| matches!(symbol.kind, "function" | "global"))
                {
                    *name = resolve_reference(name, self.module, *span, self.scope)?;
                    return Ok(());
                }
                if let Some((enum_name, variant_name)) = name.rsplit_once("::") {
                    let resolved_enum =
                        resolve_reference(enum_name, self.module, *span, self.scope)?;
                    if self
                        .scope
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
                        return Ok(());
                    }
                }
                if let Expr::Variable(name, span) = expr {
                    *name = resolve_reference(name, self.module, *span, self.scope)?;
                }
            }
            Expr::Integer(..)
            | Expr::Float32(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::String(..)
            | Expr::Unit(..) => {}
        }
        Ok(())
    }
}

/// Resolve type and enum names inside a pattern.
///
/// In a match arm, a bare binding whose name is a directly imported enum
/// variant becomes that variant pattern instead of a new binding.
fn resolve_pattern(
    pattern: &mut crate::ast::MatchPattern,
    module: &str,
    span: crate::ast::Span,
    scope: &Scope<'_>,
    in_match_arm: bool,
) -> Result<(), Diagnostic> {
    use crate::ast::MatchPattern;
    match pattern {
        MatchPattern::Tuple(patterns) | MatchPattern::Array(patterns) => {
            for pattern in patterns {
                resolve_pattern(pattern, module, span, scope, in_match_arm)?;
            }
        }
        MatchPattern::Struct {
            name,
            fields,
            shorthand,
            ..
        } => {
            *name = resolve_reference(name, module, span, scope)?;
            for ((_, pattern, field_span), shorthand) in fields.iter_mut().zip(shorthand) {
                resolve_pattern(
                    pattern,
                    module,
                    *field_span,
                    scope,
                    in_match_arm && !*shorthand,
                )?;
            }
        }
        MatchPattern::Variant {
            enum_name,
            bindings,
            ..
        } => {
            *enum_name = resolve_reference(enum_name, module, span, scope)?;
            if let Some(bindings) = bindings {
                for pattern in bindings {
                    resolve_pattern(pattern, module, span, scope, in_match_arm)?;
                }
            }
        }
        MatchPattern::Binding(name) => {
            if in_match_arm
                && let Some((enum_name, variant_name)) = scope.variant_import(module, name)
            {
                *pattern = MatchPattern::Variant {
                    enum_name: enum_name.to_owned(),
                    variant_name: variant_name.to_owned(),
                    bindings: None,
                };
            }
        }
        MatchPattern::ImportedVariant { name, bindings } => {
            let Some((enum_name, variant_name)) = scope.variant_import(module, name) else {
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "`{name}` does not name an imported enum variant, so it cannot take a payload pattern"
                    ),
                ));
            };
            let (enum_name, variant_name) = (enum_name.to_owned(), variant_name.to_owned());
            for binding in bindings.iter_mut() {
                resolve_pattern(binding, module, span, scope, in_match_arm)?;
            }
            *pattern = MatchPattern::Variant {
                enum_name,
                variant_name,
                bindings: Some(std::mem::take(bindings)),
            };
        }
        MatchPattern::Wildcard
        | MatchPattern::Bool(_)
        | MatchPattern::I64(_)
        | MatchPattern::F32(_)
        | MatchPattern::F64(_) => {}
    }
    Ok(())
}
