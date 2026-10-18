use super::expressions::{codegen_borrow_root, codegen_mutable_borrow_origin};
use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_function(&mut self, function: &Function) -> Result<(), Diagnostic> {
        self.module_context = function.module.clone();
        let function_value = *self
            .functions
            .get(&function.name)
            .ok_or("internal error: function was not declared")?;
        let entry = self.context.append_basic_block(function_value, "entry");
        self.builder.position_at_end(entry);
        self.current_function = Some(function_value);
        self.return_type = self.resolve_type(&function.return_type, function.span)?;
        if matches!(
            self.return_type,
            ValueType::Reference(_) | ValueType::MutableReference(_)
        ) {
            return Err(Diagnostic::at(
                function.span.into(),
                format!("functions cannot return references"),
            ));
        }
        self.scopes.clear();
        self.loop_targets.clear();
        self.scopes.push(HashMap::new());

        let is_lambda = function.name.starts_with("vaca.lambda.");
        let captures = self
            .lambda_capture_types
            .get(&function.name)
            .cloned()
            .unwrap_or_default();
        let mut parameter_offset = 0;
        if is_lambda {
            let environment = function_value
                .get_nth_param(0)
                .ok_or("internal error: lambda is missing its environment parameter")?
                .into_pointer_value();
            if !captures.is_empty() {
                let environment_type = self.closure_environment_type(&captures)?;
                for (index, (name, ty)) in captures.iter().enumerate() {
                    let field = self
                        .builder
                        .build_struct_gep(
                            environment_type,
                            environment,
                            index as u32,
                            "capture.ptr",
                        )
                        .map_err(|error| error.to_string())?;
                    let value = self
                        .builder
                        .build_load(
                            self.llvm_type(*ty).ok_or("capture has no LLVM type")?,
                            field,
                            name,
                        )
                        .map_err(|error| error.to_string())?;
                    let alloca = self.create_entry_alloca(*ty, name)?;
                    self.builder
                        .build_store(alloca, value)
                        .map_err(|error| error.to_string())?;
                    let binding_id = self.next_binding_id();
                    self.scopes[0].insert(
                        name.clone(),
                        Local {
                            pointer: alloca,
                            ty: *ty,
                            immutable: true,
                            loop_variable: false,
                            binding_id,
                            mutable_reference_origin: None,
                        },
                    );
                }
            }
            parameter_offset = 1;
        }

        for (index, param) in function.params.iter().enumerate() {
            let param_value = function_value
                .get_nth_param(index as u32 + parameter_offset)
                .ok_or("internal error: missing LLVM parameter")?;
            param_value.set_name(&param.name);
            let ty: ValueType = self.resolve_type(&param.ty, param.span)?;
            let alloca = self.create_entry_alloca(ty, &param.name)?;
            self.builder
                .build_store(alloca, param_value)
                .map_err(|e| e.to_string())?;
            let binding_id = self.next_binding_id();
            self.scopes[0].insert(
                param.name.clone(),
                Local {
                    pointer: alloca,
                    ty,
                    immutable: matches!(
                        ty,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ),
                    loop_variable: false,
                    binding_id,
                    // A function parameter's caller-side origin is unknown.
                    mutable_reference_origin: None,
                },
            );
        }

        let (terminated, tail) = self.emit_block(&function.body, true)?;
        if !terminated {
            if let Some(value) = tail {
                if value.ty != self.return_type {
                    return Err(Diagnostic::at(
                        function.span.into(),
                        format!(
                            "function `{}` returns {}, but its declared return type is {}",
                            function.name,
                            self.type_name(value.ty),
                            self.type_name(self.return_type)
                        ),
                    ));
                }
                if let Some(returned) = value.value {
                    self.builder
                        .build_return(Some(&returned))
                        .map_err(|e| e.to_string())?;
                } else if self.return_type == ValueType::Unit {
                    self.builder.build_return(None).map_err(|e| e.to_string())?;
                } else {
                    return Err("internal error: return value missing".into());
                }
            } else if self.return_type == ValueType::Unit {
                self.builder.build_return(None).map_err(|e| e.to_string())?;
            } else {
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!(
                        "function `{}` can reach its end without returning a value",
                        function.name
                    ),
                ));
            }
        }
        self.current_function = None;
        self.scopes.clear();
        Ok(())
    }

    // The function body's final expression is its implicit return value. Nested blocks
    // evaluate, but do not return, their final expression.

    pub(super) fn emit_block(
        &mut self,
        block: &Block,
        function_body: bool,
    ) -> Result<(bool, Option<TypedValue<'ctx>>), Diagnostic> {
        self.scopes.push(HashMap::new());
        let mut terminated = false;
        for statement in &block.statements {
            if self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some()
            {
                terminated = true;
                break;
            }
            if self.emit_statement(statement)? {
                terminated = true;
                break;
            }
        }
        let mut tail_value = None;
        if !terminated && let Some(expr) = &block.tail {
            let value = if function_body {
                self.emit_expression_expected(expr, self.return_type)?
            } else {
                self.emit_expression(expr)?
            };
            if function_body {
                tail_value = Some(value);
            }
        }
        self.scopes.pop();
        Ok((terminated, tail_value))
    }

    pub(super) fn emit_statement(&mut self, statement: &Stmt) -> Result<bool, Diagnostic> {
        match statement {
            Stmt::Let {
                pattern,
                ty,
                value,
                immutable,
                span,
            } => {
                let emitted_value = if let Some(ty) = ty.as_ref() {
                    let array_type = self.resolve_type(ty, *span)?;
                    self.emit_expression_expected(value, array_type)?
                } else {
                    self.emit_expression(value)?
                };
                if matches!(emitted_value.ty, ValueType::MutableReference(_))
                    && !matches!(
                        value,
                        Expr::Unary {
                            op: UnaryOp::BorrowMut,
                            ..
                        }
                    )
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        "mutable references cannot be copied into another binding",
                    ));
                }
                if emitted_value.ty == ValueType::Unit {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("cannot bind a value of type `unit`"),
                    ));
                }
                if matches!(
                    emitted_value.ty,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) && !*immutable
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("reference bindings must be immutable"),
                    ));
                }
                let ty = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => emitted_value.ty,
                };
                if ty != emitted_value.ty {
                    let context = match pattern {
                        MatchPattern::Binding(name) => format!("initializer for `{name}`"),
                        _ => "binding initializer".to_owned(),
                    };
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "{context} has type {}, expected {}",
                            self.type_name(emitted_value.ty),
                            self.type_name(ty)
                        ),
                    ));
                }
                let aggregate = emitted_value.value.ok_or("missing initializer value")?;
                let binding_origin = if matches!(ty, ValueType::MutableReference(_)) {
                    match value {
                        Expr::Unary {
                            op: UnaryOp::BorrowMut,
                            value: borrowed,
                            ..
                        } => codegen_borrow_root(borrowed)
                            .and_then(|source| self.lookup_local(source))
                            .and_then(|local| {
                                codegen_mutable_borrow_origin(borrowed, local.binding_id)
                            })
                            .or(Some(MutableReferenceOrigin::Unknown)),
                        _ => None,
                    }
                } else {
                    None
                };
                let mut bound_names = Vec::new();
                fn collect_names(pattern: &MatchPattern, names: &mut Vec<String>) {
                    match pattern {
                        MatchPattern::Binding(name) => names.push(name.clone()),
                        MatchPattern::Tuple(patterns) | MatchPattern::Array(patterns) => {
                            for pattern in patterns {
                                collect_names(pattern, names);
                            }
                        }
                        MatchPattern::Struct { fields, .. } => {
                            for (_, pattern, _) in fields {
                                collect_names(pattern, names);
                            }
                        }
                        _ => {}
                    }
                }
                collect_names(pattern, &mut bound_names);
                self.emit_pattern_bindings(pattern, ty, aggregate, *immutable, *span)?;
                for name in bound_names {
                    if let Some(local) = self
                        .scopes
                        .last_mut()
                        .and_then(|scope| scope.get_mut(&name))
                        && matches!(local.ty, ValueType::MutableReference(_))
                    {
                        local.mutable_reference_origin = binding_origin.clone();
                    }
                }
                Ok(false)
            }
            Stmt::Assign { .. } | Stmt::AssignPlace { .. } => {
                self.emit_assignment_statement(statement)
            }
            Stmt::Expr(expr) => {
                let value = self.emit_expression(expr)?;
                if value.ty != ValueType::Unit { /* expression value intentionally discarded */ }
                Ok(false)
            }
            Stmt::Return { value, span } => {
                let value = if let Some(expr) = value {
                    let value = self.emit_expression_expected(expr, self.return_type)?;
                    if value.ty != self.return_type {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!(
                                "return type mismatch: expected {}, got {}",
                                self.type_name(self.return_type),
                                self.type_name(value.ty)
                            ),
                        ));
                    }
                    value.value
                } else {
                    if self.return_type != ValueType::Unit {
                        return Err("`return;` is only valid in a function returning `unit`".into());
                    }
                    None
                };
                if let Some(value) = value {
                    self.builder
                        .build_return(Some(&value))
                        .map_err(|e| e.to_string())?;
                } else {
                    self.builder.build_return(None).map_err(|e| e.to_string())?;
                }
                Ok(true)
            }
            Stmt::Break { value, span } => {
                let is_value_loop = self
                    .loop_targets
                    .last()
                    .is_some_and(|target| target.value_loop);
                if is_value_loop != value.is_some() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "{}",
                            if is_value_loop {
                                "a value-producing loop requires `break` with a value"
                            } else {
                                "`break` with a value is only valid inside a value-producing `loop`"
                            }
                        ),
                    ));
                }
                let emitted_value = value
                    .as_ref()
                    .map(|expr| self.emit_expression(expr))
                    .transpose()?;
                if emitted_value.as_ref().is_some_and(|value| {
                    matches!(
                        value.ty,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    )
                }) {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references cannot escape through loop breaks"),
                    ));
                }
                let break_block = self
                    .builder
                    .get_insert_block()
                    .ok_or("break has no insertion block")?;
                if self.loop_targets.is_empty() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`break` is only valid inside a loop"),
                    ));
                }
                if let Some(value) = emitted_value {
                    if value.ty == ValueType::Unit {
                        return Err("a loop break value cannot have type `unit`".into());
                    }
                    let expected = self
                        .loop_targets
                        .last()
                        .and_then(|target| target.result_type);
                    if let Some(expected) = expected {
                        if expected != value.ty {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!(
                                    "loop break values have different types: {} and {}",
                                    self.type_name(expected),
                                    self.type_name(value.ty)
                                ),
                            ));
                        }
                    } else {
                        self.loop_targets.last_mut().unwrap().result_type = Some(value.ty);
                    }
                    self.loop_targets.last_mut().unwrap().incoming.push((
                        value.value.ok_or("loop break value is missing")?,
                        break_block,
                    ));
                }
                let break_target = self.loop_targets.last().unwrap().break_target;
                self.builder
                    .build_unconditional_branch(break_target)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Stmt::Continue { span } => {
                let continue_target = self
                    .loop_targets
                    .last()
                    .map(|target| target.continue_target)
                    .ok_or_else(|| {
                        Diagnostic::at(
                            span.into(),
                            format!("`continue` is only valid inside a loop"),
                        )
                    })?;
                self.builder
                    .build_unconditional_branch(continue_target)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => self.emit_if(condition, then_block, else_block.as_ref()),
            Stmt::While {
                condition, body, ..
            } => self.emit_while(condition, body),
            Stmt::For {
                name,
                start,
                end,
                body,
                ..
            } => self.emit_for(name, start, end, body),
            Stmt::ForArray {
                name,
                array,
                body,
                span,
            } => self.emit_for_array(name, array, body, *span),
        }
    }

    pub(super) fn emit_if(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<bool, Diagnostic> {
        let cond = self.emit_expression(condition)?;
        let cond_value = self.expect_bool(cond, "if condition", codegen_span_of_expr(condition))?;
        let function = self.current_function.ok_or("if outside function")?;
        let then_bb = self.context.append_basic_block(function, "if.then");
        let merge_bb = self.context.append_basic_block(function, "if.end");
        let else_bb = else_block
            .map(|_| self.context.append_basic_block(function, "if.else"))
            .unwrap_or(merge_bb);
        self.builder
            .build_conditional_branch(cond_value, then_bb, else_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        let (then_terminated, _) = self.emit_block(then_block, false)?;
        let then_terminated = then_terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some();
        if !then_terminated {
            self.builder
                .build_unconditional_branch(merge_bb)
                .map_err(|e| e.to_string())?;
        }

        let mut else_terminated = false;
        if let Some(else_block) = else_block {
            self.builder.position_at_end(else_bb);
            let (terminated, _) = self.emit_block(else_block, false)?;
            else_terminated = terminated
                || self
                    .builder
                    .get_insert_block()
                    .and_then(|b| b.get_terminator())
                    .is_some();
            if !else_terminated {
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(|e| e.to_string())?;
            }
        }

        if else_block.is_some() && then_terminated && else_terminated {
            self.builder.position_at_end(merge_bb);
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
            Ok(true)
        } else {
            self.builder.position_at_end(merge_bb);
            Ok(false)
        }
    }
}
