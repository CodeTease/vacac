use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_allocator_builtin(&mut self) -> Result<(), String> {
        let pointer = self.context.ptr_type(AddressSpace::default());
        let size_type = self.i64_type();
        let realloc = self.module.add_function(
            "realloc",
            pointer.fn_type(&[pointer.into(), size_type.into()], false),
            None,
        );
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing runtime exit declaration")?;
        let allocate = self.module.add_function(
            "vaca_alloc",
            pointer.fn_type(&[pointer.into(), size_type.into()], false),
            None,
        );
        let entry = self.context.append_basic_block(allocate, "entry");
        let failed = self
            .context
            .append_basic_block(allocate, "allocation.error");
        let success = self
            .context
            .append_basic_block(allocate, "allocation.success");

        self.builder.position_at_end(entry);
        let old_pointer = allocate
            .get_nth_param(0)
            .ok_or("missing pointer to resize")?;
        let size = allocate.get_nth_param(1).ok_or("missing allocation size")?;
        let result = self
            .builder
            .build_call(realloc, &[old_pointer.into(), size.into()], "allocation")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("realloc returned void")?
            .into_pointer_value();
        let is_null = self
            .builder
            .build_is_null(result, "allocation.failed")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(is_null, failed, success)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(failed);
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "allocation.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(success);
        self.builder
            .build_return(Some(&result))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
