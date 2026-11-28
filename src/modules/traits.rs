use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct TraitInfo {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) module: String,
    pub(crate) generic_params: Vec<GenericParam>,
    pub(crate) functions: Vec<TraitFunction>,
    pub(crate) is_public: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ImplInfo {
    pub(crate) target: Type,
    pub(crate) trait_arguments: Vec<Type>,
    pub(crate) generic_params: Vec<GenericParam>,
    pub(crate) methods: HashMap<String, String>,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TraitRegistry {
    pub(crate) traits: Vec<TraitInfo>,
    pub(crate) impls: Vec<ImplInfo>,
    pub(crate) by_trait: Vec<Vec<usize>>,
    trait_names: HashMap<String, usize>,
}

impl TraitRegistry {
    pub(crate) fn trait_by_name(&self, name: &str) -> Option<&TraitInfo> {
        self.trait_names.get(name).map(|id| &self.traits[*id])
    }

    pub(crate) fn impls_for(&self, trait_id: usize) -> &[usize] {
        &self.by_trait[trait_id]
    }

    pub(crate) fn impl_info(&self, impl_id: usize) -> &ImplInfo {
        &self.impls[impl_id]
    }

    pub(crate) fn trait_info(&self, trait_id: usize) -> &TraitInfo {
        &self.traits[trait_id]
    }

    pub(crate) fn method_function(&self, impl_id: usize, method: &str) -> Option<&str> {
        self.impls[impl_id].methods.get(method).map(String::as_str)
    }
}

pub(crate) fn prepare(program: &mut Program) -> Result<TraitRegistry, Diagnostic> {
    let trait_declarations = program.traits.clone();
    let impl_declarations = program.impls.clone();
    let shapes = type_shapes(program);
    validate_declaration_types(&trait_declarations, &impl_declarations, &shapes)?;
    program.traits.clear();
    program.impls.clear();

    let mut registry = TraitRegistry::default();
    for (id, declaration) in trait_declarations.into_iter().enumerate() {
        if registry.trait_names.contains_key(&declaration.name) {
            return Err(at(
                declaration.span,
                format!("trait `{}` is defined more than once", declaration.name),
            ));
        }
        registry.trait_names.insert(declaration.name.clone(), id);
        registry.traits.push(TraitInfo {
            id,
            module: declaration.module,
            name: declaration.name,
            generic_params: declaration.generic_params,
            functions: declaration.functions,
            is_public: declaration.is_public,
        });
        registry.by_trait.push(Vec::new());
    }

    validate_generic_bounds_for_program(program, &registry, &shapes)?;

    let mut synthetic_functions = Vec::new();
    for (id, declaration) in impl_declarations.into_iter().enumerate() {
        let Some(trait_id) = registry.trait_names.get(&declaration.trait_name).copied() else {
            return Err(at(
                declaration.trait_span,
                format!("unknown trait `{}`", declaration.trait_name),
            ));
        };
        let trait_info = registry.trait_info(trait_id);
        if declaration.trait_arguments.len() != trait_info.generic_params.len() {
            return Err(at(
                declaration.trait_span,
                format!(
                    "trait `{}` expects {} type argument(s), got {}",
                    trait_info.name,
                    trait_info.generic_params.len(),
                    declaration.trait_arguments.len()
                ),
            ));
        }
        validate_impl_target(&declaration)?;
        validate_impl_bounds(&declaration, &registry, &shapes)?;
        let info = validate_and_lower_impl(
            trait_id,
            declaration,
            &registry,
            &mut synthetic_functions,
            &shapes,
        )?;
        registry.by_trait[trait_id].push(id);
        registry.impls.push(info);
    }

    reject_overlapping_impls(&registry)?;
    program.functions.extend(synthetic_functions);
    Ok(registry)
}

fn type_shapes(program: &Program) -> TypeShapes {
    let mut generic_arity = HashMap::new();
    let mut known_types = HashSet::new();
    for declaration in &program.structs {
        known_types.insert(declaration.name.clone());
        if !declaration.generic_params.is_empty() {
            generic_arity.insert(declaration.name.clone(), declaration.generic_params.len());
        }
    }
    for declaration in &program.enums {
        known_types.insert(declaration.name.clone());
        if !declaration.generic_params.is_empty() {
            generic_arity.insert(declaration.name.clone(), declaration.generic_params.len());
        }
    }
    TypeShapes {
        generic_arity,
        known_types,
    }
}

fn validate_declaration_types(
    traits: &[TraitDecl],
    impls: &[ImplDecl],
    shapes: &TypeShapes,
) -> Result<(), Diagnostic> {
    for declaration in traits {
        let trait_parameters = declaration
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<HashSet<_>>();
        for function in &declaration.functions {
            let parameters = trait_parameters
                .iter()
                .cloned()
                .chain(
                    function
                        .generic_params
                        .iter()
                        .map(|parameter| parameter.name.clone()),
                )
                .collect::<HashSet<_>>();
            let mut seen_params = HashSet::new();
            for param in &function.params {
                if !seen_params.insert(param.name.as_str()) {
                    return Err(at(
                        param.span,
                        format!(
                            "parameter `{}` is defined more than once in trait function `{}::{}`",
                            param.name, declaration.name, function.name
                        ),
                    ));
                }
                if param.ty == Type::Unit {
                    return Err(at(param.span, "parameters cannot have type `unit`"));
                }
                validate_type_shape(&param.ty, &parameters, true, shapes, param.span)?;
                validate_signature_type(&param.ty, true, false, param.span)?;
            }
            if is_reference(&function.return_type) {
                return Err(at(
                    function.span,
                    "trait functions cannot return references until lifetime-generic trait functions are supported",
                ));
            }
            validate_type_shape(
                &function.return_type,
                &parameters,
                true,
                shapes,
                function.span,
            )?;
            validate_signature_type(&function.return_type, false, true, function.span)?;
        }
    }
    for declaration in impls {
        let impl_parameters = declaration
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<HashSet<_>>();
        validate_type_shape(
            &declaration.target,
            &impl_parameters,
            false,
            shapes,
            declaration.target_span,
        )?;
        validate_signature_type(&declaration.target, true, true, declaration.target_span)?;
        for argument in &declaration.trait_arguments {
            validate_type_shape(
                argument,
                &impl_parameters,
                false,
                shapes,
                declaration.trait_span,
            )?;
            validate_signature_type(argument, false, true, declaration.trait_span)?;
        }
        for function in &declaration.functions {
            let parameters = impl_parameters
                .iter()
                .cloned()
                .chain(
                    function
                        .generic_params
                        .iter()
                        .map(|parameter| parameter.name.clone()),
                )
                .collect::<HashSet<_>>();
            for param in &function.params {
                validate_type_shape(&param.ty, &parameters, true, shapes, param.span)?;
                validate_signature_type(&param.ty, true, false, param.span)?;
            }
            if is_reference(&function.return_type) {
                return Err(at(
                    function.span,
                    "impl functions cannot return references until lifetime-generic impl functions are supported",
                ));
            }
            validate_type_shape(
                &function.return_type,
                &parameters,
                true,
                shapes,
                function.span,
            )?;
            validate_signature_type(&function.return_type, false, true, function.span)?;
        }
    }
    Ok(())
}

struct TypeShapes {
    generic_arity: HashMap<String, usize>,
    known_types: HashSet<String>,
}

fn validate_type_shape(
    ty: &Type,
    parameters: &HashSet<String>,
    self_allowed: bool,
    shapes: &TypeShapes,
    span: Span,
) -> Result<(), Diagnostic> {
    match ty {
        Type::Parameter(name) if !parameters.contains(name) => {
            Err(at(span, format!("unknown generic type parameter `{name}`")))
        }
        Type::SelfType if !self_allowed => {
            Err(at(span, "`Self` is not available in this impl target"))
        }
        Type::Named(name) if shapes.generic_arity.contains_key(name) => Err(at(
            span,
            format!("generic type `{name}` requires explicit type arguments"),
        )),
        Type::Named(name)
            if !shapes.known_types.contains(name)
                && !matches!(
                    name.as_str(),
                    "i64" | "f32" | "f64" | "bool" | "str" | "unit"
                ) =>
        {
            Err(at(span, format!("unknown type `{name}`")))
        }
        Type::Applied(name, arguments) => {
            let Some(expected) = shapes.generic_arity.get(name).copied() else {
                return Err(at(span, format!("`{name}` is not a generic type")));
            };
            if arguments.len() != expected {
                return Err(at(
                    span,
                    format!(
                        "generic type `{name}` expects {expected} type argument(s), got {}",
                        arguments.len()
                    ),
                ));
            }
            for argument in arguments {
                validate_type_shape(argument, parameters, self_allowed, shapes, span)?;
            }
            Ok(())
        }
        Type::Array(element, _) | Type::Vector(element) => {
            validate_type_shape(element, parameters, self_allowed, shapes, span)
        }
        Type::Reference(element, _)
        | Type::MutableReference(element, _)
        | Type::RawPointer(element, _) => {
            validate_type_shape(element, parameters, self_allowed, shapes, span)
        }
        Type::Tuple(elements) => {
            for element in elements {
                validate_type_shape(element, parameters, self_allowed, shapes, span)?;
            }
            Ok(())
        }
        Type::Function(params, result) => {
            for param in params {
                if *param == Type::Unit {
                    return Err(at(span, "function type parameters cannot have type `unit`"));
                }
                validate_type_shape(param, parameters, self_allowed, shapes, span)?;
            }
            if matches!(
                result.as_ref(),
                Type::Reference(_, _) | Type::MutableReference(_, _)
            ) {
                return Err(at(span, "function types cannot return references"));
            }
            validate_type_shape(result, parameters, self_allowed, shapes, span)
        }
        Type::I64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Named(_)
        | Type::Parameter(_)
        | Type::SelfType => Ok(()),
    }
}

fn is_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_, _) | Type::MutableReference(_, _))
}

