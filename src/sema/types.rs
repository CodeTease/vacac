use super::*;

impl Checker {
    pub(super) fn resolve_type(&self, ty: &Type, span: Span) -> Result<ValueType, String> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Array(element, length) => ValueType::Array(*element, *length),
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

    pub(super) fn check_block_result(
        &mut self,
        block: &Block,
        span: Span,
    ) -> Result<ValueType, String> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        let result = (|| {
            for statement in &block.statements {
                match statement {
                    Stmt::Let { .. }
                    | Stmt::Assign { .. }
                    | Stmt::AssignIndex { .. }
                    | Stmt::AssignField { .. }
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
    ) -> Result<(), String> {
        if actual == expected {
            Ok(())
        } else {
            Err(at(
                span,
                format!(
                    "{context} has type {}, expected {}",
                    type_name(actual, &self.enums),
                    type_name(expected, &self.enums)
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
        | Expr::Variant { span, .. } => *span,
        Expr::Bool(_, span) | Expr::String(_, span) => *span,
    }
}

pub(super) fn at(span: Span, message: impl std::fmt::Display) -> String {
    format!(
        "line {}:{}:{}: {message}",
        span.line, span.column, span.end_column
    )
}

pub(super) fn type_name(ty: ValueType, enums: &[EnumDecl]) -> String {
    match ty {
        ValueType::I64 => "i64".into(),
        ValueType::Bool => "bool".into(),
        ValueType::Str => "str".into(),
        ValueType::Unit => "unit".into(),
        ValueType::Array(element, length) => {
            format!("[{}; {length}]", type_name(element.into(), enums))
        }
        ValueType::Enum(index) => enums
            .get(index)
            .map(|declaration| declaration.name.clone())
            .unwrap_or_else(|| "<invalid enum>".into()),
        ValueType::Struct(index) => format!("struct#{index}"),
    }
}

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
        }
    }
}
