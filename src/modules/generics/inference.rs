use super::*;

impl Specializer {
    pub(super) fn unify(
        &self,
        pattern: &Type,
        actual: &Type,
        substitutions: &mut Substitution,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match (pattern, actual) {
            (Type::Parameter(name), actual) => {
                if let Some(previous) = substitutions.get(name) {
                    if previous != actual {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!(
                                "generic type parameter `{name}` is inferred as both `{}` and `{}`",
                                type_key(previous),
                                type_key(actual)
                            ),
                        ));
                    }
                } else {
                    substitutions.insert(name.clone(), actual.clone());
                }
                Ok(())
            }
            (Type::Vector(pattern), Type::Vector(actual)) => {
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::Array(pattern, pattern_length), Type::Array(actual, actual_length)) => {
                if pattern_length != actual_length {
                    return Err(Diagnostic::at(
                        span.into(),
                        "generic argument array lengths do not match",
                    ));
                }
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::Tuple(patterns), Type::Tuple(actuals)) if patterns.len() == actuals.len() => {
                for (pattern, actual) in patterns.iter().zip(actuals) {
                    self.unify(pattern, actual, substitutions, span)?;
                }
                Ok(())
            }
            (Type::Tuple(_), Type::Tuple(_)) => Err(Diagnostic::at(
                span.into(),
                "generic tuple argument arities do not match",
            )),
            (Type::Applied(pattern_name, pattern_args), Type::Named(actual_name)) => {
                let Some((actual_template, actual_args)) = self.struct_instances.get(actual_name)
                else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                };
                if pattern_name != actual_template || pattern_args.len() != actual_args.len() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                }
                for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args) {
                    self.unify(pattern_arg, actual_arg, substitutions, span)?;
                }
                Ok(())
            }
            (
                Type::Applied(pattern_name, pattern_args),
                Type::Applied(actual_name, actual_args),
            ) => {
                if pattern_name != actual_name || pattern_args.len() != actual_args.len() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                }
                for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args) {
                    self.unify(pattern_arg, actual_arg, substitutions, span)?;
                }
                Ok(())
            }
            (Type::Named(pattern), Type::Named(actual)) if pattern == actual => Ok(()),
            (Type::I64, Type::I64)
            | (Type::F32, Type::F32)
            | (Type::F64, Type::F64)
            | (Type::Bool, Type::Bool)
            | (Type::Str, Type::Str)
            | (Type::Unit, Type::Unit) => Ok(()),
            _ if pattern == actual => Ok(()),
            _ => Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic argument has type `{}`, expected `{}`",
                    type_key(actual),
                    type_key(pattern)
                ),
            )),
        }
    }

    pub(super) fn infer_expr_type(
        &self,
        expr: &Expr,
        variables: &HashMap<String, Type>,
    ) -> Option<Type> {
        match expr {
            Expr::Integer(..) => Some(Type::I64),
            Expr::Float32(..) => Some(Type::F32),
            Expr::Float(..) => Some(Type::F64),
            Expr::Bool(..) => Some(Type::Bool),
            Expr::String(..) => Some(Type::Str),
            Expr::Variable(name, _) => variables.get(name).cloned(),
            Expr::Array(values, _) => {
                let first = self.infer_expr_type(values.first()?, variables)?;
                values
                    .iter()
                    .all(|value| self.infer_expr_type(value, variables).as_ref() == Some(&first))
                    .then(|| Type::Array(Box::new(first), values.len() as u32))
            }
            Expr::Tuple(values, _) => Some(Type::Tuple(
                values
                    .iter()
                    .map(|value| self.infer_expr_type(value, variables))
                    .collect::<Option<Vec<_>>>()?,
            )),
            Expr::Struct {
                name,
                type_arguments,
                ..
            } => {
                if type_arguments.is_empty() {
                    Some(Type::Named(name.clone()))
                } else {
                    let concrete_name = format!(
                        "{name}::<{}>",
                        type_arguments
                            .iter()
                            .map(type_key)
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    Some(Type::Named(concrete_name))
                }
            }
            Expr::Call { name, .. } => self.signatures.get(name).map(|(_, result)| result.clone()),
            Expr::Unary { value, .. } => self.infer_expr_type(value, variables),
            Expr::Binary { op, left, .. } => match op {
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::And
                | BinaryOp::Or => Some(Type::Bool),
                _ => self.infer_expr_type(left, variables),
            },
            Expr::Index { array, .. } => match self.infer_expr_type(array, variables)? {
                Type::Array(element, _) | Type::Vector(element) => Some(*element),
                _ => None,
            },
            Expr::Field { value, name, .. } => {
                let Type::Named(struct_name) = self.infer_expr_type(value, variables)? else {
                    return None;
                };
                self.concrete_structs
                    .get(&struct_name)
                    .or_else(|| {
                        self.generated_structs
                            .iter()
                            .find(|declaration| declaration.name == struct_name)
                    })?
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .map(|field| field.ty.clone())
            }
            Expr::If { then_block, .. } => then_block
                .tail
                .as_ref()
                .and_then(|value| self.infer_expr_type(value, variables)),
            Expr::Match { arms, .. } => arms
                .first()
                .and_then(|arm| arm.body.tail.as_ref())
                .and_then(|value| self.infer_expr_type(value, variables)),
            Expr::Variant { enum_name, .. } => self
                .enum_names
                .contains(enum_name)
                .then(|| Type::Named(enum_name.clone())),
            Expr::Loop { .. } => None,
        }
    }
}

pub(super) fn type_key(ty: &Type) -> String {
    match ty {
        Type::I64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Unit => "unit".into(),
        Type::Named(name) | Type::Parameter(name) => name.clone(),
        Type::Applied(name, arguments) => format!(
            "{name}<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Type::Array(element, length) => format!("[{};{length}]", type_key(element)),
        Type::Tuple(elements) => {
            let trailing_comma = if elements.len() == 1 { "," } else { "" };
            format!(
                "({}{trailing_comma})",
                elements.iter().map(type_key).collect::<Vec<_>>().join(",")
            )
        }
        Type::Vector(element) => format!("Vec<{}>", type_key(element)),
    }
}
