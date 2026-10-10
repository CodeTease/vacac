use super::*;

impl Checker {
    pub(super) fn check_block(
        &mut self,
        block: &Block,
        return_type: ValueType,
        function_body: bool,
    ) -> Result<bool, String> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        let mut returns = false;
        for statement in &block.statements {
            let statement_returns = self.check_statement(statement, return_type)?;
            returns |= statement_returns;
        }

        if let Some(tail) = &block.tail {
            let tail_type = self.check_expr(tail, false)?;
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
    ) -> Result<bool, String> {
        match statement {
            Stmt::Let {
                name,
                ty,
                value,
                immutable,
                span,
            } => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(at(
                        *span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                let value_type = if matches!(value, Expr::Array(elements, _) if elements.is_empty())
                {
                    let Some(ty) = ty else {
                        return Err(at(
                            span_of_expr(value),
                            "empty array literals require an array type annotation",
                        ));
                    };
                    let annotated = self.resolve_type(ty, *span)?;
                    if !matches!(annotated, ValueType::Array(_, 0)) {
                        return Err(at(
                            span_of_expr(value),
                            "empty array literal must have array length 0",
                        ));
                    }
                    annotated
                } else {
                    self.check_expr(value, false)?
                };
                if value_type == ValueType::Unit {
                    return Err(at(*span, "cannot bind a value of type `unit`"));
                }
                let binding_type = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => value_type,
                };
                self.expect_type(
                    value_type,
                    binding_type,
                    span_of_expr(value),
                    &format!("initializer for `{name}`"),
                )?;
                self.scopes
                    .last_mut()
                    .expect("block scope exists")
                    .insert(name.clone(), binding_type);
                if *immutable {
                    self.immutable_scopes
                        .last_mut()
                        .expect("block scope exists")
                        .insert(name.clone());
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
                let value_type = self.check_expr(value, false)?;
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
                let target_type = self.check_place(target, binding_type)?;
                let value_type = self.check_expr(value, false)?;
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
                self.loop_depth += 1;
                let body_result = self.check_block(body, return_type, false);
                self.loop_depth -= 1;
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
                self.loop_depth += 1;
                let body_result = self.check_block(body, return_type, false);
                self.loop_depth -= 1;
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
                let ValueType::Array(element, _) = array_type else {
                    return Err(at(*span, "array iteration requires an array"));
                };
                self.scopes
                    .push(HashMap::from([(name.clone(), ValueType::from(element))]));
                self.immutable_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_depth += 1;
                let body_result = self.check_block(body, return_type, false);
                self.loop_depth -= 1;
                body_result?;
                self.scopes.pop();
                self.immutable_scopes.pop();
                self.loop_scopes.pop();
                Ok(false)
            }
            Stmt::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(at(*span, "`break` is only valid inside a loop"));
                }
                Ok(false)
            }
            Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(at(*span, "`continue` is only valid inside a loop"));
                }
                Ok(false)
            }
            Stmt::Return { value, span } => {
                if let Some(value) = value {
                    let value_type = self.check_expr(value, false)?;
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
