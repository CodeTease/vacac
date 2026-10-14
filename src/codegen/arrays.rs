use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn vector_element_size(&self, element: ValueType) -> Result<IntValue<'ctx>, String> {
        let llvm_type = self
            .llvm_type(element)
            .ok_or("vector element has no LLVM type")?;
        let size = llvm_type.size_of().ok_or("vector element has no size")?;
        if size.get_zero_extended_constant() == Some(0) {
            Ok(self.i64_type().const_int(1, false))
        } else {
            Ok(size)
        }
    }

    pub(super) fn vector_element_byte_size(
        &self,
        element: ValueType,
    ) -> Result<IntValue<'ctx>, String> {
        self.llvm_type(element)
            .ok_or("vector element has no LLVM type")?
            .size_of()
            .ok_or_else(|| "vector element has no size".to_owned())
    }

    pub(super) fn emit_expression_expected(
        &mut self,
        expression: &Expr,
        expected: ValueType,
    ) -> Result<TypedValue<'ctx>, String> {
        let needs_context = match (expression, expected) {
            (Expr::Array(elements, _), ValueType::Array(index)) => {
                elements.is_empty()
                    || matches!(
                        self.array_shapes[index].element,
                        ArrayElementType::Array(_)
                            | ArrayElementType::Vector(_)
                            | ArrayElementType::Tuple(_)
                            | ArrayElementType::Function(_)
                    )
            }
            (Expr::Tuple(_, _), ValueType::Tuple(_)) => true,
            (Expr::Call { name, args, .. }, ValueType::Vector(_)) => {
                name == "vec_new" && args.is_empty()
            }
            _ => false,
        };
        if needs_context {
            let span = match expression {
                Expr::Array(_, span) => *span,
                Expr::Tuple(_, span) => *span,
                Expr::Call { span, .. } => *span,
                _ => unreachable!(),
            };
            self.emit_array_literal_with_type(expression, expected, span)
        } else {
            self.emit_expression(expression)
        }
    }

    pub(super) fn emit_array_literal_with_type(
        &mut self,
        expression: &Expr,
        array_type: ValueType,
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if let (Expr::Call { name, args, .. }, ValueType::Vector(vector_id)) =
            (expression, array_type)
            && name == "vec_new"
        {
            if !args.is_empty() {
                return Err(format!("`vec_new` expects 0 arguments, got {}", args.len()));
            }
            let _element_size = self
                .vector_element_byte_size(ValueType::from(self.vector_shapes[vector_id].element))?;
            let function = self
                .module
                .get_function("vaca_vec_new")
                .ok_or("missing vector runtime constructor")?;
            let value = self
                .builder
                .build_call(function, &[], "vec.new")
                .map_err(|error| error.to_string())?
                .try_as_basic_value()
                .basic()
                .ok_or("vector constructor returned void")?;
            return Ok(TypedValue {
                value: Some(value),
                ty: array_type,
            });
        }
        if let (Expr::Tuple(elements, tuple_span), ValueType::Tuple(tuple_id)) =
            (expression, array_type)
        {
            let expected_elements = self.tuple_shapes[tuple_id].clone();
            if elements.len() != expected_elements.len() {
                return Err(format!(
                    "line {}:{}: tuple expression has {} element(s), expected {}",
                    tuple_span.line,
                    tuple_span.column,
                    elements.len(),
                    expected_elements.len()
                ));
            }
            let llvm_type = self.llvm_type(array_type).ok_or("tuple has no LLVM type")?;
            let mut aggregate = llvm_type.const_zero().as_basic_value_enum();
            for (index, (expression, element_type)) in
                elements.iter().zip(expected_elements).enumerate()
            {
                let value = self.emit_expression_expected(expression, element_type)?;
                if value.ty != element_type {
                    return Err(format!(
                        "tuple element has type {}, expected {}",
                        self.type_name(value.ty),
                        self.type_name(element_type)
                    ));
                }
                aggregate = self
                    .builder
                    .build_insert_value(
                        aggregate.into_struct_value(),
                        value.value.ok_or("tuple element has no value")?,
                        index as u32,
                        "tuple.insert",
                    )
                    .map_err(|error| error.to_string())?
                    .as_basic_value_enum();
            }
            return Ok(TypedValue {
                value: Some(aggregate),
                ty: array_type,
            });
        }
        let Expr::Array(elements, _) = expression else {
            return self.emit_expression(expression);
        };
        let ValueType::Array(array_id) = array_type else {
            return self.emit_array_literal(elements, span);
        };
        let shape = self.array_shapes[array_id];
        if elements.is_empty() {
            if shape.length != 0 {
                return Err(format!(
                    "line {}:{}: empty array literal must have array length 0",
                    span.line, span.column
                ));
            }
            return Ok(TypedValue {
                value: Some(
                    self.llvm_type(array_type)
                        .ok_or("array has no LLVM type")?
                        .const_zero(),
                ),
                ty: array_type,
            });
        }
        if elements.len() != shape.length as usize {
            return Err(format!(
                "line {}:{}: array literal has length {}, expected {}",
                span.line,
                span.column,
                elements.len(),
                shape.length
            ));
        }
        let llvm_type = self
            .llvm_array_type(array_id)
            .ok_or("array has no LLVM type")?;
        let mut aggregate = llvm_type.const_zero().as_basic_value_enum();
        for (index, expression) in elements.iter().enumerate() {
            let element_type = ValueType::from(shape.element);
            let value = if matches!(
                shape.element,
                ArrayElementType::Array(_)
                    | ArrayElementType::Vector(_)
                    | ArrayElementType::Tuple(_)
                    | ArrayElementType::Function(_)
            ) {
                self.emit_expression_expected(expression, element_type)?
            } else {
                self.emit_expression(expression)?
            };
            if value.ty != element_type {
                return Err(format!(
                    "array element has type {}, expected {}",
                    self.type_name(value.ty),
                    self.type_name(element_type)
                ));
            }
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_array_value(),
                    value.value.ok_or("array element has no value")?,
                    index as u32,
                    "array.insert",
                )
                .map_err(|error| error.to_string())?
                .as_basic_value_enum();
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: array_type,
        })
    }

    pub(super) fn emit_array_literal(
        &mut self,
        elements: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if elements.is_empty() {
            return Err(format!(
                "line {}:{}: empty array literals require an array type annotation",
                span.line, span.column
            ));
        }
        let Some(first) = elements.first() else {
            return Err(format!(
                "line {}:{}: array literals must contain at least one element",
                span.line, span.column
            ));
        };
        if elements.len() > u32::MAX as usize {
            return Err(format!(
                "line {}:{}: array literal is too large",
                span.line, span.column
            ));
        }
        let first = self.emit_expression(first)?;
        let element = match first.ty {
            ValueType::I64 => ArrayElementType::I64,
            ValueType::F32 => ArrayElementType::F32,
            ValueType::F64 => ArrayElementType::F64,
            ValueType::Bool => ArrayElementType::Bool,
            ValueType::Str => ArrayElementType::Str,
            ValueType::Enum(index) => ArrayElementType::Enum(index),
            ValueType::Struct(index) => ArrayElementType::Struct(index),
            ValueType::Array(index) => ArrayElementType::Array(index),
            ValueType::Vector(index) => ArrayElementType::Vector(index),
            ValueType::Tuple(index) => ArrayElementType::Tuple(index),
            ValueType::Function(index) => ArrayElementType::Function(index),
            ValueType::Reference(_) => {
                return Err("references cannot be stored in arrays or vectors".into());
            }
            ValueType::Unit => return Err("array elements cannot have type `unit`".into()),
        };
        let array_type = self.array_type(element, elements.len() as u32);
        let ValueType::Array(array_id) = array_type else {
            unreachable!()
        };
        let llvm_array_type = self
            .llvm_array_type(array_id)
            .ok_or("array has no LLVM type")?;
        let mut aggregate = llvm_array_type.const_zero().as_basic_value_enum();
        let first_value = first.value.ok_or("array element has no value")?;
        aggregate = self
            .builder
            .build_insert_value(aggregate.into_array_value(), first_value, 0, "array.init")
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        for (index, expression) in elements.iter().enumerate().skip(1) {
            let value = if matches!(
                element,
                ArrayElementType::Array(_)
                    | ArrayElementType::Vector(_)
                    | ArrayElementType::Tuple(_)
                    | ArrayElementType::Function(_)
            ) {
                self.emit_expression_expected(expression, ValueType::from(element))?
            } else {
                self.emit_expression(expression)?
            };
            if value.ty != ValueType::from(element) {
                return Err(format!(
                    "array element has type {}, expected {}",
                    self.type_name(value.ty),
                    self.type_name(ValueType::from(element))
                ));
            }
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_array_value(),
                    value.value.ok_or("array element has no value")?,
                    index as u32,
                    "array.insert",
                )
                .map_err(|e| e.to_string())?
                .as_basic_value_enum();
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: array_type,
        })
    }

    pub(super) fn emit_tuple_literal(
        &mut self,
        elements: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if elements.is_empty() {
            return Err(format!(
                "line {}:{}: tuple literals must contain at least one element",
                span.line, span.column
            ));
        }
        let mut values = Vec::with_capacity(elements.len());
        let mut types = Vec::with_capacity(elements.len());
        for element in elements {
            let value = self.emit_expression(element)?;
            if value.ty == ValueType::Unit {
                return Err(format!(
                    "line {}:{}: tuple elements cannot have type `unit`",
                    span.line, span.column
                ));
            }
            types.push(value.ty);
            values.push(value.value.ok_or("tuple element has no value")?);
        }
        let tuple_type = self.tuple_type(types);
        let llvm_type = self.llvm_type(tuple_type).ok_or("tuple has no LLVM type")?;
        let mut aggregate = llvm_type.const_zero().as_basic_value_enum();
        for (index, value) in values.into_iter().enumerate() {
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_struct_value(),
                    value,
                    index as u32,
                    "tuple.insert",
                )
                .map_err(|error| error.to_string())?
                .as_basic_value_enum();
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: tuple_type,
        })
    }

    pub(super) fn array_element_pointer(
        &self,
        pointer: PointerValue<'ctx>,
        array_type: ValueType,
        index: IntValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, String> {
        let zero = self.i64_type().const_zero();
        let llvm_array_type = self.llvm_type(array_type).ok_or("array has no LLVM type")?;
        unsafe {
            self.builder
                .build_in_bounds_gep(
                    llvm_array_type,
                    pointer,
                    &[zero, index],
                    "array.element.ptr",
                )
                .map_err(|e| e.to_string())
        }
    }

    pub(super) fn emit_bounds_check(
        &mut self,
        index: IntValue<'ctx>,
        length: u32,
    ) -> Result<(), String> {
        let is_negative = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                index,
                self.i64_type().const_zero(),
                "index.negative",
            )
            .map_err(|e| e.to_string())?;
        let too_large = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                index,
                self.i64_type().const_int(u64::from(length), false),
                "index.too.large",
            )
            .map_err(|e| e.to_string())?;
        let invalid = self
            .builder
            .build_or(is_negative, too_large, "index.invalid")
            .map_err(|e| e.to_string())?;
        let function = self
            .current_function
            .ok_or("array index outside function")?;
        let failure = self.context.append_basic_block(function, "bounds.error");
        let valid = self.context.append_basic_block(function, "bounds.valid");
        self.builder
            .build_conditional_branch(invalid, failure, valid)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(failure);
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing runtime exit declaration")?;
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "bounds.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }

    pub(super) fn emit_dynamic_bounds_check(
        &mut self,
        index: IntValue<'ctx>,
        length: IntValue<'ctx>,
    ) -> Result<(), String> {
        let negative = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                index,
                self.i64_type().const_zero(),
                "index.negative",
            )
            .map_err(|error| error.to_string())?;
        let too_large = self
            .builder
            .build_int_compare(IntPredicate::SGE, index, length, "index.too.large")
            .map_err(|error| error.to_string())?;
        let invalid = self
            .builder
            .build_or(negative, too_large, "index.invalid")
            .map_err(|error| error.to_string())?;
        let function = self
            .current_function
            .ok_or("vector index outside function")?;
        let failure = self.context.append_basic_block(function, "bounds.error");
        let valid = self.context.append_basic_block(function, "bounds.valid");
        self.builder
            .build_conditional_branch(invalid, failure, valid)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(failure);
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing runtime exit declaration")?;
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "bounds.exit",
            )
            .map_err(|error| error.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }
}
