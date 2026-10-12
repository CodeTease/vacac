use super::*;

impl Checker {
    pub(super) fn check_block(
        &mut self,
        block: &Block,
        return_type: ValueType,
        function_body: bool,
    ) -> Result<bool, Diagnostic> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        let mut returns = false;
        for statement in &block.statements {
            let statement_returns = self.check_statement(statement, return_type)?;
            returns |= statement_returns;
        }

        if let Some(tail) = &block.tail {
            let tail_type = if function_body {
                self.check_expr_expected(tail, return_type)?
            } else {
                self.check_expr(tail, false)?
            };
            if function_body {
                self.expect_type(
                    tail_type,
                    return_type,
                    span_of_expr(tail),
                    "function result",
                )?;
                returns = true;
            }
        } else if function_body && return_type == ValueType::Unit {
            // Unit functions may complete normally without an explicit return.
            returns = true;
        }
        self.scopes.pop();
        self.immutable_scopes.pop();
        self.loop_scopes.pop();
        Ok(returns)
    }

    pub(super) fn check_statement(
        &mut self,
        statement: &Stmt,
        return_type: ValueType,
    ) -> Result<bool, Diagnostic> {
        match statement {
            Stmt::Let {
                pattern,
                ty,
                value,
                immutable,
                span,
            } => {
                let value_type = if let Some(ty) = ty {
                    let expected = self.resolve_type(ty, *span)?;
                    self.check_expr_expected(value, expected)?
                } else if let Expr::Call { name, args, .. } = value
                    && name == "vec_new"
                    && args.is_empty()
                {
                    return Err(at(
                        span_of_expr(value),
                        "`vec_new()` requires a `Vec<T>` type annotation",
                    ));
                } else {
                    self.check_expr(value, false)?
                };
                if matches!(value, Expr::Array(elements, _) if elements.is_empty()) && ty.is_none()
                {
                    return Err(at(
                        span_of_expr(value),
                        "empty array literals require an array type annotation",
                    ));
                }
                if value_type == ValueType::Unit {
                    return Err(at(*span, "cannot bind a value of type `unit`"));
                }
                let binding_type = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => value_type,
                };
                let context = match (pattern, ty) {
                    (MatchPattern::Binding(name), Some(_)) => format!("initializer for `{name}`"),
                    _ => "binding initializer".to_owned(),
                };
                self.expect_type(value_type, binding_type, span_of_expr(value), &context)?;
                let mut bindings = Vec::new();
                let mut seen = std::collections::HashSet::new();
                self.check_pattern(pattern, binding_type, *span, &mut bindings, &mut seen)?;
                let scope = self.scopes.last_mut().expect("block scope exists");
                if let Some((name, _)) = bindings.iter().find(|(name, _)| scope.contains_key(name))
                {
                    return Err(at(
                        *span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                let immutable_scope = self
                    .immutable_scopes
                    .last_mut()
                    .expect("block scope exists");
                for (name, binding_type) in bindings {
                    scope.insert(name.clone(), binding_type);
                    if *immutable {
                        immutable_scope.insert(name);
                    }
                }
                Ok(false)
            }
            Stmt::Assign { name, value, span } => {
                let binding_type = self
                    .lookup_local(name)
                    .ok_or_else(|| at(*span, format!("undefined variable `{name}`")))?;
                if self.is_loop_variable(name) {
                    return Err(at(
                        *span,
                        format!("cannot assign to immutable loop variable `{name}`"),
                    ));
                }
                if self.is_immutable_binding(name) {
                    return Err(at(
                        *span,
                        format!("cannot assign to immutable binding `{name}`"),
                    ));
                }
                let value_type = self.check_expr_expected(value, binding_type)?;
                self.expect_type(
                    value_type,
                    binding_type,
                    span_of_expr(value),
                    &format!("assignment to `{name}`"),
                )?;
                Ok(false)
            }
            Stmt::AssignPlace {
                target,
                value,
                span,
            } => {
                let root = assignment_root(target).ok_or_else(|| {
                    at(*span, "assignment target must start with a local variable")
                })?;
                let binding_type = self
                    .lookup_local(root)
                    .ok_or_else(|| at(*span, format!("undefined variable `{root}`")))?;
                let target_type = self.check_place(target, binding_type)?;
                let vector_place = self.place_crosses_vector(target, binding_type)?;
                if vector_place {
                    let value_type = self.check_expr_expected(value, target_type)?;
                    self.expect_type(
                        value_type,
                        target_type,
                        span_of_expr(value),
                        "vector element assignment",
                    )?;
                    return Ok(false);
                }
                if self.is_loop_variable(root) {
                    return Err(at(
                        *span,
                        format!("cannot assign through immutable loop variable `{root}`"),
                    ));
                }
                if self.is_immutable_binding(root) {
                    return Err(at(
                        *span,
                        format!("cannot assign through immutable binding `{root}`"),
                    ));
                }
                let value_type = self.check_expr_expected(value, target_type)?;
                let context = match target {
                    Expr::Field { name, .. } => format!("assignment to field `{name}`"),
                    Expr::Index { .. } => format!("array element assignment to `{root}`"),
                    _ => format!("assignment to `{root}`"),
                };
                self.expect_type(value_type, target_type, span_of_expr(value), &context)?;
                Ok(false)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "if condition",
                )?;
                let then_returns = self.check_block(then_block, return_type, false)?;
                let else_returns = if let Some(block) = else_block {
                    self.check_block(block, return_type, false)?
                } else {
                    false
                };
                Ok(then_returns && else_returns)
            }
            Stmt::While {
                condition, body, ..
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "while condition",
                )?;
                self.loop_contexts.push(LoopContext {
                    value_loop: false,
                    result_type: None,
                });
                let body_result = self.check_block(body, return_type, false);
                self.loop_contexts.pop();
                body_result?;
                Ok(false)
            }
            Stmt::For {
                name,
                start,
                end,
                body,
                ..
            } => {
                let start_type = self.check_expr(start, false)?;
                self.expect_type(
                    start_type,
                    ValueType::I64,
                    span_of_expr(start),
                    "for range start",
                )?;
                let end_type = self.check_expr(end, false)?;
                self.expect_type(end_type, ValueType::I64, span_of_expr(end), "for range end")?;

                self.scopes
                    .push(HashMap::from([(name.clone(), ValueType::I64)]));
                self.immutable_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_contexts.push(LoopContext {
                    value_loop: false,
                    result_type: None,
                });
                let body_result = self.check_block(body, return_type, false);
                self.loop_contexts.pop();
                body_result?;
                self.scopes.pop();
                self.immutable_scopes.pop();
                self.loop_scopes.pop();
                Ok(false)
            }
            Stmt::ForArray {
                name,
                array,
                body,
                span,
            } => {
                let array_type = self.check_expr(array, false)?;
                let element = match array_type {
                    ValueType::Array(index) => self.array_shapes[index].element,
                    ValueType::Vector(index) => self.vector_shapes[index].element,
                    _ => return Err(at(*span, "array iteration requires an array or vector")),
                };
                self.scopes
                    .push(HashMap::from([(name.clone(), ValueType::from(element))]));
                self.immutable_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_contexts.push(LoopContext {
                    value_loop: false,
                    result_type: None,
                });
                let body_result = self.check_block(body, return_type, false);
                self.loop_contexts.pop();
                body_result?;
                self.scopes.pop();
                self.immutable_scopes.pop();
                self.loop_scopes.pop();
                Ok(false)
            }
            Stmt::Break { value, span } => {
                let Some(context) = self.loop_contexts.last() else {
                    return Err(at(*span, "`break` is only valid inside a loop"));
                };
                if context.value_loop != value.is_some() {
                    return Err(at(
                        *span,
                        if context.value_loop {
                            "a value-producing loop requires `break` with a value"
                        } else {
                            "`break` with a value is only valid inside a value-producing `loop`"
                        },
                    ));
                }
                if let Some(value) = value {
                    let value_type = self.check_expr(value, false)?;
                    if value_type == ValueType::Unit {
                        return Err(at(*span, "a loop break value cannot have type `unit`"));
                    }
                    let expected = self
                        .loop_contexts
                        .last()
                        .and_then(|context| context.result_type);
                    if let Some(expected) = expected {
                        self.expect_type(
                            value_type,
                            expected,
                            span_of_expr(value),
                            "loop break value",
                        )?;
                    } else {
                        self.loop_contexts.last_mut().unwrap().result_type = Some(value_type);
                    }
                }
                Ok(false)
            }
            Stmt::Continue { span } => {
                if self.loop_contexts.is_empty() {
                    return Err(at(*span, "`continue` is only valid inside a loop"));
                }
                Ok(false)
            }
            Stmt::Return { value, span } => {
                if let Some(value) = value {
                    let value_type = self.check_expr_expected(value, return_type)?;
                    self.expect_type(value_type, return_type, span_of_expr(value), "return value")?;
                } else if return_type != ValueType::Unit {
                    return Err(at(
                        *span,
                        "`return;` is only valid in a function returning `unit`",
                    ));
                }
                Ok(true)
            }
            Stmt::Expr(expr) => {
                self.check_expr(expr, false)?;
                Ok(false)
            }
        }
    }
}
