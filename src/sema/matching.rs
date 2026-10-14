use super::*;

impl Checker {
    pub(super) fn check_match_expr(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let value_type = self.check_expr(value, false)?;
        if matches!(
            value_type,
            ValueType::Bool | ValueType::I64 | ValueType::F32 | ValueType::F64
        ) {
            let result = self.check_scalar_match(value_type, arms, span)?;
            if matches!(
                result,
                ValueType::Reference(_) | ValueType::MutableReference(_)
            ) {
                return Err(at(
                    span,
                    "references cannot escape through match expressions",
                ));
            }
            return Ok(result);
        }
        if matches!(
            value_type,
            ValueType::Tuple(_) | ValueType::Array(_) | ValueType::Struct(_)
        ) {
            if arms.len() != 1 {
                return Err(at(
                    span,
                    "tuple, array, and struct matches require exactly one destructuring arm",
                ));
            }
            let arm = &arms[0];
            if matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ) {
                return Err(at(
                    arm.span,
                    "destructuring match arm must use a tuple, array, or struct pattern",
                ));
            }
            let mut bindings = Vec::new();
            let mut seen = std::collections::HashSet::new();
            self.check_pattern(&arm.pattern, value_type, arm.span, &mut bindings, &mut seen)?;
            let mut scope = bindings.into_iter().collect::<HashMap<_, _>>();
            self.scopes.push(std::mem::take(&mut scope));
            self.immutable_scopes.push(std::collections::HashSet::new());
            self.loop_scopes.push(std::collections::HashSet::new());
            self.mutable_borrows.push(std::collections::HashSet::new());
            let result = self.check_block_result(&arm.body, arm.span);
            self.scopes.pop();
            self.immutable_scopes.pop();
            self.loop_scopes.pop();
            self.mutable_borrows.pop();
            let result = result?;
            if result == ValueType::Unit {
                return Err(at(arm.span, "match arms must produce a value"));
            }
            if matches!(
                result,
                ValueType::Reference(_) | ValueType::MutableReference(_)
            ) {
                return Err(at(
                    span,
                    "references cannot escape through match expressions",
                ));
            }
            return Ok(result);
        }
        if !matches!(value_type, ValueType::Enum(_)) {
            return Err(at(
                span,
                format!(
                    "match requires an enum or destructurable aggregate value, got {}",
                    self.type_name(value_type)
                ),
            ));
        }
        let result = self.check_destructuring_match(value_type, arms, span)?;
        if matches!(
            result,
            ValueType::Reference(_) | ValueType::MutableReference(_)
        ) {
            return Err(at(
                span,
                "references cannot escape through match expressions",
            ));
        }
        Ok(result)
    }

    fn check_scalar_match(
        &mut self,
        value_type: ValueType,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let mut seen_bool = std::collections::HashSet::new();
        let mut seen_i64 = std::collections::HashSet::new();
        let mut seen_f32 = Vec::new();
        let mut seen_f64 = Vec::new();
        let mut wildcard_seen = false;
        let mut result_type = None;
        for arm in arms {
            match &arm.pattern {
                MatchPattern::Wildcard => {
                    if value_type == ValueType::Bool && seen_bool.len() == 2 {
                        return Err(at(
                            arm.span,
                            "unreachable wildcard match arm after all values are covered",
                        ));
                    }
                    wildcard_seen = true;
                }
                MatchPattern::Binding(name) => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if value_type == ValueType::Bool && seen_bool.len() == 2 {
                        return Err(at(
                            arm.span,
                            "unreachable wildcard match arm after all values are covered",
                        ));
                    }
                    wildcard_seen = true;
                    let mut values = HashMap::new();
                    values.insert(name.clone(), value_type);
                    self.scopes.push(values);
                    self.immutable_scopes.push(std::collections::HashSet::new());
                    self.loop_scopes.push(std::collections::HashSet::new());
                    let result = self.check_block_result(&arm.body, arm.span);
                    self.scopes.pop();
                    self.immutable_scopes.pop();
                    self.loop_scopes.pop();
                    let actual = result?;
                    if actual == ValueType::Unit {
                        return Err(at(arm.span, "match arms must produce a value"));
                    }
                    if let Some(expected) = result_type {
                        self.expect_type(actual, expected, arm.span, "match arm result")?;
                    } else {
                        result_type = Some(actual);
                    }
                    continue;
                }
                MatchPattern::Bool(value) if value_type == ValueType::Bool => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if !seen_bool.insert(*value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                }
                MatchPattern::I64(value) if value_type == ValueType::I64 => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if !seen_i64.insert(*value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                }
                MatchPattern::F32(value) if value_type == ValueType::F32 => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if seen_f32.iter().any(|seen| *seen == *value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                    seen_f32.push(*value);
                }
                MatchPattern::F64(value) if value_type == ValueType::F64 => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if seen_f64.iter().any(|seen| *seen == *value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                    seen_f64.push(*value);
                }
                MatchPattern::Bool(_)
                | MatchPattern::I64(_)
                | MatchPattern::F32(_)
                | MatchPattern::F64(_) => {
                    return Err(at(
                        arm.span,
                        format!(
                            "match pattern type does not match {} value",
                            self.type_name(value_type)
                        ),
                    ));
                }
                _ => {
                    return Err(at(
                        arm.span,
                        "destructuring patterns cannot match scalar values",
                    ));
                }
            }
            let actual = self.check_block_result(&arm.body, arm.span)?;
            if actual == ValueType::Unit {
                return Err(at(arm.span, "match arms must produce a value"));
            }
            if let Some(expected) = result_type {
                self.expect_type(actual, expected, arm.span, "match arm result")?;
            } else {
                result_type = Some(actual);
            }
        }
        if !wildcard_seen {
            if value_type == ValueType::Bool && seen_bool.len() != 2 {
                let missing = if !seen_bool.contains(&false) {
                    "false"
                } else {
                    "true"
                };
                return Err(at(
                    arms.last().map(|arm| arm.span).unwrap_or(span),
                    format!("non-exhaustive match on `bool`; missing value: {missing}"),
                ));
            }
            if value_type == ValueType::I64 {
                return Err(at(
                    span,
                    "non-exhaustive match on `i64`; add a wildcard (`_`) arm",
                ));
            }
            if matches!(value_type, ValueType::F32 | ValueType::F64) {
                return Err(at(
                    span,
                    format!(
                        "non-exhaustive match on `{}`; add a wildcard (`_`) arm",
                        self.type_name(value_type)
                    ),
                ));
            }
        }
        result_type.ok_or_else(|| at(span, "match expression must have at least one arm"))
    }

    fn check_destructuring_match(
        &mut self,
        value_type: ValueType,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let mut wildcard_seen = false;
        let mut seen_variants = std::collections::HashSet::new();
        let mut result_type = None;
        for arm in arms {
            if wildcard_seen {
                return Err(at(
                    arm.span,
                    if matches!(arm.pattern, MatchPattern::Wildcard) {
                        "duplicate wildcard match arm"
                    } else {
                        "unreachable match arm after wildcard"
                    },
                ));
            }
            let mut bindings = Vec::new();
            let mut seen_bindings = std::collections::HashSet::new();
            match &arm.pattern {
                MatchPattern::Wildcard => {
                    if wildcard_seen {
                        return Err(at(arm.span, "duplicate wildcard match arm"));
                    }
                    if let ValueType::Enum(enum_id) = value_type
                        && seen_variants.len() == self.enums[enum_id].variants.len()
                    {
                        return Err(at(
                            arm.span,
                            "unreachable wildcard match arm after all variants are covered",
                        ));
                    }
                    wildcard_seen = true;
                }
                MatchPattern::Binding(name) => {
                    if !matches!(value_type, ValueType::Enum(_)) {
                        return Err(at(
                            arm.span,
                            "binding patterns are only supported for scalar or enum match values",
                        ));
                    }
                    let ValueType::Enum(enum_id) = value_type else {
                        unreachable!()
                    };
                    if seen_variants.len() == self.enums[enum_id].variants.len() {
                        return Err(at(
                            arm.span,
                            "unreachable wildcard match arm after all variants are covered",
                        ));
                    }
                    wildcard_seen = true;
                    bindings.push((name.clone(), value_type));
                    seen_bindings.insert(name.clone());
                }
                MatchPattern::Variant {
                    enum_name,
                    variant_name,
                    bindings: payload,
                } => {
                    let ValueType::Enum(enum_id) = value_type else {
                        return Err(at(arm.span, "enum variant patterns require an enum value"));
                    };
                    let declaration = self.enums[enum_id].clone();
                    if enum_name != &declaration.name {
                        return Err(at(
                            arm.span,
                            format!(
                                "match pattern uses enum `{enum_name}`, expected `{}`",
                                declaration.name
                            ),
                        ));
                    }
                    let variant_id = declaration
                        .variants
                        .iter()
                        .position(|variant| variant.name == *variant_name)
                        .ok_or_else(|| {
                            at(
                                arm.span,
                                format!(
                                    "enum `{}` has no variant `{variant_name}`",
                                    declaration.name
                                ),
                            )
                        })?;
                    if !seen_variants.insert(variant_id) {
                        return Err(at(
                            arm.span,
                            format!(
                                "duplicate match arm for `{}::{variant_name}`",
                                declaration.name
                            ),
                        ));
                    }
                    let variant = &declaration.variants[variant_id];
                    match (payload, variant.payload.is_empty()) {
                        (None, false) => {
                            return Err(at(
                                arm.span,
                                format!(
                                    "match arm `{}::{variant_name}` must bind its payload",
                                    declaration.name
                                ),
                            ));
                        }
                        (Some(_), true) => {
                            return Err(at(
                                arm.span,
                                format!(
                                    "match arm `{}::{variant_name}` has no payload to bind or ignore",
                                    declaration.name
                                ),
                            ));
                        }
                        (Some(patterns), false) if patterns.len() != variant.payload.len() => {
                            return Err(at(
                                arm.span,
                                format!(
                                    "match arm `{}::{variant_name}` expects {} payload binding(s), got {}",
                                    declaration.name,
                                    variant.payload.len(),
                                    patterns.len()
                                ),
                            ));
                        }
                        _ => {}
                    }
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if let Some(patterns) = payload {
                        for (pattern, ty) in patterns.iter().zip(&variant.payload) {
                            let ty = self.resolve_type(ty, arm.span)?;
                            self.check_pattern(
                                pattern,
                                ty,
                                arm.span,
                                &mut bindings,
                                &mut seen_bindings,
                            )?;
                        }
                    }
                }
                MatchPattern::Bool(_)
                | MatchPattern::I64(_)
                | MatchPattern::F32(_)
                | MatchPattern::F64(_) => {
                    return Err(at(
                        arm.span,
                        "scalar patterns cannot match aggregate values",
                    ));
                }
                pattern => {
                    self.check_pattern(
                        pattern,
                        value_type,
                        arm.span,
                        &mut bindings,
                        &mut seen_bindings,
                    )?;
                    if matches!(value_type, ValueType::Enum(_)) {
                        return Err(at(
                            arm.span,
                            "enum matches require a variant pattern or wildcard",
                        ));
                    }
                }
            }
            self.scopes.push(HashMap::new());
            self.immutable_scopes.push(std::collections::HashSet::new());
            self.loop_scopes.push(std::collections::HashSet::new());
            for (name, ty) in bindings {
                self.scopes.last_mut().unwrap().insert(name, ty);
            }
            let arm_result = self.check_block_result(&arm.body, arm.span);
            self.scopes.pop();
            self.immutable_scopes.pop();
            self.loop_scopes.pop();
            let result = arm_result?;
            if result == ValueType::Unit {
                return Err(at(arm.span, "match arms must produce a value"));
            }
            if let Some(expected) = result_type {
                self.expect_type(result, expected, arm.span, "match arm result")?;
            } else {
                result_type = Some(result);
            }
        }
        if let ValueType::Enum(enum_id) = value_type {
            if !wildcard_seen && seen_variants.len() != self.enums[enum_id].variants.len() {
                let declaration = &self.enums[enum_id];
                let missing = declaration
                    .variants
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !seen_variants.contains(index))
                    .map(|(_, variant)| variant.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(at(
                    span,
                    format!(
                        "non-exhaustive match on `{}`; missing variant(s): {missing}",
                        declaration.name
                    ),
                ));
            }
        } else if !wildcard_seen {
            return Err(at(
                span,
                format!(
                    "non-exhaustive match on `{}`; add a wildcard (`_`) arm",
                    self.type_name(value_type)
                ),
            ));
        }
        result_type.ok_or_else(|| at(span, "match expression must have at least one arm"))
    }

    pub(super) fn check_pattern(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        span: Span,
        bindings: &mut Vec<(String, ValueType)>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Wildcard => Ok(()),
            MatchPattern::Binding(name) => {
                if !seen.insert(name.clone()) {
                    return Err(at(
                        span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                bindings.push((name.clone(), ty));
                Ok(())
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err(at(
                        span,
                        format!("tuple pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if patterns.len() != types.len() {
                    return Err(at(
                        span,
                        format!(
                            "tuple pattern has {} element(s), expected {}",
                            patterns.len(),
                            types.len()
                        ),
                    ));
                }
                for (pattern, ty) in patterns.iter().zip(types) {
                    self.check_pattern(pattern, ty, span, bindings, seen)?;
                }
                Ok(())
            }
            MatchPattern::Array(patterns) => {
                let ValueType::Array(array_id) = ty else {
                    return Err(at(
                        span,
                        format!("array pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let shape = self.array_shapes[array_id];
                if patterns.len() != shape.length as usize {
                    return Err(at(
                        span,
                        format!(
                            "array pattern has {} element(s), expected {}",
                            patterns.len(),
                            shape.length
                        ),
                    ));
                }
                for pattern in patterns {
                    self.check_pattern(
                        pattern,
                        ValueType::from(shape.element),
                        span,
                        bindings,
                        seen,
                    )?;
                }
                Ok(())
            }
            MatchPattern::Struct { name, fields, rest } => {
                let ValueType::Struct(struct_id) = ty else {
                    return Err(at(
                        span,
                        format!("struct pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let declaration = self.structs[struct_id].clone();
                if name != &declaration.name {
                    return Err(at(
                        span,
                        format!(
                            "struct pattern uses `{name}`, expected `{}`",
                            declaration.name
                        ),
                    ));
                }
                if declaration.module != self.module_context && !declaration.is_public {
                    return Err(at(span, format!("struct `{name}` is private")));
                }
                let mut seen_fields = std::collections::HashSet::new();
                for (field_name, pattern, field_span) in fields {
                    if !seen_fields.insert(field_name.as_str()) {
                        return Err(at(
                            *field_span,
                            format!(
                                "struct field `{field_name}` appears more than once in pattern"
                            ),
                        ));
                    }
                    let field = declaration
                        .fields
                        .iter()
                        .find(|field| field.name == *field_name)
                        .ok_or_else(|| {
                            at(
                                *field_span,
                                format!("struct `{name}` has no field `{field_name}`"),
                            )
                        })?;
                    let field_type = self.resolve_type(&field.ty, *field_span)?;
                    self.check_pattern(pattern, field_type, *field_span, bindings, seen)?;
                }
                if !rest && fields.len() != declaration.fields.len() {
                    let missing = declaration
                        .fields
                        .iter()
                        .filter(|field| !seen_fields.contains(field.name.as_str()))
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(at(
                        span,
                        format!(
                            "struct pattern for `{name}` is missing field(s): {missing}; add `..` to ignore remaining fields"
                        ),
                    ));
                }
                Ok(())
            }
            MatchPattern::Variant { .. } => Err(at(
                span,
                "enum variant pattern is not valid in this pattern position",
            )),
            MatchPattern::Bool(_)
            | MatchPattern::I64(_)
            | MatchPattern::F32(_)
            | MatchPattern::F64(_) => Err(at(
                span,
                "literal patterns are only supported for scalar match expressions",
            )),
        }
    }
}
