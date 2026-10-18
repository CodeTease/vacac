use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_indirect_call(
        &mut self,
        callee: TypedValue<'ctx>,
        args: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let ValueType::Function(function_id) = callee.ty else {
            return Err(Diagnostic::at(
                span.into(),
                format!("called expression must have a function type"),
            ));
        };
        let shape = self.function_shapes[function_id].clone();
        if args.len() != shape.params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "function value expects {} argument(s), got {}",
                    shape.params.len(),
                    args.len()
                ),
            ));
        }
        self.reject_overlapping_mutable_arguments(args, &shape.params)?;
        let closure = callee
            .value
            .ok_or("function value has no closure")?
            .into_struct_value();
        let code_pointer = self
            .builder
            .build_extract_value(closure, 0, "closure.code")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let environment = self
            .builder
            .build_extract_value(closure, 1, "closure.environment")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let mut values: Vec<BasicMetadataValueEnum<'ctx>> = Vec::with_capacity(args.len() + 1);
        values.push(environment.into());
        for (index, (arg, expected)) in args.iter().zip(shape.params.iter()).enumerate() {
            let value = self.emit_expression_expected(arg, *expected)?;
            if value.ty != *expected {
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "argument {} to function value expects {}, got {}",
                        index + 1,
                        self.type_name(*expected),
                        self.type_name(value.ty)
                    ),
                ));
            }
            values.push(value.value.ok_or("call argument has no value")?.into());
        }
        let result = self
            .builder
            .build_indirect_call(
                self.llvm_function_type(function_id)?,
                code_pointer,
                &values,
                "call.indirect",
            )
            .map_err(|error| error.to_string())?;
        let value = result.try_as_basic_value().basic();
        if (shape.return_type == ValueType::Unit) != value.is_none() {
            return Err("internal error: indirect call return type mismatch".into());
        }
        Ok(TypedValue {
            value,
            ty: shape.return_type,
        })
    }

    pub(super) fn emit_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        if let Some(local) = self.lookup_local(name) {
            let value = self
                .builder
                .build_load(
                    self.llvm_type(local.ty)
                        .ok_or("function value has no LLVM type")?,
                    local.pointer,
                    name,
                )
                .map_err(|error| error.to_string())?;
            return self.emit_indirect_call(
                TypedValue {
                    value: Some(value),
                    ty: local.ty,
                },
                args,
                span,
            );
        }
        if name == "vec_push" {
            if args.len() != 2 {
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "function `vec_push` expects 2 argument(s), got {}",
                        args.len()
                    ),
                ));
            }
            let vector = self.emit_expression(&args[0])?;
            let ValueType::Vector(vector_id) = vector.ty else {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("argument 1 to `vec_push` must be a vector"),
                ));
            };
            let element_type = ValueType::from(self.vector_shapes[vector_id].element);
            let value = self.emit_expression_expected(&args[1], element_type)?;
            if value.ty != element_type {
                return Err(Diagnostic::at(
                    codegen_span_of_expr(&args[1]).into(),
                    format!(
                        "argument 2 to `vec_push` has type {}, expected {}",
                        self.type_name(value.ty),
                        self.type_name(element_type)
                    ),
                ));
            }
            let header = vector
                .value
                .ok_or("vector has no handle")?
                .into_pointer_value();
            let element = value.value.ok_or("vector element has no value")?;
            let slot = self.create_entry_alloca(element_type, "vec.push.element")?;
            self.builder
                .build_store(slot, element)
                .map_err(|error| error.to_string())?;
            let source = self
                .builder
                .build_pointer_cast(
                    slot,
                    self.context.ptr_type(AddressSpace::default()),
                    "vec.push.source",
                )
                .map_err(|error| error.to_string())?;
            let size = self.vector_element_byte_size(element_type)?;
            let stride = self.vector_element_size(element_type)?;
            let runtime = self
                .module
                .get_function("vaca_vec_push")
                .ok_or("missing vector runtime append")?;
            self.builder
                .build_call(
                    runtime,
                    &[header.into(), source.into(), stride.into(), size.into()],
                    "vec.push",
                )
                .map_err(|error| error.to_string())?;
            return Ok(TypedValue {
                value: None,
                ty: ValueType::Unit,
            });
        }
        if name == "len" {
            if args.len() != 1 {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("function `len` expects 1 argument(s), got {}", args.len()),
                ));
            }
            let value = self.emit_expression(&args[0])?;
            let length = match value.ty {
                ValueType::Array(index) => self
                    .i64_type()
                    .const_int(u64::from(self.array_shapes[index].length), false),
                ValueType::Vector(_) => {
                    let header = value
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
                    let length_ptr = self
                        .builder
                        .build_struct_gep(header_type, header, 1, "vec.len.ptr")
                        .map_err(|error| error.to_string())?;
                    self.builder
                        .build_load(self.i64_type(), length_ptr, "vec.len")
                        .map_err(|error| error.to_string())?
                        .into_int_value()
                }
                _ => {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("argument 1 to `len` must be an array (or vector)"),
                    ));
                }
            };
            return Ok(TypedValue {
                value: Some(length.into()),
                ty: ValueType::I64,
            });
        }
        let Some(function_name) = self.resolve_function_name(name) else {
            let callee = self.emit_expression(&Expr::Variable(name.to_owned(), span))?;
            return self.emit_indirect_call(callee, args, span);
        };
        let (params, ret) = self.signatures[&function_name].clone();
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
        if args.len() != params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "function `{name}` expects {} argument(s), got {}",
                    params.len(),
                    args.len()
                ),
            ));
        }
        self.reject_overlapping_mutable_arguments(args, &params)?;
        let mut values: Vec<BasicMetadataValueEnum<'ctx>> = Vec::with_capacity(args.len());
        for (index, (arg, expected)) in args.iter().zip(params.iter()).enumerate() {
            let value = self.emit_expression_expected(arg, *expected)?;
            if value.ty != *expected {
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "argument {} to `{name}` expects {}, got {}",
                        index + 1,
                        self.type_name(*expected),
                        self.type_name(value.ty)
                    ),
                ));
            }
            values.push(value.value.ok_or("call argument has no value")?.into());
        }
        let function = *self
            .functions
            .get(&function_name)
            .ok_or_else(|| Diagnostic::at(span.into(), format!("missing function `{name}`")))?;
        let call = self
            .builder
            .build_call(function, &values, "call")
            .map_err(|e| e.to_string())?;
        let value = call.try_as_basic_value().basic();
        if ret == ValueType::Unit && value.is_some() {
            return Err("internal error: void call returned a value".into());
        }
        if ret != ValueType::Unit && value.is_none() {
            return Err("internal error: value call returned void".into());
        }
        Ok(TypedValue { value, ty: ret })
    }
}
