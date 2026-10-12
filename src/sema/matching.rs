use super::*;

impl Checker {
    pub(super) fn check_match_expr(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let value_type = self.check_expr(value, false)?;
        if value_type == ValueType::F64 {
            return Err(at(span, "match does not support `f64` values"));
        }
        if matches!(value_type, ValueType::Bool | ValueType::I64) {
            self.check_scalar_match(value_type, arms, span)
        } else {
            let enum_id = match value_type {
                ValueType::Enum(enum_id) => enum_id,
                actual => {
                    return Err(at(
                        span,
                        format!(
                            "match requires an enum value, got {}",
                            self.type_name(actual)
                        ),
                    ));
                }
            };
            let declaration = self.enums[enum_id].clone();
            let mut seen = std::collections::HashSet::new();
            let mut wildcard_seen = false;
            let mut result_type = None;
            for arm in arms {
                let variant = match &arm.pattern {
                    MatchPattern::Wildcard => {
                        if wildcard_seen {
                            return Err(at(arm.span, "duplicate wildcard match arm"));
                        }
                        if seen.len() == declaration.variants.len() {
                            return Err(at(
                                arm.span,
                                "unreachable wildcard match arm after all variants are covered",
                            ));
                        }
                        wildcard_seen = true;
                        None
                    }
                    MatchPattern::Variant {
                        enum_name,
                        variant_name,
                        bindings,
                    } => {
                        if wildcard_seen {
                            return Err(at(arm.span, "unreachable match arm after wildcard"));
                        }
                        if enum_name != &declaration.name {
                            return Err(at(
                                arm.span,
                                format!(
                                    "match pattern uses enum `{enum_name}`, expected `{}`",
                                    declaration.name
                                ),
                            ));
                        }
                        let variant_index = declaration
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
                        if !seen.insert(variant_index) {
                            return Err(at(
                                arm.span,
                                format!(
                                    "duplicate match arm for `{}::{variant_name}`",
                                    declaration.name
                                ),
                            ));
                        }
                        let variant = &declaration.variants[variant_index];
                        match (&variant.payload[..], bindings) {
                            ([], Some(_)) => {
                                return Err(at(
                                    arm.span,
                                    format!(
                                        "match arm `{}::{}` has no payload to bind or ignore",
                                        declaration.name, variant.name
                                    ),
                                ));
                            }
                            (_, None) if !variant.payload.is_empty() => {
                                return Err(at(
                                    arm.span,
                                    format!(
                                        "match arm `{}::{}` must bind its payload or use `_`",
                                        declaration.name, variant.name
                                    ),
                                ));
                            }
                            (payload_types, Some(bindings))
                                if payload_types.len() != bindings.len() =>
                            {
                                return Err(at(
                                    arm.span,
                                    format!(
                                        "match arm `{}::{}` expects {} payload binding(s), got {}",
                                        declaration.name,
                                        variant.name,
                                        payload_types.len(),
                                        bindings.len()
                                    ),
                                ));
                            }
                            _ => {}
                        }
                        Some(variant)
                    }
                    MatchPattern::Bool(_) | MatchPattern::I64(_) => {
                        return Err(at(
                            arm.span,
                            format!(
                                "scalar patterns cannot match enum `{}` values",
                                declaration.name
                            ),
                        ));
                    }
                };
                self.scopes.push(HashMap::new());
                self.immutable_scopes.push(std::collections::HashSet::new());
                self.loop_scopes.push(std::collections::HashSet::new());
                let bindings = match &arm.pattern {
                    MatchPattern::Variant {
                        bindings: Some(bindings),
                        ..
                    } => Some(bindings),
                    _ => None,
                };
                if let (Some(bindings), Some(variant)) = (bindings, variant) {
                    let mut seen_bindings = std::collections::HashSet::new();
                    for (binding, payload_type) in bindings.iter().zip(&variant.payload) {
                        if let Some(binding) = binding {
                            if !seen_bindings.insert(binding.as_str()) {
                                return Err(at(
                                    arm.span,
                                    format!("`{binding}` is already declared in this scope"),
                                ));
                            }
                            let binding_type = self.resolve_type(payload_type, arm.span)?;
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(binding.clone(), binding_type);
                        }
                    }
                }
                let arm_result = self.check_block_result(&arm.body, arm.span);
                self.scopes.pop();
                self.immutable_scopes.pop();
                self.loop_scopes.pop();
                let actual = arm_result?;
                if actual == ValueType::Unit {
                    return Err(at(arm.span, "match arms must produce a value"));
                }
                if let Some(expected) = result_type {
                    self.expect_type(actual, expected, arm.span, "match arm result")?;
                } else {
                    result_type = Some(actual);
                }
            }
            if !wildcard_seen && seen.len() != declaration.variants.len() {
                let missing = declaration
                    .variants
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !seen.contains(index))
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
            result_type.ok_or_else(|| at(span, "match expression must have at least one arm"))
        }
    }

    fn check_scalar_match(
        &mut self,
        value_type: ValueType,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let mut seen_bool = std::collections::HashSet::new();
        let mut seen_i64 = std::collections::HashSet::new();
        let mut wildcard_seen = false;
        let mut result_type = None;
        for arm in arms {
            match &arm.pattern {
                MatchPattern::Wildcard => {
                    if wildcard_seen {
                        return Err(at(arm.span, "duplicate wildcard match arm"));
                    }
                    let exhaustive = match value_type {
                        ValueType::Bool => seen_bool.len() == 2,
                        ValueType::I64 => false,
                        _ => unreachable!(),
                    };
                    if exhaustive {
                        return Err(at(
                            arm.span,
                            "unreachable wildcard match arm after all values are covered",
                        ));
                    }
                    wildcard_seen = true;
                }
                MatchPattern::Bool(value) if value_type == ValueType::Bool => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if !seen_bool.insert(value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                }
                MatchPattern::I64(value) if value_type == ValueType::I64 => {
                    if wildcard_seen {
                        return Err(at(arm.span, "unreachable match arm after wildcard"));
                    }
                    if !seen_i64.insert(value) {
                        return Err(at(arm.span, format!("duplicate match arm for `{value}`")));
                    }
                }
                MatchPattern::Bool(_) | MatchPattern::I64(_) => {
                    return Err(at(
                        arm.span,
                        format!(
                            "match pattern type does not match {} value",
                            self.type_name(value_type)
                        ),
                    ));
                }
                MatchPattern::Variant { .. } => {
                    return Err(at(
                        arm.span,
                        format!(
                            "enum variant patterns cannot match {} values",
                            self.type_name(value_type)
                        ),
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
                let diagnostic_span = arms.last().map(|arm| arm.span).unwrap_or(span);
                return Err(at(
                    diagnostic_span,
                    format!("non-exhaustive match on `bool`; missing value: {missing}"),
                ));
            }
            if value_type == ValueType::I64 {
                return Err(at(
                    span,
                    "non-exhaustive match on `i64`; add a wildcard (`_`) arm",
                ));
            }
        }
        result_type.ok_or_else(|| at(span, "match expression must have at least one arm"))
    }
}
