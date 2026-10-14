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

    pub(super) fn check_raw_pointer_place(
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
}