fn validate_signature_type(
    ty: &Type,
    allow_root_reference: bool,
    allow_unit: bool,
    span: Span,
) -> Result<(), Diagnostic> {
    match ty {
        Type::Unit if !allow_unit => Err(at(
            span,
            "type `unit` is not valid in this function type position",
        )),
        Type::Reference(element, _) | Type::MutableReference(element, _) => {
            if !allow_root_reference {
                return Err(at(
                    span,
                    "references cannot be stored in arrays, vectors, tuples, or generic arguments",
                ));
            }
            validate_signature_type(element, false, false, span)
        }
        Type::Array(element, _) | Type::Vector(element) => {
            validate_signature_type(element, false, false, span)
        }
        Type::Tuple(elements) => {
            if elements.is_empty() {
                return Err(at(span, "tuple types must contain at least one element"));
            }
            for element in elements {
                validate_signature_type(element, false, false, span)?;
            }
            Ok(())
        }
        Type::Function(params, result) => {
            for param in params {
                validate_signature_type(param, true, false, span)?;
            }
            if is_reference(result) {
                return Err(at(span, "function types cannot return references"));
            }
            validate_signature_type(result, false, true, span)
        }
        Type::RawPointer(element, _) => validate_signature_type(element, false, false, span),
        Type::Applied(_, arguments) => {
            for argument in arguments {
                validate_signature_type(argument, false, true, span)?;
            }
            Ok(())
        }
        Type::I64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Named(_)
        | Type::Parameter(_)
        | Type::SelfType => Ok(()),
    }
}

