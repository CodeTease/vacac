use super::*;

impl Specializer {
    pub(super) fn process_expr_with_expected(
        &mut self,
        expr: &mut Expr,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
        expected: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match expr {
            Expr::Array(values, _) if let Type::Array(element, length) = expected => {
                if values.len() != *length as usize {
                    return self.process_expr(expr, substitutions, variables);
                }
                for value in values {
                    self.process_expr_with_expected(
                        value,
                        substitutions,
                        variables,
                        element,
                        span,
                    )?;
                }
                Ok(())
            }
            Expr::Tuple(values, _) if let Type::Tuple(elements) = expected => {
                if values.len() != elements.len() {
                    return self.process_expr(expr, substitutions, variables);
                }
                for (value, element) in values.iter_mut().zip(elements) {
                    self.process_expr_with_expected(
                        value,
                        substitutions,
                        variables,
                        element,
                        span,
                    )?;
                }
                Ok(())
            }
            Expr::Struct {
                name,
                type_arguments,
                fields,
                span: struct_span,
            } => {
                let concrete_name = if self.generic_structs.contains_key(name) {
                    if !type_arguments.is_empty() {
                        let arguments = type_arguments
                            .iter()
                            .cloned()
                            .map(|ty| self.normalize_type(ty, substitutions, *struct_span))
                            .collect::<Result<Vec<_>, _>>()?;
                        match self.instantiate_struct(name, arguments, *struct_span)? {
                            Type::Named(concrete) => concrete,
                            _ => unreachable!(),
                        }
                    } else if let Type::Named(expected_name) = expected {
                        match self.struct_instances.get(expected_name) {
                            Some((template, _)) if template == name => expected_name.clone(),
                            _ => return self.process_expr(expr, substitutions, variables),
                        }
                    } else {
                        return self.process_expr(expr, substitutions, variables);
                    }
                } else {
                    if !type_arguments.is_empty() {
                        return Err(Diagnostic::at(
                            (*struct_span).into(),
                            format!("struct `{name}` does not accept generic type arguments"),
                        ));
                    }
                    name.clone()
                };
                let declaration = self
                    .concrete_structs
                    .get(&concrete_name)
                    .or_else(|| {
                        self.generated_structs
                            .iter()
                            .find(|declaration| declaration.name == concrete_name)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::at(
                            (*struct_span).into(),
                            format!("unknown struct `{concrete_name}`"),
                        )
                    })?;
                *name = concrete_name;
                type_arguments.clear();
                for (field_name, value, field_span) in fields {
                    let field = declaration
                        .fields
                        .iter()
                        .find(|field| field.name == *field_name)
                        .ok_or_else(|| {
                            Diagnostic::at(
                                (*field_span).into(),
                                format!(
                                    "struct `{}` has no field `{field_name}`",
                                    declaration.name
                                ),
                            )
                        })?;
                    self.process_expr_with_expected(
                        value,
                        substitutions,
                        variables,
                        &field.ty,
                        *field_span,
                    )?;
                }
                Ok(())
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span: if_span,
            } => {
                self.process_expr(condition, substitutions, variables)?;
                self.process_block_with_expected(
                    then_block,
                    substitutions,
                    variables,
                    expected,
                    *if_span,
                )?;
                self.process_block_with_expected(
                    else_block,
                    substitutions,
                    variables,
                    expected,
                    *if_span,
                )
            }
            Expr::Match {
                value,
                arms,
                span: match_span,
            } => {
                self.process_expr(value, substitutions, variables)?;
                let matched_type = self.infer_expr_type(value, variables);
                for arm in arms {
                    let mut arm_variables = variables.clone();
                    if let (
                        MatchPattern::Variant { enum_name, .. },
                        Some(Type::Named(concrete_name)),
                    ) = (&mut arm.pattern, matched_type.as_ref())
                        && self.enum_instances.contains_key(concrete_name)
                    {
                        *enum_name = concrete_name.clone();
                    }
                    self.normalize_pattern(&mut arm.pattern, substitutions, *match_span)?;
                    if let Some(matched_type) = &matched_type {
                        self.infer_pattern_bindings(
                            &mut arm.pattern,
                            matched_type,
                            &mut arm_variables,
                        );
                    }
                    self.process_block_with_expected(
                        &mut arm.body,
                        substitutions,
                        &mut arm_variables,
                        expected,
                        *match_span,
                    )?;
                }
                Ok(())
            }
            Expr::Lambda {
                params,
                return_type,
                body,
                span: lambda_span,
            } if let Type::Function(_, expected_return) = expected => {
                let Type::Function(expected_params, _) = expected else {
                    unreachable!()
                };
                if params.len() != expected_params.len() {
                    return self.process_expr(expr, substitutions, variables);
                }
                for (param, expected_param) in params.iter().zip(expected_params) {
                    let actual =
                        self.normalize_type(param.ty.clone(), substitutions, param.span)?;
                    let expected =
                        self.normalize_type(expected_param.clone(), substitutions, span)?;
                    if actual != expected {
                        return self.process_expr(expr, substitutions, variables);
                    }
                }
                let actual_return =
                    self.normalize_type(return_type.clone(), substitutions, *lambda_span)?;
                let expected_return =
                    self.normalize_type((**expected_return).clone(), substitutions, span)?;
                if actual_return != expected_return {
                    return self.process_expr(expr, substitutions, variables);
                }
                for (param, expected_param) in params.iter_mut().zip(expected_params) {
                    param.ty = self.normalize_type(expected_param.clone(), substitutions, span)?;
                }
                *return_type = expected_return;
                let mut lambda_variables = params
                    .iter()
                    .map(|param| (param.name.clone(), param.ty.clone()))
                    .collect::<HashMap<_, _>>();
                let previous_return_type = self.current_return_type.replace(return_type.clone());
                let result =
                    self.process_block(body, substitutions, &mut lambda_variables, *lambda_span);
                self.current_return_type = previous_return_type;
                result
            }
            Expr::Variant {
                enum_name,
                type_arguments,
                variant_name,
                payload,
                span: variant_span,
            } if self.generic_enums.contains_key(enum_name) => {
                let template = self.generic_enums[enum_name].clone();
                let variant = template
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                    .ok_or_else(|| {
                        Diagnostic::at(
                            (*variant_span).into(),
                            format!("enum `{enum_name}` has no variant `{variant_name}`"),
                        )
                    })?;
                let mut inferred = HashMap::new();
                if let Some(values) = payload {
                    if values.len() != variant.payload.len() {
                        return Err(Diagnostic::at(
                            (*variant_span).into(),
                            format!(
                                "variant `{enum_name}::{variant_name}` expects {} payload value(s), got {}",
                                variant.payload.len(),
                                values.len()
                            ),
                        ));
                    }
                    for (formal, value) in variant.payload.iter().zip(values) {
                        if let Some(actual) = self.infer_expr_type(value, variables) {
                            self.unify(formal, &actual, &mut inferred, *variant_span)?;
                        }
                    }
                }
                let mut resolved_arguments = if !type_arguments.is_empty() {
                    type_arguments
                        .iter()
                        .cloned()
                        .map(|ty| self.normalize_type(ty, substitutions, *variant_span))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    match expected {
                        Type::Named(concrete_name) => self
                            .enum_instances
                            .get(concrete_name)
                            .filter(|(template, _)| template == enum_name)
                            .map(|(_, arguments)| arguments.clone())
                            .ok_or_else(|| {
                                Diagnostic::at(
                                    (*variant_span).into(),
                                    format!(
                                        "cannot infer generic arguments for enum `{enum_name}`"
                                    ),
                                )
                            })?,
                        _ => {
                            return self.process_expr(expr, substitutions, variables);
                        }
                    }
                };
                {
                    for (parameter, argument) in
                        template.generic_params.iter().zip(&mut resolved_arguments)
                    {
                        if let Some(inferred) = inferred.get(parameter) {
                            if argument != inferred {
                                return Err(Diagnostic::at(
                                    (*variant_span).into(),
                                    format!(
                                        "generic enum argument for `{parameter}` conflicts with inferred type"
                                    ),
                                ));
                            }
                        }
                    }
                }
                if resolved_arguments.len() != template.generic_params.len() {
                    return Err(Diagnostic::at(
                        (*variant_span).into(),
                        format!(
                            "generic enum `{enum_name}` expects {} type argument(s), got {}",
                            template.generic_params.len(),
                            resolved_arguments.len()
                        ),
                    ));
                }
                let concrete = self.instantiate_enum(
                    enum_name,
                    std::mem::take(&mut resolved_arguments),
                    *variant_span,
                )?;
                let Type::Named(concrete_name) = concrete else {
                    unreachable!()
                };
                *enum_name = concrete_name;
                type_arguments.clear();
                let specialized_variant = self
                    .enum_declarations
                    .get(enum_name)
                    .and_then(|declaration| {
                        declaration
                            .variants
                            .iter()
                            .find(|variant| variant.name == *variant_name)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::at(
                            (*variant_span).into(),
                            format!("enum `{enum_name}` has no variant `{variant_name}`"),
                        )
                    })?;
                if let Some(values) = payload {
                    for (value, expected_payload) in
                        values.iter_mut().zip(&specialized_variant.payload)
                    {
                        self.process_expr_with_expected(
                            value,
                            substitutions,
                            variables,
                            expected_payload,
                            *variant_span,
                        )?;
                    }
                }
                Ok(())
            }
            _ => self.process_expr(expr, substitutions, variables),
        }
    }

    fn process_block_with_expected(
        &mut self,
        block: &mut Block,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
        expected: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let mut tail = block.tail.take();
        let result = self.process_block(block, substitutions, variables, span);
        let result = result.and_then(|()| {
            if let Some(tail) = &mut tail {
                self.process_expr_with_expected(tail, substitutions, variables, expected, span)?;
            }
            Ok(())
        });
        block.tail = tail;
        result
    }
}
