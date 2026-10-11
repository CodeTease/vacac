use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_loop_expression(
        &mut self,
        body: &Block,
    ) -> Result<TypedValue<'ctx>, String> {
        let function = self.current_function.ok_or("loop outside function")?;
        let body_bb = self.context.append_basic_block(function, "loop.body");
        let end_bb = self.context.append_basic_block(function, "loop.end");
        self.builder
            .build_unconditional_branch(body_bb)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(body_bb);
        self.loop_targets.push(LoopTarget {
            continue_target: body_bb,
            break_target: end_bb,
            value_loop: true,
            result_type: None,
            incoming: Vec::new(),
        });
        let body_result = self.emit_block(body, false);
        let target = self.loop_targets.pop().expect("loop target exists");
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|block| block.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(body_bb)
                .map_err(|error| error.to_string())?;
        }
        self.builder.position_at_end(end_bb);
        let ty = target
            .result_type
            .ok_or("value-producing loop must contain a `break` with a value")?;
        if target.incoming.is_empty() {
            return Err("value-producing loop has no reachable value-producing break".into());
        }
        let phi = self
            .builder
            .build_phi(self.llvm_type(ty).ok_or("unit loop result")?, "loop.value")
            .map_err(|error| error.to_string())?;
        let incoming_refs: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = target
            .incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&incoming_refs);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty,
        })
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
        self.loop_targets.push(LoopTarget {
            continue_target: cond_bb,
            break_target: end_bb,
            value_loop: false,
            result_type: None,
            incoming: Vec::new(),
        });
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
        self.loop_targets.push(LoopTarget {
            continue_target: step_bb,
            break_target: end_bb,
            value_loop: false,
            result_type: None,
            incoming: Vec::new(),
        });
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
        let ValueType::Array(array_id) = array.ty else {
            return Err(format!(
                "line {}:{}: array iteration requires an array",
                span.line, span.column
            ));
        };
        let shape = self.array_shapes[array_id];
        let array_type = array.ty;
        let array_slot = self.create_entry_alloca(array_type, "for.array")?;
        self.builder
            .build_store(array_slot, array.value.ok_or("array has no value")?)
            .map_err(|e| e.to_string())?;
        let index_slot = self.create_entry_alloca(ValueType::I64, "for.array.index")?;
        self.builder
            .build_store(index_slot, self.i64_type().const_zero())
            .map_err(|e| e.to_string())?;
        let element_type = ValueType::from(shape.element);
        let value_slot = self.create_entry_alloca(element_type, name)?;

        self.scopes.push(HashMap::from([(
            name.to_owned(),
            Local {
                pointer: value_slot,
                ty: element_type,
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
                self.i64_type().const_int(u64::from(shape.length), false),
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
                self.llvm_element_type(shape.element)
                    .ok_or("array element has no LLVM type")?,
                element_pointer,
                "for.array.value",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(value_slot, element_value)
            .map_err(|e| e.to_string())?;
        self.loop_targets.push(LoopTarget {
            continue_target: step_bb,
            break_target: end_bb,
            value_loop: false,
            result_type: None,
            incoming: Vec::new(),
        });
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
