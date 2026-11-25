use super::inference::type_key;
use super::*;
use crate::modules::traits::{substitute_parameters, type_name};

impl Specializer {
    pub(super) fn check_generic_bounds(
        &mut self,
        parameters: &[GenericParam],
        arguments: &[Type],
        span: Span,
    ) -> Result<(), Diagnostic> {
        if parameters.len() != arguments.len() {
            return Err(Diagnostic::at(
                span.into(),
                "generic bound argument count does not match its parameter list",
            ));
        }
        let substitutions = parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments.iter().cloned())
            .collect::<Substitution>();
        for (parameter, argument) in parameters.iter().zip(arguments) {
            for bound in &parameter.bounds {
                let Some(trait_info) = self.traits.trait_by_name(&bound.name) else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "unknown trait `{}` in bound for `{}`",
                            bound.name, parameter.name
                        ),
                    ));
                };
                let trait_info = trait_info.clone();
                if bound.type_arguments.len() != trait_info.generic_params.len() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "trait `{}` expects {} type argument(s), got {}",
                            bound.name,
                            trait_info.generic_params.len(),
                            bound.type_arguments.len()
                        ),
                    ));
                }
                let bound_arguments = bound
                    .type_arguments
                    .iter()
                    .map(|bound_argument| {
                        self.normalize_type(bound_argument.clone(), &substitutions, span)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.prove_trait_bound(argument, trait_info.id, &bound_arguments, span)?;
            }
        }
        Ok(())
    }

    pub(super) fn prove_trait_bound(
        &mut self,
        self_type: &Type,
        trait_id: usize,
        trait_arguments: &[Type],
        span: Span,
    ) -> Result<usize, Diagnostic> {
        self.prove_trait_bound_selection(self_type, trait_id, trait_arguments, span)
            .map(|(impl_id, _)| impl_id)
    }

    fn prove_trait_bound_selection(
        &mut self,
        self_type: &Type,
        trait_id: usize,
        trait_arguments: &[Type],
        span: Span,
    ) -> Result<(usize, Substitution), Diagnostic> {
        let trait_info = self.traits.trait_info(trait_id).clone();
        if contains_unresolved_type(self_type)
            || trait_arguments.iter().any(contains_unresolved_type)
        {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "cannot resolve trait `{}` for unresolved type `{}`",
                    trait_description(&trait_info.name, trait_arguments),
                    short_type_name(self_type)
                ),
            ));
        }
        let key = trait_goal_key(trait_id, self_type, trait_arguments);
        if let Some((impl_id, substitutions)) = self.trait_selections.get(&key) {
            return Ok((*impl_id, substitutions.clone()));
        }
        if !self.active_trait_selections.insert(key.clone()) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "cyclic trait requirements while proving `{}` for `{}`",
                    trait_description(&trait_info.name, trait_arguments),
                    short_type_name(self_type)
                ),
            ));
        }
        if self.trait_proof_depth >= MAX_TRAIT_PROOF_DEPTH {
            self.active_trait_selections.remove(&key);
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "trait bound proof exceeded the limit of {MAX_TRAIT_PROOF_DEPTH} nested requirements while proving `{}` for `{}`",
                    trait_description(&trait_info.name, trait_arguments),
                    short_type_name(self_type)
                ),
            ));
        }
        self.trait_proof_depth += 1;
        let result = (|| {
            let (impl_id, substitutions) =
                self.select_impl(trait_id, self_type, trait_arguments, span)?;
            let trait_parameter_substitutions = trait_info
                .generic_params
                .iter()
                .map(|parameter| parameter.name.clone())
                .zip(trait_arguments.iter().cloned())
                .collect::<Substitution>();
            for (parameter, argument) in trait_info.generic_params.iter().zip(trait_arguments) {
                for bound in &parameter.bounds {
                    let bound_trait = self
                        .traits
                        .trait_by_name(&bound.name)
                        .ok_or_else(|| {
                            Diagnostic::at(span.into(), format!("unknown trait `{}`", bound.name))
                        })?
                        .id;
                    let bound_arguments = bound
                        .type_arguments
                        .iter()
                        .map(|bound_argument| {
                            self.normalize_type(
                                bound_argument.clone(),
                                &trait_parameter_substitutions,
                                span,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    self.prove_trait_bound(argument, bound_trait, &bound_arguments, span)?;
                }
            }
            let impl_info = self.traits.impl_info(impl_id).clone();
            for parameter in &impl_info.generic_params {
                let argument = substitutions.get(&parameter.name).ok_or_else(|| {
                    Diagnostic::at(
                        span.into(),
                        format!(
                            "cannot infer impl type parameter `{}` for `{}`",
                            parameter.name, trait_info.name
                        ),
                    )
                })?;
                for bound in &parameter.bounds {
                    let bound_trait = self
                        .traits
                        .trait_by_name(&bound.name)
                        .ok_or_else(|| {
                            Diagnostic::at(span.into(), format!("unknown trait `{}`", bound.name))
                        })?
                        .id;
                    let bound_arguments = bound
                        .type_arguments
                        .iter()
                        .map(|bound_argument| {
                            self.normalize_type(bound_argument.clone(), &substitutions, span)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    self.prove_trait_bound(argument, bound_trait, &bound_arguments, span)?;
                }
            }
            self.trait_selections
                .insert(key.clone(), (impl_id, substitutions.clone()));
            Ok((impl_id, substitutions))
        })();
        self.trait_proof_depth -= 1;
        self.active_trait_selections.remove(&key);
        result
    }

    fn select_impl(
        &self,
        trait_id: usize,
        self_type: &Type,
        trait_arguments: &[Type],
        span: Span,
    ) -> Result<(usize, Substitution), Diagnostic> {
        let trait_info = self.traits.trait_info(trait_id);
        let mut selected = None;
        for impl_id in self.traits.impls_for(trait_id) {
            let info = self.traits.impl_info(*impl_id);
            if info.trait_arguments.len() != trait_arguments.len() {
                continue;
            }
            let mut substitutions = Substitution::new();
            let target_matches = self
                .unify(&info.target, self_type, &mut substitutions, span)
                .is_ok();
            let arguments_match =
                target_matches
                    && info.trait_arguments.iter().zip(trait_arguments).all(
                        |(pattern, argument)| {
                            self.unify(pattern, argument, &mut substitutions, span)
                                .is_ok()
                        },
                    );
            if arguments_match {
                if selected.is_some() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "multiple implementations of trait `{}` apply to `{}`",
                            trait_description(&trait_info.name, trait_arguments),
                            short_type_name(self_type)
                        ),
                    ));
                }
                selected = Some((*impl_id, substitutions));
            }
        }
        selected.ok_or_else(|| {
            Diagnostic::at(
                span.into(),
                format!(
                    "type `{}` does not implement trait `{}`",
                    short_type_name(self_type),
                    trait_description(&trait_info.name, trait_arguments)
                ),
            )
        })
    }

    pub(super) fn process_trait_call(
        &mut self,
        expr: &mut Expr,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        let Expr::TraitCall {
            trait_name,
            trait_arguments,
            self_type,
            function_name,
            type_arguments: function_arguments,
            args,
            span,
        } = expr
        else {
            unreachable!("process_trait_call requires a trait call")
        };
        let trait_name = trait_name.clone();
        let function_name = function_name.clone();
        let explicit_trait_arguments = trait_arguments.clone();
        let explicit_self = self_type.clone();
        let explicit_function_arguments = function_arguments.clone();
        let call_span = *span;

        let trait_info = self
            .traits
            .trait_by_name(&trait_name)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at(call_span.into(), format!("unknown trait `{trait_name}`"))
            })?;
        let function = trait_info
            .functions
            .iter()
            .find(|function| function.name == function_name)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at(
                    call_span.into(),
                    format!("trait `{trait_name}` has no function `{function_name}`"),
                )
            })?;
        if args.len() != function.params.len() {
            return Err(Diagnostic::at(
                call_span.into(),
                format!(
                    "trait function `{}::{}` expects {} argument(s), got {}",
                    trait_name,
                    function_name,
                    function.params.len(),
                    args.len()
                ),
            ));
        }
        if !explicit_trait_arguments.is_empty()
            && explicit_trait_arguments.len() != trait_info.generic_params.len()
        {
            return Err(Diagnostic::at(
                call_span.into(),
                format!(
                    "trait `{trait_name}` expects {} type argument(s), got {}",
                    trait_info.generic_params.len(),
                    explicit_trait_arguments.len()
                ),
            ));
        }
        if !explicit_function_arguments.is_empty()
            && explicit_function_arguments.len() != function.generic_params.len()
        {
            return Err(Diagnostic::at(
                call_span.into(),
                format!(
                    "trait function `{}::{}` expects {} type argument(s), got {}",
                    trait_name,
                    function_name,
                    function.generic_params.len(),
                    explicit_function_arguments.len()
                ),
            ));
        }

        let explicit_self = explicit_self
            .map(|self_type| self.normalize_type(self_type, substitutions, call_span))
            .transpose()?;
        let explicit_trait_arguments = explicit_trait_arguments
            .into_iter()
            .map(|argument| self.normalize_type(argument, substitutions, call_span))
            .collect::<Result<Vec<_>, _>>()?;
        let explicit_function_arguments = explicit_function_arguments
            .into_iter()
            .map(|argument| self.normalize_type(argument, substitutions, call_span))
            .collect::<Result<Vec<_>, _>>()?;

        let mut inferred = Substitution::new();
        if let Some(self_type) = &explicit_self {
            inferred.insert("#Self".to_owned(), self_type.clone());
        }
        for (parameter, argument) in trait_info
            .generic_params
            .iter()
            .zip(&explicit_trait_arguments)
        {
            inferred.insert(parameter.name.clone(), argument.clone());
        }
        for (parameter, argument) in function
            .generic_params
            .iter()
            .zip(&explicit_function_arguments)
        {
            inferred.insert(parameter.name.clone(), argument.clone());
        }

        let mut formal_substitutions = HashMap::new();
        formal_substitutions.insert("Self".to_owned(), Type::Parameter("#Self".to_owned()));
        let formals = function
            .params
            .iter()
            .map(|parameter| substitute_parameters(&parameter.ty, &formal_substitutions))
            .collect::<Vec<_>>();

        for (index, argument) in args.iter().enumerate() {
            if let Some(actual) = self.infer_trait_argument_type(argument, variables) {
                let actual = self.normalize_type(actual, substitutions, call_span)?;
                self.note_trait_argument_evidence(
                    &formals[index],
                    &actual,
                    &mut inferred,
                    explicit_self.is_some(),
                    call_span,
                )?;
            }
        }

        let mut processed = vec![false; args.len()];
        loop {
            if processed.iter().all(|processed| *processed) {
                break;
            }
            let mut progress = false;
            for index in 0..args.len() {
                if processed[index] {
                    continue;
                }
                if let Some(expected) = instantiate_markers(&formals[index], &inferred) {
                    let expected = self.normalize_type(expected, substitutions, call_span)?;
                    self.process_expr_with_expected(
                        &mut args[index],
                        substitutions,
                        variables,
                        &expected,
                        call_span,
                    )?;
                    processed[index] = true;
                    progress = true;
                } else if self
                    .infer_trait_argument_type(&args[index], variables)
                    .is_some()
                {
                    self.process_expr(&mut args[index], substitutions, variables)?;
                    if let Some(actual) = self.infer_trait_argument_type(&args[index], variables) {
                        let actual = self.normalize_type(actual, substitutions, call_span)?;
                        self.note_trait_argument_evidence(
                            &formals[index],
                            &actual,
                            &mut inferred,
                            explicit_self.is_some(),
                            call_span,
                        )?;
                    }
                    processed[index] = true;
                    progress = true;
                }
            }
            if progress {
                continue;
            }
            let Some(index) = processed.iter().position(|processed| !*processed) else {
                break;
            };
            self.process_expr(&mut args[index], substitutions, variables)?;
            if let Some(actual) = self.infer_trait_argument_type(&args[index], variables) {
                let actual = self.normalize_type(actual, substitutions, call_span)?;
                self.note_trait_argument_evidence(
                    &formals[index],
                    &actual,
                    &mut inferred,
                    explicit_self.is_some(),
                    call_span,
                )?;
            }
            processed[index] = true;
        }

        let self_type = inferred.get("#Self").cloned().ok_or_else(|| {
            Diagnostic::at(
                call_span.into(),
                format!(
                    "cannot infer `Self` for `{}::{}`; use `<Type as {}<...>>::{}(...)`",
                    trait_name, function_name, trait_name, function_name
                ),
            )
        })?;
        let self_type = self.normalize_type(self_type, substitutions, call_span)?;
        if contains_unresolved_type(&self_type) {
            return Err(Diagnostic::at(
                call_span.into(),
                format!(
                    "cannot infer a concrete `Self` for `{}::{}`; use `<Type as {}<...>>::{}(...)`",
                    trait_name, function_name, trait_name, function_name
                ),
            ));
        }
        let trait_arguments = trait_info
            .generic_params
            .iter()
            .map(|parameter| {
                let argument = inferred.get(&parameter.name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        call_span.into(),
                        format!(
                            "cannot infer trait type argument `{}` for `{}::{}`; use `<Self as {}<...>>::{}(...)`",
                            parameter.name, trait_name, function_name, trait_name, function_name
                        ),
                    )
                })?;
                if contains_unresolved_type(&argument) {
                    return Err(Diagnostic::at(
                        call_span.into(),
                        format!(
                            "cannot infer a concrete trait type argument `{}` for `{}::{}`; use `<Self as {}<...>>::{}(...)`",
                            parameter.name, trait_name, function_name, trait_name, function_name
                        ),
                    ));
                }
                self.normalize_type(argument, substitutions, call_span)
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let function_arguments = function
            .generic_params
            .iter()
            .map(|parameter| {
                let argument = inferred.get(&parameter.name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        call_span.into(),
                        format!(
                            "cannot infer generic type parameter `{}` for `{}::{}`; provide explicit type arguments with `::<...>`",
                            parameter.name, trait_name, function_name
                        ),
                    )
                })?;
                if contains_unresolved_type(&argument) {
                    return Err(Diagnostic::at(
                        call_span.into(),
                        format!(
                            "cannot infer a concrete generic type parameter `{}` for `{}::{}`; provide explicit type arguments with `::<...>`",
                            parameter.name, trait_name, function_name
                        ),
                    ));
                }
                self.normalize_type(argument, substitutions, call_span)
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;

        let (impl_id, impl_substitutions) = self.prove_trait_bound_selection(
            &self_type,
            trait_info.id,
            &trait_arguments,
            call_span,
        )?;
        let impl_info = self.traits.impl_info(impl_id).clone();
        let internal_name = self
            .traits
            .method_function(impl_id, &function_name)
            .ok_or_else(|| {
                Diagnostic::at(
                    call_span.into(),
                    format!(
                        "implementation of `{}` for `{}` has no function `{function_name}`",
                        trait_name,
                        type_name(&impl_info.target)
                    ),
                )
            })?
            .to_owned();
        let mut type_arguments = impl_info
            .generic_params
            .iter()
            .map(|parameter| {
                impl_substitutions
                    .get(&parameter.name)
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::at(
                            call_span.into(),
                            format!(
                                "cannot infer impl type parameter `{}` for `{}`",
                                parameter.name, trait_name
                            ),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        type_arguments.extend(function_arguments);
        let concrete_name = if type_arguments.is_empty() {
            internal_name
        } else {
            self.instantiate_function(&internal_name, type_arguments, call_span)?
        };

        let expected_parameters = self
            .signatures
            .get(&concrete_name)
            .map(|(params, _)| params.clone())
            .ok_or_else(|| {
                Diagnostic::at(
                    call_span.into(),
                    format!(
                        "trait function `{}::{}` was not registered after specialization",
                        trait_name, function_name
                    ),
                )
            })?;
        for (index, (argument, expected)) in args.iter().zip(&expected_parameters).enumerate() {
            let Some(actual) = self.infer_expr_type(argument, variables) else {
                continue;
            };
            let expected = self.normalize_type(expected.clone(), substitutions, call_span)?;
            let actual = self.normalize_type(actual, substitutions, call_span)?;
            let mut checked = Substitution::new();
            if self
                .unify(&expected, &actual, &mut checked, call_span)
                .is_err()
            {
                return Err(Diagnostic::at(
                    call_span.into(),
                    format!(
                        "argument {} to `{}::{}` has type `{}`, expected `{}`",
                        index + 1,
                        trait_name,
                        function_name,
                        type_name(&actual),
                        type_name(&expected)
                    ),
                ));
            }
        }

        let payload = std::mem::take(args);
        *expr = Expr::Call {
            name: concrete_name,
            type_arguments: Vec::new(),
            args: payload,
            type_arguments_before_path: false,
            span: call_span,
        };
        Ok(())
    }

    fn note_trait_argument_evidence(
        &self,
        formal: &Type,
        actual: &Type,
        inferred: &mut Substitution,
        explicit_self: bool,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if contains_unresolved_type(actual) {
            return Ok(());
        }
        match self.unify(formal, actual, inferred, span) {
            Ok(()) => Ok(()),
            // An explicit `Self` wins over conflicting argument evidence; the
            // concrete call checker reports the mismatching argument instead of
            // a confusing inference conflict.
            Err(error) if explicit_self && error.message.contains("`#Self`") => Ok(()),
            Err(error) => Err(map_self_marker(error)),
        }
    }

    fn infer_trait_argument_type(
        &self,
        expr: &Expr,
        variables: &HashMap<String, Type>,
    ) -> Option<Type> {
        match expr {
            Expr::Struct {
                name,
                type_arguments,
                ..
            } if !type_arguments.is_empty() => {
                Some(Type::Applied(name.clone(), type_arguments.clone()))
            }
            Expr::Variant {
                enum_name,
                type_arguments,
                ..
            } if !type_arguments.is_empty() => {
                Some(Type::Applied(enum_name.clone(), type_arguments.clone()))
            }
            Expr::Struct {
                name,
                type_arguments,
                fields,
                span,
            } if type_arguments.is_empty() && self.generic_structs.contains_key(name) => {
                let template = self.generic_structs.get(name)?;
                let mut inferred = Substitution::new();
                for (field_name, value, _) in fields {
                    let field = template
                        .fields
                        .iter()
                        .find(|field| field.name == *field_name)?;
                    let actual = self.infer_trait_argument_type(value, variables)?;
                    self.unify(&field.ty, &actual, &mut inferred, *span).ok()?;
                }
                let arguments = template
                    .generic_params
                    .iter()
                    .map(|parameter| inferred.get(&parameter.name).cloned())
                    .collect::<Option<Vec<_>>>()?;
                Some(Type::Applied(name.clone(), arguments))
            }
            Expr::Variant {
                enum_name,
                type_arguments,
                variant_name,
                payload,
                span,
            } if self.generic_enums.contains_key(enum_name) => {
                if !type_arguments.is_empty() {
                    return Some(Type::Applied(enum_name.clone(), type_arguments.clone()));
                }
                let template = self.generic_enums.get(enum_name)?;
                let variant = template
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)?;
                let payload = payload.as_ref()?;
                if payload.len() != variant.payload.len() {
                    return None;
                }
                let mut inferred = Substitution::new();
                for (formal, value) in variant.payload.iter().zip(payload) {
                    let actual = self.infer_trait_argument_type(value, variables)?;
                    self.unify(formal, &actual, &mut inferred, *span).ok()?;
                }
                let arguments = template
                    .generic_params
                    .iter()
                    .map(|parameter| inferred.get(&parameter.name).cloned())
                    .collect::<Option<Vec<_>>>()?;
                Some(Type::Applied(enum_name.clone(), arguments))
            }
            _ => self.infer_expr_type(expr, variables),
        }
    }
}

