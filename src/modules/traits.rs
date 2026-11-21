use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct TraitInfo {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) functions: Vec<TraitFunction>,
    pub(crate) is_public: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ImplInfo {
    pub(crate) target: Type,
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
    validate_declaration_types(program, &trait_declarations, &impl_declarations)?;
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
            name: declaration.name,
            functions: declaration.functions,
            is_public: declaration.is_public,
        });
        registry.by_trait.push(Vec::new());
    }

    validate_generic_bounds_for_program(program, &registry)?;

    let mut synthetic_functions = Vec::new();
    for (id, declaration) in impl_declarations.into_iter().enumerate() {
        let Some(trait_id) = registry.trait_names.get(&declaration.trait_name).copied() else {
            return Err(at(
                declaration.trait_span,
                format!("unknown trait `{}`", declaration.trait_name),
            ));
        };
        validate_impl_target(&declaration)?;
        validate_impl_bounds(&declaration, &registry)?;
        let info = validate_and_lower_impl(
            id,
            trait_id,
            declaration,
            &registry,
            &mut synthetic_functions,
        )?;
        registry.by_trait[trait_id].push(id);
        registry.impls.push(info);
    }

    reject_overlapping_impls(&registry)?;
    program.functions.extend(synthetic_functions);
    Ok(registry)
}

