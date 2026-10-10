use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
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
            let ValueType::Array(index) = value.ty else {
                return Err(format!(
                    "line {}:{}: argument 1 to `len` must be an array",
                    span.line, span.column
                ));
            };
            let length = self.array_shapes[index].length;
            return Ok(TypedValue {
                value: Some(self.i64_type().const_int(u64::from(length), false).into()),
                ty: ValueType::I64,
            });
        }
        let (params, ret) = self.signatures.get(name).cloned().ok_or_else(|| {
            format!(
                "line {}:{}: unknown function `{name}`",
                span.line, span.column
            )
        })?;
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
