use super::inference::type_key;
use super::*;
impl Specializer {
    pub(super) fn normalize_pattern(
        &mut self,
        pattern: &mut MatchPattern,
        _substitutions: &Substitution,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Tuple(patterns) | MatchPattern::Array(patterns) => {
                for pattern in patterns {
                    self.normalize_pattern(pattern, _substitutions, span)?;
                }
            }
            MatchPattern::Struct { name, fields, .. } => {
                if let Some(template_name) = self
                    .struct_instances
                    .get(name)
                    .map(|(template, _)| template.clone())
                {
                    *name = template_name;
                }
                for (_, pattern, field_span) in fields {
                    self.normalize_pattern(pattern, _substitutions, *field_span)?;
                }
            }
            MatchPattern::Variant { bindings, .. } => {
                if let Some(patterns) = bindings {
                    for pattern in patterns {
                        self.normalize_pattern(pattern, _substitutions, span)?;
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
                if let Type::Named(actual_name) = ty {
                    if name.rsplit("::").next() == actual_name.rsplit("::").next()
                        || actual_name.starts_with(&format!("{name}::<"))
                    {
                        *name = actual_name.clone();
                    }
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
                self.instantiate_struct(&name, arguments, span)?
            }
            Type::Named(name) if self.generic_structs.contains_key(&name) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic struct `{name}` requires explicit type arguments"),
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
            Type::Vector(element) => Type::Vector(Box::new(self.normalize_type(
                *element,
                substitutions,
                span,
            )?)),
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
            .cloned()
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
            .cloned()
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
