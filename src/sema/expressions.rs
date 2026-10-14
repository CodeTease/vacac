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
                    || matches!(
                        self.array_shapes[index].element,
                        ArrayElementType::Array(_)
                            | ArrayElementType::Vector(_)
                            | ArrayElementType::Tuple(_)
                            | ArrayElementType::Function(_)
                    )
            }
            (Expr::Tuple(elements, _), ValueType::Tuple(_)) => !elements.is_empty(),
            (Expr::Call { name, args, .. }, ValueType::Vector(_)) => {
                name == "vec_new" && args.is_empty()
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
        if let (Expr::Tuple(elements, span), ValueType::Tuple(tuple_id)) = (expr, expected) {
            let expected_elements = self.tuple_shapes[tuple_id].clone();
            if elements.len() != expected_elements.len() {
                return Err(at(
                    *span,
                    format!(
                        "tuple expression has {} element(s), expected {}",
                        elements.len(),
                        expected_elements.len()
                    ),
                ));
            }
            for (element, expected_element) in elements.iter().zip(expected_elements) {
                let actual = self.check_expr_expected(element, expected_element)?;
                self.expect_type(
                    actual,
                    expected_element,
                    span_of_expr(element),
                    "tuple element",
                )?;
            }
            return Ok(expected);
        }
        if let (
            Expr::Call {
                name, args, span, ..
            },
            ValueType::Vector(index),
        ) = (expr, expected)
            && name == "vec_new"
        {
            if !args.is_empty() {
                return Err(at(
                    *span,
                    format!(
                        "function `vec_new` expects 0 argument(s), got {}",
                        args.len()
                    ),
                ));
            }
            let _ = index;
            return Ok(expected);
        }
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
            let actual = if matches!(
                shape.element,
                ArrayElementType::Array(_)
                    | ArrayElementType::Vector(_)
                    | ArrayElementType::Tuple(_)
            ) || matches!(element, Expr::Array(children, _) if children.is_empty())
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
            Expr::Unary {
                op: UnaryOp::Dereference,
                value,
                span,
            } => match self.check_expr(value, false)? {
                ValueType::Reference(index) => Ok(self.reference_types[index]),
                ValueType::MutableReference(index) => Ok(self.mutable_reference_types[index]),
                ValueType::RawPointer(index) => Ok(self.raw_pointer_types[index].0),
                other => Err(at(
                    *span,
                    format!("cannot dereference {}", self.type_name(other)),
                )),
            },
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
                if let Expr::Unary {
                    op: UnaryOp::Dereference,
                    ..
                } = array.as_ref()
                {
                    let array_type = self.check_place(array, ty)?;
                    let index_type = self.check_expr(index, false)?;
                    self.expect_type(
                        index_type,
                        ValueType::I64,
                        span_of_expr(index),
                        "array index",
                    )?;
                    return match array_type {
                        ValueType::Array(index) => {
                            Ok(ValueType::from(self.array_shapes[index].element))
                        }
                        ValueType::Vector(index) => {
                            Ok(ValueType::from(self.vector_shapes[index].element))
                        }
                        _ => Err(at(*span, "indexing requires an array (or vector)")),
                    };
                }
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
                    ValueType::Vector(index) => {
                        Ok(ValueType::from(self.vector_shapes[index].element))
                    }
                    _ => Err(at(*span, "indexing requires an array (or vector)")),
                }
            }
            _ => Err(at(
                span_of_expr(expr),
                "assignment target must be a local variable, field, or array element",
            )),
        }
    }

    fn check_raw_pointer_place(
        &mut self,
        expr: &Expr,
        span: Span,
        mutable: bool,
    ) -> Result<ValueType, Diagnostic> {
        if !matches!(expr, Expr::Variable(_, _)) {
            return Err(at(
                span,
                "raw address-taking currently requires a local or global variable",
            ));
        }
        let root = assignment_root(expr).ok_or_else(|| {
            at(
                span,
                "raw address-taking requires a variable, field, or array element",
            )
        })?;
        let root_type = if let Some(ty) = self.lookup_local(root) {
            self.reject_mutably_borrowed(root, span)?;
            if mutable && self.is_immutable_binding(root) {
                return Err(at(
                    span,
                    "cannot take a mutable raw pointer to an immutable binding",
                ));
            }
            ty
        } else if let Some(global) = self.find_global_name(root) {
            let declaration = self.global_declarations[&global].clone();
            if declaration.module != self.module_context && !declaration.is_public {
                return Err(at(span, format!("global `{root}` is private")));
            }
            if mutable && !declaration.mutable {
                return Err(at(
                    span,
                    "cannot take a mutable raw pointer to an immutable global",
                ));
            }
            self.check_global(&global, span)?
        } else {
            return Err(at(span, format!("undefined variable `{root}`")));
        };
        if matches!(expr, Expr::Variable(_, _)) {
            return Ok(root_type);
        }
        self.check_place(expr, root_type)
    }

    pub(super) fn place_crosses_vector(
        &mut self,
        expr: &Expr,
        ty: ValueType,
    ) -> Result<bool, Diagnostic> {
        match expr {
            Expr::Variable(_, _) => Ok(false),
            Expr::Field { value, .. } => self.place_crosses_vector(value, ty),
            Expr::Index { array, .. } => {
                let base_type = self.check_place(array, ty)?;
                match base_type {
                    ValueType::Vector(_) => Ok(true),
                    ValueType::Array(_) => self.place_crosses_vector(array, ty),
                    _ => Ok(false),
                }
            }
            _ => Ok(false),
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
            Expr::Float32(_, _) => Ok(ValueType::F32),
            Expr::Float(_, _) => Ok(ValueType::F64),
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
                    ValueType::RawPointer(index) => ArrayElementType::RawPointer(index),
                    ValueType::Reference(_) | ValueType::MutableReference(_) => {
                        return Err(at(
                            *span,
                            "references cannot be stored in arrays or vectors",
                        ));
                    }
                    ValueType::Unit => {
                        return Err(at(*span, "array elements cannot have type `unit`"));
                    }
                };
                for value in &elements[1..] {
                    let value_type = if matches!(
                        element,
                        ArrayElementType::Array(_)
                            | ArrayElementType::Vector(_)
                            | ArrayElementType::Tuple(_)
                            | ArrayElementType::Function(_)
                    ) {
                        self.check_expr_expected(value, first_type)?
                    } else {
                        self.check_expr(value, false)?
                    };
                    self.expect_type(value_type, first_type, span_of_expr(value), "array element")?;
                }
                Ok(self.array_type(element, elements.len() as u32))
            }
            Expr::Tuple(elements, span) => {
                let mut types = Vec::with_capacity(elements.len());
                for element in elements {
                    let ty = self.check_expr(element, false)?;
                    if ty == ValueType::Unit {
                        return Err(at(*span, "tuple elements cannot have type `unit`"));
                    }
                    if matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_)) {
                        return Err(at(*span, "references cannot be stored in tuples"));
                    }
                    types.push(ty);
                }
                Ok(self.tuple_type(types))
            }
            Expr::Variable(name, span) => {
                self.reject_mutably_borrowed(name, *span)?;
                if let Some((scope_index, ty)) = self.lookup_local_with_scope(name) {
                    if matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_))
                        && self
                            .lambda_scope_boundaries
                            .last()
                            .is_some_and(|boundary| scope_index < *boundary)
                    {
                        return Err(at(*span, "closures cannot capture references"));
                    }
                    self.record_lambda_capture(name, scope_index);
                    Ok(ty)
                } else if let Some(global) = self.find_global_name(name) {
                    let declaration = self.global_declarations[&global].clone();
                    if declaration.module != self.module_context && !declaration.is_public {
                        return Err(at(*span, format!("global `{name}` is private")));
                    }
                    self.check_global(&global, *span)
                } else if let Some(function_name) = self.find_function_name(name)
                    && let Some(signature) = self.signatures.get(&function_name).cloned()
                {
                    if self
                        .function_visibility
                        .get(&function_name)
                        .is_some_and(|(module, public)| module != &self.module_context && !public)
                    {
                        return Err(at(*span, format!("function `{name}` is private")));
                    }
                    Ok(self.function_type(signature.params, signature.return_type))
                } else {
                    Err(at(*span, format!("undefined variable `{name}`")))
                }
            }
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
                    ValueType::Vector(index) => {
                        Ok(ValueType::from(self.vector_shapes[index].element))
                    }
                    _ => Err(at(*span, "indexing requires an array (or vector)")),
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
            Expr::Struct {
                name, fields, span, ..
            } => {
                let struct_id = self
                    .struct_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| at(*span, format!("unknown struct `{name}`")))?;
                if self.structs[struct_id].module != self.module_context
                    && !self.structs[struct_id].is_public
                {
                    return Err(at(*span, format!("struct `{name}` is private")));
                }
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
                if matches!(op, UnaryOp::Borrow) {
                    let Expr::Variable(name, _) = value.as_ref() else {
                        return Err(at(
                            *span,
                            "borrowing currently requires a named local binding",
                        ));
                    };
                    let Some((scope_index, target_type)) = self.lookup_local_with_scope(name)
                    else {
                        return Err(at(*span, "only local bindings can be borrowed"));
                    };
                    if !self.is_immutable_binding(name) {
                        return Err(at(*span, "only immutable local bindings can be borrowed"));
                    }
                    if self
                        .lambda_scope_boundaries
                        .last()
                        .is_some_and(|boundary| scope_index < *boundary)
                    {
                        return Err(at(*span, "closures cannot capture references"));
                    }
                    if matches!(
                        target_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(at(*span, "references to references are not supported"));
                    }
                    if !self.reference_target_is_supported(
                        target_type,
                        &mut std::collections::HashSet::new(),
                    )? {
                        return Err(at(
                            *span,
                            "references to values containing vectors are not supported",
                        ));
                    }
                    self.record_lambda_capture(name, scope_index);
                    return Ok(self.reference_type(target_type));
                }
                if matches!(op, UnaryOp::BorrowMut) {
                    if !self.allow_mutable_borrow_initializer {
                        return Err(at(
                            *span,
                            "mutable borrows must initialize a local reference binding",
                        ));
                    }
                    let Expr::Variable(name, _) = value.as_ref() else {
                        return Err(at(
                            *span,
                            "mutable borrowing currently requires a named local binding",
                        ));
                    };
                    let Some((scope_index, target_type)) = self.lookup_local_with_scope(name)
                    else {
                        return Err(at(*span, "only local bindings can be borrowed"));
                    };
                    if self.is_immutable_binding(name) || self.is_loop_variable(name) {
                        return Err(at(
                            *span,
                            "mutable borrowing requires a mutable local binding",
                        ));
                    }
                    if self.is_mutably_borrowed(name) {
                        return Err(at(*span, format!("`{name}` is already mutably borrowed")));
                    }
                    if self
                        .lambda_scope_boundaries
                        .last()
                        .is_some_and(|boundary| scope_index < *boundary)
                    {
                        return Err(at(*span, "closures cannot capture mutable references"));
                    }
                    if matches!(
                        target_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(at(*span, "references to references are not supported"));
                    }
                    if !self.reference_target_is_supported(
                        target_type,
                        &mut std::collections::HashSet::new(),
                    )? {
                        return Err(at(
                            *span,
                            "references to values containing vectors are not supported",
                        ));
                    }
                    return Ok(self.mutable_reference_type(target_type));
                }
                if matches!(op, UnaryOp::RawBorrowConst | UnaryOp::RawBorrowMut) {
                    let place_type = self.check_raw_pointer_place(
                        value,
                        *span,
                        matches!(op, UnaryOp::RawBorrowMut),
                    )?;
                    return Ok(
                        self.raw_pointer_type(place_type, matches!(op, UnaryOp::RawBorrowMut))
                    );
                }
                if matches!(op, UnaryOp::Dereference) {
                    return match self.check_expr(value, false)? {
                        ValueType::Reference(index) => Ok(self.reference_types[index]),
                        ValueType::MutableReference(index) => {
                            Ok(self.mutable_reference_types[index])
                        }
                        ValueType::RawPointer(index) => Ok(self.raw_pointer_types[index].0),
                        other => Err(at(
                            *span,
                            format!(
                                "unary `*` requires a reference or raw pointer, got {}",
                                self.type_name(other)
                            ),
                        )),
                    };
                }
                let value_type = self.check_expr(
                    value,
                    matches!(op, UnaryOp::Negate) && matches!(value.as_ref(), Expr::Integer(_, _)),
                )?;
                let expected = match op {
                    UnaryOp::Negate if value_type == ValueType::F32 => ValueType::F32,
                    UnaryOp::Negate if value_type == ValueType::F64 => ValueType::F64,
                    UnaryOp::Negate => ValueType::I64,
                    UnaryOp::Not => ValueType::Bool,
                    UnaryOp::Borrow
                    | UnaryOp::BorrowMut
                    | UnaryOp::RawBorrowConst
                    | UnaryOp::RawBorrowMut
                    | UnaryOp::Dereference => unreachable!(),
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
                        if matches!(left_type, ValueType::F32 | ValueType::F64) {
                            return Ok(left_type);
                        }
                        self.expect_type(left_type, ValueType::I64, *span, "arithmetic operands")?;
                        ValueType::I64
                    }
                    BinaryOp::BitAnd | BinaryOp::BitXor | BinaryOp::BitOr => {
                        self.expect_type(left_type, ValueType::I64, *span, "bitwise operands")?;
                        ValueType::I64
                    }
                    BinaryOp::ShiftLeft | BinaryOp::ShiftRight => {
                        self.expect_type(left_type, ValueType::I64, *span, "shift operands")?;
                        ValueType::I64
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == ValueType::Unit {
                            return Err(at(*span, "void expressions cannot be compared"));
                        }
                        if matches!(
                            left_type,
                            ValueType::Reference(_)
                                | ValueType::MutableReference(_)
                                | ValueType::RawPointer(_)
                        ) {
                            return Err(at(
                                *span,
                                "references and raw pointers cannot be compared",
                            ));
                        }
                        if self.contains_raw_pointer(
                            left_type,
                            &mut std::collections::HashSet::new(),
                        )? {
                            return Err(at(
                                *span,
                                "values containing raw pointers cannot be compared",
                            ));
                        }
                        ValueType::Bool
                    }
                    BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => {
                        if matches!(left_type, ValueType::F32 | ValueType::F64) {
                            return Ok(ValueType::Bool);
                        }
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
            Expr::Call {
                name, args, span, ..
            } => {
                if name == "vec_new" {
                    if !args.is_empty() {
                        return Err(at(
                            *span,
                            format!(
                                "function `vec_new` expects 0 argument(s), got {}",
                                args.len()
                            ),
                        ));
                    }
                    return Err(at(*span, "`vec_new()` requires a `Vec<T>` type annotation"));
                }
                if name == "vec_push" {
                    if args.len() != 2 {
                        return Err(at(
                            *span,
                            format!(
                                "function `vec_push` expects 2 argument(s), got {}",
                                args.len()
                            ),
                        ));
                    }
                    let vector_type = self.check_expr(&args[0], false)?;
                    let ValueType::Vector(index) = vector_type else {
                        return Err(at(
                            span_of_expr(&args[0]),
                            "argument 1 to `vec_push` must be a vector",
                        ));
                    };
                    let element_type = ValueType::from(self.vector_shapes[index].element);
                    let value_type = self.check_expr_expected(&args[1], element_type)?;
                    self.expect_type(
                        value_type,
                        element_type,
                        span_of_expr(&args[1]),
                        "vector element",
                    )?;
                    return Ok(ValueType::Unit);
                }
                if name == "len" {
                    if args.len() != 1 {
                        return Err(at(
                            *span,
                            format!("function `len` expects 1 argument(s), got {}", args.len()),
                        ));
                    }
                    let array_type = self.check_expr(&args[0], false)?;
                    if !matches!(array_type, ValueType::Array(_) | ValueType::Vector(_)) {
                        return Err(at(
                            span_of_expr(&args[0]),
                            "argument 1 to `len` must be an array (or vector)",
                        ));
                    }
                    return Ok(ValueType::I64);
                }
                let local_callee = self.lookup_local(name).is_some();
                if !local_callee
                    && self.find_global_name(name).is_none()
                    && self.find_function_name(name).is_none()
                {
                    return Err(at(*span, format!("unknown function `{name}`")));
                }
                let Some(signature) = (!local_callee)
                    .then(|| self.find_function_name(name))
                    .flatten()
                    .and_then(|name| self.signatures.get(&name).cloned())
                else {
                    let callee = self.check_expr(&Expr::Variable(name.clone(), *span), false)?;
                    let ValueType::Function(function_id) = callee else {
                        return Err(at(*span, format!("unknown function `{name}`")));
                    };
                    return self.check_function_value_call(function_id, args, *span);
                };
                let function_name = self
                    .find_function_name(name)
                    .unwrap_or_else(|| name.clone());
                if let Some((module, is_public)) = self.function_visibility.get(&function_name)
                    && module != &self.module_context
                    && !is_public
                {
                    return Err(at(*span, format!("function `{name}` is private")));
                }
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
                let params = signature.params;
                let return_type = signature.return_type;
                for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
                    if let Expr::Unary {
                        op: UnaryOp::BorrowMut,
                        value,
                        ..
                    } = arg
                        && let Expr::Variable(source, source_span) = value.as_ref()
                    {
                        if self.is_mutably_borrowed(source) {
                            return Err(at(
                                *source_span,
                                format!("`{source}` is already mutably borrowed"),
                            ));
                        }
                    }
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
            Expr::CallValue { callee, args, span } => {
                let callee_type = self.check_expr(callee, false)?;
                let ValueType::Function(function_id) = callee_type else {
                    return Err(at(*span, "called expression must have a function type"));
                };
                self.check_function_value_call(function_id, args, *span)
            }
            Expr::Lambda {
                params,
                return_type,
                body,
                span,
            } => {
                let mut param_types = Vec::with_capacity(params.len());
                let mut seen = std::collections::HashSet::new();
                for param in params {
                    if !seen.insert(param.name.as_str()) {
                        return Err(at(
                            param.span,
                            format!("duplicate lambda parameter `{}`", param.name),
                        ));
                    }
                    let ty = self.resolve_type(&param.ty, param.span)?;
                    if ty == ValueType::Unit {
                        return Err(at(param.span, "parameters cannot have type `unit`"));
                    }
                    param_types.push(ty);
                }
                let return_type = self.resolve_type(return_type, *span)?;
                if matches!(
                    return_type,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(*span, "functions and lambdas cannot return references"));
                }
                let boundary = self.scopes.len();
                self.lambda_scope_boundaries.push(boundary);
                self.lambda_capture_scopes.push(Vec::new());
                self.scopes.push(HashMap::new());
                self.immutable_scopes.push(std::collections::HashSet::new());
                self.loop_scopes.push(std::collections::HashSet::new());
                self.mutable_borrows.push(std::collections::HashSet::new());
                for (param, ty) in params.iter().zip(param_types.iter()) {
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(param.name.clone(), *ty);
                    if matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_)) {
                        self.immutable_scopes
                            .last_mut()
                            .unwrap()
                            .insert(param.name.clone());
                    }
                }
                let previous_return_type = self.current_return_type;
                let previous_loops = std::mem::take(&mut self.loop_contexts);
                self.current_return_type = return_type;
                let checked = self.check_block(body, return_type, true);
                self.current_return_type = previous_return_type;
                self.loop_contexts = previous_loops;
                self.scopes.pop();
                self.immutable_scopes.pop();
                self.loop_scopes.pop();
                self.mutable_borrows.pop();
                self.lambda_scope_boundaries.pop();
                let captures = self.lambda_capture_scopes.pop().unwrap_or_default();
                self.lambda_captures
                    .insert((span.file, span.line, span.column), captures);
                let returns = checked?;
                if return_type != ValueType::Unit && !returns {
                    return Err(at(
                        *span,
                        "lambda can reach its end without returning a value",
                    ));
                }
                Ok(self.function_type(param_types, return_type))
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
                if matches!(
                    then_type,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(
                        *span,
                        "references cannot escape through `if` expressions",
                    ));
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
                if self.enums[enum_id].module != self.module_context
                    && !self.enums[enum_id].is_public
                {
                    return Err(at(*span, format!("enum `{enum_name}` is private")));
                }
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
            Expr::Match { value, arms, span } => self.check_match_expr(value, arms, *span),
            Expr::Loop { body, span } => {
                self.loop_contexts.push(LoopContext {
                    value_loop: true,
                    result_type: None,
                });
                let body_result = self.check_block(body, self.current_return_type, false);
                let context = self.loop_contexts.pop().expect("loop context exists");
                body_result?;
                context.result_type.ok_or_else(|| {
                    at(
                        *span,
                        "value-producing loop must contain a `break` with a value",
                    )
                })
            }
        }
    }

    fn check_function_value_call(
        &mut self,
        function_id: usize,
        args: &[Expr],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let shape = self.function_shapes[function_id].clone();
        if args.len() != shape.params.len() {
            return Err(at(
                span,
                format!(
                    "function value expects {} argument(s), got {}",
                    shape.params.len(),
                    args.len()
                ),
            ));
        }
        for (index, (arg, expected)) in args.iter().zip(shape.params).enumerate() {
            let actual = self.check_expr_expected(arg, expected)?;
            self.expect_type(
                actual,
                expected,
                span_of_expr(arg),
                &format!("argument {} to function value", index + 1),
            )?;
        }
        Ok(shape.return_type)
    }
}
