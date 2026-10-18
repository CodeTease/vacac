use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_pattern_bindings(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        value: BasicValueEnum<'ctx>,
        immutable: bool,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Wildcard => Ok(()),
            MatchPattern::Binding(name) => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                let alloca = self.create_entry_alloca(ty, name)?;
                self.builder
                    .build_store(alloca, value)
                    .map_err(|error| error.to_string())?;
                let binding_id = self.next_binding_id();
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
                            binding_id,
                            mutable_reference_origin: None,
                        },
                    );
                Ok(())
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err(Diagnostic::at(span.into(), "tuple pattern type mismatch"));
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
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("struct `{name}` is private"),
                    ));
                }
                let declarations = self.structs[struct_id].fields.clone();
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
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let enum_id = *self
            .enum_ids
            .get(enum_name)
            .ok_or_else(|| Diagnostic::at(span.into(), format!("unknown enum `{enum_name}`")))?;
        if self
            .enum_visibility
            .get(enum_name)
            .is_some_and(|(owner, public)| owner != &self.module_context && !public)
        {
            return Err(Diagnostic::at(
                span.into(),
                format!("enum `{enum_name}` is private"),
            ));
        }
        let variant_index = self.enums[enum_id]
            .variants
            .iter()
            .position(|variant| variant.name == variant_name)
            .ok_or_else(|| {
                Diagnostic::at(
                    span.into(),
                    format!("enum `{enum_name}` has no variant `{variant_name}`"),
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
                        return Err(Diagnostic::at(
                            codegen_span_of_expr(expression).into(),
                            format!(
                                "enum payload has type {}, expected {}",
                                self.type_name(payload.ty),
                                self.type_name(expected)
                            ),
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
                return Err(Diagnostic::at(
                    span.into(),
                    format!(
                        "variant `{enum_name}::{variant_name}` expects {} payload value(s), got {}",
                        expected.len(),
                        expressions.len()
                    ),
                ));
            }
            (_, None) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("variant `{enum_name}::{variant_name}` requires a payload"),
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
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
        let struct_id = *self
            .struct_ids
            .get(name)
            .ok_or_else(|| Diagnostic::at(span.into(), format!("unknown struct `{name}`")))?;
        if self
            .struct_visibility
            .get(name)
            .is_some_and(|(owner, public)| owner != &self.module_context && !public)
        {
            return Err(Diagnostic::at(
                span.into(),
                format!("struct `{name}` is private"),
            ));
        }
        let declaration = self.structs[struct_id].clone();
        let struct_type = self.struct_types[struct_id];
        let mut aggregate = struct_type.const_zero().as_basic_value_enum();
        let mut initialized = std::collections::HashSet::new();
        for (field_name, expression, field_span) in fields {
            if !initialized.insert(field_name.as_str()) {
                return Err(Diagnostic::at(
                    field_span.into(),
                    format!("struct field `{field_name}` is initialized more than once"),
                ));
            }
            let Some(index) = declaration
                .fields
                .iter()
                .position(|field| field.name == *field_name)
            else {
                return Err(Diagnostic::at(
                    field_span.into(),
                    format!("struct `{name}` has no field `{field_name}`"),
                ));
            };
            let expected = self.resolve_type(
                &declaration.fields[index].ty,
                declaration.fields[index].span,
            )?;
            let value = self.emit_expression_expected(expression, expected)?;
            if value.ty != expected {
                return Err(Diagnostic::at(
                    field_span.into(),
                    format!(
                        "initializer for field `{field_name}` has type {}, expected {}",
                        self.type_name(value.ty),
                        self.type_name(expected)
                    ),
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
            return Err(Diagnostic::at(
                span.into(),
                format!("struct `{name}` is missing field(s): {missing}"),
            ));
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: ValueType::Struct(struct_id),
        })
    }
}
