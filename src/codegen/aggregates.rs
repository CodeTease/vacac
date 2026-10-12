use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_pattern_bindings(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        value: BasicValueEnum<'ctx>,
        immutable: bool,
        span: Span,
    ) -> Result<(), String> {
        match pattern {
            MatchPattern::Wildcard => Ok(()),
            MatchPattern::Binding(name) => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(format!(
                        "line {}:{}: `{name}` is already declared in this scope",
                        span.line, span.column
                    ));
                }
                let alloca = self.create_entry_alloca(ty, name)?;
                self.builder
                    .build_store(alloca, value)
                    .map_err(|error| error.to_string())?;
                self.scopes
                    .last_mut()
                    .ok_or("pattern binding outside scope")?
                    .insert(
                        name.clone(),
                        Local {
                            pointer: alloca,
                            ty,
                            immutable,
                            loop_variable: false,
                        },
                    );
                Ok(())
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err("tuple pattern type mismatch".into());
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if patterns.len() != types.len() {
                    return Err("tuple pattern arity mismatch".into());
                }
                for (index, (pattern, element_type)) in patterns.iter().zip(types).enumerate() {
                    let element = self
                        .builder
                        .build_extract_value(
                            value.into_struct_value(),
                            index as u32,
                            "tuple.pattern.element",
                        )
                        .map_err(|error| error.to_string())?;
                    self.emit_pattern_bindings(pattern, element_type, element, immutable, span)?;
                }
                Ok(())
            }
            MatchPattern::Array(patterns) => {
                let ValueType::Array(array_id) = ty else {
                    return Err("array pattern type mismatch".into());
                };
                let shape = self.array_shapes[array_id];
                if patterns.len() != shape.length as usize {
                    return Err("array pattern arity mismatch".into());
                }
                let element_type = ValueType::from(shape.element);
                for (index, pattern) in patterns.iter().enumerate() {
                    let element = self
                        .builder
                        .build_extract_value(
                            value.into_array_value(),
                            index as u32,
                            "array.pattern.element",
                        )
                        .map_err(|error| error.to_string())?;
                    self.emit_pattern_bindings(pattern, element_type, element, immutable, span)?;
                }
                Ok(())
            }
            MatchPattern::Struct { name, fields, .. } => {
                let ValueType::Struct(struct_id) = ty else {
                    return Err("struct pattern type mismatch".into());
                };
                if name != &self.structs[struct_id].name {
                    return Err("struct pattern name mismatch".into());
                }
                if self
                    .struct_visibility
                    .get(name)
                    .is_some_and(|(owner, public)| owner != &self.module_context && !public)
                {
                    return Err(format!(
                        "line {}:{}: struct `{name}` is private",
                        span.line, span.column
                    ));
                }
                let declarations = self.structs[struct_id].fields.clone();
                for (field_name, pattern, field_span) in fields {
                    let index = declarations
                        .iter()
                        .position(|field| field.name == *field_name)
                        .ok_or_else(|| format!("struct `{name}` has no field `{field_name}`"))?;
                    let field_ty =
                        self.resolve_type(&declarations[index].ty, declarations[index].span)?;
                    let element = self
                        .builder
                        .build_extract_value(
                            value.into_struct_value(),
                            index as u32,
                            "struct.pattern.field",
                        )
                        .map_err(|error| error.to_string())?;
                    self.emit_pattern_bindings(pattern, field_ty, element, immutable, *field_span)?;
                }
                Ok(())
            }
            MatchPattern::Variant { .. } => {
                Err("nested enum variant patterns are not supported".into())
            }
            MatchPattern::Bool(_)
            | MatchPattern::I64(_)
            | MatchPattern::F32(_)
            | MatchPattern::F64(_) => Err("literal patterns are unsupported here".into()),
        }
    }

    pub(super) fn enum_payload_field(
        &self,
        _enum_id: usize,
        variant_id: usize,
        payload_index: usize,
    ) -> (u32, u32) {
        ((variant_id + 1) as u32, payload_index as u32)
    }

    pub(super) fn emit_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payload_expr: Option<&[Expr]>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let enum_id = *self.enum_ids.get(enum_name).ok_or_else(|| {
            format!(
                "line {}:{}: unknown enum `{enum_name}`",
                span.line, span.column
            )
        })?;
        if self
            .enum_visibility
            .get(enum_name)
            .is_some_and(|(owner, public)| owner != &self.module_context && !public)
        {
            return Err(format!(
                "line {}:{}: enum `{enum_name}` is private",
                span.line, span.column
            ));
        }
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
        let payload_values = match (&variant.payload[..], payload_expr) {
            ([], None) => Vec::new(),
            (expected, Some(expressions)) if expected.len() == expressions.len() => {
                let mut values = Vec::with_capacity(expected.len());
                for (payload_type, expression) in expected.iter().zip(expressions) {
                    let expected = self.resolve_type(payload_type, variant.span)?;
                    let payload = self.emit_expression_expected(expression, expected)?;
                    if payload.ty != expected {
                        return Err(format!(
                            "enum payload has type {}, expected {}",
                            self.type_name(payload.ty),
                            self.type_name(expected)
                        ));
                    }
                    values.push((
                        payload.ty,
                        payload.value.ok_or("enum payload has no value")?,
                    ));
                }
                values
            }
            (expected, Some(expressions)) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` expects {} payload value(s), got {}",
                    expected.len(),
                    expressions.len()
                ));
            }
            (_, None) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` requires a payload"
                ));
            }
        };
        let mut payload_value = self.enum_payload_types[enum_id][variant_index]
            .const_zero()
            .as_basic_value_enum();
        for (payload_index, (_ty, payload)) in payload_values.into_iter().enumerate() {
            payload_value = self
                .builder
                .build_insert_value(
                    payload_value.into_struct_value(),
                    payload,
                    payload_index as u32,
                    "enum.payload.field",
                )
                .map_err(|e| e.to_string())?
                .as_basic_value_enum();
        }
        let enum_type = self.enum_types[enum_id];
        let mut value = self
            .builder
            .build_insert_value(
                enum_type.const_zero(),
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
                payload_value,
                (variant_index + 1) as u32,
                "enum.payload",
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
        if self
            .struct_visibility
            .get(name)
            .is_some_and(|(owner, public)| owner != &self.module_context && !public)
        {
            return Err(format!(
                "line {}:{}: struct `{name}` is private",
                span.line, span.column
            ));
        }
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
            let expected = self.resolve_type(
                &declaration.fields[index].ty,
                declaration.fields[index].span,
            )?;
            let value = self.emit_expression_expected(expression, expected)?;
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
        if matches!(
            value.ty,
            ValueType::Tuple(_) | ValueType::Array(_) | ValueType::Struct(_)
        ) {
            if arms.len() != 1 {
                return Err(format!(
                    "line {}:{}: tuple, array, and struct matches require exactly one destructuring arm",
                    span.line, span.column
                ));
            }
            let arm = &arms[0];
            if matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ) {
                return Err(format!(
                    "line {}:{}: destructuring match arm must use a tuple, array, or struct pattern",
                    arm.span.line, arm.span.column
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
                return Err("destructuring match arm unexpectedly terminated".into());
            }
            return result.ok_or_else(|| "destructuring match arm must produce a value".to_owned());
        }
        if matches!(
            value.ty,
            ValueType::Bool | ValueType::I64 | ValueType::F32 | ValueType::F64
        ) {
            return self.emit_scalar_match(value, arms, span);
        }
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
                        },
                    );
                }
                _ => {}
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

    fn emit_scalar_match(
        &mut self,
        value: TypedValue<'ctx>,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
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
                        return Err(format!(
                            "line {}:{}: duplicate wildcard match arm",
                            arm.span.line, arm.span.column
                        ));
                    }
                    wildcard_seen = true;
                    if is_bool && seen_bool.len() == 2 {
                        return Err(format!(
                            "line {}:{}: unreachable wildcard match arm after all values are covered",
                            arm.span.line, arm.span.column
                        ));
                    }
                    None
                }
                MatchPattern::Binding(_) => {
                    if wildcard_seen {
                        return Err(format!(
                            "line {}:{}: unreachable match arm after wildcard",
                            arm.span.line, arm.span.column
                        ));
                    }
                    if is_bool && seen_bool.len() == 2 {
                        return Err(format!(
                            "line {}:{}: unreachable wildcard match arm after all values are covered",
                            arm.span.line, arm.span.column
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
                        return Err(format!(
                            "line {}:{}: unreachable match arm after wildcard",
                            arm.span.line, arm.span.column
                        ));
                    }
                    if !seen_bool.insert(*pattern) {
                        return Err(format!(
                            "line {}:{}: duplicate match arm for `{pattern}`",
                            arm.span.line, arm.span.column
                        ));
                    }
                    Some(MatchPattern::Bool(*pattern))
                }
                MatchPattern::I64(pattern) if !is_bool && !is_float => {
                    if wildcard_seen {
                        return Err(format!(
                            "line {}:{}: unreachable match arm after wildcard",
                            arm.span.line, arm.span.column
                        ));
                    }
                    if !seen_i64.insert(*pattern) {
                        return Err(format!(
                            "line {}:{}: duplicate match arm for `{pattern}`",
                            arm.span.line, arm.span.column
                        ));
                    }
                    Some(MatchPattern::I64(*pattern))
                }
                MatchPattern::F32(pattern) if value.ty == ValueType::F32 => {
                    if wildcard_seen {
                        return Err(format!(
                            "line {}:{}: unreachable match arm after wildcard",
                            arm.span.line, arm.span.column
                        ));
                    }
                    if seen_f32.iter().any(|seen| *seen == *pattern) {
                        return Err(format!(
                            "line {}:{}: duplicate match arm for `{pattern}`",
                            arm.span.line, arm.span.column
                        ));
                    }
                    seen_f32.push(*pattern);
                    Some(MatchPattern::F32(*pattern))
                }
                MatchPattern::F64(pattern) if value.ty == ValueType::F64 => {
                    if wildcard_seen {
                        return Err(format!(
                            "line {}:{}: unreachable match arm after wildcard",
                            arm.span.line, arm.span.column
                        ));
                    }
                    if seen_f64.iter().any(|seen| *seen == *pattern) {
                        return Err(format!(
                            "line {}:{}: duplicate match arm for `{pattern}`",
                            arm.span.line, arm.span.column
                        ));
                    }
                    seen_f64.push(*pattern);
                    Some(MatchPattern::F64(*pattern))
                }
                MatchPattern::Bool(_)
                | MatchPattern::I64(_)
                | MatchPattern::F32(_)
                | MatchPattern::F64(_) => {
                    return Err(format!(
                        "line {}:{}: match pattern type does not match {} value",
                        arm.span.line,
                        arm.span.column,
                        self.type_name(value.ty)
                    ));
                }
                MatchPattern::Variant { .. } => {
                    return Err(format!(
                        "line {}:{}: enum variant patterns cannot match {} values",
                        arm.span.line,
                        arm.span.column,
                        self.type_name(value.ty)
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
            return Err(format!(
                "line {}:{}: non-exhaustive match on `{}`",
                span.line,
                span.column,
                self.type_name(value.ty)
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
                    },
                );
            }
            let result = self.emit_block(&arm.body, true);
            self.scopes.pop();
            let (terminated, result) = result?;
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
