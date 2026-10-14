use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_print_builtins(&mut self) -> Result<(), Diagnostic> {
        let i8_ptr = self.context.ptr_type(AddressSpace::default());
        let printf_type = self.context.i32_type().fn_type(&[i8_ptr.into()], true);
        let printf = self.module.add_function("printf", printf_type, None);
        self.emit_printf_wrapper(printf, "print_i64", ValueType::I64, "%lld\n")?;
        self.emit_printf_wrapper(printf, "print_f32", ValueType::F32, "%.9g\n")?;
        self.emit_printf_wrapper(printf, "print_f64", ValueType::F64, "%.17g\n")?;
        self.emit_printf_wrapper(printf, "print", ValueType::Str, "%s")?;
        self.emit_printf_wrapper(printf, "println", ValueType::Str, "%s\n")?;
        Ok(())
    }

    pub(in crate::codegen) fn emit_printf_wrapper(
        &mut self,
        printf: FunctionValue<'ctx>,
        name: &str,
        param: ValueType,
        format: &str,
    ) -> Result<(), Diagnostic> {
        let function_type = self.llvm_fn_type(&[param], ValueType::Unit)?;
        let function = self.module.add_function(name, function_type, None);
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        let fmt = self
            .builder
            .build_global_string_ptr(format, &format!("vaca_{name}_fmt"))
            .map_err(|e| e.to_string())?;
        let value = function
            .get_nth_param(0)
            .ok_or_else(|| Diagnostic::new(format!("missing {name} parameter")))?;
        let value = if param == ValueType::F32 {
            self.builder
                .build_float_ext(
                    value.into_float_value(),
                    self.context.f64_type(),
                    "print_f32.promoted",
                )
                .map_err(|e| e.to_string())?
                .into()
        } else {
            value
        };
        self.builder
            .build_call(
                printf,
                &[fmt.as_pointer_value().into(), value.into()],
                "printf_result",
            )
            .map_err(|e| e.to_string())?;
        self.builder.build_return(None).map_err(|e| e.to_string())?;
        self.functions.insert(name.into(), function);
        Ok(())
    }
}
