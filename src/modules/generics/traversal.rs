use super::inference::type_key;
use super::*;

impl Specializer {
    pub(super) fn process_function(
        &mut self,
        function: &mut Function,
        substitutions: &Substitution,
    ) -> Result<(), Diagnostic> {
        let mut variables = function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect::<HashMap<_, _>>();
        self.process_block(
            &mut function.body,
            substitutions,
            &mut variables,
            function.span,
        )
    }

    pub(super) fn process_block(
        &mut self,
        block: &mut Block,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        for statement in &mut block.statements {
            match statement {
                Stmt::Let {
                    pattern,
                    ty,
                    value,
                    span,
                    ..
                } => {
                    if let Some(ty) = ty {
                        *ty = self.normalize_type(ty.clone(), substitutions, *span)?;
                    }
                    self.process_expr(value, substitutions, variables)?;
                    self.normalize_pattern(pattern, substitutions, *span)?;
                    if let Some(ty) = ty
                        .clone()
                        .or_else(|| self.infer_expr_type(value, variables))
                    {
                        self.infer_pattern_bindings(pattern, &ty, variables);
                    }
                }
                Stmt::Assign { name, value, .. } => {
                    self.process_expr(value, substitutions, variables)?;
                    if let Some(ty) = self.infer_expr_type(value, variables) {
                        variables.insert(name.clone(), ty);
                    }
                }
                Stmt::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.process_expr(condition, substitutions, variables)?;
                    let mut then_variables = variables.clone();
                    self.process_block(then_block, substitutions, &mut then_variables, span)?;
                    if let Some(else_block) = else_block {
                        let mut else_variables = variables.clone();
                        self.process_block(else_block, substitutions, &mut else_variables, span)?;
                    }
                }
                Stmt::While { condition, body } => {
                    self.process_expr(condition, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::For {
                    name,
                    start,
                    end,
                    body,
                } => {
                    self.process_expr(start, substitutions, variables)?;
                    self.process_expr(end, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    body_variables.insert(name.clone(), Type::I64);
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::ForArray {
                    name, array, body, ..
                } => {
                    self.process_expr(array, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    if let Some(element) =
                        self.infer_expr_type(array, variables)
                            .and_then(|ty| match ty {
                                Type::Array(element, _) | Type::Vector(element) => Some(*element),
                                _ => None,
                            })
                    {
                        body_variables.insert(name.clone(), element);
                    }
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                    if let Some(value) = value {
                        self.process_expr(value, substitutions, variables)?;
                    }
                }
                Stmt::Continue { .. } => {}
                Stmt::AssignPlace { target, value, .. } => {
                    self.process_expr(target, substitutions, variables)?;
                    self.process_expr(value, substitutions, variables)?;
                }
                Stmt::Expr(value) => self.process_expr(value, substitutions, variables)?,
            }
        }
        if let Some(tail) = &mut block.tail {
            self.process_expr(tail, substitutions, variables)?;
        }
        Ok(())
    }

    pub(super) fn process_expr(
        &mut self,
        expr: &mut Expr,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        match expr {
            Expr::Array(values, _) => {
                for value in values {
                    self.process_expr(value, substitutions, variables)?;
                }
            }
            Expr::Tuple(values, _) => {
                for value in values {
                    self.process_expr(value, substitutions, variables)?;
                }
            }
            Expr::Index { array, index, .. } => {
                self.process_expr(array, substitutions, variables)?;
                self.process_expr(index, substitutions, variables)?;
            }
            Expr::Field { value, .. } | Expr::Unary { value, .. } => {
                self.process_expr(value, substitutions, variables)?;
            }
            Expr::Struct {
                name,
                type_arguments,
                fields,
                span,
            } => {
                for argument in type_arguments.iter_mut() {
                    *argument = self.normalize_type(argument.clone(), substitutions, *span)?;
                }
                if self.generic_structs.contains_key(name) {
                    let concrete_type =
                        self.instantiate_struct(name, type_arguments.clone(), *span)?;
                    let Type::Named(concrete_name) = concrete_type else {
                        unreachable!()
                    };
                    *name = concrete_name;
                    type_arguments.clear();
                } else if !type_arguments.is_empty() {
                    return Err(Diagnostic::at(
                        (*span).into(),
                        format!("struct `{name}` does not accept generic type arguments"),
                    ));
                }
                for (_, value, _) in fields {
                    self.process_expr(value, substitutions, variables)?;
                }
            }
            Expr::Binary { left, right, .. } => {
                self.process_expr(left, substitutions, variables)?;
                self.process_expr(right, substitutions, variables)?;
            }
            Expr::Call {
                name,
                type_arguments,
                args,
                span,
            } => {
                for argument in args.iter_mut() {
                    self.process_expr(argument, substitutions, variables)?;
                }
                for argument in type_arguments.iter_mut() {
                    *argument = self.normalize_type(argument.clone(), substitutions, *span)?;
                }
                if self.generic_functions.contains_key(name) {
                    let template = self.generic_functions.get(name).unwrap().clone();
                    if args.len() != template.params.len() {
                        return Err(Diagnostic::at(
                            (*span).into(),
                            format!(
                                "function `{name}` expects {} argument(s), got {}",
                                template.params.len(),
                                args.len()
                            ),
                        ));
                    }
                    let mut inferred = HashMap::new();
                    for (parameter, argument) in template.params.iter().zip(args.iter()) {
                        if let Some(actual) = self.infer_expr_type(argument, variables) {
                            self.unify(&parameter.ty, &actual, &mut inferred, *span)?;
                        }
                    }
                    if !type_arguments.is_empty() {
                        if type_arguments.len() != template.generic_params.len() {
                            return Err(Diagnostic::at(
                                (*span).into(),
                                format!(
                                    "generic function `{name}` expects {} type argument(s), got {}",
                                    template.generic_params.len(),
                                    type_arguments.len()
                                ),
                            ));
                        }
                        let explicit_arguments = type_arguments
                            .iter()
                            .map(|argument| {
                                self.normalize_type(argument.clone(), substitutions, *span)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        for (parameter, argument) in template
                            .generic_params
                            .iter()
                            .zip(explicit_arguments.iter())
                        {
                            if let Some(inferred_type) = inferred.get(parameter)
                                && inferred_type != argument
                            {
                                return Err(Diagnostic::at(
                                    (*span).into(),
                                    format!(
                                        "explicit type argument `{}` conflicts with inferred type `{}`",
                                        type_key(argument),
                                        type_key(inferred_type)
                                    ),
                                ));
                            }
                            inferred.insert(parameter.clone(), argument.clone());
                        }
                    }
                    let mut resolved_arguments = Vec::new();
                    for parameter in &template.generic_params {
                        resolved_arguments.push(inferred.get(parameter).cloned().ok_or_else(|| {
                            Diagnostic::at(
                                (*span).into(),
                                format!(
                                    "cannot infer generic type parameter `{parameter}` for `{name}`; provide explicit type arguments with `::<...>`"
                                ),
                            )
                        })?);
                    }
                    for (parameter, argument) in template.params.iter().zip(args.iter()) {
                        if let Some(actual) = self.infer_expr_type(argument, variables) {
                            let mut checked = inferred.clone();
                            self.unify(&parameter.ty, &actual, &mut checked, *span)?;
                        }
                    }
                    *name = self.instantiate_function(name, resolved_arguments, *span)?;
                    type_arguments.clear();
                } else if !type_arguments.is_empty() {
                    return Err(Diagnostic::at(
                        (*span).into(),
                        format!("function `{name}` is not generic"),
                    ));
                }
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                self.process_expr(condition, substitutions, variables)?;
                let mut then_variables = variables.clone();
                self.process_block(then_block, substitutions, &mut then_variables, *span)?;
                let mut else_variables = variables.clone();
                self.process_block(else_block, substitutions, &mut else_variables, *span)?;
            }
            Expr::Match { value, arms, span } => {
                self.process_expr(value, substitutions, variables)?;
                let matched_type = self.infer_expr_type(value, variables);
                for arm in arms {
                    let mut arm_variables = variables.clone();
                    self.normalize_pattern(&mut arm.pattern, substitutions, *span)?;
                    if let Some(matched_type) = &matched_type {
                        self.infer_pattern_bindings(
                            &mut arm.pattern,
                            matched_type,
                            &mut arm_variables,
                        );
                    }
                    self.process_block(&mut arm.body, substitutions, &mut arm_variables, *span)?;
                }
            }
            Expr::Loop { body, span } => {
                let mut body_variables = variables.clone();
                self.process_block(body, substitutions, &mut body_variables, *span)?;
            }
            Expr::Variant { payload, .. } => {
                if let Some(payload) = payload {
                    for value in payload {
                        self.process_expr(value, substitutions, variables)?;
                    }
                }
            }
            Expr::Integer(..)
            | Expr::Float32(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::String(..)
            | Expr::Variable(..) => {}
        }
        Ok(())
    }
}
