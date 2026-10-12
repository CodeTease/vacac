use super::*;

impl Checker {
    pub(super) fn resolve_type(&mut self, ty: &Type, span: Span) -> Result<ValueType, Diagnostic> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::F32 => ValueType::F32,
            Type::F64 => ValueType::F64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Array(element, length) => {
                let element = self.resolve_array_element(element, span)?;
                self.array_type(element, *length)
            }
            Type::Vector(element) => {
                let element = self.resolve_array_element(element, span)?;
                self.vector_type(element)
            }
            Type::Named(name) => {
                if let Some(index) = self.enum_ids.get(name) {
                    ValueType::Enum(*index)
                } else if let Some(index) = self.struct_ids.get(name) {
                    ValueType::Struct(*index)
                } else {
                    return Err(at(span, format!("unknown type `{name}`")));
                }
            }
        })
    }

    pub(super) fn type_name(&self, ty: ValueType) -> String {
        match ty {
            ValueType::Array(index) => self.array_shape_name(index),
            ValueType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
            ValueType::Enum(index) => self
                .enums
                .get(index)
                .map(|declaration| declaration.name.clone())
                .unwrap_or_else(|| "<invalid enum>".into()),
            ValueType::Struct(index) => self
                .structs
                .get(index)
                .map(|declaration| declaration.name.clone())
                .unwrap_or_else(|| "<invalid struct>".into()),
            ValueType::I64 => "i64".into(),
            ValueType::F32 => "f32".into(),
            ValueType::F64 => "f64".into(),
            ValueType::Bool => "bool".into(),
            ValueType::Str => "str".into(),
            ValueType::Unit => "unit".into(),
        }
    }

    fn array_element_type_name(&self, element: ArrayElementType) -> String {
        match element {
            ArrayElementType::I64 => "i64".into(),
            ArrayElementType::F32 => "f32".into(),
            ArrayElementType::F64 => "f64".into(),
            ArrayElementType::Bool => "bool".into(),
            ArrayElementType::Str => "str".into(),
            ArrayElementType::Enum(index) => self.enums[index].name.clone(),
            ArrayElementType::Struct(index) => self.structs[index].name.clone(),
            ArrayElementType::Array(index) => self.array_shape_name(index),
            ArrayElementType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
        }
    }

    fn array_shape_name(&self, index: usize) -> String {
        let shape = self.array_shapes[index];
        format!(
            "[{}; {}]",
            self.array_element_type_name(shape.element),
            shape.length
        )
    }

    pub(super) fn array_type(&mut self, element: ArrayElementType, length: u32) -> ValueType {
        let key = (element, length);
        let index = if let Some(index) = self.array_shape_ids.get(&key) {
            *index
        } else {
            let index = self.array_shapes.len();
            self.array_shapes.push(ArrayShape { element, length });
            self.array_shape_ids.insert(key, index);
            index
        };
        ValueType::Array(index)
    }

    pub(super) fn vector_type(&mut self, element: ArrayElementType) -> ValueType {
        let index = if let Some(index) = self.vector_shape_ids.get(&element) {
            *index
        } else {
            let index = self.vector_shapes.len();
            self.vector_shapes.push(VectorShape { element });
            self.vector_shape_ids.insert(element, index);
            index
        };
        ValueType::Vector(index)
    }

    fn resolve_array_element(
        &mut self,
        element: &Type,
        span: Span,
    ) -> Result<ArrayElementType, Diagnostic> {
        Ok(match element {
            Type::I64 => ArrayElementType::I64,
            Type::F32 => ArrayElementType::F32,
            Type::F64 => ArrayElementType::F64,
            Type::Bool => ArrayElementType::Bool,
            Type::Str => ArrayElementType::Str,
            Type::Named(name) => {
                if let Some(index) = self.enum_ids.get(name) {
                    ArrayElementType::Enum(*index)
                } else if let Some(index) = self.struct_ids.get(name) {
                    ArrayElementType::Struct(*index)
                } else {
                    return Err(at(span, format!("unknown type `{name}`")));
                }
            }
            Type::Array(nested_element, length) => {
                let nested_element = self.resolve_array_element(nested_element, span)?;
                let ValueType::Array(index) = self.array_type(nested_element, *length) else {
                    unreachable!()
                };
                ArrayElementType::Array(index)
            }
            Type::Vector(nested_element) => {
                let nested_element = self.resolve_array_element(nested_element, span)?;
                let ValueType::Vector(index) = self.vector_type(nested_element) else {
                    unreachable!()
                };
                ArrayElementType::Vector(index)
            }
            Type::Unit => return Err(at(span, "array elements cannot have type `unit`")),
        })
    }

    pub(super) fn check_block_result(
        &mut self,
        block: &Block,
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        let result = (|| {
            for statement in &block.statements {
                match statement {
                    Stmt::Let { .. }
                    | Stmt::Assign { .. }
                    | Stmt::AssignPlace { .. }
                    | Stmt::Expr(_) => {
                        self.check_statement(statement, ValueType::Unit)?;
                    }
                    _ => {
                        return Err(at(
                            span,
                            "`if` expression branches do not allow control-flow statements",
                        ));
                    }
                }
            }
            let Some(tail) = &block.tail else {
                return Err(at(
                    span,
                    "`if` expression branches must end with a value expression",
                ));
            };
            self.check_expr(tail, false)
        })();
        self.scopes.pop();
        self.immutable_scopes.pop();
        self.loop_scopes.pop();
        result
    }

    pub(super) fn expect_type(
        &self,
        actual: ValueType,
        expected: ValueType,
        span: Span,
        context: &str,
    ) -> Result<(), Diagnostic> {
        if actual == expected {
            Ok(())
        } else {
            Err(at(
                span,
                format!(
                    "{context} has type {}, expected {}",
                    self.type_name(actual),
                    self.type_name(expected)
                ),
            ))
        }
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<ValueType> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub(super) fn is_immutable_binding(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .zip(&self.immutable_scopes)
            .rev()
            .find_map(|(scope, immutable)| {
                scope.contains_key(name).then(|| immutable.contains(name))
            })
            .unwrap_or(false)
    }

    pub(super) fn is_loop_variable(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .zip(&self.loop_scopes)
            .rev()
            .find_map(|(scope, loop_variables)| {
                scope
                    .contains_key(name)
                    .then(|| loop_variables.contains(name))
            })
            .unwrap_or(false)
    }
}

pub(super) fn span_of_expr(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Float32(_, span)
        | Expr::Float(_, span)
        | Expr::Variable(_, span)
        | Expr::Call { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Array(_, span)
        | Expr::Index { span, .. }
        | Expr::Field { span, .. }
        | Expr::Struct { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Loop { span, .. }
        | Expr::Variant { span, .. } => *span,
        Expr::Bool(_, span) | Expr::String(_, span) => *span,
    }
}

pub(super) fn assignment_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name, _) => Some(name),
        Expr::Field { value, .. } => assignment_root(value),
        Expr::Index { array, .. } => assignment_root(array),
        _ => None,
    }
}

pub(super) fn at(span: Span, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::at(span.into(), message.to_string())
}

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::F32 => Self::F32,
            ArrayElementType::F64 => Self::F64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
            ArrayElementType::Enum(index) => Self::Enum(index),
            ArrayElementType::Struct(index) => Self::Struct(index),
            ArrayElementType::Array(index) => Self::Array(index),
            ArrayElementType::Vector(index) => Self::Vector(index),
        }
    }
}
