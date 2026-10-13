use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn declare_globals(&mut self, program: &Program) -> Result<(), String> {
        for declaration in &program.globals {
            self.module_context = declaration.module.clone();
            let ty = declaration
                .ty
                .as_ref()
                .ok_or_else(|| format!("global `{}` has no resolved type", declaration.name))?;
            let ty = self.resolve_type(ty, declaration.span)?;
            if ty == ValueType::Unit || matches!(ty, ValueType::Vector(_)) {
                return Err(format!(
                    "line {}:{}: global initializer type cannot be `unit` or a vector",
                    declaration.span.line, declaration.span.column
                ));
            }
            let llvm_type = self.llvm_type(ty).ok_or("global has no LLVM type")?;
            let symbol = llvm_global_name(&declaration.name);
            let global = self
                .module
                .add_global(llvm_type, Some(AddressSpace::default()), &symbol);
            global.set_linkage(if declaration.is_public {
                Linkage::External
            } else {
                Linkage::Internal
            });
            global.set_constant(!declaration.mutable);
            self.global_types.insert(declaration.name.clone(), ty);
            self.globals.insert(declaration.name.clone(), global);
            self.global_declarations
                .insert(declaration.name.clone(), declaration.clone());
            self.global_states.insert(declaration.name.clone(), 0);
        }
        Ok(())
    }

    pub(super) fn define_globals(&mut self, program: &Program) -> Result<(), String> {
        for declaration in &program.globals {
            let value = self.evaluate_global(&declaration.name)?;
            let initializer = self.lower_constant(&value)?;
            let ty = self.global_types[&declaration.name];
            if value.ty != ty {
                return Err(format!(
                    "line {}:{}: initializer for global `{}` has type {}, expected {}",
                    declaration.span.line,
                    declaration.span.column,
                    declaration.name,
                    self.type_name(value.ty),
                    self.type_name(ty)
                ));
            }
            self.globals[&declaration.name].set_initializer(&initializer);
        }
        Ok(())
    }

    fn evaluate_global(&mut self, name: &str) -> Result<ConstantValue, String> {
        if let Some(value) = self.global_constant_values.get(name) {
            return Ok(value.clone());
        }
        match self.global_states.get(name).copied().unwrap_or(0) {
            1 => return Err(format!("global initializer cycle involving `{name}`")),
            2 => {
                return self
                    .global_constant_values
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("global `{name}` has no initializer value"));
            }
            _ => {}
        }
        self.global_states.insert(name.to_owned(), 1);
        let declaration = self
            .global_declarations
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown global `{name}`"))?;
        let previous_module = self.module_context.clone();
        self.module_context = declaration.module.clone();
        let ty = self.global_types[name];
        let result = self.evaluate_constant_expr(&declaration.value, Some(ty));
        self.module_context = previous_module;
        match result {
            Ok(value) => {
                self.global_constant_values
                    .insert(name.to_owned(), value.clone());
                self.global_states.insert(name.to_owned(), 2);
                Ok(value)
            }
            Err(error) => {
                self.global_states.insert(name.to_owned(), 0);
                Err(format!(
                    "line {}:{}: {error}",
                    declaration.span.line, declaration.span.column
                ))
            }
        }
    }

    fn evaluate_constant_expr(
        &mut self,
        expr: &Expr,
        expected: Option<ValueType>,
    ) -> Result<ConstantValue, String> {
        let fail =
            |span: Span, message: &str| format!("line {}:{}: {message}", span.line, span.column);
        let value = match expr {
            Expr::Integer(value, span) => {
                let value = if let Some(value) = value.checked_into_i64() {
                    value
                } else {
                    return Err(fail(*span, "integer literal is out of range for i64"));
                };
                ConstantValue {
                    ty: ValueType::I64,
                    data: ConstantData::I64(value),
                }
            }
            Expr::Float32(value, _) => ConstantValue {
                ty: ValueType::F32,
                data: ConstantData::F32(*value),
            },
            Expr::Float(value, _) => ConstantValue {
                ty: ValueType::F64,
                data: ConstantData::F64(*value),
            },
            Expr::Bool(value, _) => ConstantValue {
                ty: ValueType::Bool,
                data: ConstantData::Bool(*value),
            },
            Expr::String(value, _) => ConstantValue {
                ty: ValueType::Str,
                data: ConstantData::Str(value.clone()),
            },
            Expr::Variable(name, span) => {
                if self.lookup_local(name).is_some() {
                    return Err(fail(
                        *span,
                        "global initializers cannot refer to local variables",
                    ));
                }
                let global_name = self
                    .resolve_global_name(name)
                    .ok_or_else(|| fail(*span, &format!("undefined variable `{name}`")))?;
                let declaration = self.global_declarations[&global_name].clone();
                if declaration.mutable {
                    return Err(fail(
                        *span,
                        "global initializers may reference only immutable globals",
                    ));
                }
                self.evaluate_global(&global_name)?
            }
            Expr::Array(elements, span) => {
                let array_type = if let Some(ValueType::Array(index)) = expected {
                    ValueType::Array(index)
                } else {
                    if elements.is_empty() {
                        return Err(fail(
                            *span,
                            "empty array literals require an array type annotation",
                        ));
                    }
                    let first = self.evaluate_constant_expr(&elements[0], None)?;
                    if elements.len() > u32::MAX as usize {
                        return Err(fail(*span, "array literal is too large"));
                    }
                    let element = self.array_element_type(first.ty).ok_or_else(|| {
                        fail(*span, "array elements cannot have type `unit` or a vector")
                    })?;
                    self.array_type(element, elements.len() as u32)
                };
                let ValueType::Array(array_id) = array_type else {
                    unreachable!()
                };
                let shape = self.array_shapes[array_id];
                if elements.len() != shape.length as usize {
                    return Err(fail(
                        *span,
                        &format!(
                            "array literal has length {}, expected {}",
                            elements.len(),
                            shape.length
                        ),
                    ));
                }
                let element_type = ValueType::from(shape.element);
                let values = elements
                    .iter()
                    .map(|item| self.evaluate_constant_expr(item, Some(element_type)))
                    .collect::<Result<Vec<_>, _>>()?;
                if values.iter().any(|item| item.ty != element_type) {
                    return Err(fail(*span, "array element type mismatch"));
                }
                ConstantValue {
                    ty: array_type,
                    data: ConstantData::Array(values),
                }
            }
            Expr::Tuple(elements, span) => {
                let tuple_type = if let Some(ValueType::Tuple(index)) = expected {
                    ValueType::Tuple(index)
                } else {
                    let values = elements
                        .iter()
                        .map(|item| self.evaluate_constant_expr(item, None))
                        .collect::<Result<Vec<_>, _>>()?;
                    let types = values.iter().map(|item| item.ty).collect();
                    let ty = self.tuple_type(types);
                    return Ok(ConstantValue {
                        ty,
                        data: ConstantData::Tuple(values),
                    });
                };
                let ValueType::Tuple(tuple_id) = tuple_type else {
                    unreachable!()
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if types.len() != elements.len() {
                    return Err(fail(*span, "tuple expression arity mismatch"));
                }
                let values = elements
                    .iter()
                    .zip(types)
                    .map(|(item, ty)| self.evaluate_constant_expr(item, Some(ty)))
                    .collect::<Result<Vec<_>, _>>()?;
                ConstantValue {
                    ty: tuple_type,
                    data: ConstantData::Tuple(values),
                }
            }
            Expr::Struct {
                name, fields, span, ..
            } => {
                let struct_id = *self
                    .struct_ids
                    .get(name)
                    .ok_or_else(|| fail(*span, &format!("unknown struct `{name}`")))?;
                if self
                    .struct_visibility
                    .get(name)
                    .is_some_and(|(owner, public)| owner != &self.module_context && !public)
                {
                    return Err(fail(*span, &format!("struct `{name}` is private")));
                }
                let declaration = self.structs[struct_id].clone();
                let mut values = Vec::with_capacity(declaration.fields.len());
                for field in &declaration.fields {
                    let (_, expression, field_span) = fields
                        .iter()
                        .find(|(field_name, _, _)| field_name == &field.name)
                        .ok_or_else(|| {
                            fail(
                                *span,
                                &format!("struct `{name}` is missing field `{}`", field.name),
                            )
                        })?;
                    let ty = self.resolve_type(&field.ty, field.span)?;
                    let value =
                        self.evaluate_constant_expr(expression, Some(ty))
                            .map_err(|error| {
                                if error.is_empty() {
                                    fail(*field_span, "invalid constant field")
                                } else {
                                    error
                                }
                            })?;
                    values.push(value);
                }
                if fields.len() != declaration.fields.len() {
                    return Err(fail(
                        *span,
                        &format!("struct `{name}` initializer has an incorrect field count"),
                    ));
                }
                ConstantValue {
                    ty: ValueType::Struct(struct_id),
                    data: ConstantData::Struct(values),
                }
            }
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
            } => {
                let enum_id = *self
                    .enum_ids
                    .get(enum_name)
                    .ok_or_else(|| fail(*span, &format!("unknown enum `{enum_name}`")))?;
                if self
                    .enum_visibility
                    .get(enum_name)
                    .is_some_and(|(owner, public)| owner != &self.module_context && !public)
                {
                    return Err(fail(*span, &format!("enum `{enum_name}` is private")));
                }
                let variant_index = self.enums[enum_id]
                    .variants
                    .iter()
                    .position(|variant| variant.name == *variant_name)
                    .ok_or_else(|| {
                        fail(
                            *span,
                            &format!("enum `{enum_name}` has no variant `{variant_name}"),
                        )
                    })?;
                let declaration = self.enums[enum_id].variants[variant_index].clone();
                let expressions = payload.as_deref().unwrap_or(&[]);
                if expressions.len() != declaration.payload.len() {
                    return Err(fail(
                        *span,
                        &format!("variant `{enum_name}::{variant_name}` payload arity mismatch"),
                    ));
                }
                let types = declaration
                    .payload
                    .iter()
                    .map(|ty| self.resolve_type(ty, declaration.span))
                    .collect::<Result<Vec<_>, _>>()?;
                let values = expressions
                    .iter()
                    .zip(types)
                    .map(|(expression, ty)| self.evaluate_constant_expr(expression, Some(ty)))
                    .collect::<Result<Vec<_>, _>>()?;
                ConstantValue {
                    ty: ValueType::Enum(enum_id),
                    data: ConstantData::Enum {
                        variant: variant_index,
                        payload: values,
                    },
                }
            }
            Expr::Unary { op, value, span } => {
                if matches!(op, UnaryOp::Negate)
                    && matches!(value.as_ref(), Expr::Integer(n, _) if *n == (i64::MAX as u64) + 1)
                {
                    ConstantValue {
                        ty: ValueType::I64,
                        data: ConstantData::I64(i64::MIN),
                    }
                } else {
                    let value = self.evaluate_constant_expr(value, None)?;
                    match (op, value.data) {
                        (UnaryOp::Negate, ConstantData::I64(value)) => ConstantValue {
                            ty: ValueType::I64,
                            data: ConstantData::I64(value.checked_neg().ok_or_else(|| {
                                fail(*span, "integer overflow in constant expression")
                            })?),
                        },
                        (UnaryOp::Negate, ConstantData::F32(value)) => ConstantValue {
                            ty: ValueType::F32,
                            data: ConstantData::F32(-value),
                        },
                        (UnaryOp::Negate, ConstantData::F64(value)) => ConstantValue {
                            ty: ValueType::F64,
                            data: ConstantData::F64(-value),
                        },
                        (UnaryOp::Not, ConstantData::Bool(value)) => ConstantValue {
                            ty: ValueType::Bool,
                            data: ConstantData::Bool(!value),
                        },
                        _ => return Err(fail(*span, "invalid unary operator constant")),
                    }
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left = self.evaluate_constant_expr(left, None)?;
                let right = self.evaluate_constant_expr(right, Some(left.ty))?;
                if left.ty != right.ty {
                    return Err(fail(*span, "operator operands have different types"));
                }
                self.evaluate_binary_constant(*op, left, right, *span)?
            }
            Expr::Field { value, name, span } => {
                let value = self.evaluate_constant_expr(value, None)?;
                let ValueType::Struct(struct_id) = value.ty else {
                    return Err(fail(*span, "field access requires a struct value"));
                };
                let index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| {
                        fail(
                            *span,
                            &format!(
                                "struct `{}` has no field `{name}`",
                                self.structs[struct_id].name
                            ),
                        )
                    })?;
                let ConstantData::Struct(fields) = value.data else {
                    unreachable!()
                };
                fields[index].clone()
            }
            Expr::Index { array, index, span } => {
                let array = self.evaluate_constant_expr(array, None)?;
                let index = self.evaluate_constant_expr(index, Some(ValueType::I64))?;
                let ConstantData::I64(index) = index.data else {
                    unreachable!()
                };
                let values = match array.data {
                    ConstantData::Array(values) | ConstantData::Tuple(values) => values,
                    _ => {
                        return Err(fail(
                            *span,
                            "indexing requires a fixed array or tuple constant",
                        ));
                    }
                };
                let index = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < values.len())
                    .ok_or_else(|| fail(*span, "constant index is out of bounds"))?;
                values[index].clone()
            }
            Expr::Call { span, .. }
            | Expr::CallValue { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::If { span, .. }
            | Expr::Match { span, .. }
            | Expr::Loop { span, .. } => {
                return Err(fail(
                    *span,
                    "global initializers must be compile-time constant expressions",
                ));
            }
        };
        if let Some(expected) = expected
            && value.ty != expected
        {
            return Err(fail(
                span_of_expr(expr),
                &format!(
                    "constant expression has type {}, expected {}",
                    self.type_name(value.ty),
                    self.type_name(expected)
                ),
            ));
        }
        Ok(value)
    }

    fn evaluate_binary_constant(
        &self,
        op: BinaryOp,
        left: ConstantValue,
        right: ConstantValue,
        span: Span,
    ) -> Result<ConstantValue, String> {
        let error = |message: &str| format!("line {}:{}: {message}", span.line, span.column);
        let aggregate_equal = constant_values_equal(&left, &right);
        let (ty, data) = match (left.data.clone(), right.data.clone()) {
            (ConstantData::I64(a), ConstantData::I64(b)) => match op {
                BinaryOp::Add => (
                    ValueType::I64,
                    ConstantData::I64(
                        a.checked_add(b)
                            .ok_or_else(|| error("integer overflow in constant expression"))?,
                    ),
                ),
                BinaryOp::Subtract => (
                    ValueType::I64,
                    ConstantData::I64(
                        a.checked_sub(b)
                            .ok_or_else(|| error("integer overflow in constant expression"))?,
                    ),
                ),
                BinaryOp::Multiply => (
                    ValueType::I64,
                    ConstantData::I64(
                        a.checked_mul(b)
                            .ok_or_else(|| error("integer overflow in constant expression"))?,
                    ),
                ),
                BinaryOp::Divide if b == 0 || (a == i64::MIN && b == -1) => {
                    return Err(error("invalid integer division in constant expression"));
                }
                BinaryOp::Divide => (ValueType::I64, ConstantData::I64(a / b)),
                BinaryOp::Remainder if b == 0 || (a == i64::MIN && b == -1) => {
                    return Err(error("invalid integer remainder in constant expression"));
                }
                BinaryOp::Remainder => (ValueType::I64, ConstantData::I64(a % b)),
                BinaryOp::BitAnd => (ValueType::I64, ConstantData::I64(a & b)),
                BinaryOp::BitOr => (ValueType::I64, ConstantData::I64(a | b)),
                BinaryOp::BitXor => (ValueType::I64, ConstantData::I64(a ^ b)),
                BinaryOp::ShiftLeft if !(0..64).contains(&b) => {
                    return Err(error("shift amount must be between 0 and 63"));
                }
                BinaryOp::ShiftLeft => {
                    (ValueType::I64, ConstantData::I64(a.wrapping_shl(b as u32)))
                }
                BinaryOp::ShiftRight if !(0..64).contains(&b) => {
                    return Err(error("shift amount must be between 0 and 63"));
                }
                BinaryOp::ShiftRight => (ValueType::I64, ConstantData::I64(a >> b)),
                BinaryOp::Equal => (ValueType::Bool, ConstantData::Bool(a == b)),
                BinaryOp::NotEqual => (ValueType::Bool, ConstantData::Bool(a != b)),
                BinaryOp::Less => (ValueType::Bool, ConstantData::Bool(a < b)),
                BinaryOp::LessEqual => (ValueType::Bool, ConstantData::Bool(a <= b)),
                BinaryOp::Greater => (ValueType::Bool, ConstantData::Bool(a > b)),
                BinaryOp::GreaterEqual => (ValueType::Bool, ConstantData::Bool(a >= b)),
                _ => return Err(error("unsupported integer constant operator")),
            },
            (ConstantData::F32(a), ConstantData::F32(b)) => {
                self.eval_f32_binary(op, a, b, error)?
            }
            (ConstantData::F64(a), ConstantData::F64(b)) => {
                self.eval_float_binary(op, a, b, ValueType::F64, error)?
            }
            (ConstantData::Bool(a), ConstantData::Bool(b)) => match op {
                BinaryOp::And => (ValueType::Bool, ConstantData::Bool(a && b)),
                BinaryOp::Or => (ValueType::Bool, ConstantData::Bool(a || b)),
                BinaryOp::Equal => (ValueType::Bool, ConstantData::Bool(a == b)),
                BinaryOp::NotEqual => (ValueType::Bool, ConstantData::Bool(a != b)),
                _ => return Err(error("unsupported bool constant operator")),
            },
            (ConstantData::Str(a), ConstantData::Str(b)) => match op {
                BinaryOp::Add => (ValueType::Str, ConstantData::Str(format!("{a}{b}"))),
                BinaryOp::Equal => (ValueType::Bool, ConstantData::Bool(a == b)),
                BinaryOp::NotEqual => (ValueType::Bool, ConstantData::Bool(a != b)),
                _ => return Err(error("unsupported string constant operator")),
            },
            _ => match op {
                BinaryOp::Equal | BinaryOp::NotEqual => {
                    let equal = aggregate_equal;
                    (
                        ValueType::Bool,
                        ConstantData::Bool(if matches!(op, BinaryOp::Equal) {
                            equal
                        } else {
                            !equal
                        }),
                    )
                }
                _ => return Err(error("unsupported aggregate constant operator")),
            },
        };
        Ok(ConstantValue { ty, data })
    }

    fn eval_float_binary<F>(
        &self,
        op: BinaryOp,
        a: f64,
        b: f64,
        ty: ValueType,
        error: F,
    ) -> Result<(ValueType, ConstantData), String>
    where
        F: Fn(&str) -> String,
    {
        let value = match op {
            BinaryOp::Add => Some(a + b),
            BinaryOp::Subtract => Some(a - b),
            BinaryOp::Multiply => Some(a * b),
            BinaryOp::Divide => Some(a / b),
            BinaryOp::Remainder => Some(a % b),
            _ => None,
        };
        if let Some(value) = value {
            return Ok(match ty {
                ValueType::F32 => (ty, ConstantData::F32(value as f32)),
                _ => (ty, ConstantData::F64(value)),
            });
        }
        let value = match op {
            BinaryOp::Equal => a == b,
            BinaryOp::NotEqual => a != b,
            BinaryOp::Less => a < b,
            BinaryOp::LessEqual => a <= b,
            BinaryOp::Greater => a > b,
            BinaryOp::GreaterEqual => a >= b,
            _ => return Err(error("unsupported floating-point constant operator")),
        };
        Ok((ValueType::Bool, ConstantData::Bool(value)))
    }

    fn eval_f32_binary<F>(
        &self,
        op: BinaryOp,
        a: f32,
        b: f32,
        error: F,
    ) -> Result<(ValueType, ConstantData), String>
    where
        F: Fn(&str) -> String,
    {
        let value = match op {
            BinaryOp::Add => Some(a + b),
            BinaryOp::Subtract => Some(a - b),
            BinaryOp::Multiply => Some(a * b),
            BinaryOp::Divide => Some(a / b),
            BinaryOp::Remainder => Some(a % b),
            _ => None,
        };
        if let Some(value) = value {
            return Ok((ValueType::F32, ConstantData::F32(value)));
        }
        let value = match op {
            BinaryOp::Equal => a == b,
            BinaryOp::NotEqual => a != b,
            BinaryOp::Less => a < b,
            BinaryOp::LessEqual => a <= b,
            BinaryOp::Greater => a > b,
            BinaryOp::GreaterEqual => a >= b,
            _ => return Err(error("unsupported floating-point constant operator")),
        };
        Ok((ValueType::Bool, ConstantData::Bool(value)))
    }

    fn lower_constant(&mut self, value: &ConstantValue) -> Result<BasicValueEnum<'ctx>, String> {
        Ok(match &value.data {
            ConstantData::I64(value) => self.i64_type().const_int(*value as u64, true).into(),
            ConstantData::F32(value) => self
                .context
                .f32_type()
                .const_float(f64::from(*value))
                .into(),
            ConstantData::F64(value) => self.context.f64_type().const_float(*value).into(),
            ConstantData::Bool(value) => {
                self.bool_type().const_int(u64::from(*value), false).into()
            }
            ConstantData::Str(value) => self.string_global_literal(value)?.into(),
            ConstantData::Array(values) => {
                let ValueType::Array(index) = value.ty else {
                    return Err("array constant type mismatch".into());
                };
                let element_type = self
                    .llvm_element_type(self.array_shapes[index].element)
                    .ok_or("array has no LLVM type")?;
                let elements = values
                    .iter()
                    .map(|value| self.lower_constant(value))
                    .collect::<Result<Vec<_>, _>>()?;
                unsafe { ArrayValue::new_const_array(&element_type, &elements) }.into()
            }
            ConstantData::Tuple(values) => {
                let ValueType::Tuple(index) = value.ty else {
                    return Err("tuple constant type mismatch".into());
                };
                let ty = self
                    .llvm_type(ValueType::Tuple(index))
                    .ok_or("tuple has no LLVM type")?
                    .into_struct_type();
                let elements = values
                    .iter()
                    .map(|value| self.lower_constant(value))
                    .collect::<Result<Vec<_>, _>>()?;
                ty.const_named_struct(&elements).into()
            }
            ConstantData::Struct(values) => {
                let ValueType::Struct(index) = value.ty else {
                    return Err("struct constant type mismatch".into());
                };
                let elements = values
                    .iter()
                    .map(|value| self.lower_constant(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.struct_types[index]
                    .const_named_struct(&elements)
                    .into()
            }
            ConstantData::Enum { variant, payload } => {
                let ValueType::Enum(index) = value.ty else {
                    return Err("enum constant type mismatch".into());
                };
                let mut fields = Vec::with_capacity(self.enums[index].variants.len() + 1);
                fields.push(
                    self.context
                        .i32_type()
                        .const_int(*variant as u64, false)
                        .into(),
                );
                for variant_index in 0..self.enums[index].variants.len() {
                    if variant_index == *variant {
                        let values = payload
                            .iter()
                            .map(|value| self.lower_constant(value))
                            .collect::<Result<Vec<_>, _>>()?;
                        fields.push(
                            self.enum_payload_types[index][variant_index]
                                .const_named_struct(&values)
                                .into(),
                        );
                    } else {
                        fields.push(
                            self.enum_payload_types[index][variant_index]
                                .const_zero()
                                .into(),
                        );
                    }
                }
                self.enum_types[index].const_named_struct(&fields).into()
            }
        })
    }

    fn string_global_literal(&mut self, value: &str) -> Result<PointerValue<'ctx>, String> {
        let symbol = format!("vaca.global.str.{}", self.string_literals.len());
        let ty = self.context.i8_type().array_type(value.len() as u32 + 1);
        let global = self
            .module
            .add_global(ty, Some(AddressSpace::default()), &symbol);
        global.set_linkage(Linkage::Private);
        global.set_constant(true);
        let initializer = self.context.const_string(value.as_bytes(), true);
        global.set_initializer(&initializer);
        let zero = self.i64_type().const_zero();
        let ptr = unsafe {
            global
                .as_pointer_value()
                .const_in_bounds_gep(ty, &[zero, zero])
        };
        self.string_literals.insert(value.to_owned(), ptr);
        Ok(ptr)
    }

    pub(super) fn resolve_global_name(&self, name: &str) -> Option<String> {
        if self.globals.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.globals.contains_key(&qualified).then_some(qualified)
        }
    }

    fn array_element_type(&self, ty: ValueType) -> Option<ArrayElementType> {
        match ty {
            ValueType::I64 => Some(ArrayElementType::I64),
            ValueType::F32 => Some(ArrayElementType::F32),
            ValueType::F64 => Some(ArrayElementType::F64),
            ValueType::Bool => Some(ArrayElementType::Bool),
            ValueType::Str => Some(ArrayElementType::Str),
            ValueType::Array(index) => Some(ArrayElementType::Array(index)),
            ValueType::Vector(index) => Some(ArrayElementType::Vector(index)),
            ValueType::Enum(index) => Some(ArrayElementType::Enum(index)),
            ValueType::Struct(index) => Some(ArrayElementType::Struct(index)),
            ValueType::Tuple(index) => Some(ArrayElementType::Tuple(index)),
            ValueType::Function(index) => Some(ArrayElementType::Function(index)),
            ValueType::Unit => None,
        }
    }
}

