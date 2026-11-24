use super::*;

/// Write access granted by the first pointer dereferenced along a place path.
#[derive(Clone, Copy)]
enum DerefMutability {
    Mutable,
    SharedReference,
    Immutable,
}

/// An address recipe whose selectors have already been evaluated. Vector data
/// pointers are reloaded when the plan is applied, so an assignment's right-hand
/// side can grow a vector without leaving the store pointed at freed storage.
struct PlacePlan<'ctx> {
    root_pointer: PointerValue<'ctx>,
    root_type: ValueType,
    steps: Vec<PlaceStep<'ctx>>,
    final_type: ValueType,
    mutability: Option<DerefMutability>,
}

enum PlaceStep<'ctx> {
    Deref {
        pointer: PointerValue<'ctx>,
        ty: ValueType,
    },
    Field {
        struct_id: usize,
        field_index: u32,
    },
    Array {
        array_id: usize,
        index: IntValue<'ctx>,
    },
    Vector {
        handle: PointerValue<'ctx>,
        index: IntValue<'ctx>,
        element_type: ValueType,
    },
}

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_place_pointer(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<(PointerValue<'ctx>, ValueType), Diagnostic> {
        let (pointer, ty, _) =
            self.emit_place_pointer_with_deref(expr, root_name, root_pointer, root_type)?;
        Ok((pointer, ty))
    }

    /// Computes the address of a place expression. The third element reports the
    /// mutability of the first dereference in the place path so assignment code
    /// can check write access without evaluating the pointer expression twice.
    fn emit_place_pointer_with_deref(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<(PointerValue<'ctx>, ValueType, Option<DerefMutability>), Diagnostic> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok((root_pointer, root_type, None)),
            Expr::Unary {
                op: UnaryOp::Dereference,
                value,
                span,
            } => {
                let pointer_value = self.emit_expression(value)?;
                let (target_type, mutability) = match pointer_value.ty {
                    ValueType::MutableReference(index) => (
                        self.mutable_reference_types[index],
                        DerefMutability::Mutable,
                    ),
                    ValueType::Reference(index) => (
                        self.reference_types[index],
                        DerefMutability::SharedReference,
                    ),
                    ValueType::RawPointer(index) => {
                        let (target_type, mutable) = self.raw_pointer_types[index];
                        let mutability = if mutable {
                            DerefMutability::Mutable
                        } else {
                            DerefMutability::Immutable
                        };
                        (target_type, mutability)
                    }
                    other => {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("cannot dereference {}", self.type_name(other)),
                        ));
                    }
                };
                let pointer = pointer_value
                    .value
                    .ok_or("dereference pointer has no value")?
                    .into_pointer_value();
                Ok((pointer, target_type, Some(mutability)))
            }
            Expr::Field { value, name, span } => {
                let (base_pointer, base_type, mutability) =
                    self.emit_place_pointer_with_deref(value, root_name, root_pointer, root_type)?;
                let ValueType::Struct(struct_id) = base_type else {
                    return Err(Diagnostic::at(
                        span.into(),
                        "field access requires a struct value".to_string(),
                    ));
                };
                let field_index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| {
                        Diagnostic::at(
                            span.into(),
                            format!(
                                "struct `{}` has no field `{name}`",
                                self.structs[struct_id].name
                            ),
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
                Ok((field_pointer, field_type, mutability))
            }
            Expr::Index { array, index, span } => {
                let (array_pointer, array_type, mutability) =
                    self.emit_place_pointer_with_deref(array, root_name, root_pointer, root_type)?;
                if let ValueType::Vector(vector_id) = array_type {
                    let index_value = self.emit_expression(index)?;
                    let index_value = self.expect_i64(index_value, "vector index", *span)?;
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
                    return Ok((pointer, element_type, mutability));
                }
                let ValueType::Array(array_id) = array_type else {
                    return Err(Diagnostic::at(
                        span.into(),
                        "indexing requires an array (or vector)".to_string(),
                    ));
                };
                let index_value = self.emit_expression(index)?;
                let index_value = self.expect_i64(index_value, "array index", *span)?;
                let shape = self.array_shapes[array_id];
                self.emit_bounds_check(index_value, shape.length)?;
                let element_pointer =
                    self.array_element_pointer(array_pointer, array_type, index_value)?;
                Ok((element_pointer, ValueType::from(shape.element), mutability))
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    fn plan_place_path(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<
        (
            PointerValue<'ctx>,
            ValueType,
            Vec<PlaceStep<'ctx>>,
            Option<DerefMutability>,
        ),
        Diagnostic,
    > {
        match expr {
            Expr::Variable(name, _) if name == root_name => {
                Ok((root_pointer, root_type, Vec::new(), None))
            }
            Expr::Unary {
                op: UnaryOp::Dereference,
                value,
                span,
            } => {
                let pointer_value = self.emit_expression(value)?;
                let (target_type, mutability) = match pointer_value.ty {
                    ValueType::MutableReference(index) => (
                        self.mutable_reference_types[index],
                        DerefMutability::Mutable,
                    ),
                    ValueType::Reference(index) => (
                        self.reference_types[index],
                        DerefMutability::SharedReference,
                    ),
                    ValueType::RawPointer(index) => {
                        let (target_type, mutable) = self.raw_pointer_types[index];
                        let mutability = if mutable {
                            DerefMutability::Mutable
                        } else {
                            DerefMutability::Immutable
                        };
                        (target_type, mutability)
                    }
                    other => {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("cannot dereference {}", self.type_name(other)),
                        ));
                    }
                };
                let pointer = pointer_value
                    .value
                    .ok_or("dereference pointer has no value")?
                    .into_pointer_value();
                Ok((
                    pointer,
                    target_type,
                    vec![PlaceStep::Deref {
                        pointer,
                        ty: target_type,
                    }],
                    Some(mutability),
                ))
            }
            Expr::Field { value, name, span } => {
                let (base_pointer, base_type, mut steps, mutability) =
                    self.plan_place_path(value, root_name, root_pointer, root_type)?;
                let ValueType::Struct(struct_id) = base_type else {
                    return Err(Diagnostic::at(
                        span.into(),
                        "field access requires a struct value".to_string(),
                    ));
                };
                let field_index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| {
                        Diagnostic::at(
                            span.into(),
                            format!(
                                "struct `{}` has no field `{name}`",
                                self.structs[struct_id].name
                            ),
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
                steps.push(PlaceStep::Field {
                    struct_id,
                    field_index: field_index as u32,
                });
                Ok((field_pointer, field_type, steps, mutability))
            }
            Expr::Index { array, index, span } => {
                let (array_pointer, array_type, mut steps, mutability) =
                    self.plan_place_path(array, root_name, root_pointer, root_type)?;
                if let ValueType::Vector(vector_id) = array_type {
                    let index_value = self.emit_expression(index)?;
                    let index_value = self.expect_i64(index_value, "vector index", *span)?;
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
                    let element_type = ValueType::from(self.vector_shapes[vector_id].element);
                    let element_pointer =
                        self.vector_element_pointer(handle, index_value, element_type)?;
                    steps.push(PlaceStep::Vector {
                        handle,
                        index: index_value,
                        element_type,
                    });
                    return Ok((element_pointer, element_type, steps, mutability));
                }
                let ValueType::Array(array_id) = array_type else {
                    return Err(Diagnostic::at(
                        span.into(),
                        "indexing requires an array (or vector)".to_string(),
                    ));
                };
                let index_value = self.emit_expression(index)?;
                let index_value = self.expect_i64(index_value, "array index", *span)?;
                let shape = self.array_shapes[array_id];
                self.emit_bounds_check(index_value, shape.length)?;
                let element_pointer =
                    self.array_element_pointer(array_pointer, array_type, index_value)?;
                steps.push(PlaceStep::Array {
                    array_id,
                    index: index_value,
                });
                Ok((
                    element_pointer,
                    ValueType::from(shape.element),
                    steps,
                    mutability,
                ))
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    fn plan_assignment_place(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<PlacePlan<'ctx>, Diagnostic> {
        let (_, final_type, steps, mutability) =
            self.plan_place_path(expr, root_name, root_pointer, root_type)?;
        Ok(PlacePlan {
            root_pointer,
            root_type,
            steps,
            final_type,
            mutability,
        })
    }

    fn apply_place_plan(
        &mut self,
        plan: &PlacePlan<'ctx>,
    ) -> Result<PointerValue<'ctx>, Diagnostic> {
        let mut pointer = plan.root_pointer;
        let mut ty = plan.root_type;
        for step in &plan.steps {
            match step {
                PlaceStep::Deref {
                    pointer: dereferenced,
                    ty: target_type,
                } => {
                    pointer = *dereferenced;
                    ty = *target_type;
                }
                PlaceStep::Field {
                    struct_id,
                    field_index,
                } => {
                    let ValueType::Struct(_) = ty else {
                        return Err("field assignment target is not a struct".into());
                    };
                    pointer = self
                        .builder
                        .build_struct_gep(
                            self.struct_types[*struct_id],
                            pointer,
                            *field_index,
                            "struct.field.ptr",
                        )
                        .map_err(|error| error.to_string())?;
                    let field = &self.structs[*struct_id].fields[*field_index as usize];
                    let field_declaration_type = field.ty.clone();
                    let field_span = field.span;
                    ty = self.resolve_type(&field_declaration_type, field_span)?;
                }
                PlaceStep::Array { array_id, index } => {
                    pointer =
                        self.array_element_pointer(pointer, ValueType::Array(*array_id), *index)?;
                    ty = ValueType::from(self.array_shapes[*array_id].element);
                }
                PlaceStep::Vector {
                    handle,
                    index,
                    element_type,
                } => {
                    pointer = self.vector_element_pointer(*handle, *index, *element_type)?;
                    ty = *element_type;
                }
            }
        }
        if ty != plan.final_type {
            return Err("assignment target type changed while planning the store".into());
        }
        Ok(pointer)
    }

    fn vector_element_pointer(
        &mut self,
        handle: PointerValue<'ctx>,
        index: IntValue<'ctx>,
        element_type: ValueType,
    ) -> Result<PointerValue<'ctx>, Diagnostic> {
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
        let data = self
            .builder
            .build_load(
                self.context.ptr_type(AddressSpace::default()),
                data_ptr,
                "vec.data",
            )
            .map_err(|error| error.to_string())?
            .into_pointer_value();
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
        Ok(self
            .builder
            .build_pointer_cast(
                address,
                self.context.ptr_type(AddressSpace::default()),
                "vec.element.ptr",
            )
            .map_err(|error| error.to_string())?)
    }

    pub(super) fn emit_address_of_place(
        &mut self,
        expr: &Expr,
    ) -> Result<(PointerValue<'ctx>, ValueType), Diagnostic> {
        if !matches!(expr, Expr::Variable(_, _)) {
            return Err(Diagnostic::at(
                codegen_span_of_expr(expr).into(),
                "raw address-taking currently requires a local or global variable",
            ));
        }
        let span = codegen_span_of_expr(expr);
        let name = codegen_assignment_root(expr)
            .ok_or("raw address-taking requires a variable, field, or array element")?;
        let (root_pointer, root_type) = if let Some(local) = self.lookup_local(name) {
            (local.pointer, local.ty)
        } else {
            let global_name = self.resolve_global_name(name).ok_or_else(|| {
                Diagnostic::at(span.into(), format!("undefined variable `{name}`"))
            })?;
            let declaration = self.global_declarations[&global_name].clone();
            if declaration.module != self.module_context && !declaration.is_public {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("global `{name}` is private"),
                ));
            }
            (
                self.globals[&global_name].as_pointer_value(),
                self.global_types[&global_name],
            )
        };
        self.emit_place_pointer(expr, name, root_pointer, root_type)
    }

    pub(super) fn emit_assignment_statement(
        &mut self,
        statement: &Stmt,
    ) -> Result<bool, Diagnostic> {
        match statement {
            Stmt::Assign { name, value, span } => {
                let (pointer, ty) = if let Some(local) = self.lookup_local(name) {
                    if matches!(
                        local.ty,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(Diagnostic::at(
                            span.into(),
                            "reference bindings cannot be reassigned".to_string(),
                        ));
                    }
                    if local.immutable {
                        let reason = if local.loop_variable {
                            format!("cannot assign to immutable loop variable `{name}`")
                        } else {
                            format!("cannot assign to immutable binding `{name}`")
                        };
                        return Err(Diagnostic::at(span.into(), reason.to_string()));
                    }
                    (local.pointer, local.ty)
                } else {
                    let global_name = self.resolve_global_name(name).ok_or_else(|| {
                        Diagnostic::at(span.into(), format!("undefined variable `{name}`"))
                    })?;
                    let declaration = self.global_declarations[&global_name].clone();
                    if declaration.module != self.module_context && !declaration.is_public {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("global `{name}` is private"),
                        ));
                    }
                    if !declaration.mutable {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("cannot assign to immutable global `{name}`"),
                        ));
                    }
                    (
                        self.globals[&global_name].as_pointer_value(),
                        self.global_types[&global_name],
                    )
                };
                let value = self.emit_expression_expected(value, ty)?;
                if ty != value.ty {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "cannot assign {} to `{name}` of type {}",
                            self.type_name(value.ty),
                            self.type_name(ty)
                        ),
                    ));
                }
                if ty == ValueType::Unit {
                    return Ok(false);
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
                    Diagnostic::at(
                        span.into(),
                        "assignment target must start with a variable".to_string(),
                    )
                })?;
                if matches!(target, Expr::Variable(_, _))
                    && self.lookup_local(name).is_some_and(|local| {
                        matches!(
                            local.ty,
                            ValueType::Reference(_) | ValueType::MutableReference(_)
                        )
                    })
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        "reference bindings cannot be reassigned".to_string(),
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
                            Diagnostic::at(span.into(), format!("undefined variable `{name}`"))
                        })?;
                        let declaration = self.global_declarations[&global_name].clone();
                        if declaration.module != self.module_context && !declaration.is_public {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!("global `{name}` is private"),
                            ));
                        }
                        (
                            self.globals[&global_name].as_pointer_value(),
                            self.global_types[&global_name],
                            declaration.mutable,
                            false,
                        )
                    };
                let through_pointer = codegen_place_contains_dereference(target);
                let vector_place =
                    !through_pointer && self.place_mutates_vector(target, name, root_type)?;
                if !through_pointer && !mutable && !vector_place {
                    let reason = if loop_variable {
                        format!("cannot assign through immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign through immutable binding `{name}`")
                    };
                    return Err(Diagnostic::at(span.into(), reason.to_string()));
                }
                let plan = self.plan_assignment_place(target, name, root_pointer, root_type)?;
                let target_type = plan.final_type;
                match plan.mutability {
                    None | Some(DerefMutability::Mutable) => {}
                    Some(DerefMutability::SharedReference) => {
                        return Err(Diagnostic::at(
                            span.into(),
                            "assignment target must start with a variable".to_string(),
                        ));
                    }
                    Some(DerefMutability::Immutable) => {
                        return Err(Diagnostic::at(
                            span.into(),
                            "cannot assign through an immutable pointer".to_string(),
                        ));
                    }
                }
                let value = self.emit_expression_expected(value, target_type)?;
                if value.ty != target_type {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "assignment target has type {}, expected {}",
                            self.type_name(value.ty),
                            self.type_name(target_type)
                        ),
                    ));
                }
                if value.ty == ValueType::Unit {
                    return Ok(false);
                }
                let target_pointer = self.apply_place_plan(&plan)?;
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

    pub(super) fn place_mutates_vector(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_type: ValueType,
    ) -> Result<bool, Diagnostic> {
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
    ) -> Result<ValueType, Diagnostic> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok(root_type),
            Expr::Field { value, name, span } => {
                let ValueType::Struct(struct_id) =
                    self.place_expression_type(value, root_name, root_type)?
                else {
                    return Err(Diagnostic::at(
                        span.into(),
                        "field access requires a struct".to_string(),
                    ));
                };
                let field = self.structs[struct_id]
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| {
                        Diagnostic::at(span.into(), format!("struct has no field `{name}`"))
                    })?;
                self.resolve_type(&field.ty.clone(), field.span)
            }
            Expr::Index { array, span, .. } => match self
                .place_expression_type(array, root_name, root_type)?
            {
                ValueType::Array(index) => Ok(ValueType::from(self.array_shapes[index].element)),
                ValueType::Vector(index) => Ok(ValueType::from(self.vector_shapes[index].element)),
                _ => Err(Diagnostic::at(
                    span.into(),
                    "indexing requires an array or vector".to_string(),
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
        Expr::Unary {
            op:
                UnaryOp::Dereference
                | UnaryOp::RawBorrowConst
                | UnaryOp::RawBorrowMut
                | UnaryOp::BorrowMut
                | UnaryOp::Borrow,
            value,
            ..
        } => codegen_assignment_root(value),
        _ => None,
    }
}

fn codegen_place_contains_dereference(expr: &Expr) -> bool {
    match expr {
        Expr::Unary {
            op: UnaryOp::Dereference,
            ..
        } => true,
        Expr::Field { value, .. } => codegen_place_contains_dereference(value),
        Expr::Index { array, .. } => codegen_place_contains_dereference(array),
        _ => false,
    }
}
