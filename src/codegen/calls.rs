use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if name == "vec_push" {
            if args.len() != 2 {
                return Err(format!(
                    "line {}:{}: function `vec_push` expects 2 argument(s), got {}",
                    span.line,
                    span.column,
                    args.len()
                ));
            }
            let vector = self.emit_expression(&args[0])?;
            let ValueType::Vector(vector_id) = vector.ty else {
                return Err(format!(
                    "line {}:{}: argument 1 to `vec_push` must be a vector",
                    span.line, span.column
                ));
            };
            let element_type = ValueType::from(self.vector_shapes[vector_id].element);
            let value = self.emit_expression_expected(&args[1], element_type)?;
            if value.ty != element_type {
                return Err(format!(
                    "argument 2 to `vec_push` has type {}, expected {}",
                    self.type_name(value.ty),
                    self.type_name(element_type)
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
                .get_function("apex_vec_push")
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
                return Err(format!(
                    "line {}:{}: function `len` expects 1 argument(s), got {}",
                    span.line,
                    span.column,
                    args.len()
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
                    return Err(format!(
                        "line {}:{}: argument 1 to `len` must be an array (or vector)",
                        span.line, span.column
                    ));
                }
            };
            return Ok(TypedValue {
                value: Some(length.into()),
                ty: ValueType::I64,
            });
        }
        let (params, ret) = self.signatures.get(name).cloned().ok_or_else(|| {
            format!(
                "line {}:{}: unknown function `{name}`",
                span.line, span.column
            )
        })?;
        if self
            .function_visibility
            .get(name)
            .is_some_and(|(owner, public)| owner != &self.module_context && !public)
        {
            return Err(format!(
                "line {}:{}: function `{name}` is private",
                span.line, span.column
            ));
        }
        if args.len() != params.len() {
            return Err(format!(
                "line {}:{}: function `{name}` expects {} argument(s), got {}",
                span.line,
                span.column,
                params.len(),
                args.len()
            ));
        }
        let mut values: Vec<BasicMetadataValueEnum<'ctx>> = Vec::with_capacity(args.len());
        for (index, (arg, expected)) in args.iter().zip(params.iter()).enumerate() {
            let value = self.emit_expression_expected(arg, *expected)?;
            if value.ty != *expected {
                return Err(format!(
                    "line {}:{}: argument {} to `{name}` expects {}, got {}",
                    span.line,
                    span.column,
                    index + 1,
                    self.type_name(*expected),
                    self.type_name(value.ty)
                ));
            }
            values.push(value.value.ok_or("call argument has no value")?.into());
        }
        let function = *self.functions.get(name).ok_or_else(|| {
            format!(
                "line {}:{}: missing function `{name}`",
                span.line, span.column
            )
        })?;
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
