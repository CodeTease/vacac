use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_string_concat_builtin(&mut self) -> Result<(), String> {
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

    pub(super) fn emit_print_builtins(&mut self) -> Result<(), String> {
        let i8_ptr = self.context.ptr_type(AddressSpace::default());
        let printf_type = self.context.i32_type().fn_type(&[i8_ptr.into()], true);
        let printf = self.module.add_function("printf", printf_type, None);
        self.emit_printf_wrapper(printf, "print_i64", ValueType::I64, "%lld\n")?;
        self.emit_printf_wrapper(printf, "print", ValueType::Str, "%s")?;
        self.emit_printf_wrapper(printf, "println", ValueType::Str, "%s\n")?;
        Ok(())
    }

    pub(super) fn emit_read_i64_builtin(&mut self) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.i64_type();
        let bool_type = self.bool_type();
        let getchar = self
            .module
            .add_function("getchar", i32_type.fn_type(&[], false), None);
        let exit = self.module.add_function(
            "exit",
            self.context.void_type().fn_type(&[i32_type.into()], false),
            None,
        );
        let function = self
            .module
            .add_function("read_i64", i64_type.fn_type(&[], false), None);
        self.functions.insert("read_i64".into(), function);

        let entry = self.context.append_basic_block(function, "entry");
        let skip_test = self.context.append_basic_block(function, "input.skip.test");
        let skip = self.context.append_basic_block(function, "input.skip");
        let after_whitespace = self
            .context
            .append_basic_block(function, "input.after.whitespace");
        let sign_check = self
            .context
            .append_basic_block(function, "input.sign.check");
        let plus = self.context.append_basic_block(function, "input.plus");
        let minus = self.context.append_basic_block(function, "input.minus");
        let no_sign = self.context.append_basic_block(function, "input.no.sign");
        let parse = self.context.append_basic_block(function, "input.parse");
        let digit = self.context.append_basic_block(function, "input.digit");
        let digit_accumulate = self
            .context
            .append_basic_block(function, "input.digit.accumulate");
        let not_digit = self.context.append_basic_block(function, "input.not.digit");
        let success = self.context.append_basic_block(function, "input.success");
        let invalid = self.context.append_basic_block(function, "input.invalid");

        self.builder.position_at_end(entry);
        let character_slot = self
            .builder
            .build_alloca(i32_type, "input.character")
            .map_err(|e| e.to_string())?;
        let negative_slot = self
            .builder
            .build_alloca(bool_type, "input.negative")
            .map_err(|e| e.to_string())?;
        let accumulator_slot = self
            .builder
            .build_alloca(i64_type, "input.magnitude")
            .map_err(|e| e.to_string())?;
        let digit_seen_slot = self
            .builder
            .build_alloca(bool_type, "input.digit.seen")
            .map_err(|e| e.to_string())?;
        let first_character = self
            .builder
            .build_call(getchar, &[], "input.first")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?
            .into_int_value();
        self.builder
            .build_store(character_slot, first_character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(accumulator_slot, i64_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(digit_seen_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(skip_test)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(skip_test);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let whitespace = self.is_ascii_whitespace(character)?;
        self.builder
            .build_conditional_branch(whitespace, skip, after_whitespace)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(skip);
        let character = self
            .builder
            .build_call(getchar, &[], "input.whitespace")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(skip_test)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(after_whitespace);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let at_eof = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(u64::MAX, false),
                "input.eof",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(at_eof, invalid, sign_check)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(sign_check);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_plus = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b'+' as u64, false),
                "input.is.plus",
            )
            .map_err(|e| e.to_string())?;
        let sign_check_minus = self
            .context
            .append_basic_block(function, "input.sign.check.minus");
        self.builder
            .build_conditional_branch(is_plus, plus, sign_check_minus)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(sign_check_minus);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_minus = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b'-' as u64, false),
                "input.is.minus",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(is_minus, minus, no_sign)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(plus);
        self.builder
            .build_store(negative_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.after.plus")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(minus);
        self.builder
            .build_store(negative_slot, bool_type.const_all_ones())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.after.minus")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(no_sign);
        self.builder
            .build_store(negative_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(parse);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_digit = self.is_ascii_digit(character)?;
        self.builder
            .build_conditional_branch(is_digit, digit, not_digit)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(digit);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let digit_i32 = self
            .builder
            .build_int_sub(
                character,
                i32_type.const_int(b'0' as u64, false),
                "input.digit.i32",
            )
            .map_err(|e| e.to_string())?;
        let digit_i64 = self
            .builder
            .build_int_z_extend(digit_i32, i64_type, "input.digit")
            .map_err(|e| e.to_string())?;
        let negative = self
            .builder
            .build_load(bool_type, negative_slot, "input.negative")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negative_limit = i64_type.const_int(1u64 << 63, false);
        let positive_limit = i64_type.const_int(i64::MAX as u64, false);
        let limit = self
            .builder
            .build_select(negative, negative_limit, positive_limit, "input.limit")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let remaining = self
            .builder
            .build_int_sub(limit, digit_i64, "input.remaining")
            .map_err(|e| e.to_string())?;
        let threshold = self
            .builder
            .build_int_unsigned_div(remaining, i64_type.const_int(10, false), "input.threshold")
            .map_err(|e| e.to_string())?;
        let accumulator = self
            .builder
            .build_load(i64_type, accumulator_slot, "input.magnitude")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let overflows = self
            .builder
            .build_int_compare(IntPredicate::UGT, accumulator, threshold, "input.overflow")
            .map_err(|e| e.to_string())?;
        let overflow = self
            .context
            .append_basic_block(function, "input.overflow.error");
        self.builder
            .build_conditional_branch(overflows, overflow, digit_accumulate)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(overflow);
        self.builder
            .build_unconditional_branch(invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(digit_accumulate);
        let accumulator = self
            .builder
            .build_int_mul(accumulator, i64_type.const_int(10, false), "input.multiply")
            .map_err(|e| e.to_string())?;
        let accumulator = self
            .builder
            .build_int_add(accumulator, digit_i64, "input.add")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(accumulator_slot, accumulator)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(digit_seen_slot, bool_type.const_all_ones())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.next")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(not_digit);
        let digit_seen = self
            .builder
            .build_load(bool_type, digit_seen_slot, "input.digit.seen")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let delimiter_check = self
            .context
            .append_basic_block(function, "input.delimiter.check");
        self.builder
            .build_conditional_branch(digit_seen, delimiter_check, invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(delimiter_check);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let at_eof = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(u64::MAX, false),
                "input.eof",
            )
            .map_err(|e| e.to_string())?;
        let whitespace = self.is_ascii_whitespace(character)?;
        let valid_delimiter = self
            .builder
            .build_or(at_eof, whitespace, "input.valid.delimiter")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(valid_delimiter, success, invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(success);
        let accumulator = self
            .builder
            .build_load(i64_type, accumulator_slot, "input.magnitude")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negative = self
            .builder
            .build_load(bool_type, negative_slot, "input.negative")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negated = self
            .builder
            .build_int_neg(accumulator, "input.negated")
            .map_err(|e| e.to_string())?;
        let result = self
            .builder
            .build_select(negative, negated, accumulator, "input.result")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&result))
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(invalid);
        self.builder
            .build_call(exit, &[i32_type.const_int(1, false).into()], "input.exit")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub(super) fn is_ascii_whitespace(
        &self,
        character: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let low = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                character,
                i32_type.const_int(9, false),
                "input.whitespace.low",
            )
            .map_err(|e| e.to_string())?;
        let high = self
            .builder
            .build_int_compare(
                IntPredicate::SLE,
                character,
                i32_type.const_int(13, false),
                "input.whitespace.high",
            )
            .map_err(|e| e.to_string())?;
        let control_whitespace = self
            .builder
            .build_and(low, high, "input.whitespace.control")
            .map_err(|e| e.to_string())?;
        let space = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b' ' as u64, false),
                "input.whitespace.space",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_or(control_whitespace, space, "input.whitespace")
            .map_err(|e| e.to_string())
    }

    pub(super) fn is_ascii_digit(
        &self,
        character: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let low = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                character,
                i32_type.const_int(b'0' as u64, false),
                "input.digit.low",
            )
            .map_err(|e| e.to_string())?;
        let high = self
            .builder
            .build_int_compare(
                IntPredicate::SLE,
                character,
                i32_type.const_int(b'9' as u64, false),
                "input.digit.high",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_and(low, high, "input.is.digit")
            .map_err(|e| e.to_string())
    }

    // Defines `name(value) -> unit` as `printf(format, value)`.

    pub(super) fn emit_printf_wrapper(
        &mut self,
        printf: FunctionValue<'ctx>,
        name: &str,
        param: ValueType,
        format: &str,
    ) -> Result<(), String> {
        let function_type = self.llvm_fn_type(&[param], ValueType::Unit)?;
        let function = self.module.add_function(name, function_type, None);
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        let fmt = self
            .builder
            .build_global_string_ptr(format, &format!("apex_{name}_fmt"))
            .map_err(|e| e.to_string())?;
        let value = function
            .get_nth_param(0)
            .ok_or_else(|| format!("missing {name} parameter"))?;
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

    pub(super) fn strcmp_function(&self) -> FunctionValue<'ctx> {
        self.module.get_function("strcmp").unwrap_or_else(|| {
            let ptr = self.context.ptr_type(AddressSpace::default());
            let strcmp_type = self
                .context
                .i32_type()
                .fn_type(&[ptr.into(), ptr.into()], false);
            self.module.add_function("strcmp", strcmp_type, None)
        })
    }

    pub(super) fn string_literal(&mut self, value: &str) -> Result<PointerValue<'ctx>, String> {
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

    pub(super) fn emit_c_main_wrapper(&mut self) -> Result<(), String> {
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
