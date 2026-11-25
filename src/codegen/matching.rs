use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_match(
        &mut self,
        value_expr: &Expr,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let value = self.emit_expression(value_expr)?;
        if !matches!(
            value.ty,
            ValueType::Bool
                | ValueType::I64
                | ValueType::F32
                | ValueType::F64
                | ValueType::Enum(_)
                | ValueType::Struct(_)
                | ValueType::Tuple(_)
                | ValueType::Array(_)
        ) {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "match requires an enum or destructurable aggregate value, got {}",
                    self.type_name(value.ty)
                ),
            ));
        }
        let scrutinee = value.value.ok_or("match value has no LLVM value")?;
        let function = self.current_function.ok_or("match outside function")?;
        let end_bb = self.context.append_basic_block(function, "match.end");
        let fallback_bb = self.context.append_basic_block(function, "match.invalid");
        let arm_blocks: Vec<_> = arms
            .iter()
            .map(|_| self.context.append_basic_block(function, "match.arm"))
            .collect();
        let mut incoming = Vec::new();
        let mut result_type = None;
        for (index, arm) in arms.iter().enumerate() {
            let test = self.emit_pattern_test(&arm.pattern, value.ty, scrutinee, arm.span)?;
            let next_bb = if index + 1 == arms.len() {
                fallback_bb
            } else {
                self.context.append_basic_block(function, "match.next")
            };
            match test {
                Some(test) => {
                    self.builder
                        .build_conditional_branch(test, arm_blocks[index], next_bb)
                        .map_err(|error| error.to_string())?;
                }
                None => {
                    self.builder
                        .build_unconditional_branch(arm_blocks[index])
                        .map_err(|error| error.to_string())?;
                }
            }
            self.builder.position_at_end(arm_blocks[index]);
            self.scopes.push(HashMap::new());
            self.emit_pattern_bindings(&arm.pattern, value.ty, scrutinee, false, arm.span)?;
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
            let result_value = if result.ty == ValueType::Unit {
                None
            } else {
                Some(result.value.ok_or("match arm has no LLVM value")?)
            };
            let result_end = self
                .builder
                .get_insert_block()
                .ok_or("match arm has no insertion block")?;
            self.builder
                .build_unconditional_branch(end_bb)
                .map_err(|error| error.to_string())?;
            if let Some(result_value) = result_value {
                incoming.push((result_value, result_end));
            }
            if index + 1 != arms.len() {
                self.builder.position_at_end(next_bb);
            }
        }
        // Semantic analysis proved that no value can reach the fallthrough case.
        self.builder.position_at_end(fallback_bb);
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(end_bb);
        let ty = result_type.ok_or("match expression has no arms")?;
        if ty == ValueType::Unit {
            return Ok(TypedValue {
                value: None,
                ty: ValueType::Unit,
            });
        }
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

    /// Emit the tests for one arm. `None` means the pattern is irrefutable at
    /// this position; otherwise the result is an `i1` that is true on a match.
    pub(super) fn emit_pattern_test(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        value: BasicValueEnum<'ctx>,
        span: Span,
    ) -> Result<Option<IntValue<'ctx>>, Diagnostic> {
        match pattern {
            MatchPattern::Wildcard | MatchPattern::Binding(_) => Ok(None),
            MatchPattern::Bool(expected) => {
                if ty != ValueType::Bool {
                    return Err(self.pattern_type_mismatch(ty, span));
                }
                Ok(Some(
                    self.builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            value.into_int_value(),
                            self.context
                                .bool_type()
                                .const_int(u64::from(*expected), false),
                            "match.scalar.test",
                        )
                        .map_err(|error| error.to_string())?,
                ))
            }
            MatchPattern::I64(expected) => {
                if ty != ValueType::I64 {
                    return Err(self.pattern_type_mismatch(ty, span));
                }
                Ok(Some(
                    self.builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            value.into_int_value(),
                            self.i64_type().const_int(*expected as u64, false),
                            "match.scalar.test",
                        )
                        .map_err(|error| error.to_string())?,
                ))
            }
            MatchPattern::F32(expected) => {
                if ty != ValueType::F32 {
                    return Err(self.pattern_type_mismatch(ty, span));
                }
                Ok(Some(
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OEQ,
                            value.into_float_value(),
                            self.context.f32_type().const_float(f64::from(*expected)),
                            "match.scalar.test",
                        )
                        .map_err(|error| error.to_string())?,
                ))
            }
            MatchPattern::F64(expected) => {
                if ty != ValueType::F64 {
                    return Err(self.pattern_type_mismatch(ty, span));
                }
                Ok(Some(
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OEQ,
                            value.into_float_value(),
                            self.context.f64_type().const_float(*expected),
                            "match.scalar.test",
                        )
                        .map_err(|error| error.to_string())?,
                ))
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err(self.pattern_type_mismatch(ty, span));
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if patterns.len() != types.len() {
                    return Err("tuple pattern arity mismatch".into());
                }
                let tuple = value.into_struct_value();
                let mut test = None;
                for (index, (pattern, element_type)) in patterns.iter().zip(types).enumerate() {
                    let element = self
                        .builder
                        .build_extract_value(tuple, index as u32, "tuple.match.element")
                        .map_err(|error| error.to_string())?;
                    let child_test =
                        self.emit_pattern_test(pattern, element_type, element, span)?;
                    self.and_pattern_test(&mut test, child_test)?;
                }
                Ok(test)
            }
            MatchPattern::Array(patterns) => {
                let ValueType::Array(array_id) = ty else {
                    return Err(self.pattern_type_mismatch(ty, span));
                };
                let shape = self.array_shapes[array_id];
                if patterns.len() != shape.length as usize {
                    return Err("array pattern arity mismatch".into());
                }
                let element_type = ValueType::from(shape.element);
                let array = value.into_array_value();
                let mut test = None;
                for (index, pattern) in patterns.iter().enumerate() {
                    let element = self
                        .builder
                        .build_extract_value(array, index as u32, "array.match.element")
                        .map_err(|error| error.to_string())?;
                    let child_test =
                        self.emit_pattern_test(pattern, element_type, element, span)?;
                    self.and_pattern_test(&mut test, child_test)?;
                }
                Ok(test)
            }
            MatchPattern::Struct { name, fields, .. } => {
                let ValueType::Struct(struct_id) = ty else {
                    return Err(self.pattern_type_mismatch(ty, span));
                };
                if name != &self.structs[struct_id].name {
                    return Err("struct pattern name mismatch".into());
                }
                let declarations = self.structs[struct_id].fields.clone();
                let structure = value.into_struct_value();
                let mut test = None;
                for (field_name, pattern, field_span) in fields {
                    let index = declarations
                        .iter()
                        .position(|field| field.name == *field_name)
                        .ok_or_else(|| {
                            Diagnostic::at(
                                (*field_span).into(),
                                format!("struct `{name}` has no field `{field_name}`"),
                            )
                        })?;
                    let field_ty =
                        self.resolve_type(&declarations[index].ty, declarations[index].span)?;
                    let element = self
                        .builder
                        .build_extract_value(structure, index as u32, "struct.match.field")
                        .map_err(|error| error.to_string())?;
                    let child_test =
                        self.emit_pattern_test(pattern, field_ty, element, *field_span)?;
                    self.and_pattern_test(&mut test, child_test)?;
                }
                Ok(test)
            }
            MatchPattern::Variant {
                variant_name,
                bindings,
                ..
            } => {
                let ValueType::Enum(enum_id) = ty else {
                    return Err(self.pattern_type_mismatch(ty, span));
                };
                let variant_index = self.variant_index(enum_id, variant_name, span)?;
                let enumeration = value.into_struct_value();
                let tag = self
                    .builder
                    .build_extract_value(enumeration, 0, "enum.match.tag")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                let mut test = Some(
                    self.builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            tag,
                            self.context
                                .i32_type()
                                .const_int(variant_index as u64, false),
                            "enum.match.test",
                        )
                        .map_err(|error| error.to_string())?,
                );
                if let Some(patterns) = bindings {
                    let payload_types = self.enums[enum_id].variants[variant_index].payload.clone();
                    let (variant_field, _) = self.enum_payload_field(enum_id, variant_index, 0);
                    let payload_tuple = self
                        .builder
                        .build_extract_value(enumeration, variant_field, "enum.match.tuple")
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    for (payload_index, (pattern, payload_type)) in
                        patterns.iter().zip(&payload_types).enumerate()
                    {
                        let payload_ty = self.resolve_type(payload_type, span)?;
                        let (_, payload_field) =
                            self.enum_payload_field(enum_id, variant_index, payload_index);
                        let payload = self
                            .builder
                            .build_extract_value(payload_tuple, payload_field, "enum.match.payload")
                            .map_err(|error| error.to_string())?;
                        let child_test =
                            self.emit_pattern_test(pattern, payload_ty, payload, span)?;
                        self.and_pattern_test(&mut test, child_test)?;
                    }
                }
                Ok(test)
            }
            MatchPattern::ImportedVariant { .. } => {
                Err("imported enum variant pattern reached code generation".into())
            }
        }
    }

    fn and_pattern_test(
        &mut self,
        accumulator: &mut Option<IntValue<'ctx>>,
        test: Option<IntValue<'ctx>>,
    ) -> Result<(), Diagnostic> {
        let Some(test) = test else {
            return Ok(());
        };
        *accumulator = Some(match *accumulator {
            Some(previous) => self
                .builder
                .build_and(previous, test, "match.pattern.test")
                .map_err(|error| error.to_string())?,
            None => test,
        });
        Ok(())
    }

    pub(super) fn variant_index(
        &self,
        enum_id: usize,
        variant_name: &str,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        self.enums[enum_id]
            .variants
            .iter()
            .position(|variant| variant.name == *variant_name)
            .ok_or_else(|| Diagnostic::at(span.into(), format!("unknown variant `{variant_name}`")))
    }

    fn pattern_type_mismatch(&self, ty: ValueType, span: Span) -> Diagnostic {
        Diagnostic::at(
            span.into(),
            format!(
                "match pattern type does not match {} value",
                self.type_name(ty)
            ),
        )
    }
}