fn validate_declaration_types(
    program: &Program,
    traits: &[TraitDecl],
    impls: &[ImplDecl],
) -> Result<(), Diagnostic> {
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
    let shapes = TypeShapes {
        generic_arity,
        known_types,
    };

    for declaration in traits {
        for function in &declaration.functions {
            for param in &function.params {
                validate_type_shape(&param.ty, &HashSet::new(), true, &shapes, param.span)?;
            }
            validate_type_shape(
                &function.return_type,
                &HashSet::new(),
                true,
                &shapes,
                function.span,
            )?;
        }
    }
    for declaration in impls {
        let parameters = declaration
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<HashSet<_>>();
        validate_type_shape(
            &declaration.target,
            &parameters,
            false,
            &shapes,
            declaration.target_span,
        )?;
        for function in &declaration.functions {
            for param in &function.params {
                validate_type_shape(&param.ty, &parameters, true, &shapes, param.span)?;
            }
            validate_type_shape(
                &function.return_type,
                &parameters,
                true,
                &shapes,
                function.span,
            )?;
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
                validate_type_shape(param, parameters, self_allowed, shapes, span)?;
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

fn validate_generic_bounds_for_program(
    program: &Program,
    registry: &TraitRegistry,
) -> Result<(), Diagnostic> {
    for declaration in &program.structs {
        validate_bounds(&declaration.generic_params, declaration.is_public, registry)?;
    }
    for declaration in &program.enums {
        validate_bounds(&declaration.generic_params, declaration.is_public, registry)?;
    }
    for function in &program.functions {
        validate_bounds(&function.generic_params, function.is_public, registry)?;
    }
    Ok(())
}

fn validate_impl_bounds(
    declaration: &ImplDecl,
    registry: &TraitRegistry,
) -> Result<(), Diagnostic> {
    validate_bounds(&declaration.generic_params, false, registry)
}

fn validate_bounds(
    parameters: &[GenericParam],
    is_public: bool,
    registry: &TraitRegistry,
) -> Result<(), Diagnostic> {
    for parameter in parameters {
        let mut seen = HashSet::new();
        for bound in &parameter.bounds {
            if !seen.insert(bound.name.as_str()) {
                return Err(at(
                    bound.span,
                    format!(
                        "trait `{}` is listed more than once in the bounds for `{}`",
                        bound.name, parameter.name
                    ),
                ));
            }
            let Some(trait_info) = registry.trait_by_name(&bound.name) else {
                return Err(at(
                    bound.span,
                    format!(
                        "unknown trait `{}` in bound for `{}`",
                        bound.name, parameter.name
                    ),
                ));
            };
            if is_public && !trait_info.is_public {
                return Err(at(
                    bound.span,
                    format!(
                        "private trait `{}` cannot appear in a public trait bound",
                        bound.name
                    ),
                ));
            }
        }
    }
    Ok(())
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
    for parameter in &declaration.generic_params {
        if !used.contains(parameter.name.as_str()) {
            return Err(at(
                parameter.span,
                format!(
                    "impl type parameter `{}` is not constrained by the impl target",
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
    id: usize,
    trait_id: usize,
    mut declaration: ImplDecl,
    registry: &TraitRegistry,
    synthetic_functions: &mut Vec<Function>,
) -> Result<ImplInfo, Diagnostic> {
    let trait_info = registry.trait_info(trait_id);
    let mut by_name = HashMap::new();
    for required in &trait_info.functions {
        by_name.insert(required.name.as_str(), required);
    }
    let mut seen = HashSet::new();
    let mut methods = HashMap::new();
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
            let expected = substitute_self(&expected.ty, &declaration.target);
            if actual_ty != expected {
                return Err(at(
                    actual.span,
                    format!(
                        "parameter `{}` of `{}::{}` has type `{}`, expected `{}`",
                        actual.name,
                        trait_info.name,
                        function.name,
                        type_name(&actual_ty),
                        type_name(&expected)
                    ),
                ));
            }
        }
        let actual_return = substitute_self(&function.return_type, &declaration.target);
        let expected_return = substitute_self(&required.return_type, &declaration.target);
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

        let internal_name = format!("impl#{id}::{}", function.name);
        methods.insert(function.name.clone(), internal_name.clone());
        function.name = internal_name;
        function.generic_params = declaration.generic_params.clone();
        function.is_public = true;
        for param in &mut function.params {
            param.ty = substitute_self(&param.ty, &declaration.target);
        }
        function.return_type = substitute_self(&function.return_type, &declaration.target);
        synthetic_functions.push(function.clone());
    }
    for required in &trait_info.functions {
        if !seen.contains(&required.name) {
            return Err(at(
                declaration.span,
                format!(
                    "implementation of `{}` for `{}` is missing required function `{}`",
                    trait_info.name,
                    type_name(&declaration.target),
                    required.name
                ),
            ));
        }
    }
    Ok(ImplInfo {
        target: declaration.target,
        generic_params: declaration.generic_params,
        methods,
        span: declaration.span,
    })
}

pub(crate) fn substitute_self(ty: &Type, target: &Type) -> Type {
    match ty {
        Type::SelfType => target.clone(),
        Type::Array(element, length) => {
            Type::Array(Box::new(substitute_self(element, target)), *length)
        }
        Type::Vector(element) => Type::Vector(Box::new(substitute_self(element, target))),
        Type::Reference(element, lifetime) => {
            Type::Reference(Box::new(substitute_self(element, target)), lifetime.clone())
        }
        Type::MutableReference(element, lifetime) => {
            Type::MutableReference(Box::new(substitute_self(element, target)), lifetime.clone())
        }
        Type::RawPointer(element, mutable) => {
            Type::RawPointer(Box::new(substitute_self(element, target)), *mutable)
        }
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute_self(element, target))
                .collect(),
        ),
        Type::Function(params, result) => Type::Function(
            params
                .iter()
                .map(|param| substitute_self(param, target))
                .collect(),
            Box::new(substitute_self(result, target)),
        ),
        Type::Applied(name, arguments) => Type::Applied(
            name.clone(),
            arguments
                .iter()
                .map(|argument| substitute_self(argument, target))
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
                if impl_targets_overlap(&left.target, &right.target) {
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

fn impl_targets_overlap(left: &Type, right: &Type) -> bool {
    let mut left_names = HashMap::new();
    let mut right_names = HashMap::new();
    let left = rename_overlap_variables(left, "L", &mut left_names);
    let right = rename_overlap_variables(right, "R", &mut right_names);
    unify_overlap(&left, &right, &mut HashMap::new()).is_ok()
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
    if let Type::Parameter(name) = ty
        && name == variable
    {
        return Ok(());
    }
    if let Some(previous) = bindings.get(variable).cloned() {
        return unify_overlap(&previous, ty, bindings);
    }
    if occurs_overlap(variable, ty) {
        return Err(());
    }
    bindings.insert(variable.to_owned(), ty.clone());
    Ok(())
}

fn occurs_overlap(variable: &str, ty: &Type) -> bool {
    match ty {
        Type::Parameter(name) => name == variable,
        Type::Array(element, _) | Type::Vector(element) => occurs_overlap(variable, element),
        Type::Reference(element, _)
        | Type::MutableReference(element, _)
        | Type::RawPointer(element, _) => occurs_overlap(variable, element),
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| occurs_overlap(variable, element)),
        Type::Function(params, result) => {
            params.iter().any(|param| occurs_overlap(variable, param))
                || occurs_overlap(variable, result)
        }
        Type::Applied(_, arguments) => arguments
            .iter()
            .any(|argument| occurs_overlap(variable, argument)),
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

    #[test]
    fn generic_and_concrete_impl_targets_overlap() {
        assert!(impl_targets_overlap(
            &boxed(parameter("T")),
            &boxed(Type::I64)
        ));
        assert!(impl_targets_overlap(
            &boxed(Type::I64),
            &boxed(parameter("U"))
        ));
        assert!(impl_targets_overlap(
            &boxed(parameter("T")),
            &boxed(parameter("U"))
        ));
    }

    #[test]
    fn distinct_outer_constructors_do_not_overlap() {
        assert!(!impl_targets_overlap(&boxed(parameter("T")), &Type::I64));
        assert!(!impl_targets_overlap(
            &Type::Vector(Box::new(parameter("T"))),
            &boxed(parameter("U"))
        ));
    }

    #[test]
    fn repeated_and_cross_parameter_overlap_is_detected() {
        let left = Type::Tuple(vec![parameter("T"), parameter("T")]);
        let right = Type::Tuple(vec![parameter("U"), Type::I64]);
        assert!(impl_targets_overlap(&left, &right));

        let left = Type::Tuple(vec![parameter("T"), Type::I64]);
        let right = Type::Tuple(vec![Type::Bool, parameter("U")]);
        assert!(impl_targets_overlap(&left, &right));

        let left = Type::Tuple(vec![parameter("T"), parameter("T")]);
        let right = Type::Tuple(vec![Type::Bool, Type::I64]);
        assert!(!impl_targets_overlap(&left, &right));
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
