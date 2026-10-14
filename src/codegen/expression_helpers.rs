use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_if_expression(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: &Block,
    ) -> Result<TypedValue<'ctx>, String> {
        let condition = self.emit_expression(condition)?;
        let condition = self.expect_bool(condition, "if condition")?;
        let function = self
            .current_function
            .ok_or("if expression outside function")?;
        let then_bb = self.context.append_basic_block(function, "if.expr.then");
        let else_bb = self.context.append_basic_block(function, "if.expr.else");
        let merge_bb = self.context.append_basic_block(function, "if.expr.end");
        self.builder
            .build_conditional_branch(condition, then_bb, else_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        let (then_terminated, then_value) = self.emit_block(then_block, true)?;
        if then_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let then_value = then_value.ok_or("if expression branch has no value")?;
        let then_llvm_value = then_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let then_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression then branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(else_bb);
        let (else_terminated, else_value) = self.emit_block(else_block, true)?;
        if else_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let else_value = else_value.ok_or("if expression branch has no value")?;
        if else_value.ty != then_value.ty {
            return Err(format!(
                "if expression branches have different types: {} and {}",
                self.type_name(then_value.ty),
                self.type_name(else_value.ty)
            ));
        }
        let else_llvm_value = else_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let else_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression else branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(then_value.ty).ok_or("unit if expression")?,
                "if.value",
            )
            .map_err(|e| e.to_string())?;
        phi.add_incoming(&[(&then_llvm_value, then_end), (&else_llvm_value, else_end)]);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty: then_value.ty,
        })
    }

    pub(super) fn expect_i64(
        &self,
        value: TypedValue<'ctx>,
        context: &str,
    ) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::I64 {
            return Err(format!(
                "{context} must be i64, got {}",
                self.type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    pub(super) fn expect_bool(
        &self,
        value: TypedValue<'ctx>,
        context: &str,
    ) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::Bool {
            return Err(format!(
                "{context} must be bool, got {}",
                self.type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<Local<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub(super) fn resolve_function_name(&self, name: &str) -> Option<String> {
        if self.functions.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.functions.contains_key(&qualified).then_some(qualified)
        }
    }

    pub(super) fn create_entry_alloca(
        &self,
        ty: ValueType,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let function = self.current_function.ok_or("alloca outside function")?;
        let entry = function
            .get_first_basic_block()
            .ok_or("function has no entry block")?;
        let alloca_builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            alloca_builder.position_before(&first);
        } else {
            alloca_builder.position_at_end(entry);
        }
        alloca_builder
            .build_alloca(self.llvm_type(ty).ok_or("cannot allocate unit")?, name)
            .map_err(|e| e.to_string())
    }
}
