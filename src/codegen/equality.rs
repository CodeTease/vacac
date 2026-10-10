use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_value_equality(
        &mut self,
        ty: ValueType,
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        match ty {
            ValueType::I64 | ValueType::Bool => self
                .builder
                .build_int_compare(
                    IntPredicate::EQ,
                    left.into_int_value(),
                    right.into_int_value(),
                    "value.eq",
                )
                .map_err(|e| e.to_string()),
            ValueType::Str => {
                let result = self.emit_string_equality(
                    BinaryOp::Equal,
                    TypedValue {
                        value: Some(left),
                        ty: ValueType::Str,
                    },
                    TypedValue {
                        value: Some(right),
                        ty: ValueType::Str,
                    },
                )?;
                Ok(result
                    .value
                    .ok_or("string comparison has no value")?
                    .into_int_value())
            }
            ValueType::Array(array_id) => {
                let length = self.array_shapes[array_id].length;
                let mut equal = self.bool_type().const_int(1, false);
                for index in 0..length {
                    let left_element = self
                        .builder
                        .build_extract_value(left.into_array_value(), index, "array.eq.left")
                        .map_err(|e| e.to_string())?;
                    let right_element = self
                        .builder
                        .build_extract_value(right.into_array_value(), index, "array.eq.right")
                        .map_err(|e| e.to_string())?;
                    let element_equal = self.emit_value_equality(
                        ValueType::from(self.array_shapes[array_id].element),
                        left_element,
                        right_element,
                    )?;
                    equal = self
                        .builder
                        .build_and(equal, element_equal, "array.eq")
                        .map_err(|e| e.to_string())?;
                }
                Ok(equal)
            }
            ValueType::Struct(struct_id) => {
                let fields = self.structs[struct_id].fields.clone();
                let mut equal = self.bool_type().const_int(1, false);
                for (index, field) in fields.iter().enumerate() {
                    let field_type = self.resolve_type(&field.ty, field.span)?;
                    let left_field = self
                        .builder
                        .build_extract_value(
                            left.into_struct_value(),
                            index as u32,
                            "struct.eq.left",
                        )
                        .map_err(|e| e.to_string())?;
                    let right_field = self
                        .builder
                        .build_extract_value(
                            right.into_struct_value(),
                            index as u32,
                            "struct.eq.right",
                        )
                        .map_err(|e| e.to_string())?;
                    let field_equal =
                        self.emit_value_equality(field_type, left_field, right_field)?;
                    equal = self
                        .builder
                        .build_and(equal, field_equal, "struct.eq")
                        .map_err(|e| e.to_string())?;
                }
                Ok(equal)
            }
            ValueType::Enum(enum_id) => self.emit_enum_equality(enum_id, left, right),
            ValueType::Unit => Err("void expressions cannot be compared".into()),
        }
    }

    fn emit_enum_equality(
        &mut self,
        enum_id: usize,
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let left_tag = self
            .builder
            .build_extract_value(left.into_struct_value(), 0, "enum.eq.left.tag")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let right_tag = self
            .builder
            .build_extract_value(right.into_struct_value(), 0, "enum.eq.right.tag")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let same_tag = self
            .builder
            .build_int_compare(IntPredicate::EQ, left_tag, right_tag, "enum.eq.tag")
            .map_err(|e| e.to_string())?;

        let function = self
            .current_function
            .ok_or("enum comparison outside function")?;
        let dispatch = self
            .context
            .append_basic_block(function, "enum.eq.dispatch");
        let different = self
            .context
            .append_basic_block(function, "enum.eq.different");
        let invalid_tag = self.context.append_basic_block(function, "enum.eq.invalid");
        let end = self.context.append_basic_block(function, "enum.eq.end");
        self.builder
            .build_conditional_branch(same_tag, dispatch, different)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(different);
        self.builder
            .build_unconditional_branch(end)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(invalid_tag);
        self.builder
            .build_unconditional_branch(end)
            .map_err(|e| e.to_string())?;

        let declaration = self.enums[enum_id].clone();
        let mut variant_blocks = Vec::with_capacity(declaration.variants.len());
        let mut cases = Vec::with_capacity(declaration.variants.len());
        for variant_index in 0..declaration.variants.len() {
            let block = self.context.append_basic_block(function, "enum.eq.variant");
            variant_blocks.push(block);
            cases.push((
                self.context
                    .i32_type()
                    .const_int(variant_index as u64, false),
                block,
            ));
        }

        self.builder.position_at_end(dispatch);
        self.builder
            .build_switch(left_tag, invalid_tag, &cases)
            .map_err(|e| e.to_string())?;

        let mut incoming = vec![
            (self.bool_type().const_zero(), different),
            (self.bool_type().const_zero(), invalid_tag),
        ];
        for (variant_index, (variant, block)) in
            declaration.variants.iter().zip(variant_blocks).enumerate()
        {
            self.builder.position_at_end(block);
            let mut payload_equal = self.bool_type().const_int(1, false);
            for (payload_index, payload_type) in variant.payload.iter().enumerate() {
                let (variant_field, payload_field) =
                    self.enum_payload_field(enum_id, variant_index, payload_index);
                let left_tuple = self
                    .builder
                    .build_extract_value(
                        left.into_struct_value(),
                        variant_field,
                        "enum.eq.left.tuple",
                    )
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                let right_tuple = self
                    .builder
                    .build_extract_value(
                        right.into_struct_value(),
                        variant_field,
                        "enum.eq.right.tuple",
                    )
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                let left_payload = self
                    .builder
                    .build_extract_value(left_tuple, payload_field, "enum.eq.left.payload")
                    .map_err(|e| e.to_string())?;
                let right_payload = self
                    .builder
                    .build_extract_value(right_tuple, payload_field, "enum.eq.right.payload")
                    .map_err(|e| e.to_string())?;
                let payload_type = self.resolve_type(payload_type, variant.span)?;
                let field_equal =
                    self.emit_value_equality(payload_type, left_payload, right_payload)?;
                payload_equal = self
                    .builder
                    .build_and(payload_equal, field_equal, "enum.eq.payload")
                    .map_err(|e| e.to_string())?;
            }
            let predecessor = self
                .builder
                .get_insert_block()
                .ok_or("enum comparison has no insertion block")?;
            self.builder
                .build_unconditional_branch(end)
                .map_err(|e| e.to_string())?;
            incoming.push((payload_equal, predecessor));
        }

        self.builder.position_at_end(end);
        let phi = self
            .builder
            .build_phi(self.bool_type(), "enum.eq.result")
            .map_err(|e| e.to_string())?;
        let incoming_refs: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&incoming_refs);
        Ok(phi.as_basic_value().into_int_value())
    }
}