fn trait_description(name: &str, arguments: &[Type]) -> String {
    if arguments.is_empty() {
        name.to_owned()
    } else {
        format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

fn short_type_name(ty: &Type) -> String {
    const MAX_LENGTH: usize = 80;
    let name = type_name(ty);
    if name.chars().count() <= MAX_LENGTH {
        name
    } else {
        let mut shortened = name.chars().take(MAX_LENGTH).collect::<String>();
        shortened.push_str("...");
        shortened
    }
}

fn trait_goal_key(
    trait_id: usize,
    self_type: &Type,
    trait_arguments: &[Type],
) -> (usize, String, Vec<String>) {
    (
        trait_id,
        type_key(self_type),
        trait_arguments.iter().map(type_key).collect(),
    )
}

/// Substitute inferred arguments into a marker-bearing signature type, failing
/// while any inference variable is still unresolved.
fn instantiate_markers(ty: &Type, inferred: &Substitution) -> Option<Type> {
    Some(match ty {
        Type::Parameter(name) => {
            let value = inferred.get(name)?;
            if contains_unresolved_type(value) {
                return None;
            }
            value.clone()
        }
        Type::Array(element, length) => {
            Type::Array(Box::new(instantiate_markers(element, inferred)?), *length)
        }
        Type::Vector(element) => Type::Vector(Box::new(instantiate_markers(element, inferred)?)),
        Type::Reference(element, lifetime) => Type::Reference(
            Box::new(instantiate_markers(element, inferred)?),
            lifetime.clone(),
        ),
        Type::MutableReference(element, lifetime) => Type::MutableReference(
            Box::new(instantiate_markers(element, inferred)?),
            lifetime.clone(),
        ),
        Type::RawPointer(element, mutable) => {
            Type::RawPointer(Box::new(instantiate_markers(element, inferred)?), *mutable)
        }
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| instantiate_markers(element, inferred))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Function(params, result) => Type::Function(
            params
                .iter()
                .map(|param| instantiate_markers(param, inferred))
                .collect::<Option<Vec<_>>>()?,
            Box::new(instantiate_markers(result, inferred)?),
        ),
        Type::Applied(name, arguments) => Type::Applied(
            name.clone(),
            arguments
                .iter()
                .map(|argument| instantiate_markers(argument, inferred))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::SelfType => return None,
        other => other.clone(),
    })
}

fn map_self_marker(mut error: Diagnostic) -> Diagnostic {
    error.message = error
        .message
        .replace("generic type parameter `#Self`", "trait `Self`")
        .replace("#Self", "Self");
    error
}

fn contains_unresolved_type(ty: &Type) -> bool {
    match ty {
        Type::Parameter(_) | Type::SelfType => true,
        Type::Array(element, _) | Type::Vector(element) => contains_unresolved_type(element),
        Type::Reference(element, _)
        | Type::MutableReference(element, _)
        | Type::RawPointer(element, _) => contains_unresolved_type(element),
        Type::Tuple(elements) => elements.iter().any(contains_unresolved_type),
        Type::Function(params, result) => {
            params.iter().any(contains_unresolved_type) || contains_unresolved_type(result)
        }
        Type::Applied(_, arguments) => arguments.iter().any(contains_unresolved_type),
        Type::I64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Named(_) => false,
    }
}