fn validate_generic_bounds_for_program(
    program: &Program,
    registry: &TraitRegistry,
    shapes: &TypeShapes,
) -> Result<(), Diagnostic> {
    for declaration in &program.structs {
        validate_bounds(
            &declaration.generic_params,
            declaration.generic_params.iter(),
            declaration.is_public,
            false,
            registry,
            shapes,
        )?;
    }
    for declaration in &program.enums {
        validate_bounds(
            &declaration.generic_params,
            declaration.generic_params.iter(),
            declaration.is_public,
            false,
            registry,
            shapes,
        )?;
    }
    for function in &program.functions {
        validate_bounds(
            &function.generic_params,
            function.generic_params.iter(),
            function.is_public,
            false,
            registry,
            shapes,
        )?;
    }
    for trait_info in &registry.traits {
        validate_bounds(
            &trait_info.generic_params,
            trait_info.generic_params.iter(),
            trait_info.is_public,
            false,
            registry,
            shapes,
        )?;
        for function in &trait_info.functions {
            let parameters = trait_info
                .generic_params
                .iter()
                .chain(&function.generic_params)
                .cloned()
                .collect::<Vec<_>>();
            validate_bounds(
                &function.generic_params,
                parameters.iter(),
                trait_info.is_public,
                true,
                registry,
                shapes,
            )?;
        }
    }
    Ok(())
}

fn validate_impl_bounds(
    declaration: &ImplDecl,
    registry: &TraitRegistry,
    shapes: &TypeShapes,
) -> Result<(), Diagnostic> {
    validate_bounds(
        &declaration.generic_params,
        declaration.generic_params.iter(),
        false,
        false,
        registry,
        shapes,
    )
}

fn validate_bounds<'a>(
    parameters: &[GenericParam],
    scope: impl IntoIterator<Item = &'a GenericParam>,
    is_public: bool,
    self_allowed: bool,
    registry: &TraitRegistry,
    shapes: &TypeShapes,
) -> Result<(), Diagnostic> {
    let scope = scope
        .into_iter()
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();
    for parameter in parameters {
        let mut seen = HashSet::new();
        for bound in &parameter.bounds {
            let Some(trait_info) = registry.trait_by_name(&bound.name) else {
                return Err(at(
                    bound.span,
                    format!(
                        "unknown trait `{}` in bound for `{}`",
                        bound.name, parameter.name
                    ),
                ));
            };
            if bound.type_arguments.len() != trait_info.generic_params.len() {
                return Err(at(
                    bound.span,
                    format!(
                        "trait `{}` expects {} type argument(s), got {}",
                        bound.name,
                        trait_info.generic_params.len(),
                        bound.type_arguments.len()
                    ),
                ));
            }
            let key = bound_key(&bound.name, &bound.type_arguments);
            if !seen.insert(key) {
                return Err(at(
                    bound.span,
                    format!(
                        "trait `{}` is listed more than once in the bounds for `{}`",
                        bound.name, parameter.name
                    ),
                ));
            }
            if is_public && !trait_info.is_public {
                return Err(at(
                    bound.span,
                    format!(
                        "private trait `{}` cannot appear in a public trait bound",
                        bound.name
                    ),
                ));
            }
            for argument in &bound.type_arguments {
                validate_type_shape(argument, &scope, self_allowed, shapes, bound.span)?;
                validate_signature_type(argument, false, true, bound.span)?;
            }
        }
    }
    Ok(())
}

