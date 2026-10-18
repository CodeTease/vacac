use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_match(
        &mut self,
        value_expr: &Expr,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let value = self.emit_expression(value_expr)?;
        if matches!(
            value.ty,
            ValueType::Tuple(_) | ValueType::Array(_) | ValueType::Struct(_)
        ) {
            if arms.len() != 1 {
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "tuple, array, and struct matches require exactly one destructuring arm"
                    ),
                ));
            }
            let arm = &arms[0];
            if matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ) {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    format!("destructuring match arm must use a tuple, array, or struct pattern"),
                ));
            }
            let aggregate = value
                .value
                .ok_or("destructured match value has no LLVM value")?;
            self.scopes.push(HashMap::new());
            self.emit_pattern_bindings(&arm.pattern, value.ty, aggregate, false, arm.span)?;
            let (terminated, result) = self.emit_block(&arm.body, true)?;
            self.scopes.pop();
            if terminated {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    "destructuring match arm unexpectedly terminated",
                ));
            }
            return Ok(result.ok_or_else(|| {
                Diagnostic::at(
                    arm.span.into(),
                    "destructuring match arm must produce a value",
                )
            })?);
        }
        if matches!(
            value.ty,
            ValueType::Bool | ValueType::I64 | ValueType::F32 | ValueType::F64
        ) {
            return self.emit_scalar_match(value, arms, span);
        }
        let ValueType::Enum(enum_id) = value.ty else {
            return Err(Diagnostic::at(
                span.into(),
                format!("match requires an enum value"),
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
            if matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ) {
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
                    Diagnostic::at(arm.span.into(), format!("unknown variant `{variant_name}`"))
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
            match &arm.pattern {
                MatchPattern::Variant {
                    bindings: Some(bindings),
                    variant_name,
                    ..
                } => {
                    let variant_index = self.enums[enum_id]
                        .variants
                        .iter()
                        .position(|variant| variant.name == *variant_name)
                        .ok_or("unknown match variant")?;
                    let payload_types = self.enums[enum_id].variants[variant_index].payload.clone();
                    for (payload_index, (pattern, payload_type)) in
                        bindings.iter().zip(&payload_types).enumerate()
                    {
                        let ty = self.resolve_type(payload_type, arm.span)?;
                        let (variant_field, payload_field) =
                            self.enum_payload_field(enum_id, variant_index, payload_index);
                        let payload_tuple = self
                            .builder
                            .build_extract_value(enum_value, variant_field, "enum.match.tuple")
                            .map_err(|e| e.to_string())?
                            .into_struct_value();
                        let payload = self
                            .builder
                            .build_extract_value(payload_tuple, payload_field, "enum.match.payload")
                            .map_err(|e| e.to_string())?;
                        self.emit_pattern_bindings(pattern, ty, payload, false, arm.span)?;
                    }
                }
                MatchPattern::Binding(name) => {
                    let alloca = self.create_entry_alloca(value.ty, name)?;
                    let binding_id = self.next_binding_id();
                    self.builder
                        .build_store(alloca, value.value.ok_or("match value has no LLVM value")?)
                        .map_err(|error| error.to_string())?;
                    self.scopes.last_mut().unwrap().insert(
                        name.clone(),
                        Local {
                            pointer: alloca,
                            ty: value.ty,
                            immutable: false,
                            loop_variable: false,
                            binding_id,
                            mutable_reference_origin: matches!(
                                value.ty,
                                ValueType::MutableReference(_)
                            )
                            .then_some(binding_id),
                        },
                    );
                }
                _ => {}
            }
            let (terminated, result) = self.emit_block(&arm.body, true)?;
            self.scopes.pop();
            if terminated {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    "match arm unexpectedly terminated",
                ));
            }
            let result = result
                .ok_or_else(|| Diagnostic::at(arm.span.into(), "match arm must produce a value"))?;
            if result.ty == ValueType::Unit {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    "match arms must produce a value",
                ));
            }
            if let Some(expected) = result_type {
                if expected != result.ty {
                    return Err(Diagnostic::at(
                        arm.span.into(),
                        format!(
                            "match arms have different types: {} and {}",
                            self.type_name(expected),
                            self.type_name(result.ty)
                        ),
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

    fn emit_scalar_match(
        &mut self,
        value: TypedValue<'ctx>,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let scalar_value = value.value.ok_or("match value has no LLVM value")?;
        let is_float = matches!(value.ty, ValueType::F32 | ValueType::F64);
        let is_bool = value.ty == ValueType::Bool;
        let int_scalar = (!is_float).then(|| scalar_value.into_int_value());
        let float_scalar = is_float.then(|| scalar_value.into_float_value());
        let function = self.current_function.ok_or("match outside function")?;
        let end_bb = self.context.append_basic_block(function, "match.end");
        let arm_blocks: Vec<_> = arms
            .iter()
            .map(|_| self.context.append_basic_block(function, "match.arm"))
            .collect();
        let fallback_bb = self.context.append_basic_block(function, "match.invalid");
        let mut seen_bool = std::collections::HashSet::new();
        let mut seen_i64 = std::collections::HashSet::new();
        let mut seen_f32 = Vec::new();
        let mut seen_f64 = Vec::new();
        let mut wildcard_seen = false;
        for (index, arm) in arms.iter().enumerate() {
            let test_pattern = match &arm.pattern {
                MatchPattern::Wildcard => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("duplicate wildcard match arm"),
                        ));
                    }
                    wildcard_seen = true;
                    if is_bool && seen_bool.len() == 2 {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable wildcard match arm after all values are covered"),
                        ));
                    }
                    None
                }
                MatchPattern::Binding(_) => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable match arm after wildcard"),
                        ));
                    }
                    if is_bool && seen_bool.len() == 2 {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable wildcard match arm after all values are covered"),
                        ));
                    }
                    wildcard_seen = true;
                    None
                }
                MatchPattern::Tuple(_) | MatchPattern::Array(_) | MatchPattern::Struct { .. } => {
                    return Err(
                        "internal error: aggregate patterns are unsupported for scalar dispatch"
                            .into(),
                    );
                }
                MatchPattern::Bool(pattern) if is_bool => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable match arm after wildcard"),
                        ));
                    }
                    if !seen_bool.insert(*pattern) {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("duplicate match arm for `{pattern}`"),
                        ));
                    }
                    Some(MatchPattern::Bool(*pattern))
                }
                MatchPattern::I64(pattern) if !is_bool && !is_float => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable match arm after wildcard"),
                        ));
                    }
                    if !seen_i64.insert(*pattern) {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("duplicate match arm for `{pattern}`"),
                        ));
                    }
                    Some(MatchPattern::I64(*pattern))
                }
                MatchPattern::F32(pattern) if value.ty == ValueType::F32 => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable match arm after wildcard"),
                        ));
                    }
                    if seen_f32.iter().any(|seen| *seen == *pattern) {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("duplicate match arm for `{pattern}`"),
                        ));
                    }
                    seen_f32.push(*pattern);
                    Some(MatchPattern::F32(*pattern))
                }
                MatchPattern::F64(pattern) if value.ty == ValueType::F64 => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("unreachable match arm after wildcard"),
                        ));
                    }
                    if seen_f64.iter().any(|seen| *seen == *pattern) {
                        return Err(Diagnostic::at(
                            arm.span.into(),
                            format!("duplicate match arm for `{pattern}`"),
                        ));
                    }
                    seen_f64.push(*pattern);
                    Some(MatchPattern::F64(*pattern))
                }
                MatchPattern::Bool(_)
                | MatchPattern::I64(_)
                | MatchPattern::F32(_)
                | MatchPattern::F64(_) => {
                    return Err(Diagnostic::at(
                        arm.span.into(),
                        format!(
                            "match pattern type does not match {} value",
                            self.type_name(value.ty)
                        ),
                    ));
                }
                MatchPattern::Variant { .. } => {
                    return Err(Diagnostic::at(
                        arm.span.into(),
                        format!(
                            "enum variant patterns cannot match {} values",
                            self.type_name(value.ty)
                        ),
                    ));
                }
            };
            let next = if index + 1 == arms.len() {
                fallback_bb
            } else {
                self.context.append_basic_block(function, "match.next")
            };
            if let Some(pattern) = &test_pattern {
                let test = match pattern {
                    MatchPattern::Bool(pattern) => self.builder.build_int_compare(
                        IntPredicate::EQ,
                        int_scalar.expect("boolean match uses an integer value"),
                        self.context
                            .bool_type()
                            .const_int(u64::from(*pattern), false),
                        "match.scalar.test",
                    ),
                    MatchPattern::I64(pattern) => self.builder.build_int_compare(
                        IntPredicate::EQ,
                        int_scalar.expect("integer match uses an integer value"),
                        self.i64_type().const_int(*pattern as u64, false),
                        "match.scalar.test",
                    ),
                    MatchPattern::F32(pattern) => self.builder.build_float_compare(
                        FloatPredicate::OEQ,
                        float_scalar.expect("f32 match uses a floating value"),
                        self.context.f32_type().const_float(f64::from(*pattern)),
                        "match.scalar.test",
                    ),
                    MatchPattern::F64(pattern) => self.builder.build_float_compare(
                        FloatPredicate::OEQ,
                        float_scalar.expect("f64 match uses a floating value"),
                        self.context.f64_type().const_float(*pattern),
                        "match.scalar.test",
                    ),
                    _ => unreachable!(),
                }
                .map_err(|error| error.to_string())?;
                self.builder
                    .build_conditional_branch(test, arm_blocks[index], next)
                    .map_err(|error| error.to_string())?;
            } else {
                self.builder
                    .build_unconditional_branch(arm_blocks[index])
                    .map_err(|error| error.to_string())?;
            }
            if test_pattern.is_some() && index + 1 != arms.len() {
                self.builder.position_at_end(next);
            } else if test_pattern.is_some() {
                self.builder.position_at_end(fallback_bb);
            }
            if matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ) {
                break;
            }
        }
        if !wildcard_seen && (is_bool && seen_bool.len() == 2) {
            // Both boolean values are covered, so the final conditional's false edge
            // is impossible. It still needs a valid terminator for LLVM verification.
            self.builder.position_at_end(fallback_bb);
            self.builder
                .build_unreachable()
                .map_err(|error| error.to_string())?;
        } else if !wildcard_seen {
            return Err(Diagnostic::at(
                span.into(),
                format!("non-exhaustive match on `{}`", self.type_name(value.ty)),
            ));
        } else {
            self.builder.position_at_end(fallback_bb);
            self.builder
                .build_unreachable()
                .map_err(|error| error.to_string())?;
        }

        let mut incoming = Vec::new();
        let mut result_type = None;
        for (index, arm) in arms.iter().enumerate() {
            self.builder.position_at_end(arm_blocks[index]);
            self.scopes.push(HashMap::new());
            if let MatchPattern::Binding(name) = &arm.pattern {
                let alloca = self.create_entry_alloca(value.ty, name)?;
                let binding_id = self.next_binding_id();
                self.builder
                    .build_store(alloca, scalar_value)
                    .map_err(|error| error.to_string())?;
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        pointer: alloca,
                        ty: value.ty,
                        immutable: false,
                        loop_variable: false,
                        binding_id,
                        mutable_reference_origin: matches!(
                            value.ty,
                            ValueType::MutableReference(_)
                        )
                        .then_some(binding_id),
                    },
                );
            }
            let result = self.emit_block(&arm.body, true);
            self.scopes.pop();
            let (terminated, result) = result?;
            if terminated {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    "match arm unexpectedly terminated",
                ));
            }
            let result = result
                .ok_or_else(|| Diagnostic::at(arm.span.into(), "match arm must produce a value"))?;
            if result.ty == ValueType::Unit {
                return Err(Diagnostic::at(
                    arm.span.into(),
                    "match arms must produce a value",
                ));
            }
            if let Some(expected) = result_type {
                if expected != result.ty {
                    return Err(Diagnostic::at(
                        arm.span.into(),
                        format!(
                            "match arms have different types: {} and {}",
                            self.type_name(expected),
                            self.type_name(result.ty)
                        ),
                    ));
                }
            } else {
                result_type = Some(result.ty);
            }
            let result_end = self
                .builder
                .get_insert_block()
                .ok_or("match arm has no block")?;
            self.builder
                .build_unconditional_branch(end_bb)
                .map_err(|error| error.to_string())?;
            incoming.push((result.value.ok_or("match arm has no value")?, result_end));
        }
        self.builder.position_at_end(end_bb);
        let ty = result_type.ok_or("match expression has no arms")?;
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(ty).ok_or("unit match result")?,
                "match.value",
            )
            .map_err(|error| error.to_string())?;
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
