use super::*;

impl<'ctx> Codegen<'ctx> {
    fn emit_place_pointer(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<(PointerValue<'ctx>, ValueType), String> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok((root_pointer, root_type)),
            Expr::Field { value, name, span } => {
                let (base_pointer, base_type) =
                    self.emit_place_pointer(value, root_name, root_pointer, root_type)?;
                let ValueType::Struct(struct_id) = base_type else {
                    return Err(format!(
                        "line {}:{}: field access requires a struct value",
                        span.line, span.column
                    ));
                };
                let field_index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| {
                        format!(
                            "line {}:{}: struct `{}` has no field `{name}`",
                            span.line, span.column, self.structs[struct_id].name
                        )
                    })?;
                let field = &self.structs[struct_id].fields[field_index];
                let field_type = self.resolve_type(&field.ty, field.span)?;
                let field_pointer = self
                    .builder
                    .build_struct_gep(
                        self.struct_types[struct_id],
                        base_pointer,
                        field_index as u32,
                        "struct.field.ptr",
                    )
                    .map_err(|error| error.to_string())?;
                Ok((field_pointer, field_type))
            }
            Expr::Index { array, index, span } => {
                let (array_pointer, array_type) =
                    self.emit_place_pointer(array, root_name, root_pointer, root_type)?;
                let ValueType::Array(element, length) = array_type else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array",
                        span.line, span.column
                    ));
                };
                let index_value = self.emit_expression(index)?;
                let index_value = self.expect_i64(index_value, "array index")?;
                self.emit_bounds_check(index_value, length)?;
                let element_pointer =
                    self.array_element_pointer(array_pointer, array_type, index_value)?;
                Ok((element_pointer, ValueType::from(element)))
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    pub(super) fn emit_function(&mut self, function: &Function) -> Result<(), String> {
        let function_value = *self
            .functions
            .get(&function.name)
            .ok_or("internal error: function was not declared")?;
        let entry = self.context.append_basic_block(function_value, "entry");
        self.builder.position_at_end(entry);
        self.current_function = Some(function_value);
        self.return_type = self.resolve_type(&function.return_type, function.span)?;
        self.scopes.clear();
        self.loop_targets.clear();
        self.scopes.push(HashMap::new());

        for (index, param) in function.params.iter().enumerate() {
            let param_value = function_value
                .get_nth_param(index as u32)
                .ok_or("internal error: missing LLVM parameter")?;
            param_value.set_name(&param.name);
            let ty: ValueType = self.resolve_type(&param.ty, param.span)?;
            let alloca = self.create_entry_alloca(ty, &param.name)?;
            self.builder
                .build_store(alloca, param_value)
                .map_err(|e| e.to_string())?;
            self.scopes[0].insert(
                param.name.clone(),
                Local {
                    pointer: alloca,
                    ty,
                    immutable: false,
                    loop_variable: false,
                },
            );
        }

        let (terminated, tail) = self.emit_block(&function.body, true)?;
        if !terminated {
            if let Some(value) = tail {
                if value.ty != self.return_type {
                    return Err(format!(
                        "line {}:{}: function `{}` returns {}, but its declared return type is {}",
                        function.span.line,
                        function.span.column,
                        function.name,
                        self.type_name(value.ty),
                        self.type_name(self.return_type)
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
                return Err(format!(
                    "line {}:{}: function `{}` can reach its end without returning a value",
                    function.span.line, function.span.column, function.name
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
    ) -> Result<(bool, Option<TypedValue<'ctx>>), String> {
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
            let value = self.emit_expression(expr)?;
            if function_body {
                tail_value = Some(value);
            }
        }
        self.scopes.pop();
        Ok((terminated, tail_value))
    }

    pub(super) fn emit_statement(&mut self, statement: &Stmt) -> Result<bool, String> {
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
                    return Err(format!(
                        "line {}:{}: `{name}` is already declared in this scope",
                        span.line, span.column
                    ));
                }
                let value = if let (Some(ty), Expr::Array(elements, _)) = (ty.as_ref(), value) {
                    if elements.is_empty() {
                        let array_type = self.resolve_type(ty, *span)?;
                        if !matches!(array_type, ValueType::Array(_, 0)) {
                            return Err(format!(
                                "line {}:{}: empty array literal must have array length 0",
                                span.line, span.column
                            ));
                        }
                        let llvm_type =
                            self.llvm_type(array_type).ok_or("array has no LLVM type")?;
                        TypedValue {
                            value: Some(llvm_type.const_zero()),
                            ty: array_type,
                        }
                    } else {
                        self.emit_expression(value)?
                    }
                } else {
                    self.emit_expression(value)?
                };
                if value.ty == ValueType::Unit {
                    return Err(format!(
                        "line {}:{}: cannot bind a value of type `unit`",
                        span.line, span.column
                    ));
                }
                let ty = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => value.ty,
                };
                if ty != value.ty {
                    return Err(format!(
                        "line {}:{}: binding `{name}` has type {}, but its initializer has type {}",
                        span.line,
                        span.column,
                        self.type_name(ty),
                        self.type_name(value.ty)
                    ));
                }
                let alloca = self.create_entry_alloca(ty, name)?;
                self.builder
                    .build_store(alloca, value.value.ok_or("missing initializer value")?)
                    .map_err(|e| e.to_string())?;
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        pointer: alloca,
                        ty,
                        immutable: *immutable,
                        loop_variable: false,
                    },
                );
                Ok(false)
            }
            Stmt::Assign { name, value, span } => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                if local.immutable {
                    let reason = if local.loop_variable {
                        format!("cannot assign to immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign to immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column));
                }
                let value = self.emit_expression(value)?;
                if local.ty != value.ty {
                    return Err(format!(
                        "line {}:{}: cannot assign {} to `{name}` of type {}",
                        span.line,
                        span.column,
                        self.type_name(value.ty),
                        self.type_name(local.ty)
                    ));
                }
                self.builder
                    .build_store(
                        local.pointer,
                        value.value.ok_or("missing assignment value")?,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            Stmt::AssignPlace {
                target,
                value,
                span,
            } => {
                let name = codegen_assignment_root(target).ok_or_else(|| {
                    format!(
                        "line {}:{}: assignment target must start with a local variable",
                        span.line, span.column
                    )
                })?;
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                if local.immutable {
                    let reason = if local.loop_variable {
                        format!("cannot assign through immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign through immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column,));
                }
                let (target_pointer, target_type) =
                    self.emit_place_pointer(target, name, local.pointer, local.ty)?;
                let value = self.emit_expression(value)?;
                if value.ty != target_type {
                    return Err(format!(
                        "line {}:{}: assignment target has type {}, expected {}",
                        span.line,
                        span.column,
                        self.type_name(value.ty),
                        self.type_name(target_type)
                    ));
                }
                self.builder
                    .build_store(
                        target_pointer,
                        value.value.ok_or("assignment target value is missing")?,
                    )
                    .map_err(|error| error.to_string())?;
                Ok(false)
            }
            Stmt::Expr(expr) => {
                let value = self.emit_expression(expr)?;
                if value.ty != ValueType::Unit { /* expression value intentionally discarded */ }
                Ok(false)
            }
            Stmt::Return { value, .. } => {
                let value = if let Some(expr) = value {
                    let value = self.emit_expression(expr)?;
                    if value.ty != self.return_type {
                        return Err(format!(
                            "return type mismatch: expected {}, got {}",
                            self.type_name(self.return_type),
                            self.type_name(value.ty)
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
            Stmt::Break { span } => {
                let (_, break_target) = self.loop_targets.last().copied().ok_or_else(|| {
                    format!(
                        "line {}:{}: `break` is only valid inside a loop",
                        span.line, span.column
                    )
                })?;
                self.builder
                    .build_unconditional_branch(break_target)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Stmt::Continue { span } => {
                let (continue_target, _) = self.loop_targets.last().copied().ok_or_else(|| {
                    format!(
                        "line {}:{}: `continue` is only valid inside a loop",
                        span.line, span.column
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
    ) -> Result<bool, String> {
        let cond = self.emit_expression(condition)?;
        let cond_value = self.expect_bool(cond, "if condition")?;
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

    pub(super) fn emit_while(&mut self, condition: &Expr, body: &Block) -> Result<bool, String> {
        let function = self.current_function.ok_or("while outside function")?;
        let cond_bb = self.context.append_basic_block(function, "while.cond");
        let body_bb = self.context.append_basic_block(function, "while.body");
        let end_bb = self.context.append_basic_block(function, "while.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(cond_bb);
        let cond = self.emit_expression(condition)?;
        let cond_value = self.expect_bool(cond, "while condition")?;
        self.builder
            .build_conditional_branch(cond_value, body_bb, end_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(body_bb);
        self.loop_targets.push((cond_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(cond_bb)
                .map_err(|e| e.to_string())?;
        }
        self.builder.position_at_end(end_bb);
        Ok(false)
    }

    pub(super) fn emit_for(
        &mut self,
        name: &str,
        start_expr: &Expr,
        end_expr: &Expr,
        body: &Block,
    ) -> Result<bool, String> {
        // Evaluate the bounds once, before the loop variable enters scope.
        let start = self.emit_expression(start_expr)?;
        let end = self.emit_expression(end_expr)?;
        if start.ty != ValueType::I64 || end.ty != ValueType::I64 {
            return Err("for range bounds must be i64".into());
        }
        let start = start
            .value
            .ok_or("for range start has no value")?
            .into_int_value();
        let end = end
            .value
            .ok_or("for range end has no value")?
            .into_int_value();

        let function = self.current_function.ok_or("for loop outside function")?;
        let counter = self.create_entry_alloca(ValueType::I64, name)?;
        self.builder
            .build_store(counter, start)
            .map_err(|e| e.to_string())?;

        self.scopes.push(HashMap::new());
        self.scopes.last_mut().unwrap().insert(
            name.to_owned(),
            Local {
                pointer: counter,
                ty: ValueType::I64,
                immutable: true,
                loop_variable: true,
            },
        );

        let cond_bb = self.context.append_basic_block(function, "for.cond");
        let body_bb = self.context.append_basic_block(function, "for.body");
        let step_bb = self.context.append_basic_block(function, "for.step");
        let end_bb = self.context.append_basic_block(function, "for.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_bb);
        let current = self
            .builder
            .build_load(self.i64_type(), counter, "for.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let in_range = self
            .builder
            .build_int_compare(IntPredicate::SLT, current, end, "for.test")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(in_range, body_bb, end_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_bb);
        self.loop_targets.push((step_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|block| block.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(step_bb)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(step_bb);
        let current = self
            .builder
            .build_load(self.i64_type(), counter, "for.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(current, self.i64_type().const_int(1, false), "for.next")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(counter, next)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(end_bb);
        self.scopes.pop();
        Ok(false)
    }

    pub(super) fn emit_for_array(
        &mut self,
        name: &str,
        array_expr: &Expr,
        body: &Block,
        span: Span,
    ) -> Result<bool, String> {
        let array = self.emit_expression(array_expr)?;
        let ValueType::Array(element, length) = array.ty else {
            return Err(format!(
                "line {}:{}: array iteration requires an array",
                span.line, span.column
            ));
        };
        let array_type = array.ty;
        let array_slot = self.create_entry_alloca(array_type, "for.array")?;
        self.builder
            .build_store(array_slot, array.value.ok_or("array has no value")?)
            .map_err(|e| e.to_string())?;
        let index_slot = self.create_entry_alloca(ValueType::I64, "for.array.index")?;
        self.builder
            .build_store(index_slot, self.i64_type().const_zero())
            .map_err(|e| e.to_string())?;
        let value_slot = self.create_entry_alloca(ValueType::from(element), name)?;

        self.scopes.push(HashMap::from([(
            name.to_owned(),
            Local {
                pointer: value_slot,
                ty: ValueType::from(element),
                immutable: true,
                loop_variable: true,
            },
        )]));

        let function = self.current_function.ok_or("array loop outside function")?;
        let cond_bb = self.context.append_basic_block(function, "for.array.cond");
        let body_bb = self.context.append_basic_block(function, "for.array.body");
        let step_bb = self.context.append_basic_block(function, "for.array.step");
        let end_bb = self.context.append_basic_block(function, "for.array.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let in_bounds = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                index,
                self.i64_type().const_int(u64::from(length), false),
                "for.array.test",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(in_bounds, body_bb, end_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let element_pointer = self.array_element_pointer(array_slot, array_type, index)?;
        let element_value = self
            .builder
            .build_load(
                self.llvm_type(ValueType::from(element))
                    .ok_or("array element has no LLVM type")?,
                element_pointer,
                "for.array.value",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(value_slot, element_value)
            .map_err(|e| e.to_string())?;
        self.loop_targets.push((step_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|block| block.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(step_bb)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(step_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(index, self.i64_type().const_int(1, false), "for.array.next")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(index_slot, next)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(end_bb);
        self.scopes.pop();
        Ok(false)
    }
}

fn codegen_assignment_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name, _) => Some(name),
        Expr::Field { value, .. } => codegen_assignment_root(value),
        Expr::Index { array, .. } => codegen_assignment_root(array),
        _ => None,
    }
}
