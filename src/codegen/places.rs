use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_place_pointer(
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
                let field_declaration_type = field.ty.clone();
                let field_span = field.span;
                let field_type = self.resolve_type(&field_declaration_type, field_span)?;
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
                if let ValueType::Vector(vector_id) = array_type {
                    let index_value = self.emit_expression(index)?;
                    let index_value = self.expect_i64(index_value, "vector index")?;
                    let handle = self
                        .builder
                        .build_load(
                            self.context.ptr_type(AddressSpace::default()),
                            array_pointer,
                            "vec.handle",
                        )
                        .map_err(|error| error.to_string())?
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
                        .build_struct_gep(header_type, handle, 0, "vec.data.ptr")
                        .map_err(|error| error.to_string())?;
                    let len_ptr = self
                        .builder
                        .build_struct_gep(header_type, handle, 1, "vec.length.ptr")
                        .map_err(|error| error.to_string())?;
                    let length = self
                        .builder
                        .build_load(self.i64_type(), len_ptr, "vec.length")
                        .map_err(|error| error.to_string())?
                        .into_int_value();
                    self.emit_dynamic_bounds_check(index_value, length)?;
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
                    self.llvm_type(element_type)
                        .ok_or("vector element has no LLVM type")?;
                    let size = self.vector_element_size(element_type)?;
                    let offset = self
                        .builder
                        .build_int_mul(index_value, size, "vec.element.offset")
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
                    let pointer = self
                        .builder
                        .build_pointer_cast(
                            address,
                            self.context.ptr_type(AddressSpace::default()),
                            "vec.element.ptr",
                        )
                        .map_err(|error| error.to_string())?;
                    return Ok((pointer, element_type));
                }
                let ValueType::Array(array_id) = array_type else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array (or vector)",
                        span.line, span.column
                    ));
                };
                let index_value = self.emit_expression(index)?;
                let index_value = self.expect_i64(index_value, "array index")?;
                let shape = self.array_shapes[array_id];
                self.emit_bounds_check(index_value, shape.length)?;
                let element_pointer =
                    self.array_element_pointer(array_pointer, array_type, index_value)?;
                Ok((element_pointer, ValueType::from(shape.element)))
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    pub(super) fn emit_assignment_statement(&mut self, statement: &Stmt) -> Result<bool, String> {
        match statement {
            Stmt::Assign { name, value, span } => {
                let (pointer, ty) = if let Some(local) = self.lookup_local(name) {
                    if matches!(local.ty, ValueType::Reference(_)) {
                        return Err(format!(
                            "line {}:{}: reference bindings cannot be reassigned",
                            span.line, span.column
                        ));
                    }
                    if local.immutable {
                        let reason = if local.loop_variable {
                            format!("cannot assign to immutable loop variable `{name}`")
                        } else {
                            format!("cannot assign to immutable binding `{name}`")
                        };
                        return Err(format!("line {}:{}: {reason}", span.line, span.column));
                    }
                    (local.pointer, local.ty)
                } else {
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
                    if !declaration.mutable {
                        return Err(format!(
                            "line {}:{}: cannot assign to immutable global `{name}`",
                            span.line, span.column
                        ));
                    }
                    (
                        self.globals[&global_name].as_pointer_value(),
                        self.global_types[&global_name],
                    )
                };
                let value = self.emit_expression_expected(value, ty)?;
                if ty != value.ty {
                    return Err(format!(
                        "line {}:{}: cannot assign {} to `{name}` of type {}",
                        span.line,
                        span.column,
                        self.type_name(value.ty),
                        self.type_name(ty)
                    ));
                }
                self.builder
                    .build_store(pointer, value.value.ok_or("missing assignment value")?)
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
                        "line {}:{}: assignment target must start with a variable",
                        span.line, span.column
                    )
                })?;
                if self
                    .lookup_local(name)
                    .is_some_and(|local| matches!(local.ty, ValueType::Reference(_)))
                {
                    return Err(format!(
                        "line {}:{}: reference bindings cannot be reassigned",
                        span.line, span.column
                    ));
                }
                let (root_pointer, root_type, mutable, loop_variable) =
                    if let Some(local) = self.lookup_local(name) {
                        (
                            local.pointer,
                            local.ty,
                            !local.immutable,
                            local.loop_variable,
                        )
                    } else {
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
                        (
                            self.globals[&global_name].as_pointer_value(),
                            self.global_types[&global_name],
                            declaration.mutable,
                            false,
                        )
                    };
                let vector_place = self.place_mutates_vector(target, name, root_type)?;
                if !mutable && !vector_place {
                    let reason = if loop_variable {
                        format!("cannot assign through immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign through immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column));
                }
                let (target_pointer, target_type) =
                    self.emit_place_pointer(target, name, root_pointer, root_type)?;
                let value = self.emit_expression_expected(value, target_type)?;
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
            _ => unreachable!("non-assignment passed to assignment emitter"),
        }
    }

    fn place_mutates_vector(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_type: ValueType,
    ) -> Result<bool, String> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok(false),
            Expr::Field { value, .. } => self.place_mutates_vector(value, root_name, root_type),
            Expr::Index { array, .. } => {
                let array_type = self.place_expression_type(array, root_name, root_type)?;
                if matches!(array_type, ValueType::Vector(_)) {
                    Ok(true)
                } else {
                    self.place_mutates_vector(array, root_name, root_type)
                }
            }
            _ => Ok(false),
        }
    }

    fn place_expression_type(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_type: ValueType,
    ) -> Result<ValueType, String> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok(root_type),
            Expr::Field { value, name, span } => {
                let ValueType::Struct(struct_id) =
                    self.place_expression_type(value, root_name, root_type)?
                else {
                    return Err(format!(
                        "line {}:{}: field access requires a struct",
                        span.line, span.column
                    ));
                };
                let field = self.structs[struct_id]
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| {
                        format!(
                            "line {}:{}: struct has no field `{name}`",
                            span.line, span.column
                        )
                    })?;
                self.resolve_type(&field.ty.clone(), field.span)
            }
            Expr::Index { array, span, .. } => match self
                .place_expression_type(array, root_name, root_type)?
            {
                ValueType::Array(index) => Ok(ValueType::from(self.array_shapes[index].element)),
                ValueType::Vector(index) => Ok(ValueType::from(self.vector_shapes[index].element)),
                _ => Err(format!(
                    "line {}:{}: indexing requires an array or vector",
                    span.line, span.column
                )),
            },
            _ => Err("invalid place expression".into()),
        }
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
