use super::*;

pub(super) fn codegen_borrow_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name, _) => Some(name),
        Expr::Field { value, .. } => codegen_borrow_root(value),
        Expr::Index { array, .. } => codegen_borrow_root(array),
        _ => None,
    }
}

pub(super) fn codegen_mutable_borrow_origin(
    expr: &Expr,
    root_binding_id: usize,
) -> Option<MutableReferenceOrigin> {
    fn collect(expr: &Expr, projections: &mut Vec<MutableReferenceProjection>) -> bool {
        match expr {
            Expr::Variable(_, _) => true,
            Expr::Field { value, name, .. } => {
                if !collect(value, projections) {
                    return false;
                }
                projections.push(MutableReferenceProjection::Field(name.clone()));
                true
            }
            Expr::Index { array, index, .. } => {
                if !collect(array, projections) {
                    return false;
                }
                projections.push(MutableReferenceProjection::Index(codegen_constant_i64(
                    index,
                )));
                true
            }
            _ => false,
        }
    }

    fn codegen_constant_i64(expr: &Expr) -> Option<i64> {
        match expr {
            Expr::Integer(value, _) if *value <= i64::MAX as u64 => Some(*value as i64),
            Expr::Unary {
                op: UnaryOp::Negate,
                value,
                ..
            } => match value.as_ref() {
                Expr::Integer(value, _) if *value <= i64::MAX as u64 => Some(-(*value as i64)),
                Expr::Integer(value, _) if *value == (i64::MAX as u64) + 1 => Some(i64::MIN),
                _ => None,
            },
            _ => None,
        }
    }

    fn codegen_borrow_place_is_fixed(expr: &Expr) -> bool {
        match expr {
            Expr::Variable(_, _) => true,
            Expr::Field { value, .. } => codegen_borrow_place_is_fixed(value),
            Expr::Index { array, index, .. } => {
                codegen_constant_i64(index).is_some() && codegen_borrow_place_is_fixed(array)
            }
            _ => false,
        }
    }

    let mut projections = Vec::new();
    if !codegen_borrow_place_is_fixed(expr) {
        return Some(MutableReferenceOrigin::Unknown);
    }
    collect(expr, &mut projections).then_some(MutableReferenceOrigin::Place {
        binding_id: root_binding_id,
        projections,
    })
}

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_expression(&mut self, expr: &Expr) -> Result<TypedValue<'ctx>, Diagnostic> {
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
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`vec_new` expects 0 arguments"),
                    ));
                }
                Err(Diagnostic::at(
                    span.into(),
                    format!("`vec_new()` requires a `Vec<T>` type annotation"),
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
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("function `{name}` is private"),
                        ));
                    }
                    let (params, result) = self.signatures[&function_name].clone();
                    let ty = self.function_type(params, result);
                    let adapter = self
                        .function_adapters
                        .get(&function_name)
                        .copied()
                        .unwrap_or(function);
                    return self.make_closure(
                        adapter.as_global_value().as_pointer_value(),
                        self.context.ptr_type(AddressSpace::default()).const_null(),
                        ty,
                    );
                }
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
                if let Some(name) = codegen_borrow_root(expr)
                    && let Some(local) = self.lookup_local(name)
                {
                    let (pointer, ty) =
                        self.emit_place_pointer(expr, name, local.pointer, local.ty)?;
                    let value = self
                        .builder
                        .build_load(
                            self.llvm_type(ty).ok_or("indexed place has no LLVM type")?,
                            pointer,
                            "array.place",
                        )
                        .map_err(|error| error.to_string())?;
                    return Ok(TypedValue {
                        value: Some(value),
                        ty,
                    });
                }
                let array_value = self.emit_expression(array)?;
                let index = self.emit_expression(index)?;
                let index = self.expect_i64(index, "array index", *span)?;
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
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("indexing requires an array (or vector)"),
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
                if let Some(root) = codegen_borrow_root(expr)
                    && let Some(local) = self.lookup_local(root)
                {
                    let (pointer, ty) =
                        self.emit_place_pointer(expr, root, local.pointer, local.ty)?;
                    let value = self
                        .builder
                        .build_load(
                            self.llvm_type(ty).ok_or("field place has no LLVM type")?,
                            pointer,
                            "struct.field.value",
                        )
                        .map_err(|error| error.to_string())?;
                    return Ok(TypedValue {
                        value: Some(value),
                        ty,
                    });
                }
                let value = self.emit_expression(value)?;
                let ValueType::Struct(struct_id) = value.ty else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("field access requires a struct value"),
                    ));
                };
                let (owner, public) = self
                    .struct_visibility
                    .get(&self.structs[struct_id].name)
                    .cloned()
                    .unwrap_or_default();
                if owner != self.module_context && !public {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("struct `{}` is private", self.structs[struct_id].name),
                    ));
                }
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
            Expr::Unary { op, value, span } => {
                if matches!(op, UnaryOp::Borrow) {
                    if let Expr::Unary {
                        op: UnaryOp::Dereference,
                        value: reference,
                        ..
                    } = value.as_ref()
                    {
                        let reference = self.emit_expression(reference)?;
                        if let ValueType::Reference(_) = reference.ty {
                            return Ok(reference);
                        }
                        if matches!(reference.ty, ValueType::MutableReference(_)) {
                            return Err(Diagnostic::at(
                                span.into(),
                                "mutable reference reborrowing is not supported",
                            ));
                        }
                    }
                    let Some(name) = codegen_borrow_root(value) else {
                        return Err(Diagnostic::at(
                            span.into(),
                            "borrowed place must start with a local binding",
                        ));
                    };
                    let local = self.lookup_local(name).ok_or_else(|| {
                        Diagnostic::at(span.into(), format!("only local bindings can be borrowed"))
                    })?;
                    if self.place_mutates_vector(value, name, local.ty)? {
                        return Err(Diagnostic::at(
                            span.into(),
                            "references to values containing vectors are not supported",
                        ));
                    }
                    let (pointer, target_type) =
                        self.emit_place_pointer(value, name, local.pointer, local.ty)?;
                    if matches!(
                        target_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("references to references are not supported"),
                        ));
                    }
                    if !self.reference_target_is_supported(
                        target_type,
                        &mut std::collections::HashSet::new(),
                    )? {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("references to values containing vectors are not supported"),
                        ));
                    }
                    let reference_type = self.reference_type(target_type);
                    return Ok(TypedValue {
                        value: Some(pointer.into()),
                        ty: reference_type,
                    });
                }
                if matches!(op, UnaryOp::BorrowMut) {
                    let Some(name) = codegen_borrow_root(value) else {
                        return Err(Diagnostic::at(
                            span.into(),
                            "borrowed place must start with a local binding",
                        ));
                    };
                    let local = self.lookup_local(name).ok_or_else(|| {
                        Diagnostic::at(span.into(), format!("only local bindings can be borrowed"))
                    })?;
                    if local.immutable || local.loop_variable {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("mutable borrowing requires a mutable local binding"),
                        ));
                    }
                    if self.place_mutates_vector(value, name, local.ty)? {
                        return Err(Diagnostic::at(
                            span.into(),
                            "references to values containing vectors are not supported",
                        ));
                    }
                    let (pointer, target_type) =
                        self.emit_place_pointer(value, name, local.pointer, local.ty)?;
                    if !self.reference_target_is_supported(
                        target_type,
                        &mut std::collections::HashSet::new(),
                    )? {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("references to values containing vectors are not supported"),
                        ));
                    }
                    if matches!(
                        target_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("references to references are not supported"),
                        ));
                    }
                    let ty = self.mutable_reference_type(target_type);
                    return Ok(TypedValue {
                        value: Some(pointer.into()),
                        ty,
                    });
                }
                if matches!(op, UnaryOp::RawBorrowConst | UnaryOp::RawBorrowMut) {
                    if matches!(op, UnaryOp::RawBorrowMut) {
                        let Expr::Variable(name, _) = value.as_ref() else {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!(
                                    "raw address-taking currently requires a local or global variable"
                                ),
                            ));
                        };
                        if let Some(local) = self.lookup_local(name) {
                            if local.immutable || local.loop_variable {
                                return Err(Diagnostic::at(
                                    span.into(),
                                    format!(
                                        "cannot take a mutable raw pointer to an immutable binding"
                                    ),
                                ));
                            }
                        } else if let Some(global_name) = self.resolve_global_name(name) {
                            if !self.global_declarations[&global_name].mutable {
                                return Err(Diagnostic::at(
                                    span.into(),
                                    format!(
                                        "cannot take a mutable raw pointer to an immutable global"
                                    ),
                                ));
                            }
                        } else {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!("undefined variable `{name}`"),
                            ));
                        }
                    }
                    let (pointer, target_type) = self.emit_address_of_place(value)?;
                    let ty =
                        self.raw_pointer_type(target_type, matches!(op, UnaryOp::RawBorrowMut));
                    return Ok(TypedValue {
                        value: Some(pointer.into()),
                        ty,
                    });
                }
                if matches!(op, UnaryOp::Dereference) {
                    let reference = self.emit_expression(value)?;
                    let (target_type, pointer) = match reference.ty {
                        ValueType::Reference(index) => (
                            self.reference_types[index],
                            reference
                                .value
                                .ok_or("reference value is missing")?
                                .into_pointer_value(),
                        ),
                        ValueType::MutableReference(index) => (
                            self.mutable_reference_types[index],
                            reference
                                .value
                                .ok_or("reference value is missing")?
                                .into_pointer_value(),
                        ),
                        ValueType::RawPointer(index) => (
                            self.raw_pointer_types[index].0,
                            reference
                                .value
                                .ok_or("pointer value is missing")?
                                .into_pointer_value(),
                        ),
                        _ => {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!(
                                    "unary `*` requires a reference or raw pointer, got {}",
                                    self.type_name(reference.ty)
                                ),
                            ));
                        }
                    };
                    let loaded = self
                        .builder
                        .build_load(
                            self.llvm_type(target_type)
                                .ok_or("reference target has no LLVM type")?,
                            pointer,
                            "reference.deref",
                        )
                        .map_err(|error| error.to_string())?;
                    return Ok(TypedValue {
                        value: Some(loaded),
                        ty: target_type,
                    });
                }
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
                            return Err(Diagnostic::at(
                                span.into(),
                                format!("unary `-` requires i64, got {}", self.type_name(value.ty)),
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
                            return Err(Diagnostic::at(
                                span.into(),
                                format!(
                                    "unary `!` requires bool, got {}",
                                    self.type_name(value.ty)
                                ),
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
                    UnaryOp::Borrow
                    | UnaryOp::BorrowMut
                    | UnaryOp::RawBorrowConst
                    | UnaryOp::RawBorrowMut
                    | UnaryOp::Dereference => unreachable!(),
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
                    lifetime_params: Vec::new(),
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
                let capture_names = self
                    .lambda_captures
                    .get(&(span.file, span.line, span.column))
                    .cloned()
                    .unwrap_or_default();
                let mut captures = Vec::with_capacity(capture_names.len());
                for capture_name in capture_names {
                    let local = self.lookup_local(&capture_name).ok_or_else(|| {
                        format!("internal error: captured local `{capture_name}` is unavailable")
                    })?;
                    if matches!(
                        local.ty,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("closures cannot capture references"),
                        ));
                    }
                    captures.push((capture_name, local.ty, local.pointer));
                }
                self.lambda_capture_types.insert(
                    name.clone(),
                    captures
                        .iter()
                        .map(|(capture, ty, _)| (capture.clone(), *ty))
                        .collect(),
                );
                let llvm_function = self.module.add_function(
                    &name,
                    self.closure_function_type(&params, ret)?,
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
                let environment = if captures.is_empty() {
                    self.context.ptr_type(AddressSpace::default()).const_null()
                } else {
                    let capture_layout = captures
                        .iter()
                        .map(|(capture, ty, _)| (capture.clone(), *ty))
                        .collect::<Vec<_>>();
                    let environment_type = self.closure_environment_type(&capture_layout)?;
                    let allocator = self
                        .module
                        .get_function("vaca_alloc")
                        .ok_or("missing allocator runtime function")?;
                    let size = environment_type
                        .size_of()
                        .ok_or("closure environment has no size")?;
                    let environment = self
                        .builder
                        .build_call(
                            allocator,
                            &[
                                self.context
                                    .ptr_type(AddressSpace::default())
                                    .const_null()
                                    .into(),
                                size.into(),
                            ],
                            "closure.environment",
                        )
                        .map_err(|error| error.to_string())?
                        .try_as_basic_value()
                        .basic()
                        .ok_or("closure allocator returned void")?
                        .into_pointer_value();
                    for (index, (_, ty, source)) in captures.iter().enumerate() {
                        let field = self
                            .builder
                            .build_struct_gep(
                                environment_type,
                                environment,
                                index as u32,
                                "closure.capture.ptr",
                            )
                            .map_err(|error| error.to_string())?;
                        let value = self
                            .builder
                            .build_load(
                                self.llvm_type(*ty).ok_or("capture has no LLVM type")?,
                                *source,
                                "closure.capture",
                            )
                            .map_err(|error| error.to_string())?;
                        self.builder
                            .build_store(field, value)
                            .map_err(|error| error.to_string())?;
                    }
                    environment
                };
                self.make_closure(
                    llvm_function.as_global_value().as_pointer_value(),
                    environment,
                    function_type,
                )
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span: _,
            } => self.emit_if_expression(condition, then_block, else_block),
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
                ..
            } => self.emit_enum_variant(enum_name, variant_name, payload.as_deref(), *span),
            Expr::Try { value, span } => self.emit_try(value, *span),
            Expr::Match { value, arms, span } => {
                let value = self.emit_match(value, arms, *span)?;
                if matches!(value.ty, ValueType::MutableReference(_)) {
                    return Err(Diagnostic::at(
                        span.into(),
                        "mutable references cannot escape through match expressions",
                    ));
                }
                Ok(value)
            }
            Expr::Loop { body, span } => {
                let value = self.emit_loop_expression(body)?;
                if matches!(value.ty, ValueType::Reference(_)) {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references cannot escape through loop expressions"),
                    ));
                }
                Ok(value)
            }
        }
    }
}
