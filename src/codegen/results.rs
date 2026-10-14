use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_try(
        &mut self,
        expression: &Expr,
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let result = self.emit_expression(expression)?;
        let ValueType::Enum(result_id) = result.ty else {
            return Err(Diagnostic::at(
                span.into(),
                format!("postfix `?` requires a `Result<T, E>` value"),
            ));
        };
        if !self.enums[result_id].name.starts_with("Result::<") {
            return Err(Diagnostic::at(
                span.into(),
                format!("postfix `?` requires the built-in `Result<T, E>` type"),
            ));
        }
        let result_value = result
            .value
            .ok_or("Result value has no LLVM value")?
            .into_struct_value();
        let tag = self
            .builder
            .build_extract_value(result_value, 0, "result.tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let function = self
            .current_function
            .ok_or("postfix `?` outside a function")?;
        let ok_block = self.context.append_basic_block(function, "result.ok");
        let err_block = self.context.append_basic_block(function, "result.err");
        let test = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                tag,
                self.context.i32_type().const_zero(),
                "result.is_ok",
            )
            .map_err(|error| error.to_string())?;
        self.builder
            .build_conditional_branch(test, ok_block, err_block)
            .map_err(|error| error.to_string())?;

        let err_type = self.enums[result_id]
            .variants
            .iter()
            .find(|variant| variant.name == "Err")
            .and_then(|variant| variant.payload.first())
            .cloned()
            .ok_or("invalid built-in Result Err payload")?;
        let result_err_type = self.resolve_type(&err_type, span)?;
        let ValueType::Enum(return_id) = self.return_type else {
            return Err(Diagnostic::at(
                span.into(),
                format!("postfix `?` can only be used in a function returning `Result<T, E>`"),
            ));
        };
        if !self.enums[return_id].name.starts_with("Result::<") {
            return Err(Diagnostic::at(
                span.into(),
                format!("postfix `?` can only be used in a function returning `Result<T, E>`"),
            ));
        }
        let return_err_index = self.enums[return_id]
            .variants
            .iter()
            .position(|variant| variant.name == "Err")
            .ok_or("invalid enclosing Result Err variant")?;
        let return_err_type = self.enums[return_id].variants[return_err_index]
            .payload
            .first()
            .cloned()
            .ok_or("invalid enclosing Result Err payload")?;
        let return_err_type = self.resolve_type(&return_err_type, span)?;
        if result_err_type != return_err_type {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "propagated error has type {}, but enclosing Result error type is {}",
                    self.type_name(result_err_type),
                    self.type_name(return_err_type)
                ),
            ));
        }

        self.builder.position_at_end(err_block);
        let err_payload_tuple = self
            .builder
            .build_extract_value(result_value, 2, "result.err.payload")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let err_payload = self
            .builder
            .build_extract_value(err_payload_tuple, 0, "result.err.value")
            .map_err(|error| error.to_string())?;
        let err_enum = self.enum_types[return_id];
        let err_tuple = self.enum_payload_types[return_id][return_err_index].const_zero();
        let err_tuple = self
            .builder
            .build_insert_value(err_tuple, err_payload, 0, "result.propagated.err.payload")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let err_enum_value = self
            .builder
            .build_insert_value(
                err_enum.const_zero(),
                self.context
                    .i32_type()
                    .const_int(return_err_index as u64, false),
                0,
                "result.propagated.err.tag",
            )
            .map_err(|error| error.to_string())?;
        let err_enum_value = self
            .builder
            .build_insert_value(
                err_enum_value.into_struct_value(),
                err_tuple,
                return_err_index as u32 + 1,
                "result.propagated.err",
            )
            .map_err(|error| error.to_string())?;
        self.builder
            .build_return(Some(&err_enum_value))
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(ok_block);
        let ok_type = self.enums[result_id]
            .variants
            .iter()
            .find(|variant| variant.name == "Ok")
            .and_then(|variant| variant.payload.first())
            .cloned()
            .ok_or("invalid built-in Result Ok payload")?;
        let value_type = self.resolve_type(&ok_type, span)?;
        let ok_tuple = self
            .builder
            .build_extract_value(result_value, 1, "result.ok.payload")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let ok_value = self
            .builder
            .build_extract_value(ok_tuple, 0, "result.ok.value")
            .map_err(|error| error.to_string())?;
        Ok(TypedValue {
            value: Some(ok_value),
            ty: value_type,
        })
    }
}
