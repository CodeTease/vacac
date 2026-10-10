use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payload_expr: Option<&Expr>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let enum_id = *self.enum_ids.get(enum_name).ok_or_else(|| {
            format!(
                "line {}:{}: unknown enum `{enum_name}`",
                span.line, span.column
            )
        })?;
        let variant_index = self.enums[enum_id]
            .variants
            .iter()
            .position(|variant| variant.name == variant_name)
            .ok_or_else(|| {
                format!(
                    "line {}:{}: enum `{enum_name}` has no variant `{variant_name}`",
                    span.line, span.column
                )
            })?;
        let variant = self.enums[enum_id].variants[variant_index].clone();
        let (integer_payload, string_payload) = match (&variant.payload, payload_expr) {
            (None, None) => (
                self.i64_type().const_zero(),
                self.context.ptr_type(AddressSpace::default()).const_null(),
            ),
            (Some(payload_type), Some(expression)) => {
                let payload = self.emit_expression(expression)?;
                let expected = self.resolve_type(payload_type, variant.span)?;
                if payload.ty != expected {
                    return Err(format!(
                        "enum payload has type {}, expected {}",
                        self.type_name(payload.ty),
                        self.type_name(expected)
                    ));
                }
                match payload.ty {
                    ValueType::I64 => (
                        payload
                            .value
                            .ok_or("enum payload has no value")?
                            .into_int_value(),
                        self.context.ptr_type(AddressSpace::default()).const_null(),
                    ),
                    ValueType::Bool => {
                        let value = self
                            .builder
                            .build_int_z_extend(
                                payload
                                    .value
                                    .ok_or("enum payload has no value")?
                                    .into_int_value(),
                                self.i64_type(),
                                "enum.bool.payload",
                            )
                            .map_err(|e| e.to_string())?;
                        (
                            value,
                            self.context.ptr_type(AddressSpace::default()).const_null(),
                        )
                    }
                    ValueType::Str => (
                        self.i64_type().const_zero(),
                        payload
                            .value
                            .ok_or("enum payload has no value")?
                            .into_pointer_value(),
                    ),
                    _ => return Err("enum payloads must have type `i64`, `bool`, or `str`".into()),
                }
            }
            (Some(_), None) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` requires a payload"
                ));
            }
            (None, Some(_)) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` does not take a payload"
                ));
            }
        };
        let enum_type = self.enum_types[enum_id];
        let mut value = enum_type.const_zero().as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                self.context
                    .i32_type()
                    .const_int(variant_index as u64, false),
                0,
                "enum.tag",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                integer_payload,
                1,
                "enum.integer.payload",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                string_payload,
                2,
                "enum.string.payload",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        Ok(TypedValue {
            value: Some(value),
            ty: ValueType::Enum(enum_id),
        })
    }

    pub(super) fn emit_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, Expr, Span)],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let struct_id = *self.struct_ids.get(name).ok_or_else(|| {
            format!(
                "line {}:{}: unknown struct `{name}`",
                span.line, span.column
            )
        })?;
        let declaration = self.structs[struct_id].clone();
        let struct_type = self.struct_types[struct_id];
        let mut aggregate = struct_type.const_zero().as_basic_value_enum();
        let mut initialized = std::collections::HashSet::new();
        for (field_name, expression, field_span) in fields {
            if !initialized.insert(field_name.as_str()) {
                return Err(format!(
                    "line {}:{}: struct field `{field_name}` is initialized more than once",
                    field_span.line, field_span.column
                ));
            }
            let Some(index) = declaration
                .fields
                .iter()
                .position(|field| field.name == *field_name)
            else {
                return Err(format!(
                    "line {}:{}: struct `{name}` has no field `{field_name}`",
                    field_span.line, field_span.column
                ));
            };
            let value = self.emit_expression(expression)?;
            let expected = self.resolve_type(
                &declaration.fields[index].ty,
                declaration.fields[index].span,
            )?;
            if value.ty != expected {
                return Err(format!(
                    "line {}:{}: initializer for field `{field_name}` has type {}, expected {}",
                    field_span.line,
                    field_span.column,
                    self.type_name(value.ty),
                    self.type_name(expected)
                ));
            }
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_struct_value(),
                    value.value.ok_or("struct field has no value")?,
                    index as u32,
                    "struct.field.init",
                )
                .map_err(|e| e.to_string())?
                .as_basic_value_enum();
        }
        if fields.len() != declaration.fields.len() {
            let missing = declaration
                .fields
                .iter()
                .filter(|field| !initialized.contains(field.name.as_str()))
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "line {}:{}: struct `{name}` is missing field(s): {missing}",
                span.line, span.column
            ));
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: ValueType::Struct(struct_id),
        })
    }

    pub(super) fn emit_match(
        &mut self,
        value_expr: &Expr,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let value = self.emit_expression(value_expr)?;
        let ValueType::Enum(enum_id) = value.ty else {
            return Err(format!(
                "line {}:{}: match requires an enum value",
                span.line, span.column
            ));
        };
        let enum_value = value
            .value
            .ok_or("match value has no LLVM value")?
            .into_struct_value();
        let tag = self
            .builder
            .build_extract_value(enum_value, 0, "enum.match.tag")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let function = self.current_function.ok_or("match outside function")?;
        let end_bb = self.context.append_basic_block(function, "match.end");
        let arm_blocks: Vec<_> = arms
            .iter()
            .map(|_| self.context.append_basic_block(function, "match.arm"))
            .collect();
        let fallback_bb = self.context.append_basic_block(function, "match.invalid");
        for (index, arm) in arms.iter().enumerate() {
            if matches!(arm.pattern, MatchPattern::Wildcard) {
                self.builder
                    .build_unconditional_branch(arm_blocks[index])
                    .map_err(|e| e.to_string())?;
                break;
            }
            let MatchPattern::Variant { variant_name, .. } = &arm.pattern else {
                return Err("internal error: invalid match pattern".into());
            };
            let variant_index = self.enums[enum_id]
                .variants
                .iter()
                .position(|variant| variant.name == *variant_name)
                .ok_or_else(|| {
                    format!(
                        "line {}:{}: unknown variant `{variant_name}`",
                        arm.span.line, arm.span.column
                    )
                })?;
            let test = self
                .builder
                .build_int_compare(
                    IntPredicate::EQ,
                    tag,
                    self.context
                        .i32_type()
                        .const_int(variant_index as u64, false),
                    "enum.match.test",
                )
                .map_err(|e| e.to_string())?;
            let next = if index + 1 == arms.len() {
                fallback_bb
            } else {
                self.context.append_basic_block(function, "match.next")
            };
            self.builder
                .build_conditional_branch(test, arm_blocks[index], next)
                .map_err(|e| e.to_string())?;
            if index + 1 != arms.len() {
                self.builder.position_at_end(next);
            }
        }
        self.builder.position_at_end(fallback_bb);
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        let mut incoming = Vec::new();
        let mut result_type = None;
        for (index, arm) in arms.iter().enumerate() {
            self.builder.position_at_end(arm_blocks[index]);
            self.scopes.push(HashMap::new());
            let binding = match &arm.pattern {
                MatchPattern::Variant {
                    binding: Some(binding),
                    ..
                } => Some(binding),
                _ => None,
            };
            if let Some(binding) = binding {
                let MatchPattern::Variant { variant_name, .. } = &arm.pattern else {
                    return Err("internal error: wildcard match arm cannot bind a payload".into());
                };
                let variant = self.enums[enum_id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                    .ok_or("unknown match variant")?;
                let payload_type = variant
                    .payload
                    .as_ref()
                    .ok_or("payload binding on unit variant")?;
                let ty = self.resolve_type(payload_type, arm.span)?;
                let field = if ty == ValueType::Str { 2 } else { 1 };
                let mut payload = self
                    .builder
                    .build_extract_value(enum_value, field, "enum.match.payload")
                    .map_err(|e| e.to_string())?;
                if ty == ValueType::Bool {
                    payload = self
                        .builder
                        .build_int_truncate(
                            payload.into_int_value(),
                            self.bool_type(),
                            "enum.match.bool",
                        )
                        .map_err(|e| e.to_string())?
                        .into();
                }
                let alloca = self.create_entry_alloca(ty, binding)?;
                self.builder
                    .build_store(alloca, payload)
                    .map_err(|e| e.to_string())?;
                self.scopes.last_mut().unwrap().insert(
                    binding.clone(),
                    Local {
                        pointer: alloca,
                        ty,
                        immutable: false,
                        loop_variable: false,
                    },
                );
            }
            let (terminated, result) = self.emit_block(&arm.body, true)?;
            self.scopes.pop();
            if terminated {
                return Err("match arm unexpectedly terminated".into());
            }
            let result = result.ok_or("match arm must produce a value")?;
            if result.ty == ValueType::Unit {
                return Err("match arms must produce a value".into());
            }
            if let Some(expected) = result_type {
                if expected != result.ty {
                    return Err(format!(
                        "match arms have different types: {} and {}",
                        self.type_name(expected),
                        self.type_name(result.ty)
                    ));
                }
            } else {
                result_type = Some(result.ty);
            }
            let result_value = result.value.ok_or("match arm has no LLVM value")?;
            let result_end = self
                .builder
                .get_insert_block()
                .ok_or("match arm has no insertion block")?;
            self.builder
                .build_unconditional_branch(end_bb)
                .map_err(|e| e.to_string())?;
            incoming.push((result_value, result_end));
        }
        self.builder.position_at_end(end_bb);
        let ty = result_type.ok_or("match expression has no arms")?;
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(ty).ok_or("unit match result")?,
                "match.value",
            )
            .map_err(|e| e.to_string())?;
        let incoming_refs: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&incoming_refs);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty,
        })
    }
}
