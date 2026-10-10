use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_expression(&mut self, expr: &Expr) -> Result<TypedValue<'ctx>, String> {
        match expr {
            Expr::Integer(n, _) => Ok(TypedValue {
                value: Some(self.i64_type().const_int(*n, false).into()),
                ty: ValueType::I64,
            }),
            Expr::Bool(value, _) => Ok(TypedValue {
                value: Some(self.bool_type().const_int(u64::from(*value), false).into()),
                ty: ValueType::Bool,
            }),
            Expr::String(value, _) => Ok(TypedValue {
                value: Some(self.string_literal(value)?.into()),
                ty: ValueType::Str,
            }),
            Expr::Array(elements, span) => self.emit_array_literal(elements, *span),
            Expr::Variable(name, span) => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                let value = self
                    .builder
                    .build_load(self.llvm_type(local.ty).unwrap(), local.pointer, name)
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: local.ty,
                })
            }
            Expr::Index { array, index, span } => {
                let array_value = self.emit_expression(array)?;
                let ValueType::Array(element, length) = array_value.ty else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array",
                        span.line, span.column
                    ));
                };
                let index = self.emit_expression(index)?;
                let index = self.expect_i64(index, "array index")?;
                self.emit_bounds_check(index, length)?;
                let array_type = ValueType::Array(element, length);
                let scratch = self.create_entry_alloca(array_type, "array.index.base")?;
                self.builder
                    .build_store(scratch, array_value.value.ok_or("array has no value")?)
                    .map_err(|e| e.to_string())?;
                let pointer = self.array_element_pointer(scratch, array_type, index)?;
                let value = self
                    .builder
                    .build_load(
                        self.llvm_type(ValueType::from(element))
                            .ok_or("array element has no LLVM type")?,
                        pointer,
                        "array.element",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: ValueType::from(element),
                })
            }
            Expr::Field { value, name, span } => {
                let value = self.emit_expression(value)?;
                let ValueType::Struct(struct_id) = value.ty else {
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
                let field_ty = self.resolve_type(
                    &self.structs[struct_id].fields[field_index].ty,
                    self.structs[struct_id].fields[field_index].span,
                )?;
                let value = self
                    .builder
                    .build_extract_value(
                        value
                            .value
                            .ok_or("struct value has no LLVM value")?
                            .into_struct_value(),
                        field_index as u32,
                        "struct.field",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: field_ty,
                })
            }
            Expr::Struct { name, fields, span } => self.emit_struct_literal(name, fields, *span),
            Expr::Unary { op, value, .. } => {
                let value = self.emit_expression(value)?;
                match op {
                    UnaryOp::Negate => {
                        if value.ty != ValueType::I64 {
                            return Err(format!(
                                "unary `-` requires i64, got {}",
                                self.type_name(value.ty)
                            ));
                        }
                        let result = self
                            .builder
                            .build_int_neg(value.value.unwrap().into_int_value(), "neg")
                            .map_err(|e| e.to_string())?;
                        Ok(TypedValue {
                            value: Some(result.into()),
                            ty: ValueType::I64,
                        })
                    }
                    UnaryOp::Not => {
                        if value.ty != ValueType::Bool {
                            return Err(format!(
                                "unary `!` requires bool, got {}",
                                self.type_name(value.ty)
                            ));
                        }
                        let result = self
                            .builder
                            .build_not(value.value.unwrap().into_int_value(), "not")
                            .map_err(|e| e.to_string())?;
                        Ok(TypedValue {
                            value: Some(result.into()),
                            ty: ValueType::Bool,
                        })
                    }
                }
            }
            Expr::Binary {
                op, left, right, ..
            } => self.emit_binary(*op, left, right),
            Expr::Call { name, args, span } => self.emit_call(name, args, *span),
            Expr::If {
                condition,
                then_block,
                else_block,
                ..
            } => self.emit_if_expression(condition, then_block, else_block),
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
            } => self.emit_enum_variant(enum_name, variant_name, payload.as_deref(), *span),
            Expr::Match { value, arms, span } => self.emit_match(value, arms, *span),
        }
    }

    pub(super) fn emit_if_expression(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: &Block,
    ) -> Result<TypedValue<'ctx>, String> {
        let condition = self.emit_expression(condition)?;
        let condition = self.expect_bool(condition, "if condition")?;
        let function = self
            .current_function
            .ok_or("if expression outside function")?;
        let then_bb = self.context.append_basic_block(function, "if.expr.then");
        let else_bb = self.context.append_basic_block(function, "if.expr.else");
        let merge_bb = self.context.append_basic_block(function, "if.expr.end");
        self.builder
            .build_conditional_branch(condition, then_bb, else_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        let (then_terminated, then_value) = self.emit_block(then_block, true)?;
        if then_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let then_value = then_value.ok_or("if expression branch has no value")?;
        let then_llvm_value = then_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let then_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression then branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(else_bb);
        let (else_terminated, else_value) = self.emit_block(else_block, true)?;
        if else_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let else_value = else_value.ok_or("if expression branch has no value")?;
        if else_value.ty != then_value.ty {
            return Err(format!(
                "if expression branches have different types: {} and {}",
                self.type_name(then_value.ty),
                self.type_name(else_value.ty)
            ));
        }
        let else_llvm_value = else_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let else_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression else branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(then_value.ty).ok_or("unit if expression")?,
                "if.value",
            )
            .map_err(|e| e.to_string())?;
        phi.add_incoming(&[(&then_llvm_value, then_end), (&else_llvm_value, else_end)]);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty: then_value.ty,
        })
    }

    pub(super) fn expect_i64(
        &self,
        value: TypedValue<'ctx>,
        context: &str,
    ) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::I64 {
            return Err(format!(
                "{context} must be i64, got {}",
                self.type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    pub(super) fn expect_bool(
        &self,
        value: TypedValue<'ctx>,
        context: &str,
    ) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::Bool {
            return Err(format!(
                "{context} must be bool, got {}",
                self.type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<Local<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub(super) fn create_entry_alloca(
        &self,
        ty: ValueType,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let function = self.current_function.ok_or("alloca outside function")?;
        let entry = function
            .get_first_basic_block()
            .ok_or("function has no entry block")?;
        let alloca_builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            alloca_builder.position_before(&first);
        } else {
            alloca_builder.position_at_end(entry);
        }
        alloca_builder
            .build_alloca(self.llvm_type(ty).ok_or("cannot allocate unit")?, name)
            .map_err(|e| e.to_string())
    }
}
