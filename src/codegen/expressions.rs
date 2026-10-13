use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_expression(&mut self, expr: &Expr) -> Result<TypedValue<'ctx>, String> {
        match expr {
            Expr::Integer(n, _) => Ok(TypedValue {
                value: Some(self.i64_type().const_int(*n, false).into()),
                ty: ValueType::I64,
            }),
            Expr::Float32(n, _) => Ok(TypedValue {
                value: Some(self.context.f32_type().const_float(f64::from(*n)).into()),
                ty: ValueType::F32,
            }),
            Expr::Float(n, _) => Ok(TypedValue {
                value: Some(self.context.f64_type().const_float(*n).into()),
                ty: ValueType::F64,
            }),
            Expr::Bool(value, _) => Ok(TypedValue {
                value: Some(self.bool_type().const_int(u64::from(*value), false).into()),
                ty: ValueType::Bool,
            }),
            Expr::String(value, _) => Ok(TypedValue {
                value: Some(self.string_literal(value)?.into()),
                ty: ValueType::Str,
            }),
            Expr::Tuple(elements, span) => self.emit_tuple_literal(elements, *span),
            Expr::Array(elements, span) => self.emit_array_literal(elements, *span),
            Expr::Call {
                name, args, span, ..
            } if name == "vec_new" => {
                if !args.is_empty() {
                    return Err(format!(
                        "line {}:{}: `vec_new` expects 0 arguments",
                        span.line, span.column
                    ));
                }
                Err(format!(
                    "line {}:{}: `vec_new()` requires a `Vec<T>` type annotation",
                    span.line, span.column
                ))
            }
            Expr::Variable(name, span) => {
                if let Some(local) = self.lookup_local(name) {
                    let value = self
                        .builder
                        .build_load(self.llvm_type(local.ty).unwrap(), local.pointer, name)
                        .map_err(|e| e.to_string())?;
                    return Ok(TypedValue {
                        value: Some(value),
                        ty: local.ty,
                    });
                }
                if let Some(function_name) = self.resolve_function_name(name)
                    && let Some(function) = self.functions.get(&function_name).copied()
                {
                    if self
                        .function_visibility
                        .get(&function_name)
                        .is_some_and(|(owner, public)| owner != &self.module_context && !public)
                    {
                        return Err(format!(
                            "line {}:{}: function `{name}` is private",
                            span.line, span.column
                        ));
                    }
                    let (params, result) = self.signatures[&function_name].clone();
                    let ty = self.function_type(params, result);
                    return Ok(TypedValue {
                        value: Some(function.as_global_value().as_pointer_value().into()),
                        ty,
                    });
                }
                let global_name = self.resolve_global_name(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                let declaration = self.global_declarations[&global_name].clone();
                if declaration.module != self.module_context && !declaration.is_public {
                    return Err(format!(
                        "line {}:{}: global `{name}` is private",
                        span.line, span.column
                    ));
                }
                let ty = self.global_types[&global_name];
                let value = self
                    .builder
                    .build_load(
                        self.llvm_type(ty).ok_or("global has no LLVM type")?,
                        self.globals[&global_name].as_pointer_value(),
                        name,
                    )
                    .map_err(|error| error.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty,
                })
            }
            Expr::Index { array, index, span } => {
                let array_value = self.emit_expression(array)?;
                let index = self.emit_expression(index)?;
                let index = self.expect_i64(index, "array index")?;
                if let ValueType::Vector(vector_id) = array_value.ty {
                    let header = array_value
                        .value
                        .ok_or("vector has no handle")?
                        .into_pointer_value();
                    let header_type = self.context.struct_type(
                        &[
                            self.context.ptr_type(AddressSpace::default()).into(),
                            self.i64_type().into(),
                            self.i64_type().into(),
                        ],
                        false,
                    );
                    let data_ptr = self
                        .builder
                        .build_struct_gep(header_type, header, 0, "vec.data.ptr")
                        .map_err(|error| error.to_string())?;
                    let length_ptr = self
                        .builder
                        .build_struct_gep(header_type, header, 1, "vec.length.ptr")
                        .map_err(|error| error.to_string())?;
                    let length = self
                        .builder
                        .build_load(self.i64_type(), length_ptr, "vec.length")
                        .map_err(|error| error.to_string())?
                        .into_int_value();
                    self.emit_dynamic_bounds_check(index, length)?;
                    let data = self
                        .builder
                        .build_load(
                            self.context.ptr_type(AddressSpace::default()),
                            data_ptr,
                            "vec.data",
                        )
                        .map_err(|error| error.to_string())?
                        .into_pointer_value();
                    let element_type = ValueType::from(self.vector_shapes[vector_id].element);
                    let llvm_element_type = self
                        .llvm_type(element_type)
                        .ok_or("vector element has no LLVM type")?;
                    let size = self.vector_element_size(element_type)?;
                    let offset = self
                        .builder
                        .build_int_mul(index, size, "vec.element.offset")
                        .map_err(|error| error.to_string())?;
                    let address = unsafe {
                        self.builder.build_gep(
                            self.context.i8_type(),
                            data,
                            &[offset],
                            "vec.element.address",
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    let typed_pointer = self
                        .builder
                        .build_pointer_cast(
                            address,
                            self.context.ptr_type(AddressSpace::default()),
                            "vec.element.ptr",
                        )
                        .map_err(|error| error.to_string())?;
                    let value = self
                        .builder
                        .build_load(llvm_element_type, typed_pointer, "vec.element")
                        .map_err(|error| error.to_string())?;
                    return Ok(TypedValue {
                        value: Some(value),
                        ty: element_type,
                    });
                }
                let ValueType::Array(array_id) = array_value.ty else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array (or vector)",
                        span.line, span.column
                    ));
                };
                let shape = self.array_shapes[array_id];
                self.emit_bounds_check(index, shape.length)?;
                let array_type = array_value.ty;
                let scratch = self.create_entry_alloca(array_type, "array.index.base")?;
                self.builder
                    .build_store(scratch, array_value.value.ok_or("array has no value")?)
                    .map_err(|e| e.to_string())?;
                let pointer = self.array_element_pointer(scratch, array_type, index)?;
                let value = self
                    .builder
                    .build_load(
                        self.llvm_element_type(shape.element)
                            .ok_or("array element has no LLVM type")?,
                        pointer,
                        "array.element",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: ValueType::from(shape.element),
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
                let (owner, public) = self
                    .struct_visibility
                    .get(&self.structs[struct_id].name)
                    .cloned()
                    .unwrap_or_default();
                if owner != self.module_context && !public {
                    return Err(format!(
                        "line {}:{}: struct `{}` is private",
                        span.line, span.column, self.structs[struct_id].name
                    ));
                }
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
                let field_declaration = &self.structs[struct_id].fields[field_index];
                let field_type = field_declaration.ty.clone();
                let field_span = field_declaration.span;
                let field_ty = self.resolve_type(&field_type, field_span)?;
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
            Expr::Struct {
                name, fields, span, ..
            } => self.emit_struct_literal(name, fields, *span),
            Expr::Unary { op, value, .. } => {
                if matches!(op, UnaryOp::Negate)
                    && matches!(value.as_ref(), Expr::Integer(n, _) if *n == (i64::MAX as u64) + 1)
                {
                    return Ok(TypedValue {
                        value: Some(self.i64_type().const_int(1u64 << 63, false).into()),
                        ty: ValueType::I64,
                    });
                }
                let value = self.emit_expression(value)?;
                match op {
                    UnaryOp::Negate => {
                        if matches!(value.ty, ValueType::F32 | ValueType::F64) {
                            let float_type = if value.ty == ValueType::F32 {
                                "f32"
                            } else {
                                "f64"
                            };
                            let result = self
                                .builder
                                .build_float_neg(
                                    value
                                        .value
                                        .ok_or("floating-point value has no LLVM value")?
                                        .into_float_value(),
                                    &format!("{float_type}.neg"),
                                )
                                .map_err(|e| e.to_string())?;
                            return Ok(TypedValue {
                                value: Some(result.into()),
                                ty: value.ty,
                            });
                        }
                        if value.ty != ValueType::I64 {
                            return Err(format!(
                                "unary `-` requires i64, got {}",
                                self.type_name(value.ty)
                            ));
                        }
                        let result = self.emit_checked_i64_operation(
                            BinaryOp::Subtract,
                            self.i64_type().const_zero(),
                            value.value.unwrap().into_int_value(),
                        )?;
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
            Expr::Call {
                name, args, span, ..
            } => self.emit_call(name, args, *span),
            Expr::CallValue { callee, args, span } => {
                let callee = self.emit_expression(callee)?;
                self.emit_indirect_call(callee, args, *span)
            }
            Expr::Lambda {
                params,
                return_type,
                body,
                span,
            } => {
                let lambda_id = self.next_lambda_id;
                self.next_lambda_id += 1;
                let name = format!("vaca.lambda.{lambda_id}");
                let synthetic = Function {
                    name: name.clone(),
                    generic_params: Vec::new(),
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body: (**body).clone(),
                    span: *span,
                    is_public: false,
                    module: self.module_context.clone(),
                };
                let params = synthetic
                    .params
                    .iter()
                    .map(|param| self.resolve_type(&param.ty, param.span))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_type(&synthetic.return_type, synthetic.span)?;
                let llvm_function = self.module.add_function(
                    &name,
                    self.llvm_fn_type(&params, ret)?,
                    Some(Linkage::Internal),
                );
                self.functions.insert(name.clone(), llvm_function);
                self.signatures.insert(name.clone(), (params.clone(), ret));
                self.function_visibility
                    .insert(name.clone(), (synthetic.module.clone(), false));
                let previous_block = self.builder.get_insert_block();
                let previous_function = self.current_function;
                let previous_return_type = self.return_type;
                let previous_module =
                    std::mem::replace(&mut self.module_context, synthetic.module.clone());
                let previous_scopes = std::mem::take(&mut self.scopes);
                let previous_loop_targets = std::mem::take(&mut self.loop_targets);
                let lambda_result = self.emit_function(&synthetic);
                self.current_function = previous_function;
                self.return_type = previous_return_type;
                self.module_context = previous_module;
                self.scopes = previous_scopes;
                self.loop_targets = previous_loop_targets;
                if let Some(block) = previous_block {
                    self.builder.position_at_end(block);
                }
                lambda_result?;

                let function_type = self.function_type(params, ret);
                Ok(TypedValue {
                    value: Some(llvm_function.as_global_value().as_pointer_value().into()),
                    ty: function_type,
                })
            }
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
            Expr::Loop { body, .. } => self.emit_loop_expression(body),
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

    pub(super) fn resolve_function_name(&self, name: &str) -> Option<String> {
        if self.functions.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.functions.contains_key(&qualified).then_some(qualified)
        }
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
