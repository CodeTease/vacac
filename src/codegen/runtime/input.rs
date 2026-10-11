use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_read_i64_builtin(&mut self) -> Result<(), String> {
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
}
