use super::*;

impl Checker {
    pub(super) fn check_expr(
        &mut self,
        expr: &Expr,
        allow_min_magnitude: bool,
    ) -> Result<ValueType, String> {
        match expr {
            Expr::Integer(value, span) => {
                if *value <= i64::MAX as u64
                    || (allow_min_magnitude && *value == (i64::MAX as u64) + 1)
                {
                    Ok(ValueType::I64)
                } else {
                    Err(at(*span, "integer literal is out of range for i64"))
                }
            }
            Expr::Bool(_, _) => Ok(ValueType::Bool),
            Expr::String(_, _) => Ok(ValueType::Str),
            Expr::Array(elements, span) => {
                if elements.is_empty() {
                    return Err(at(
                        *span,
                        "empty array literals require an array type annotation",
                    ));
                }
                let Some(first) = elements.first() else {
                    unreachable!()
                };
                if elements.len() > u32::MAX as usize {
                    return Err(at(*span, "array literal is too large"));
                }
                let first_type = self.check_expr(first, false)?;
                let element = match first_type {
                    ValueType::I64 => ArrayElementType::I64,
                    ValueType::Bool => ArrayElementType::Bool,
                    ValueType::Str => ArrayElementType::Str,
                    ValueType::Unit
                    | ValueType::Array(_, _)
                    | ValueType::Enum(_)
                    | ValueType::Struct(_) => {
                        return Err(at(
                            *span,
                            "array elements must have type `i64`, `bool`, or `str`",
                        ));
                    }
                };
                for value in &elements[1..] {
                    let value_type = self.check_expr(value, false)?;
                    self.expect_type(value_type, first_type, span_of_expr(value), "array element")?;
                }
                Ok(ValueType::Array(element, elements.len() as u32))
            }
            Expr::Variable(name, span) => self
                .lookup_local(name)
                .ok_or_else(|| at(*span, format!("undefined variable `{name}`"))),
            Expr::Index { array, index, span } => {
                let array_type = self.check_expr(array, false)?;
                let index_type = self.check_expr(index, false)?;
                self.expect_type(
                    index_type,
                    ValueType::I64,
                    span_of_expr(index),
                    "array index",
                )?;
                match array_type {
                    ValueType::Array(element, _) => Ok(element.into()),
                    _ => Err(at(*span, "indexing requires an array")),
                }
            }
            Expr::Field { value, name, span } => {
                let value_type = self.check_expr(value, false)?;
                let ValueType::Struct(struct_id) = value_type else {
                    return Err(at(*span, "field access requires a struct value"));
                };
                let field = self.structs[struct_id]
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| {
                        at(
                            *span,
                            format!(
                                "struct `{}` has no field `{name}`",
                                self.structs[struct_id].name
                            ),
                        )
                    })?;
                self.resolve_type(&field.ty, field.span)
            }
            Expr::Struct { name, fields, span } => {
                let struct_id = self
                    .struct_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| at(*span, format!("unknown struct `{name}`")))?;
                let declaration = self.structs[struct_id].clone();
                let mut seen = std::collections::HashSet::new();
                for (field_name, value, field_span) in fields {
                    if !seen.insert(field_name.as_str()) {
                        return Err(at(
                            *field_span,
                            format!("struct field `{field_name}` is initialized more than once"),
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
                    let expected = self.resolve_type(&field.ty, field.span)?;
                    let actual = self.check_expr(value, false)?;
                    self.expect_type(
                        actual,
                        expected,
                        span_of_expr(value),
                        &format!("initializer for field `{field_name}`"),
                    )?;
                }
                if fields.len() != declaration.fields.len() {
                    let missing = declaration
                        .fields
                        .iter()
                        .filter(|field| !seen.contains(field.name.as_str()))
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(at(
                        *span,
                        format!("struct `{name}` is missing field(s): {missing}"),
                    ));
                }
                Ok(ValueType::Struct(struct_id))
            }
            Expr::Unary { op, value, span } => {
                let value_type = self.check_expr(
                    value,
                    matches!(op, UnaryOp::Negate) && matches!(value.as_ref(), Expr::Integer(_, _)),
                )?;
                let expected = match op {
                    UnaryOp::Negate => ValueType::I64,
                    UnaryOp::Not => ValueType::Bool,
                };
                self.expect_type(value_type, expected, *span, "unary operator operand")?;
                Ok(expected)
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left_type = self.check_expr(left, false)?;
                let right_type = self.check_expr(right, false)?;
                if left_type != right_type {
                    return Err(at(
                        *span,
                        format!(
                            "operator operands have different types: {} and {}",
                            type_name(left_type, &self.enums),
                            type_name(right_type, &self.enums)
                        ),
                    ));
                }
                let result_type = match op {
                    BinaryOp::Add
                    | BinaryOp::Subtract
                    | BinaryOp::Multiply
                    | BinaryOp::Divide
                    | BinaryOp::Remainder => {
                        self.expect_type(left_type, ValueType::I64, *span, "arithmetic operands")?;
                        ValueType::I64
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == ValueType::Unit {
                            return Err(at(*span, "void expressions cannot be compared"));
                        }
                        if matches!(left_type, ValueType::Array(_, _)) {
                            return Err(at(*span, "array values cannot be compared"));
                        }
                        if matches!(left_type, ValueType::Enum(_)) {
                            return Err(at(*span, "enum values cannot be compared"));
                        }
                        if matches!(left_type, ValueType::Struct(_)) {
                            return Err(at(*span, "struct values cannot be compared"));
                        }
                        ValueType::Bool
                    }
                    BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => {
                        self.expect_type(left_type, ValueType::I64, *span, "comparison operands")?;
                        ValueType::Bool
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        self.expect_type(left_type, ValueType::Bool, *span, "logical operands")?;
                        ValueType::Bool
                    }
                };
                Ok(result_type)
            }
            Expr::Call { name, args, span } => {
                if name == "len" {
                    if args.len() != 1 {
                        return Err(at(
                            *span,
                            format!("function `len` expects 1 argument(s), got {}", args.len()),
                        ));
                    }
                    let array_type = self.check_expr(&args[0], false)?;
                    if !matches!(array_type, ValueType::Array(_, _)) {
                        return Err(at(
                            span_of_expr(&args[0]),
                            "argument 1 to `len` must be an array",
                        ));
                    }
                    return Ok(ValueType::I64);
                }
                let signature = self
                    .signatures
                    .get(name)
                    .ok_or_else(|| at(*span, format!("unknown function `{name}`")))?;
                if args.len() != signature.params.len() {
                    return Err(at(
                        *span,
                        format!(
                            "function `{name}` expects {} argument(s), got {}",
                            signature.params.len(),
                            args.len()
                        ),
                    ));
                }
                let params = signature.params.clone();
                let return_type = signature.return_type;
                for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
                    let actual = self.check_expr(arg, false)?;
                    self.expect_type(
                        actual,
                        expected,
                        span_of_expr(arg),
                        &format!("argument {} to `{name}`", index + 1),
                    )?;
                }
                Ok(return_type)
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "if condition",
                )?;
                let then_type = self.check_block_result(then_block, *span)?;
                let else_type = self.check_block_result(else_block, *span)?;
                if then_type != else_type {
                    return Err(at(
                        *span,
                        format!(
                            "`if` expression branches have different types: {} and {}",
                            type_name(then_type, &self.enums),
                            type_name(else_type, &self.enums)
                        ),
                    ));
                }
                if then_type == ValueType::Unit {
                    return Err(at(*span, "`if` expression branches must produce a value"));
                }
                Ok(then_type)
            }
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
            } => {
                let enum_id = self
                    .enum_ids
                    .get(enum_name)
                    .copied()
                    .ok_or_else(|| at(*span, format!("unknown enum `{enum_name}`")))?;
                let variant = self.enums[enum_id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                    .ok_or_else(|| {
                        at(
                            *span,
                            format!("enum `{enum_name}` has no variant `{variant_name}`"),
                        )
                    })?;
                match (&variant.payload, payload) {
                    (None, None) => {}
                    (Some(expected), Some(value)) => {
                        let expected = self.resolve_type(expected, variant.span)?;
                        let actual = self.check_expr(value, false)?;
                        self.expect_type(actual, expected, span_of_expr(value), "enum payload")?;
                    }
                    (Some(_), None) => {
                        return Err(at(
                            *span,
                            format!("variant `{enum_name}::{variant_name}` requires a payload"),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(at(
                            *span,
                            format!(
                                "variant `{enum_name}::{variant_name}` does not take a payload"
                            ),
                        ));
                    }
                }
                Ok(ValueType::Enum(enum_id))
            }
            Expr::Match { value, arms, span } => {
                let enum_id = match self.check_expr(value, false)? {
                    ValueType::Enum(enum_id) => enum_id,
                    actual => {
                        return Err(at(
                            *span,
                            format!(
                                "match requires an enum value, got {}",
                                type_name(actual, &self.enums)
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
                            binding,
                            ignore_payload,
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
                            match (&variant.payload, binding, ignore_payload) {
                                (Some(_), None, false) => {
                                    return Err(at(
                                        arm.span,
                                        format!(
                                            "match arm `{}::{}` must bind its payload or use `_`",
                                            declaration.name, variant.name
                                        ),
                                    ));
                                }
                                (None, Some(_), _) | (None, None, true) => {
                                    return Err(at(
                                        arm.span,
                                        format!(
                                            "match arm `{}::{}` has no payload to bind or ignore",
                                            declaration.name, variant.name
                                        ),
                                    ));
                                }
                                _ => {}
                            }
                            Some(variant)
                        }
                    };
                    self.scopes.push(HashMap::new());
                    self.immutable_scopes.push(std::collections::HashSet::new());
                    self.loop_scopes.push(std::collections::HashSet::new());
                    let binding = match &arm.pattern {
                        MatchPattern::Variant {
                            binding: Some(binding),
                            ..
                        } => Some(binding),
                        _ => None,
                    };
                    let arm_result = if let (Some(binding), Some(variant)) = (binding, variant) {
                        let payload_type = variant
                            .payload
                            .as_ref()
                            .expect("payload binding validated above");
                        let binding_type = self.resolve_type(payload_type, arm.span)?;
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(binding.clone(), binding_type);
                        self.check_block_result(&arm.body, arm.span)
                    } else {
                        self.check_block_result(&arm.body, arm.span)
                    };
                    self.scopes.pop();
                    self.immutable_scopes.pop();
                    self.loop_scopes.pop();
                    let actual = arm_result?;
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
                        *span,
                        format!(
                            "non-exhaustive match on `{}`; missing variant(s): {missing}",
                            declaration.name
                        ),
                    ));
                }
                result_type.ok_or_else(|| at(*span, "match expression must have at least one arm"))
            }
        }
    }
}
