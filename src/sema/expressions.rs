use super::*;

impl Checker {
    pub(super) fn check_expr_expected(
        &mut self,
        expr: &Expr,
        expected: ValueType,
    ) -> Result<ValueType, Diagnostic> {
        let needs_context = match (expr, expected) {
            (Expr::Array(elements, _), ValueType::Array(index)) => {
                elements.is_empty()
                    || matches!(self.array_shapes[index].element, ArrayElementType::Array(_))
            }
            _ => false,
        };
        if needs_context {
            self.check_expr_with_type(expr, expected)
        } else {
            self.check_expr(expr, false)
        }
    }

    pub(super) fn check_expr_with_type(
        &mut self,
        expr: &Expr,
        expected: ValueType,
    ) -> Result<ValueType, Diagnostic> {
        let Expr::Array(elements, span) = expr else {
            return self.check_expr(expr, false);
        };
        let ValueType::Array(array_id) = expected else {
            return self.check_expr(expr, false);
        };
        let shape = self.array_shapes[array_id];
        if elements.is_empty() {
            if shape.length != 0 {
                return Err(at(*span, "empty array literal must have array length 0"));
            }
            return Ok(expected);
        }
        if elements.len() != shape.length as usize {
            return Err(at(
                *span,
                format!(
                    "array literal has length {}, expected {}",
                    elements.len(),
                    shape.length
                ),
            ));
        }
        let element_type = ValueType::from(shape.element);
        for element in elements {
            let actual = if matches!(shape.element, ArrayElementType::Array(_))
                || matches!(element, Expr::Array(children, _) if children.is_empty())
            {
                self.check_expr_with_type(element, element_type)?
            } else {
                self.check_expr(element, false)?
            };
            self.expect_type(actual, element_type, span_of_expr(element), "array element")?;
        }
        Ok(expected)
    }

    pub(super) fn check_place(
        &mut self,
        expr: &Expr,
        ty: ValueType,
    ) -> Result<ValueType, Diagnostic> {
        match expr {
            Expr::Variable(_, _) => Ok(ty),
            Expr::Field { value, name, span } => {
                let ty = self.check_place(value, ty)?;
                let ValueType::Struct(struct_id) = ty else {
                    if let Expr::Variable(name, _) = value.as_ref() {
                        return Err(at(*span, format!("`{name}` is not a struct")));
                    }
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
                let field_type = field.ty.clone();
                let field_span = field.span;
                self.resolve_type(&field_type, field_span)
            }
            Expr::Index { array, index, span } => {
                let ty = self.check_place(array, ty)?;
                let index_type = self.check_expr(index, false)?;
                self.expect_type(
                    index_type,
                    ValueType::I64,
                    span_of_expr(index),
                    "array index",
                )?;
                match ty {
                    ValueType::Array(index) => {
                        let element = self.array_shapes[index].element;
                        Ok(ValueType::from(element))
                    }
                    _ => Err(at(*span, "indexing requires an array")),
                }
            }
            _ => Err(at(
                span_of_expr(expr),
                "assignment target must be a local variable, field, or array element",
            )),
        }
    }

    pub(super) fn check_expr(
        &mut self,
        expr: &Expr,
        allow_min_magnitude: bool,
    ) -> Result<ValueType, Diagnostic> {
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
                    ValueType::Enum(index) => ArrayElementType::Enum(index),
                    ValueType::Struct(index) => ArrayElementType::Struct(index),
                    ValueType::Array(index) => ArrayElementType::Array(index),
                    ValueType::Unit => {
                        return Err(at(
                            *span,
                            "array elements cannot have type `unit` or be arrays",
                        ));
                    }
                };
                for value in &elements[1..] {
                    let value_type = self.check_expr(value, false)?;
                    self.expect_type(value_type, first_type, span_of_expr(value), "array element")?;
                }
                Ok(self.array_type(element, elements.len() as u32))
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
                    ValueType::Array(index) => {
                        Ok(ValueType::from(self.array_shapes[index].element))
                    }
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
                let field_type = field.ty.clone();
                let field_span = field.span;
                self.resolve_type(&field_type, field_span)
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
                    let actual = self.check_expr_expected(value, expected)?;
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
                            self.type_name(left_type),
                            self.type_name(right_type)
                        ),
                    ));
                }
                let result_type = match op {
                    BinaryOp::Add if left_type == ValueType::Str => ValueType::Str,
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
                    if !matches!(array_type, ValueType::Array(_)) {
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
                    let actual = self.check_expr_expected(arg, expected)?;
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
                            self.type_name(then_type),
                            self.type_name(else_type)
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
                    .cloned()
                    .ok_or_else(|| {
                        at(
                            *span,
                            format!("enum `{enum_name}` has no variant `{variant_name}`"),
                        )
                    })?;
                match (&variant.payload[..], payload) {
                    ([], None) => {}
                    (expected, Some(values)) if expected.len() == values.len() => {
                        for (expected, value) in expected.iter().zip(values) {
                            let expected = self.resolve_type(expected, variant.span)?;
                            let actual = self.check_expr_expected(value, expected)?;
                            self.expect_type(
                                actual,
                                expected,
                                span_of_expr(value),
                                "enum payload",
                            )?;
                        }
                    }
                    (expected, Some(values)) => {
                        return Err(at(
                            *span,
                            format!(
                                "variant `{enum_name}::{variant_name}` expects {} payload value(s), got {}",
                                expected.len(),
                                values.len()
                            ),
                        ));
                    }
                    (_, None) => {
                        return Err(at(
                            *span,
                            format!("variant `{enum_name}::{variant_name}` requires a payload"),
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
