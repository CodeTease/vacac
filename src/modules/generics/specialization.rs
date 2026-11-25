use super::inference::type_key;
use super::*;
impl Specializer {
    /// Rewrite pattern names and recurse using the type at each position, so a
    /// nested pattern inside a generic payload sees the concrete variant or field
    /// layout it must match.
    pub(super) fn normalize_pattern_typed(
        &mut self,
        pattern: &mut MatchPattern,
        ty: Option<&Type>,
        substitutions: &Substitution,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Tuple(patterns) => {
                let elements = match ty {
                    Some(Type::Tuple(elements)) => Some(elements.clone()),
                    _ => None,
                };
                for (index, pattern) in patterns.iter_mut().enumerate() {
                    let element = elements
                        .as_ref()
                        .and_then(|elements| elements.get(index))
                        .cloned();
                    self.normalize_pattern_typed(pattern, element.as_ref(), substitutions, span)?;
                }
            }
            MatchPattern::Array(patterns) => {
                let element = match ty {
                    Some(Type::Array(element, _)) => Some((**element).clone()),
                    _ => None,
                };
                for pattern in patterns {
                    self.normalize_pattern_typed(pattern, element.as_ref(), substitutions, span)?;
                }
            }
            MatchPattern::Struct { name, fields, .. } => {
                if let Some(concrete) = self.match_struct_instance(name, ty) {
                    *name = concrete;
                }
                let declaration = self
                    .concrete_structs
                    .get(name)
                    .or_else(|| {
                        self.generated_structs
                            .iter()
                            .find(|declaration| declaration.name == *name)
                    })
                    .cloned();
                if let Some(declaration) = declaration {
                    for (field_name, pattern, field_span) in fields.iter_mut() {
                        let field_ty = declaration
                            .fields
                            .iter()
                            .find(|field| field.name == *field_name)
                            .map(|field| {
                                self.normalize_type(field.ty.clone(), substitutions, *field_span)
                            })
                            .transpose()?;
                        self.normalize_pattern_typed(
                            pattern,
                            field_ty.as_ref(),
                            substitutions,
                            *field_span,
                        )?;
                    }
                } else {
                    for (_, pattern, field_span) in fields.iter_mut() {
                        self.normalize_pattern_typed(pattern, None, substitutions, *field_span)?;
                    }
                }
            }
            MatchPattern::Variant {
                enum_name,
                variant_name,
                bindings,
            } => {
                if let Some(concrete) = self.match_enum_instance(enum_name, ty) {
                    *enum_name = concrete;
                }
                let payload_types = self
                    .enum_declarations
                    .get(enum_name)
                    .and_then(|declaration| {
                        declaration
                            .variants
                            .iter()
                            .find(|variant| variant.name == *variant_name)
                            .map(|variant| variant.payload.clone())
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|payload| self.normalize_type(payload, substitutions, span))
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(patterns) = bindings {
                    for (pattern, payload) in patterns.iter_mut().zip(payload_types.iter()) {
                        self.normalize_pattern_typed(pattern, Some(payload), substitutions, span)?;
                    }
                }
            }
            MatchPattern::ImportedVariant { bindings, .. } => {
                for pattern in bindings {
                    self.normalize_pattern_typed(pattern, None, substitutions, span)?;
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

    /// Adopt a concrete struct instance when the expected type names the same
    /// template as the pattern.
    fn match_struct_instance(&self, name: &str, ty: Option<&Type>) -> Option<String> {
        let Some(Type::Named(concrete)) = ty else {
            return None;
        };
        if concrete == name {
            return None;
        }
        let expected_template = self
            .struct_instances
            .get(concrete)
            .map(|(template, _)| template.as_str())?;
        let pattern_template = self
            .struct_instances
            .get(name)
            .map(|(template, _)| template.as_str())
            .unwrap_or(name);
        (pattern_template == expected_template).then(|| concrete.clone())
    }

    /// Adopt a concrete enum instance when the expected type names the same
    /// template as the pattern.
    fn match_enum_instance(&self, name: &str, ty: Option<&Type>) -> Option<String> {
        let Some(Type::Named(concrete)) = ty else {
            return None;
        };
        if concrete == name {
            return None;
        }
        let expected_template = self
            .enum_instances
            .get(concrete)
            .map(|(template, _)| template.as_str())?;
        let pattern_template = self
            .enum_instances
            .get(name)
            .map(|(template, _)| template.as_str())
            .unwrap_or(name);
        (pattern_template == expected_template).then(|| concrete.clone())
    }

    /// Rewrite a match-arm variant pattern to the scrutinee's concrete enum
    /// instance when both name the same enum template.
    pub(super) fn specialize_variant_pattern(
        &self,
        enum_name: &mut String,
        concrete_name: &str,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some((template, _)) = self.enum_instances.get(concrete_name) else {
            return Ok(());
        };
        let pattern_template = self
            .enum_instances
            .get(enum_name)
            .map(|(template, _)| template.as_str())
            .unwrap_or(enum_name.as_str());
        if pattern_template != template {
            return Err(Diagnostic::at(
                span.into(),
                format!("match pattern uses enum `{enum_name}`, expected `{concrete_name}`"),
            ));
        }
        *enum_name = concrete_name.to_owned();
        Ok(())
    }

    pub(super) fn infer_pattern_bindings(
        &self,
        pattern: &mut MatchPattern,
        ty: &Type,
        variables: &mut HashMap<String, Type>,
    ) {
        match pattern {
            MatchPattern::Binding(name) => {
                variables.insert(name.clone(), ty.clone());
            }
            MatchPattern::Tuple(patterns) => {
                if let Type::Tuple(types) = ty {
                    for (pattern, ty) in patterns.iter_mut().zip(types) {
                        self.infer_pattern_bindings(pattern, ty, variables);
                    }
                }
            }
            MatchPattern::Array(patterns) => {
                if let Type::Array(element, _) = ty {
                    for pattern in patterns {
                        self.infer_pattern_bindings(pattern, element, variables);
                    }
                }
            }
            MatchPattern::Struct { name, fields, .. } => {
                if let Type::Named(actual_name) = ty
                    && (name.rsplit("::").next() == actual_name.rsplit("::").next()
                        || actual_name.starts_with(&format!("{name}::<")))
                {
                    *name = actual_name.clone();
                }
                let declaration = self.concrete_structs.get(name).cloned().or_else(|| {
                    self.generated_structs
                        .iter()
                        .find(|declaration| declaration.name == *name)
                        .cloned()
                });
                if let Some(declaration) = declaration {
                    for (field_name, pattern, _) in fields {
                        if let Some(field) = declaration
                            .fields
                            .iter()
                            .find(|field| field.name == *field_name)
                        {
                            self.infer_pattern_bindings(pattern, &field.ty, variables);
                        }
                    }
                }
            }
            MatchPattern::Variant {
                enum_name,
                variant_name,
                bindings: Some(patterns),
            } => {
                let variant = self
                    .enum_declarations
                    .get(enum_name)
                    .and_then(|declaration| {
                        declaration
                            .variants
                            .iter()
                            .find(|variant| variant.name == *variant_name)
                            .cloned()
                    });
                if let Some(variant) = variant {
                    for (pattern, ty) in patterns.iter_mut().zip(&variant.payload) {
                        self.infer_pattern_bindings(pattern, ty, variables);
                    }
                }
            }
            MatchPattern::ImportedVariant { bindings, .. } => {
                for pattern in bindings {
                    self.infer_pattern_bindings(pattern, ty, variables);
                }
            }
            MatchPattern::Wildcard
            | MatchPattern::Bool(_)
            | MatchPattern::I64(_)
            | MatchPattern::F32(_)
            | MatchPattern::F64(_)
            | MatchPattern::Variant { bindings: None, .. } => {}
        }
    }

    pub(super) fn new_instance(&mut self, span: Span) -> Result<(), Diagnostic> {
        self.instance_count += 1;
        if self.instance_count > MAX_INSTANCES {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic specialization exceeded the limit of {MAX_INSTANCES} concrete instances"
                ),
            ));
        }
        Ok(())
    }

    pub(super) fn normalize_type(
        &mut self,
        ty: Type,
        substitutions: &Substitution,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        Ok(match ty {
            Type::Parameter(name) => {
                let replacement = substitutions.get(&name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        span.into(),
                        format!("unresolved generic type parameter `{name}`"),
                    )
                })?;
                self.normalize_type(replacement, substitutions, span)?
            }
            Type::Applied(name, arguments) => {
                let arguments = arguments
                    .into_iter()
                    .map(|argument| self.normalize_type(argument, substitutions, span))
                    .collect::<Result<Vec<_>, _>>()?;
                if self.generic_structs.contains_key(&name) {
                    self.instantiate_struct(&name, arguments, span)?
                } else if self.generic_enums.contains_key(&name) {
                    self.instantiate_enum(&name, arguments, span)?
                } else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`{name}` is not a generic type"),
                    ));
                }
            }
            Type::Named(name) if self.generic_structs.contains_key(&name) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic struct `{name}` requires explicit type arguments"),
                ));
            }
            Type::Named(name) if self.generic_enums.contains_key(&name) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic enum `{name}` requires explicit type arguments"),
                ));
            }
            Type::Array(element, length) => Type::Array(
                Box::new(self.normalize_type(*element, substitutions, span)?),
                length,
            ),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .into_iter()
                    .map(|element| self.normalize_type(element, substitutions, span))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Type::Function(params, result) => Type::Function(
                params
                    .into_iter()
                    .map(|param| self.normalize_type(param, substitutions, span))
                    .collect::<Result<Vec<_>, _>>()?,
                Box::new(self.normalize_type(*result, substitutions, span)?),
            ),
            Type::Vector(element) => Type::Vector(Box::new(self.normalize_type(
                *element,
                substitutions,
                span,
            )?)),
            Type::Reference(element, lifetime) => Type::Reference(
                Box::new(self.normalize_type(*element, substitutions, span)?),
                lifetime,
            ),
            Type::MutableReference(element, lifetime) => Type::MutableReference(
                Box::new(self.normalize_type(*element, substitutions, span)?),
                lifetime,
            ),
            Type::RawPointer(element, mutable) => Type::RawPointer(
                Box::new(self.normalize_type(*element, substitutions, span)?),
                mutable,
            ),
            other => other,
        })
    }

    pub(super) fn instantiate_struct(
        &mut self,
        template_name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        let Some(template) = self.generic_structs.get(template_name).cloned() else {
            return Err(Diagnostic::at(
                span.into(),
                format!("`{template_name}` is not a generic struct"),
            ));
        };
        if arguments.len() != template.generic_params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic struct `{template_name}` expects {} type argument(s), got {}",
                    template.generic_params.len(),
                    arguments.len()
                ),
            ));
        }
        self.check_generic_bounds(&template.generic_params, &arguments, span)?;
        let concrete_name = format!(
            "{template_name}::<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        );
        if self.struct_instances.contains_key(&concrete_name) {
            return Ok(Type::Named(concrete_name));
        }
        self.new_instance(span)?;
        self.struct_instances.insert(
            concrete_name.clone(),
            (template_name.to_owned(), arguments.clone()),
        );
        let substitutions = template
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments.clone())
            .collect::<Substitution>();
        let mut concrete = template;
        concrete.name = concrete_name.clone();
        concrete.generic_params.clear();
        for field in &mut concrete.fields {
            field.ty = self.normalize_type(field.ty.clone(), &substitutions, field.span)?;
        }
        self.generated_structs.push(concrete);
        Ok(Type::Named(concrete_name))
    }

    pub(super) fn instantiate_enum(
        &mut self,
        template_name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        let Some(template) = self.generic_enums.get(template_name).cloned() else {
            return Err(Diagnostic::at(
                span.into(),
                format!("`{template_name}` is not a generic enum"),
            ));
        };
        if arguments.len() != template.generic_params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic enum `{template_name}` expects {} type argument(s), got {}",
                    template.generic_params.len(),
                    arguments.len()
                ),
            ));
        }
        self.check_generic_bounds(&template.generic_params, &arguments, span)?;
        let concrete_name = format!(
            "{template_name}::<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        );
        if self.enum_instances.contains_key(&concrete_name) {
            return Ok(Type::Named(concrete_name));
        }
        self.new_instance(span)?;
        self.enum_instances.insert(
            concrete_name.clone(),
            (template_name.to_owned(), arguments.clone()),
        );
        let substitutions = template
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments)
            .collect::<Substitution>();
        let mut concrete = template;
        concrete.name = concrete_name.clone();
        concrete.generic_params.clear();
        for variant in &mut concrete.variants {
            for payload in &mut variant.payload {
                *payload = self.normalize_type(payload.clone(), &substitutions, variant.span)?;
            }
        }
        self.generated_enums.push(concrete);
        if let Some(declaration) = self.generated_enums.last() {
            self.enum_declarations
                .insert(declaration.name.clone(), declaration.clone());
        }
        Ok(Type::Named(concrete_name))
    }

    pub(super) fn instantiate_function(
        &mut self,
        template_name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let Some(template) = self.generic_functions.get(template_name).cloned() else {
            return Err(Diagnostic::at(
                span.into(),
                format!("`{template_name}` is not a generic function"),
            ));
        };
        if arguments.len() != template.generic_params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic function `{template_name}` expects {} type argument(s), got {}",
                    template.generic_params.len(),
                    arguments.len()
                ),
            ));
        }
        self.check_generic_bounds(&template.generic_params, &arguments, span)?;
        let key = (
            template_name.to_owned(),
            arguments.iter().map(type_key).collect::<Vec<_>>(),
        );
        if let Some(name) = self.function_instances.get(&key) {
            return Ok(name.clone());
        }
        self.new_instance(span)?;
        let concrete_name = format!(
            "{template_name}::<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        );
        self.function_instances.insert(key, concrete_name.clone());
        let substitutions = template
            .generic_params
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments)
            .collect::<Substitution>();
        let mut concrete = template;
        concrete.name = concrete_name.clone();
        concrete.generic_params.clear();
        for param in &mut concrete.params {
            param.ty = self.normalize_type(param.ty.clone(), &substitutions, param.span)?;
        }
        concrete.return_type =
            self.normalize_type(concrete.return_type.clone(), &substitutions, concrete.span)?;
        self.signatures.insert(
            concrete_name.clone(),
            (
                concrete
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                concrete.return_type.clone(),
            ),
        );
        self.function_queue.push_back(PendingFunction {
            function: concrete,
            substitutions,
        });
        Ok(concrete_name)
    }
}
