use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_string_concat_builtin(&mut self) -> Result<(), String> {
        let pointer_type = self.context.ptr_type(AddressSpace::default());
        let size_type = self.i64_type();
        let string_length = self.module.add_function(
            "strlen",
            size_type.fn_type(&[pointer_type.into()], false),
            None,
        );
        let allocate = self.module.add_function(
            "malloc",
            pointer_type.fn_type(&[size_type.into()], false),
            None,
        );
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
            "apex_str_concat",
            pointer_type.fn_type(&[pointer_type.into(), pointer_type.into()], false),
            None,
        );
        let entry = self.context.append_basic_block(function, "entry");
        let invalid_size = self.context.append_basic_block(function, "size.error");
        let allocate_block = self.context.append_basic_block(function, "allocate");
        let allocation_failure_block = self
            .context
            .append_basic_block(function, "allocation.error");
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
            .build_call(allocate, &[allocation_size.into()], "str.allocation")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: malloc returned void")?
            .into_pointer_value();
        let allocation_failed = self
            .builder
            .build_is_null(result, "str.allocation.failed")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(allocation_failed, allocation_failure_block, copy_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(allocation_failure_block);
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "str.allocation.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
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
    ) -> Result<PointerValue<'ctx>, String> {
        if let Some(pointer) = self.string_literals.get(value) {
            return Ok(*pointer);
        }
        let name = format!("apex_str.{}", self.string_literals.len());
        let pointer = self
            .builder
            .build_global_string_ptr(value, &name)
            .map_err(|e| e.to_string())?
            .as_pointer_value();
        self.string_literals.insert(value.to_owned(), pointer);
        Ok(pointer)
    }

    pub(in crate::codegen) fn emit_c_main_wrapper(&mut self) -> Result<(), String> {
        let apex_main = *self.functions.get("main").ok_or("missing main function")?;
        let wrapper_type = self.context.i32_type().fn_type(&[], false);
        let wrapper = self.module.add_function("main", wrapper_type, None);
        let entry = self.context.append_basic_block(wrapper, "entry");
        self.builder.position_at_end(entry);
        let result = self
            .builder
            .build_call(apex_main, &[], "apex_result")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: Apex main returned void")?
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