fn llvm_global_name(name: &str) -> String {
    format!("vaca.global.{}", name.replace("::", "."))
}

fn constant_values_equal(left: &ConstantValue, right: &ConstantValue) -> bool {
    if left.ty != right.ty {
        return false;
    }
    match (&left.data, &right.data) {
        (ConstantData::I64(a), ConstantData::I64(b)) => a == b,
        (ConstantData::F32(a), ConstantData::F32(b)) => a == b,
        (ConstantData::F64(a), ConstantData::F64(b)) => a == b,
        (ConstantData::Bool(a), ConstantData::Bool(b)) => a == b,
        (ConstantData::Str(a), ConstantData::Str(b)) => a == b,
        (ConstantData::Array(a), ConstantData::Array(b))
        | (ConstantData::Tuple(a), ConstantData::Tuple(b))
        | (ConstantData::Struct(a), ConstantData::Struct(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| constant_values_equal(a, b))
        }
        (
            ConstantData::Enum {
                variant: a,
                payload: ap,
            },
            ConstantData::Enum {
                variant: b,
                payload: bp,
            },
        ) => {
            a == b
                && ap.len() == bp.len()
                && ap.iter().zip(bp).all(|(a, b)| constant_values_equal(a, b))
        }
        _ => false,
    }
}

fn span_of_expr(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Float32(_, span)
        | Expr::Float(_, span)
        | Expr::Bool(_, span)
        | Expr::String(_, span)
        | Expr::Tuple(_, span)
        | Expr::Array(_, span)
        | Expr::Variable(_, span)
        | Expr::Index { span, .. }
        | Expr::Field { span, .. }
        | Expr::Struct { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Call { span, .. }
        | Expr::CallValue { span, .. }
        | Expr::Lambda { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Loop { span, .. }
        | Expr::Variant { span, .. } => *span,
    }
}

trait I64Literal {
    fn checked_into_i64(self) -> Option<i64>;
}

impl I64Literal for u64 {
    fn checked_into_i64(self) -> Option<i64> {
        (self <= i64::MAX as u64).then_some(self as i64)
    }
}
