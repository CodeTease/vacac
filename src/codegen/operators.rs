use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_binary(
        &mut self,
        op: BinaryOp,
        left_expr: &Expr,
        right_expr: &Expr,
    ) -> Result<TypedValue<'ctx>, String> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.emit_logical(op, left_expr, right_expr);
        }
        let left = self.emit_expression(left_expr)?;
        let right = self.emit_expression(right_expr)?;
        if left.ty != right.ty {
            return Err(format!(
                "operator operands have different types: {} and {}",
                self.type_name(left.ty),
                self.type_name(right.ty)
            ));
        }
        if left.ty == ValueType::Unit {
            return Err("void expressions cannot be used as operands".into());
        }
        if matches!(left.ty, ValueType::Array(_, _))
            && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
        {
            return Err("array values cannot be compared".into());
        }
        if matches!(left.ty, ValueType::Enum(_))
            && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
        {
            return Err("enum values cannot be compared".into());
        }
        if left.ty == ValueType::Str {
            return self.emit_string_equality(op, left, right);
        }
        let lhs = left
            .value
            .ok_or("left operand has no value")?
            .into_int_value();
        let rhs = right
            .value
            .ok_or("right operand has no value")?
            .into_int_value();
        let result = match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "arithmetic operators require i64, got {}",
                        self.type_name(left.ty)
                    ));
                }
                if matches!(op, BinaryOp::Divide | BinaryOp::Remainder) {
                    self.guard_signed_division_operands(lhs, rhs)?;
                }
                let value = match op {
                    BinaryOp::Add => self.builder.build_int_add(lhs, rhs, "add"),
                    BinaryOp::Subtract => self.builder.build_int_sub(lhs, rhs, "sub"),
                    BinaryOp::Multiply => self.builder.build_int_mul(lhs, rhs, "mul"),
                    BinaryOp::Divide => self.builder.build_int_signed_div(lhs, rhs, "div"),
                    BinaryOp::Remainder => self.builder.build_int_signed_rem(lhs, rhs, "rem"),
                    _ => unreachable!(),
                }
                .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::I64,
                }
            }
            BinaryOp::And | BinaryOp::Or => unreachable!(),
            BinaryOp::Equal | BinaryOp::NotEqual => {
                let pred = if matches!(op, BinaryOp::Equal) {
                    IntPredicate::EQ
                } else {
                    IntPredicate::NE
                };
                let value = self
                    .builder
                    .build_int_compare(pred, lhs, rhs, "eq")
                    .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::Bool,
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "ordering comparisons require i64, got {}",
                        self.type_name(left.ty)
                    ));
                }
                let pred = match op {
                    BinaryOp::Less => IntPredicate::SLT,
                    BinaryOp::LessEqual => IntPredicate::SLE,
                    BinaryOp::Greater => IntPredicate::SGT,
                    BinaryOp::GreaterEqual => IntPredicate::SGE,
                    _ => unreachable!(),
                };
                let value = self
                    .builder
                    .build_int_compare(pred, lhs, rhs, "cmp")
                    .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::Bool,
                }
            }
        };
        Ok(result)
    }

    pub(super) fn guard_signed_division_operands(
        &mut self,
        lhs: IntValue<'ctx>,
        rhs: IntValue<'ctx>,
    ) -> Result<(), String> {
        let i64_type = self.i64_type();
        let zero_divisor = self
            .builder
            .build_int_compare(IntPredicate::EQ, rhs, i64_type.const_zero(), "div.by.zero")
            .map_err(|e| e.to_string())?;
        let is_min = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                lhs,
                i64_type.const_int(1u64 << 63, false),
                "div.is.min",
            )
            .map_err(|e| e.to_string())?;
        let is_negative_one = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                rhs,
                i64_type.const_all_ones(),
                "div.is.negative.one",
            )
            .map_err(|e| e.to_string())?;
        let min_overflow = self
            .builder
            .build_and(is_min, is_negative_one, "div.overflow")
            .map_err(|e| e.to_string())?;
        let invalid = self
            .builder
            .build_or(zero_divisor, min_overflow, "div.invalid")
            .map_err(|e| e.to_string())?;
        let function = self.current_function.ok_or("division outside function")?;
        let failure = self
            .context
            .append_basic_block(function, "arithmetic.error");
        let valid = self
            .context
            .append_basic_block(function, "arithmetic.valid");
        self.builder
            .build_conditional_branch(invalid, failure, valid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(failure);
        let exit = self
            .module
            .get_function("exit")
            .ok_or("internal error: missing runtime exit declaration")?;
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "arithmetic.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }

    pub(super) fn emit_string_equality(
        &mut self,
        op: BinaryOp,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, String> {
        let pred = match op {
            BinaryOp::Equal => IntPredicate::EQ,
            BinaryOp::NotEqual => IntPredicate::NE,
            _ => {
                return Err(format!(
                    "operator is not supported for {}",
                    self.type_name(ValueType::Str)
                ));
            }
        };
        let lhs = left.value.ok_or("left operand has no value")?;
        let rhs = right.value.ok_or("right operand has no value")?;
        let strcmp = self.strcmp_function();
        let ordering = self
            .builder
            .build_call(strcmp, &[lhs.into(), rhs.into()], "strcmp")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: strcmp returned void")?
            .into_int_value();
        let value = self
            .builder
            .build_int_compare(
                pred,
                ordering,
                self.context.i32_type().const_zero(),
                "str.eq",
            )
            .map_err(|e| e.to_string())?;
        Ok(TypedValue {
            value: Some(value.into()),
            ty: ValueType::Bool,
        })
    }

    pub(super) fn emit_logical(
        &mut self,
        op: BinaryOp,
        left_expr: &Expr,
        right_expr: &Expr,
    ) -> Result<TypedValue<'ctx>, String> {
        let left = self.emit_expression(left_expr)?;
        let lhs = self.expect_bool(left, "logical operator operand")?;
        let function = self
            .current_function
            .ok_or("logical expression outside function")?;
        let left_block = self
            .builder
            .get_insert_block()
            .ok_or("logical expression has no insertion block")?;
        let rhs_block = self.context.append_basic_block(function, "logic.rhs");
        let end_block = self.context.append_basic_block(function, "logic.end");
        let is_and = matches!(op, BinaryOp::And);
        if is_and {
            self.builder
                .build_conditional_branch(lhs, rhs_block, end_block)
                .map_err(|e| e.to_string())?;
        } else {
            self.builder
                .build_conditional_branch(lhs, end_block, rhs_block)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(rhs_block);
        let right = self.emit_expression(right_expr)?;
        let rhs = self.expect_bool(right, "logical operator operand")?;
        let rhs_end = self
            .builder
            .get_insert_block()
            .ok_or("logical right operand has no insertion block")?;
        self.builder
            .build_unconditional_branch(end_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(end_block);
        let phi = self
            .builder
            .build_phi(self.bool_type(), "logic")
            .map_err(|e| e.to_string())?;
        let short_value = self.bool_type().const_int(u64::from(!is_and), false);
        phi.add_incoming(&[(&short_value, left_block), (&rhs, rhs_end)]);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty: ValueType::Bool,
        })
    }
}