fn bound_key(name: &str, arguments: &[Type]) -> String {
    format!(
        "{name}<{}>",
        arguments
            .iter()
            .map(type_name)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn validate_impl_target(declaration: &ImplDecl) -> Result<(), Diagnostic> {
    let parameters = declaration
        .generic_params
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();
    if matches!(declaration.target, Type::Parameter(_) | Type::SelfType) {
        return Err(at(
            declaration.target_span,
            "an impl target cannot be a bare type parameter",
        ));
    }
    let mut used = HashSet::new();
    collect_target_parameters(&declaration.target, &parameters, &mut used)?;
    for argument in &declaration.trait_arguments {
        collect_target_parameters(argument, &parameters, &mut used)?;
    }
    for parameter in &declaration.generic_params {
        if !used.contains(parameter.name.as_str()) {
            return Err(at(
                parameter.span,
                format!(
                    "impl type parameter `{}` is not constrained by the impl target or trait arguments",
                    parameter.name
                ),
            ));
        }
    }
    Ok(())
}

fn collect_target_parameters(
    ty: &Type,
    allowed: &HashSet<String>,
    used: &mut HashSet<String>,
) -> Result<(), Diagnostic> {
    match ty {
        Type::Parameter(name) => {
            if !allowed.contains(name) {
                return Err(Diagnostic::new(format!(
                    "impl target uses undeclared type parameter `{name}`"
                )));
            }
            used.insert(name.clone());
        }
        Type::SelfType => {
            return Err(Diagnostic::new("`Self` cannot appear in an impl target"));
        }
        Type::Array(element, _) | Type::Vector(element) => {
            collect_target_parameters(element, allowed, used)?;
        }
        Type::Reference(element, _)
        | Type::MutableReference(element, _)
        | Type::RawPointer(element, _) => {
            collect_target_parameters(element, allowed, used)?;
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_target_parameters(element, allowed, used)?;
            }
        }
        Type::Function(params, result) => {
            for param in params {
                collect_target_parameters(param, allowed, used)?;
            }
            collect_target_parameters(result, allowed, used)?;
        }
        Type::Applied(_, arguments) => {
            for argument in arguments {
                collect_target_parameters(argument, allowed, used)?;
            }
        }
        Type::I64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Named(_) => {}
    }
    Ok(())
}

fn validate_and_lower_impl(
    trait_id: usize,
    mut declaration: ImplDecl,
    registry: &TraitRegistry,
    synthetic_functions: &mut Vec<Function>,
    shapes: &TypeShapes,
) -> Result<ImplInfo, Diagnostic> {
    let trait_info = registry.trait_info(trait_id).clone();
    let mut by_name = HashMap::new();
    for required in &trait_info.functions {
        by_name.insert(required.name.as_str(), required);
    }
    let mut seen = HashSet::new();
    let mut methods = HashMap::new();
    let target = declaration.target.clone();
    let trait_arguments = declaration.trait_arguments.clone();
    for function in &mut declaration.functions {
        if !seen.insert(function.name.clone()) {
            return Err(at(
                function.span,
                format!(
                    "implementation of `{}` defines `{}` more than once",
                    trait_info.name, function.name
                ),
            ));
        }
        let Some(required) = by_name.get(function.name.as_str()) else {
            return Err(at(
                function.span,
                format!(
                    "function `{}` is not a member of trait `{}`",
                    function.name, trait_info.name
                ),
            ));
        };
        if function.generic_params.len() != required.generic_params.len() {
            return Err(at(
                function.span,
                format!(
                    "function `{}::{}` has {} generic parameter(s), but the trait declares {}",
                    trait_info.name,
                    function.name,
                    function.generic_params.len(),
                    required.generic_params.len()
                ),
            ));
        }
        let substitutions = signature_substitutions(
            &trait_info.generic_params,
            &target,
            &trait_arguments,
            required,
            function,
        );
        let required_bounds = function_bound_keys(&required.generic_params, &substitutions);
        let mut impl_bound_substitutions = HashMap::new();
        impl_bound_substitutions.insert("Self".to_owned(), declaration.target.clone());
        let actual_bounds =
            function_bound_keys(&function.generic_params, &impl_bound_substitutions);
        if required_bounds != actual_bounds {
            return Err(at(
                function.span,
                format!(
                    "bounds on `{}::{}` do not match the trait declaration",
                    trait_info.name, function.name
                ),
            ));
        }
        if function.params.len() != required.params.len() {
            return Err(at(
                function.span,
                format!(
                    "function `{}` has {} parameter(s), but trait `{}` requires {}",
                    function.name,
                    function.params.len(),
                    trait_info.name,
                    required.params.len()
                ),
            ));
        }
        for (actual, expected) in function.params.iter().zip(&required.params) {
            let actual_ty = substitute_self(&actual.ty, &declaration.target);
            let expected_ty = substitute_parameters(&expected.ty, &substitutions);
            if actual_ty != expected_ty {
                return Err(at(
                    actual.span,
                    format!(
                        "parameter `{}` of `{}::{}` has type `{}`, expected `{}`",
                        actual.name,
                        trait_info.name,
                        function.name,
                        type_name(&actual_ty),
                        type_name(&expected_ty)
                    ),
                ));
            }
        }
        let actual_return = substitute_self(&function.return_type, &declaration.target);
        let expected_return = substitute_parameters(&required.return_type, &substitutions);
        if actual_return != expected_return {
            return Err(at(
                function.span,
                format!(
                    "function `{}::{}` returns `{}`, expected `{}`",
                    trait_info.name,
                    function.name,
                    type_name(&actual_return),
                    type_name(&expected_return)
                ),
            ));
        }
        validate_bounds(
            &function.generic_params,
            declaration
                .generic_params
                .iter()
                .chain(&function.generic_params),
            false,
            true,
            registry,
            shapes,
        )?;
        for param in &function.params {
            let actual = substitute_self(&param.ty, &declaration.target);
            validate_signature_type(&actual, true, false, param.span)?;
        }
        if is_reference(&actual_return) {
            return Err(at(
                function.span,
                "impl functions cannot return references until lifetime-generic impl functions are supported",
            ));
        }
        validate_signature_type(&actual_return, false, true, function.span)?;

        let internal_name = trait_method_name(
            &trait_info.name,
            &declaration.trait_arguments,
            &declaration.target,
            &function.name,
        );
        methods.insert(function.name.clone(), internal_name.clone());
        function.name = internal_name;
        let mut generic_params = declaration.generic_params.clone();
        generic_params.extend(function.generic_params.clone());
        function.generic_params = generic_params;
        function.is_public = true;
        substitute_self_in_function(function, &declaration.target);
        synthetic_functions.push(function.clone());
    }
    for required in &trait_info.functions {
        if seen.contains(&required.name) {
            continue;
        }
        let Some(default_body) = &required.body else {
            return Err(at(
                declaration.span,
                format!(
                    "implementation of `{}` for `{}` is missing required function `{}`",
                    trait_info.name,
                    type_name(&declaration.target),
                    required.name
                ),
            ));
        };
        // Inherited bodies are cloned per impl and specialized like any other
        // impl function, so dispatch stays fully static.
        let internal_name = trait_method_name(
            &trait_info.name,
            &declaration.trait_arguments,
            &declaration.target,
            &required.name,
        );
        let mut function_parameters = required.generic_params.clone();
        let mut substitutions = HashMap::new();
        substitutions.insert("Self".to_owned(), declaration.target.clone());
        for (parameter, argument) in trait_info
            .generic_params
            .iter()
            .zip(&declaration.trait_arguments)
        {
            substitutions.insert(parameter.name.clone(), argument.clone());
        }
        let mut used_parameter_names = declaration
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .chain(
                trait_info
                    .generic_params
                    .iter()
                    .map(|parameter| parameter.name.clone()),
            )
            .collect::<HashSet<_>>();
        used_parameter_names.insert("Self".to_owned());
        for (index, parameter) in function_parameters.iter_mut().enumerate() {
            let original_name = parameter.name.clone();
            let mut name = original_name.clone();
            let mut suffix = index;
            while used_parameter_names.contains(&name) {
                name = format!("{original_name}#{suffix}");
                suffix += 1;
            }
            used_parameter_names.insert(name.clone());
            substitutions.insert(original_name, Type::Parameter(name.clone()));
            parameter.name = name;
        }
        let mut function = Function {
            name: internal_name.clone(),
            generic_params: function_parameters,
            lifetime_params: Vec::new(),
            params: required.params.clone(),
            return_type: required.return_type.clone(),
            body: default_body.clone(),
            span: required.span,
            is_public: true,
            // The body is lexically part of the trait declaration, so privacy
            // checks inside it use the trait's module rather than the impl's.
            module: trait_info.module.clone(),
        };
        // Trait-argument substitution applies only to the method's own generic
        // parameters; impl-owned bounds stay as declared for registry matching.
        substitute_parameters_in_function(&mut function, &substitutions);
        let mut generic_params = declaration.generic_params.clone();
        generic_params.extend(function.generic_params);
        function.generic_params = generic_params;
        for param in &function.params {
            validate_signature_type(&param.ty, true, false, param.span)?;
        }
        validate_signature_type(&function.return_type, false, true, function.span)?;
        methods.insert(required.name.clone(), internal_name);
        synthetic_functions.push(function);
    }
    Ok(ImplInfo {
        target: declaration.target,
        trait_arguments: declaration.trait_arguments,
        generic_params: declaration.generic_params,
        methods,
        span: declaration.span,
    })
}

fn signature_substitutions(
    trait_parameters: &[GenericParam],
    target: &Type,
    trait_arguments: &[Type],
    required: &TraitFunction,
    function: &Function,
) -> HashMap<String, Type> {
    let mut substitutions = HashMap::new();
    substitutions.insert("Self".to_owned(), target.clone());
    for (parameter, argument) in trait_parameters.iter().zip(trait_arguments) {
        substitutions.insert(parameter.name.clone(), argument.clone());
    }
    for (trait_parameter, impl_parameter) in
        required.generic_params.iter().zip(&function.generic_params)
    {
        substitutions.insert(
            trait_parameter.name.clone(),
            Type::Parameter(impl_parameter.name.clone()),
        );
    }
    substitutions
}

fn function_bound_keys(
    parameters: &[GenericParam],
    substitutions: &HashMap<String, Type>,
) -> Vec<Vec<String>> {
    parameters
        .iter()
        .map(|parameter| {
            let mut keys = parameter
                .bounds
                .iter()
                .map(|bound| {
                    let arguments = bound
                        .type_arguments
                        .iter()
                        .map(|argument| substitute_parameters(argument, substitutions))
                        .collect::<Vec<_>>();
                    bound_key(&bound.name, &arguments)
                })
                .collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            keys
        })
        .collect()
}

fn trait_method_name(trait_name: &str, arguments: &[Type], target: &Type, method: &str) -> String {
    if arguments.is_empty() {
        format!("{trait_name} for {}::{method}", type_name(target))
    } else {
        format!(
            "{trait_name}<{}> for {}::{method}",
            arguments
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(","),
            type_name(target)
        )
    }
}

fn substitute_self_in_function(function: &mut Function, self_type: &Type) {
    let mut substitutions = HashMap::new();
    substitutions.insert("Self".to_owned(), self_type.clone());
    substitute_parameters_in_function(function, &substitutions);
}

fn substitute_parameters_in_function(
    function: &mut Function,
    substitutions: &HashMap<String, Type>,
) {
    for parameter in &mut function.generic_params {
        for bound in &mut parameter.bounds {
            for argument in &mut bound.type_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
        }
    }
    for param in &mut function.params {
        param.ty = substitute_parameters(&param.ty, substitutions);
    }
    function.return_type = substitute_parameters(&function.return_type, substitutions);
    substitute_parameters_in_block(&mut function.body, substitutions);
}

fn substitute_parameters_in_block(block: &mut Block, substitutions: &HashMap<String, Type>) {
    for statement in &mut block.statements {
        substitute_parameters_in_statement(statement, substitutions);
    }
    if let Some(tail) = &mut block.tail {
        substitute_parameters_in_expr(tail, substitutions);
    }
}

fn substitute_parameters_in_statement(statement: &mut Stmt, substitutions: &HashMap<String, Type>) {
    match statement {
        Stmt::Let { ty, value, .. } => {
            if let Some(ty) = ty {
                *ty = substitute_parameters(ty, substitutions);
            }
            substitute_parameters_in_expr(value, substitutions);
        }
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            substitute_parameters_in_expr(value, substitutions);
        }
        Stmt::If {
            condition,
            then_block,
            else_block,
        } => {
            substitute_parameters_in_expr(condition, substitutions);
            substitute_parameters_in_block(then_block, substitutions);
            if let Some(block) = else_block {
                substitute_parameters_in_block(block, substitutions);
            }
        }
        Stmt::While { condition, body } => {
            substitute_parameters_in_expr(condition, substitutions);
            substitute_parameters_in_block(body, substitutions);
        }
        Stmt::For {
            start, end, body, ..
        } => {
            substitute_parameters_in_expr(start, substitutions);
            substitute_parameters_in_expr(end, substitutions);
            substitute_parameters_in_block(body, substitutions);
        }
        Stmt::ForArray { array, body, .. } => {
            substitute_parameters_in_expr(array, substitutions);
            substitute_parameters_in_block(body, substitutions);
        }
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            if let Some(value) = value {
                substitute_parameters_in_expr(value, substitutions);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::AssignPlace { target, value, .. } => {
            substitute_parameters_in_expr(target, substitutions);
            substitute_parameters_in_expr(value, substitutions);
        }
    }
}

fn substitute_parameters_in_expr(expr: &mut Expr, substitutions: &HashMap<String, Type>) {
    match expr {
        Expr::Integer(..)
        | Expr::Float32(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::String(..)
        | Expr::Unit(..)
        | Expr::Variable(..) => {}
        Expr::Null { ty, .. } => {
            *ty = substitute_parameters(ty, substitutions);
        }
        Expr::Tuple(values, _) | Expr::Array(values, _) => {
            for value in values {
                substitute_parameters_in_expr(value, substitutions);
            }
        }
        Expr::Index { array, index, .. } => {
            substitute_parameters_in_expr(array, substitutions);
            substitute_parameters_in_expr(index, substitutions);
        }
        Expr::Field { value, .. } | Expr::Unary { value, .. } => {
            substitute_parameters_in_expr(value, substitutions);
        }
        Expr::Struct {
            type_arguments,
            fields,
            ..
        } => {
            for argument in type_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
            for (_, value, _) in fields {
                substitute_parameters_in_expr(value, substitutions);
            }
        }
        Expr::Binary { left, right, .. } => {
            substitute_parameters_in_expr(left, substitutions);
            substitute_parameters_in_expr(right, substitutions);
        }
        Expr::Call {
            type_arguments,
            args,
            ..
        } => {
            for argument in type_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
            for arg in args {
                substitute_parameters_in_expr(arg, substitutions);
            }
        }
        Expr::TraitCall {
            trait_arguments,
            self_type: call_self,
            type_arguments,
            args,
            ..
        } => {
            for argument in trait_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
            if let Some(call_self) = call_self {
                *call_self = substitute_parameters(call_self, substitutions);
            }
            for argument in type_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
            for arg in args {
                substitute_parameters_in_expr(arg, substitutions);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            substitute_parameters_in_expr(callee, substitutions);
            for arg in args {
                substitute_parameters_in_expr(arg, substitutions);
            }
        }
        Expr::Lambda {
            params,
            return_type,
            body,
            ..
        } => {
            for param in params {
                param.ty = substitute_parameters(&param.ty, substitutions);
            }
            *return_type = substitute_parameters(return_type, substitutions);
            substitute_parameters_in_block(body, substitutions);
        }
        Expr::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            substitute_parameters_in_expr(condition, substitutions);
            substitute_parameters_in_block(then_block, substitutions);
            substitute_parameters_in_block(else_block, substitutions);
        }
        Expr::Match { value, arms, .. } => {
            substitute_parameters_in_expr(value, substitutions);
            for arm in arms {
                substitute_parameters_in_block(&mut arm.body, substitutions);
            }
        }
        Expr::Try { value, .. } => substitute_parameters_in_expr(value, substitutions),
        Expr::Loop { body, .. } => substitute_parameters_in_block(body, substitutions),
        Expr::Variant {
            type_arguments,
            payload,
            ..
        } => {
            for argument in type_arguments {
                *argument = substitute_parameters(argument, substitutions);
            }
            if let Some(payload) = payload {
                for value in payload {
                    substitute_parameters_in_expr(value, substitutions);
                }
            }
        }
    }
}

pub(crate) fn substitute_self(ty: &Type, target: &Type) -> Type {
    let mut substitutions = HashMap::new();
    substitutions.insert("Self".to_owned(), target.clone());
    substitute_parameters(ty, &substitutions)
}

pub(crate) fn substitute_parameters(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::SelfType => substitutions.get("Self").cloned().unwrap_or(Type::SelfType),
        Type::Parameter(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| Type::Parameter(name.clone())),
        Type::Array(element, length) => Type::Array(
            Box::new(substitute_parameters(element, substitutions)),
            *length,
        ),
        Type::Vector(element) => {
            Type::Vector(Box::new(substitute_parameters(element, substitutions)))
        }
        Type::Reference(element, lifetime) => Type::Reference(
            Box::new(substitute_parameters(element, substitutions)),
            lifetime.clone(),
        ),
        Type::MutableReference(element, lifetime) => Type::MutableReference(
            Box::new(substitute_parameters(element, substitutions)),
            lifetime.clone(),
        ),
        Type::RawPointer(element, mutable) => Type::RawPointer(
            Box::new(substitute_parameters(element, substitutions)),
            *mutable,
        ),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute_parameters(element, substitutions))
                .collect(),
        ),
        Type::Function(params, result) => Type::Function(
            params
                .iter()
                .map(|param| substitute_parameters(param, substitutions))
                .collect(),
            Box::new(substitute_parameters(result, substitutions)),
        ),
        Type::Applied(name, arguments) => Type::Applied(
            name.clone(),
            arguments
                .iter()
                .map(|argument| substitute_parameters(argument, substitutions))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn type_name(ty: &Type) -> String {
    match ty {
        Type::I64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Unit => "unit".into(),
        Type::SelfType => "Self".into(),
        Type::Named(name) | Type::Parameter(name) => name.clone(),
        Type::Applied(name, arguments) => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(element, length) => format!("[{};{length}]", type_name(element)),
        Type::Vector(element) => format!("Vec<{}>", type_name(element)),
        Type::Tuple(elements) => {
            let trailing = if elements.len() == 1 { "," } else { "" };
            format!(
                "({}{trailing})",
                elements.iter().map(type_name).collect::<Vec<_>>().join(",")
            )
        }
        Type::Function(params, result) => format!(
            "fn({})->{}",
            params.iter().map(type_name).collect::<Vec<_>>().join(","),
            type_name(result)
        ),
        Type::Reference(element, _) => format!("&{}", type_name(element)),
        Type::MutableReference(element, _) => format!("&mut {}", type_name(element)),
        Type::RawPointer(element, mutable) => format!(
            "*{} {}",
            if *mutable { "mut" } else { "const" },
            type_name(element)
        ),
    }
}

fn reject_overlapping_impls(registry: &TraitRegistry) -> Result<(), Diagnostic> {
    for trait_id in 0..registry.traits.len() {
        let impls = registry.impls_for(trait_id);
        for (offset, left_id) in impls.iter().enumerate() {
            for right_id in &impls[offset + 1..] {
                let left = registry.impl_info(*left_id);
                let right = registry.impl_info(*right_id);
                if impl_targets_overlap(left, right) {
                    return Err(at(
                        right.span,
                        format!(
                            "conflicting implementations of trait `{}` for `{}` and `{}`",
                            registry.trait_info(trait_id).name,
                            type_name(&left.target),
                            type_name(&right.target)
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn impl_targets_overlap(left: &ImplInfo, right: &ImplInfo) -> bool {
    if left.trait_arguments.len() != right.trait_arguments.len() {
        return false;
    }
    let mut left_names = HashMap::new();
    let mut right_names = HashMap::new();
    let left_target = rename_overlap_variables(&left.target, "L", &mut left_names);
    let right_target = rename_overlap_variables(&right.target, "R", &mut right_names);
    let left_arguments = left
        .trait_arguments
        .iter()
        .map(|argument| rename_overlap_variables(argument, "L", &mut left_names))
        .collect::<Vec<_>>();
    let right_arguments = right
        .trait_arguments
        .iter()
        .map(|argument| rename_overlap_variables(argument, "R", &mut right_names))
        .collect::<Vec<_>>();
    let mut bindings = HashMap::new();
    if unify_overlap(&left_target, &right_target, &mut bindings).is_err() {
        return false;
    }
    left_arguments
        .iter()
        .zip(&right_arguments)
        .all(|(left, right)| unify_overlap(left, right, &mut bindings).is_ok())
}

fn rename_overlap_variables(ty: &Type, side: &str, names: &mut HashMap<String, usize>) -> Type {
    match ty {
        Type::Parameter(name) => {
            let next = names.len();
            let id = *names.entry(name.clone()).or_insert(next);
            Type::Parameter(format!("@{side}{id}"))
        }
        Type::Array(element, length) => Type::Array(
            Box::new(rename_overlap_variables(element, side, names)),
            *length,
        ),
        Type::Vector(element) => {
            Type::Vector(Box::new(rename_overlap_variables(element, side, names)))
        }
        Type::Reference(element, lifetime) => Type::Reference(
            Box::new(rename_overlap_variables(element, side, names)),
            lifetime.clone(),
        ),
        Type::MutableReference(element, lifetime) => Type::MutableReference(
            Box::new(rename_overlap_variables(element, side, names)),
            lifetime.clone(),
        ),
        Type::RawPointer(element, mutable) => Type::RawPointer(
            Box::new(rename_overlap_variables(element, side, names)),
            *mutable,
        ),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| rename_overlap_variables(element, side, names))
                .collect(),
        ),
        Type::Function(params, result) => Type::Function(
            params
                .iter()
                .map(|param| rename_overlap_variables(param, side, names))
                .collect(),
            Box::new(rename_overlap_variables(result, side, names)),
        ),
        Type::Applied(name, arguments) => Type::Applied(
            name.clone(),
            arguments
                .iter()
                .map(|argument| rename_overlap_variables(argument, side, names))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn unify_overlap(
    left: &Type,
    right: &Type,
    bindings: &mut HashMap<String, Type>,
) -> Result<(), ()> {
    if let Type::Parameter(name) = left {
        return bind_overlap(name, right, bindings);
    }
    if let Type::Parameter(name) = right {
        return bind_overlap(name, left, bindings);
    }
    match (left, right) {
        (Type::I64, Type::I64)
        | (Type::F32, Type::F32)
        | (Type::F64, Type::F64)
        | (Type::Bool, Type::Bool)
        | (Type::Str, Type::Str)
        | (Type::Unit, Type::Unit) => Ok(()),
        (Type::Named(left), Type::Named(right)) if left == right => Ok(()),
        (Type::Applied(left, left_args), Type::Applied(right, right_args))
            if left == right && left_args.len() == right_args.len() =>
        {
            for (left, right) in left_args.iter().zip(right_args) {
                unify_overlap(left, right, bindings)?;
            }
            Ok(())
        }
        (Type::Array(left, left_len), Type::Array(right, right_len)) if left_len == right_len => {
            unify_overlap(left, right, bindings)
        }
        (Type::Vector(left), Type::Vector(right))
        | (Type::Reference(left, _), Type::Reference(right, _))
        | (Type::MutableReference(left, _), Type::MutableReference(right, _)) => {
            unify_overlap(left, right, bindings)
        }
        (Type::RawPointer(left, left_mut), Type::RawPointer(right, right_mut))
            if left_mut == right_mut =>
        {
            unify_overlap(left, right, bindings)
        }
        (Type::Tuple(left), Type::Tuple(right)) if left.len() == right.len() => {
            for (left, right) in left.iter().zip(right) {
                unify_overlap(left, right, bindings)?;
            }
            Ok(())
        }
        (Type::Function(left_params, left_result), Type::Function(right_params, right_result))
            if left_params.len() == right_params.len() =>
        {
            for (left, right) in left_params.iter().zip(right_params) {
                unify_overlap(left, right, bindings)?;
            }
            unify_overlap(left_result, right_result, bindings)
        }
        _ => Err(()),
    }
}

fn bind_overlap(variable: &str, ty: &Type, bindings: &mut HashMap<String, Type>) -> Result<(), ()> {
    let ty = resolve_overlap(ty, bindings);
    if let Type::Parameter(name) = &ty
        && name == variable
    {
        return Ok(());
    }
    if let Some(previous) = bindings.get(variable).cloned() {
        return unify_overlap(&previous, &ty, bindings);
    }
    if occurs_overlap(variable, &ty, bindings) {
        return Err(());
    }
    bindings.insert(variable.to_owned(), ty);
    Ok(())
}

fn resolve_overlap(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    if let Type::Parameter(name) = ty
        && let Some(bound) = bindings.get(name)
    {
        return resolve_overlap(bound, bindings);
    }
    ty.clone()
}

fn occurs_overlap(variable: &str, ty: &Type, bindings: &HashMap<String, Type>) -> bool {
    match ty {
        Type::Parameter(name) => {
            name == variable
                || bindings
                    .get(name)
                    .is_some_and(|bound| occurs_overlap(variable, bound, bindings))
        }
        Type::Array(element, _) | Type::Vector(element) => {
            occurs_overlap(variable, element, bindings)
        }
        Type::Reference(element, _)
        | Type::MutableReference(element, _)
        | Type::RawPointer(element, _) => occurs_overlap(variable, element, bindings),
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| occurs_overlap(variable, element, bindings)),
        Type::Function(params, result) => {
            params
                .iter()
                .any(|param| occurs_overlap(variable, param, bindings))
                || occurs_overlap(variable, result, bindings)
        }
        Type::Applied(_, arguments) => arguments
            .iter()
            .any(|argument| occurs_overlap(variable, argument, bindings)),
        Type::I64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Named(_)
        | Type::SelfType => false,
    }
}

fn at(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::at(span.into(), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(name: &str) -> Type {
        Type::Parameter(name.into())
    }

    fn boxed(inner: Type) -> Type {
        Type::Applied("Box".into(), vec![inner])
    }

    fn head(target: Type) -> ImplInfo {
        head_with(target, Vec::new())
    }

    fn head_with(target: Type, trait_arguments: Vec<Type>) -> ImplInfo {
        ImplInfo {
            target,
            trait_arguments,
            generic_params: Vec::new(),
            methods: HashMap::new(),
            span: Span {
                line: 1,
                column: 1,
                end_column: 1,
                file: None,
            },
        }
    }

    #[test]
    fn generic_and_concrete_impl_targets_overlap() {
        assert!(impl_targets_overlap(
            &head(boxed(parameter("T"))),
            &head(boxed(Type::I64))
        ));
        assert!(impl_targets_overlap(
            &head(boxed(Type::I64)),
            &head(boxed(parameter("U")))
        ));
        assert!(impl_targets_overlap(
            &head(boxed(parameter("T"))),
            &head(boxed(parameter("U")))
        ));
    }

    #[test]
    fn distinct_outer_constructors_do_not_overlap() {
        assert!(!impl_targets_overlap(
            &head(boxed(parameter("T"))),
            &head(Type::I64)
        ));
        assert!(!impl_targets_overlap(
            &head(Type::Vector(Box::new(parameter("T")))),
            &head(boxed(parameter("U")))
        ));
    }

    #[test]
    fn indirect_recursive_targets_do_not_overlap() {
        let left = Type::Tuple(vec![parameter("T"), Type::Vector(Box::new(parameter("T")))]);
        let right = Type::Tuple(vec![parameter("U"), parameter("U")]);
        assert!(!impl_targets_overlap(
            &head(left.clone()),
            &head(right.clone())
        ));
        assert!(!impl_targets_overlap(&head(right), &head(left)));
        assert!(impl_targets_overlap(
            &head(boxed(parameter("T"))),
            &head(boxed(Type::I64))
        ));
    }

    #[test]
    fn repeated_and_cross_parameter_overlap_is_detected() {
        let left = Type::Tuple(vec![parameter("T"), parameter("T")]);
        let right = Type::Tuple(vec![parameter("U"), Type::I64]);
        assert!(impl_targets_overlap(&head(left), &head(right)));

        let left = Type::Tuple(vec![parameter("T"), Type::I64]);
        let right = Type::Tuple(vec![Type::Bool, parameter("U")]);
        assert!(impl_targets_overlap(&head(left), &head(right)));

        let left = Type::Tuple(vec![parameter("T"), parameter("T")]);
        let right = Type::Tuple(vec![Type::Bool, Type::I64]);
        assert!(!impl_targets_overlap(&head(left), &head(right)));
    }

    #[test]
    fn trait_arguments_participate_in_impl_overlap() {
        assert!(impl_targets_overlap(
            &head_with(Type::I64, vec![parameter("T")]),
            &head_with(Type::I64, vec![Type::Bool])
        ));
        assert!(impl_targets_overlap(
            &head_with(Type::I64, vec![parameter("T")]),
            &head_with(Type::I64, vec![Type::I64])
        ));
        assert!(impl_targets_overlap(
            &head_with(Type::I64, vec![parameter("T"), Type::Bool]),
            &head_with(Type::I64, vec![Type::I64, Type::Bool])
        ));
        assert!(!impl_targets_overlap(
            &head_with(Type::I64, vec![parameter("T"), Type::Bool]),
            &head_with(Type::I64, vec![parameter("U"), Type::I64])
        ));
    }

    #[test]
    fn self_substitution_is_recursive() {
        let target = boxed(Type::I64);
        let ty = Type::Tuple(vec![Type::SelfType, Type::Vector(Box::new(Type::SelfType))]);
        assert_eq!(
            substitute_self(&ty, &target),
            Type::Tuple(vec![target.clone(), Type::Vector(Box::new(target.clone()))])
        );
    }
}
