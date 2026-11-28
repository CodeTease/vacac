use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_string_concat_builtin(&mut self) -> Result<(), Diagnostic> {
        let pointer_type = self.context.ptr_type(AddressSpace::default());
        let size_type = self.i64_type();
        let string_length = self.module.add_function(
            "strlen",
            size_type.fn_type(&[pointer_type.into()], false),
            None,
        );
        let allocate = self
            .module
            .get_function("vaca_alloc")
            .ok_or("missing allocator runtime function")?;
        let copy = self.module.add_function(
            "memcpy",
            pointer_type.fn_type(
                &[pointer_type.into(), pointer_type.into(), size_type.into()],
                false,
            ),
            None,
        );
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing runtime exit declaration")?;
        let function = self.module.add_function(
            "vaca_str_concat",
            pointer_type.fn_type(&[pointer_type.into(), pointer_type.into()], false),
            None,
        );
        let entry = self.context.append_basic_block(function, "entry");
        let invalid_size = self.context.append_basic_block(function, "size.error");
        let allocate_block = self.context.append_basic_block(function, "allocate");
        let copy_block = self.context.append_basic_block(function, "copy");

        self.builder.position_at_end(entry);
        let left = function.get_nth_param(0).ok_or("missing left string")?;
        let right = function.get_nth_param(1).ok_or("missing right string")?;
        let left_length = self
            .builder
            .build_call(string_length, &[left.into()], "str.left.length")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: strlen returned void")?
            .into_int_value();
        let right_length = self
            .builder
            .build_call(string_length, &[right.into()], "str.right.length")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: strlen returned void")?
            .into_int_value();
        let max_payload_length = self
            .builder
            .build_int_sub(
                size_type.const_all_ones(),
                size_type.const_int(1, false),
                "str.maximum.payload.length",
            )
            .map_err(|e| e.to_string())?;
        let available = self
            .builder
            .build_int_sub(max_payload_length, left_length, "str.available")
            .map_err(|e| e.to_string())?;
        let left_too_long = self
            .builder
            .build_int_compare(
                IntPredicate::UGT,
                left_length,
                max_payload_length,
                "str.left.too.long",
            )
            .map_err(|e| e.to_string())?;
        let right_too_long = self
            .builder
            .build_int_compare(
                IntPredicate::UGT,
                right_length,
                available,
                "str.right.too.long",
            )
            .map_err(|e| e.to_string())?;
        let size_overflow = self
            .builder
            .build_or(left_too_long, right_too_long, "str.size.overflow")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(size_overflow, invalid_size, allocate_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(invalid_size);
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "str.size.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(allocate_block);
        let combined_length = self
            .builder
            .build_int_add(left_length, right_length, "str.combined.length")
            .map_err(|e| e.to_string())?;
        let allocation_size = self
            .builder
            .build_int_add(
                combined_length,
                size_type.const_int(1, false),
                "str.allocation.size",
            )
            .map_err(|e| e.to_string())?;
        let result = self
            .builder
            .build_call(
                allocate,
                &[pointer_type.const_null().into(), allocation_size.into()],
                "str.allocation",
            )
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: allocator returned void")?
            .into_pointer_value();
        self.builder
            .build_unconditional_branch(copy_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(copy_block);
        self.builder
            .build_call(
                copy,
                &[result.into(), left.into(), left_length.into()],
                "str.copy.left",
            )
            .map_err(|e| e.to_string())?;
        let byte_type = self.context.i8_type();
        let right_destination = unsafe {
            self.builder
                .build_gep(byte_type, result, &[left_length], "str.right.destination")
        }
        .map_err(|e| e.to_string())?;
        self.builder
            .build_call(
                copy,
                &[right_destination.into(), right.into(), right_length.into()],
                "str.copy.right",
            )
            .map_err(|e| e.to_string())?;
        let terminator = unsafe {
            self.builder.build_gep(
                byte_type,
                right_destination,
                &[right_length],
                "str.terminator",
            )
        }
        .map_err(|e| e.to_string())?;
        self.builder
            .build_store(terminator, byte_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&result))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Wrap extern functions that return `str` so a `NULL` C string cannot
    /// enter Vaca as a non-null `str`. Raw-pointer returns stay nullable.
    pub(in crate::codegen) fn emit_c_string_result_wrappers(
        &mut self,
        program: &Program,
    ) -> Result<(), Diagnostic> {
        for (index, function) in program.extern_functions.iter().enumerate() {
            if function.return_type != Type::Str {
                continue;
            }
            let link = self
                .module
                .get_function(&function.link_name)
                .ok_or_else(|| {
                    Diagnostic::at(
                        function.span.into(),
                        format!("missing C symbol `{}`", function.link_name),
                    )
                })?;
            let wrapper = self.module.add_function(
                &format!("vaca.extern.str.{index}"),
                link.get_type(),
                Some(Linkage::Internal),
            );
            let entry = self.context.append_basic_block(wrapper, "entry");
            let failure = self.context.append_basic_block(wrapper, "string.null");
            let success = self.context.append_basic_block(wrapper, "string.valid");

            self.builder.position_at_end(entry);
            let mut arguments = Vec::with_capacity(function.params.len());
            for parameter in 0..function.params.len() {
                arguments.push(
                    wrapper
                        .get_nth_param(parameter as u32)
                        .ok_or("extern string wrapper parameter missing")?
                        .into(),
                );
            }
            let result = self
                .builder
                .build_call(link, &arguments, "extern.string")
                .map_err(|error| error.to_string())?
                .try_as_basic_value()
                .basic()
                .ok_or("extern string call returned void")?
                .into_pointer_value();
            let is_null = self
                .builder
                .build_is_null(result, "extern.string.null")
                .map_err(|error| error.to_string())?;
            self.builder
                .build_conditional_branch(is_null, failure, success)
                .map_err(|error| error.to_string())?;

            self.builder.position_at_end(failure);
            let exit = self
                .module
                .get_function("exit")
                .ok_or("missing runtime exit declaration")?;
            self.builder
                .build_call(
                    exit,
                    &[self.context.i32_type().const_int(1, false).into()],
                    "extern.string.exit",
                )
                .map_err(|error| error.to_string())?;
            self.builder
                .build_unreachable()
                .map_err(|error| error.to_string())?;

            self.builder.position_at_end(success);
            self.builder
                .build_return(Some(&result))
                .map_err(|error| error.to_string())?;
            self.functions.insert(function.name.clone(), wrapper);
        }
        Ok(())
    }

    pub(in crate::codegen) fn strcmp_function(&self) -> FunctionValue<'ctx> {
        self.module.get_function("strcmp").unwrap_or_else(|| {
            let ptr = self.context.ptr_type(AddressSpace::default());
            let strcmp_type = self
                .context
                .i32_type()
                .fn_type(&[ptr.into(), ptr.into()], false);
            self.module.add_function("strcmp", strcmp_type, None)
        })
    }

    pub(in crate::codegen) fn string_literal(
        &mut self,
        value: &str,
    ) -> Result<PointerValue<'ctx>, Diagnostic> {
        if let Some(pointer) = self.string_literals.get(value) {
            return Ok(*pointer);
        }
        let name = format!("vaca_str.{}", self.string_literals.len());
        let pointer = self
            .builder
            .build_global_string_ptr(value, &name)
            .map_err(|e| e.to_string())?
            .as_pointer_value();
        self.string_literals.insert(value.to_owned(), pointer);
        Ok(pointer)
    }

    pub(in crate::codegen) fn emit_c_main_wrapper(&mut self) -> Result<(), Diagnostic> {
        let vaca_main = *self.functions.get("main").ok_or("missing main function")?;
        let wrapper_type = self.context.i32_type().fn_type(&[], false);
        let wrapper = self.module.add_function("main", wrapper_type, None);
        let entry = self.context.append_basic_block(wrapper, "entry");
        self.builder.position_at_end(entry);
        let result = self
            .builder
            .build_call(vaca_main, &[], "vaca_result")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: Vaca main returned void")?
            .into_int_value();
        let process_status = self
            .builder
            .build_int_truncate(result, self.context.i32_type(), "process.status")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&process_status))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
