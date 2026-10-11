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
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
            if left.ty == ValueType::Str {
                return self.emit_string_equality(op, left, right);
            }
            if matches!(
                left.ty,
                ValueType::Array(_)
                    | ValueType::Vector(_)
                    | ValueType::Enum(_)
                    | ValueType::Struct(_)
            ) {
                let lhs = left.value.ok_or("left operand has no value")?;
                let rhs = right.value.ok_or("right operand has no value")?;
                let mut equal = self.emit_value_equality(left.ty, lhs, rhs)?;
                if matches!(op, BinaryOp::NotEqual) {
                    equal = self
                        .builder
                        .build_not(equal, "aggregate.ne")
                        .map_err(|e| e.to_string())?;
                }
                return Ok(TypedValue {
                    value: Some(equal.into()),
                    ty: ValueType::Bool,
                });
            }
        }
        if matches!(op, BinaryOp::Add) && left.ty == ValueType::Str {
            let lhs = left
                .value
                .ok_or("left operand has no value")?
                .into_pointer_value();
            let rhs = right
                .value
                .ok_or("right operand has no value")?
                .into_pointer_value();
            let concat = self
                .module
                .get_function("vaca_str_concat")
                .ok_or("missing string concatenation runtime function")?;
            let result = self
                .builder
                .build_call(concat, &[lhs.into(), rhs.into()], "str.concat")
                .map_err(|e| e.to_string())?
                .try_as_basic_value()
                .basic()
                .ok_or("internal error: string concatenation returned void")?;
            return Ok(TypedValue {
                value: Some(result),
                ty: ValueType::Str,
            });
        }
        if left.ty == ValueType::Str {
            return Err("operator is not supported for str".into());
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
            BinaryOp::BitAnd | BinaryOp::BitXor | BinaryOp::BitOr => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "bitwise operators require i64, got {}",
                        self.type_name(left.ty)
                    ));
                }
                let value = match op {
                    BinaryOp::BitAnd => self.builder.build_and(lhs, rhs, "bit.and"),
                    BinaryOp::BitXor => self.builder.build_xor(lhs, rhs, "bit.xor"),
                    BinaryOp::BitOr => self.builder.build_or(lhs, rhs, "bit.or"),
                    _ => unreachable!(),
                }
                .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::I64,
                }
            }
            BinaryOp::ShiftLeft | BinaryOp::ShiftRight => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "shift operators require i64, got {}",
                        self.type_name(left.ty)
                    ));
                }
                self.guard_shift_amount(rhs)?;
                let value = match op {
                    BinaryOp::ShiftLeft => self.builder.build_left_shift(lhs, rhs, "shift.left"),
                    BinaryOp::ShiftRight => {
                        self.builder
                            .build_right_shift(lhs, rhs, true, "shift.right")
                    }
                    _ => unreachable!(),
                }
                .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::I64,
                }
            }
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

    fn guard_shift_amount(&mut self, amount: IntValue<'ctx>) -> Result<(), String> {
        let negative = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                amount,
                self.i64_type().const_zero(),
                "shift.negative",
            )
            .map_err(|e| e.to_string())?;
        let too_large = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                amount,
                self.i64_type().const_int(64, false),
                "shift.too.large",
            )
            .map_err(|e| e.to_string())?;
        let invalid = self
            .builder
            .build_or(negative, too_large, "shift.invalid")
            .map_err(|e| e.to_string())?;
        let function = self.current_function.ok_or("shift outside function")?;
        let failure = self.context.append_basic_block(function, "shift.error");
        let valid = self.context.append_basic_block(function, "shift.valid");
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
                "shift.exit",
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
