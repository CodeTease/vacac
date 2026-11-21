use super::inference::type_key;
use super::*;
use crate::modules::traits::{TraitInfo, substitute_self, type_name};

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
                let trait_id = trait_info.id;
                self.prove_trait_bound(argument, trait_id, span)?;
            }
        }
        Ok(())
    }

    pub(super) fn prove_trait_bound(
        &mut self,
        self_type: &Type,
        trait_id: usize,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        self.prove_trait_bound_selection(self_type, trait_id, span)
            .map(|(impl_id, _)| impl_id)
    }

    fn prove_trait_bound_selection(
        &mut self,
        self_type: &Type,
        trait_id: usize,
        span: Span,
    ) -> Result<(usize, Substitution), Diagnostic> {
        let trait_info = self.traits.trait_info(trait_id).clone();
        if contains_unresolved_type(self_type) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "cannot resolve trait `{}` for unresolved type `{}`",
                    trait_info.name,
                    type_name(self_type)
                ),
            ));
        }
        let key = (trait_id, type_key(self_type));
        if let Some((impl_id, substitutions)) = self.trait_selections.get(&key) {
            return Ok((*impl_id, substitutions.clone()));
        }
        if !self.active_trait_selections.insert(key.clone()) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "cyclic trait requirements while proving `{}` for `{}`",
                    trait_info.name,
                    type_name(self_type)
                ),
            ));
        }
        let result = (|| {
            let (impl_id, substitutions) = self.select_impl(trait_id, self_type, span)?;
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
                    self.prove_trait_bound(argument, bound_trait, span)?;
                }
            }
            self.trait_selections
                .insert(key.clone(), (impl_id, substitutions.clone()));
            Ok((impl_id, substitutions))
        })();
        self.active_trait_selections.remove(&key);
        result
    }

    fn select_impl(
        &self,
        trait_id: usize,
        self_type: &Type,
        span: Span,
    ) -> Result<(usize, Substitution), Diagnostic> {
        let trait_info = self.traits.trait_info(trait_id);
        let mut selected = None;
        for impl_id in self.traits.impls_for(trait_id) {
            let info = self.traits.impl_info(*impl_id);
            let mut substitutions = Substitution::new();
            if self
                .unify(&info.target, self_type, &mut substitutions, span)
                .is_ok()
            {
                if selected.is_some() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "multiple implementations of trait `{}` apply to `{}`",
                            trait_info.name,
                            type_name(self_type)
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
                    type_name(self_type),
                    trait_info.name
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
            self_type,
            function_name,
            args,
            span,
        } = expr
        else {
            unreachable!("process_trait_call requires a trait call")
        };
        let trait_name = trait_name.clone();
        let function_name = function_name.clone();
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

        let explicit_self = match self_type.clone() {
            Some(self_type) => Some(self.normalize_type(self_type, substitutions, call_span)?),
            None => None,
        };
        let inferred_before_processing = if explicit_self.is_none() {
            self.infer_call_self(
                &trait_info,
                &function,
                args,
                variables,
                substitutions,
                call_span,
            )
            .ok()
        } else {
            None
        };
        let expected_self = explicit_self
            .clone()
            .or_else(|| inferred_before_processing.clone());
        if let Some(expected_self) = expected_self {
            for (argument, parameter) in args.iter_mut().zip(&function.params) {
                let expected = substitute_self(&parameter.ty, &expected_self);
                let expected = self.normalize_type(expected, substitutions, call_span)?;
                self.process_expr_with_expected(
                    argument,
                    substitutions,
                    variables,
                    &expected,
                    call_span,
                )?;
            }
        } else {
            for argument in args.iter_mut() {
                self.process_expr(argument, substitutions, variables)?;
            }
        }

        let self_type = match explicit_self {
            Some(self_type) => self_type,
            None => self.infer_call_self(
                &trait_info,
                &function,
                args,
                variables,
                substitutions,
                call_span,
            )?,
        };
        let (impl_id, impl_substitutions) =
            self.prove_trait_bound_selection(&self_type, trait_info.id, call_span)?;
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
        let type_arguments = impl_info
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
            span: call_span,
        };
        Ok(())
    }

    fn infer_call_self(
        &mut self,
        trait_info: &TraitInfo,
        function: &TraitFunction,
        args: &[Expr],
        variables: &HashMap<String, Type>,
        substitutions: &Substitution,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        if args.len() != function.params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "trait function `{}::{}` expects {} argument(s), got {}",
                    trait_info.name,
                    function.name,
                    function.params.len(),
                    args.len()
                ),
            ));
        }
        let marker = Type::Parameter("#Self".into());
        let mut inferred = Substitution::new();
        for (parameter, argument) in function.params.iter().zip(args) {
            if let Some(actual) = self.infer_trait_argument_type(argument, variables) {
                let actual = self.normalize_type(actual, substitutions, span)?;
                let expected = substitute_self(&parameter.ty, &marker);
                self.unify(&expected, &actual, &mut inferred, span)
                    .map_err(|mut error| {
                        error.message = error
                            .message
                            .replace("generic type parameter `#Self`", "trait `Self`")
                            .replace("#Self", "Self");
                        error
                    })?;
            }
        }
        let self_type = inferred.remove("#Self").ok_or_else(|| {
            Diagnostic::at(
                span.into(),
                format!(
                    "cannot infer `Self` for `{}::{}`; use `<Type as {}>::{}(...)`",
                    trait_info.name, function.name, trait_info.name, function.name
                ),
            )
        })?;
        if contains_unresolved_type(&self_type) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "cannot infer a concrete `Self` for `{}::{}`; use `<Type as {}>::{}(...)`",
                    trait_info.name, function.name, trait_info.name, function.name
                ),
            ));
        }
        Ok(self_type)
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
