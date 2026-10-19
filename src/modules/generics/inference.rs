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
            (Type::Reference(pattern, _), Type::Reference(actual, _)) => {
                // Lifetimes are checked on the original function signature by
                // sema. Generic specialization only needs to infer the
                // referenced type; call-site borrows do not carry a named
                // lifetime to unify against.
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::MutableReference(pattern, _), Type::MutableReference(actual, _)) => {
                // Preserve reference mutability while inferring the target;
                // named lifetimes remain a semantic signature contract.
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::RawPointer(pattern, pattern_mut), Type::RawPointer(actual, actual_mut))
                if pattern_mut == actual_mut =>
            {
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
            (Type::Function(patterns, pattern_return), Type::Function(actuals, actual_return))
                if patterns.len() == actuals.len() =>
            {
                for (pattern, actual) in patterns.iter().zip(actuals) {
                    self.unify(pattern, actual, substitutions, span)?;
                }
                self.unify(pattern_return, actual_return, substitutions, span)
            }
            (Type::Tuple(_), Type::Tuple(_)) => Err(Diagnostic::at(
                span.into(),
                "generic tuple argument arities do not match",
            )),
            (Type::Applied(pattern_name, pattern_args), Type::Named(actual_name)) => {
                let instance = self
                    .struct_instances
                    .get(actual_name)
                    .or_else(|| self.enum_instances.get(actual_name));
                let Some((actual_template, actual_args)) = instance else {
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
        self.infer_expr_type_inner(expr, variables, &mut std::collections::HashSet::new())
    }

    fn infer_expr_type_inner(
        &self,
        expr: &Expr,
        variables: &HashMap<String, Type>,
        visiting_globals: &mut std::collections::HashSet<String>,
    ) -> Option<Type> {
        match expr {
            Expr::Integer(..) => Some(Type::I64),
            Expr::Float32(..) => Some(Type::F32),
            Expr::Float(..) => Some(Type::F64),
            Expr::Bool(..) => Some(Type::Bool),
            Expr::String(..) => Some(Type::Str),
            Expr::Variable(name, _) => {
                if let Some(ty) = variables.get(name) {
                    return Some(ty.clone());
                }
                let function_name = if self.signatures.contains_key(name) {
                    Some(name.clone())
                } else {
                    let qualified = if self.module_context.is_empty() {
                        name.clone()
                    } else {
                        format!("{}::{name}", self.module_context)
                    };
                    self.signatures
                        .contains_key(&qualified)
                        .then_some(qualified)
                };
                if let Some((params, result)) =
                    function_name.and_then(|name| self.signatures.get(&name))
                {
                    return Some(Type::Function(params.clone(), Box::new(result.clone())));
                }
                if !visiting_globals.insert(name.clone()) {
                    return None;
                }
                let qualified = if self.module_context.is_empty() {
                    name.clone()
                } else {
                    format!("{}::{name}", self.module_context)
                };
                let global = self
                    .global_declarations
                    .get(name)
                    .or_else(|| self.global_declarations.get(&qualified));
                let Some(global) = global else {
                    visiting_globals.remove(name);
                    return None;
                };
                let ty = global.ty.clone().or_else(|| {
                    self.infer_expr_type_inner(&global.value, variables, visiting_globals)
                });
                visiting_globals.remove(name);
                ty
            }
            Expr::Array(values, _) => {
                let first =
                    self.infer_expr_type_inner(values.first()?, variables, visiting_globals)?;
                values
                    .iter()
                    .all(|value| {
                        self.infer_expr_type_inner(value, variables, visiting_globals)
                            .as_ref()
                            == Some(&first)
                    })
                    .then(|| Type::Array(Box::new(first), values.len() as u32))
            }
            Expr::Tuple(values, _) => Some(Type::Tuple(
                values
                    .iter()
                    .map(|value| self.infer_expr_type_inner(value, variables, visiting_globals))
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
            Expr::Lambda {
                params,
                return_type,
                ..
            } => Some(Type::Function(
                params.iter().map(|param| param.ty.clone()).collect(),
                Box::new(return_type.clone()),
            )),
            Expr::CallValue { callee, .. } => {
                match self.infer_expr_type_inner(callee, variables, visiting_globals)? {
                    Type::Function(_, result) => Some(*result),
                    _ => None,
                }
            }
            Expr::Unary { op, value, .. } => match op {
                UnaryOp::Borrow => Some(Type::Reference(
                    Box::new(self.infer_expr_type_inner(value, variables, visiting_globals)?),
                    None,
                )),
                UnaryOp::BorrowMut => Some(Type::MutableReference(
                    Box::new(self.infer_expr_type_inner(value, variables, visiting_globals)?),
                    None,
                )),
                UnaryOp::RawBorrowConst => Some(Type::RawPointer(
                    Box::new(self.infer_expr_type_inner(value, variables, visiting_globals)?),
                    false,
                )),
                UnaryOp::RawBorrowMut => Some(Type::RawPointer(
                    Box::new(self.infer_expr_type_inner(value, variables, visiting_globals)?),
                    true,
                )),
                UnaryOp::Dereference => {
                    match self.infer_expr_type_inner(value, variables, visiting_globals)? {
                        Type::Reference(element, _) => Some(*element),
                        Type::MutableReference(element, _) | Type::RawPointer(element, _) => {
                            Some(*element)
                        }
                        _ => None,
                    }
                }
                UnaryOp::Negate => self.infer_expr_type_inner(value, variables, visiting_globals),
                UnaryOp::Not => Some(Type::Bool),
            },
            Expr::Binary { op, left, .. } => match op {
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::And
                | BinaryOp::Or => Some(Type::Bool),
                _ => self.infer_expr_type_inner(left, variables, visiting_globals),
            },
            Expr::Index { array, .. } => {
                match self.infer_expr_type_inner(array, variables, visiting_globals)? {
                    Type::Array(element, _) | Type::Vector(element) => Some(*element),
                    _ => None,
                }
            }
            Expr::Field { value, name, .. } => {
                let Type::Named(struct_name) =
                    self.infer_expr_type_inner(value, variables, visiting_globals)?
                else {
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
                .and_then(|value| self.infer_expr_type_inner(value, variables, visiting_globals)),
            Expr::Match { arms, .. } => arms
                .first()
                .and_then(|arm| arm.body.tail.as_ref())
                .and_then(|value| self.infer_expr_type_inner(value, variables, visiting_globals)),
            Expr::Variant { enum_name, .. } => self
                .enum_names
                .contains(enum_name)
                .then(|| Type::Named(enum_name.clone()))
                .or_else(|| {
                    self.enum_instances
                        .contains_key(enum_name)
                        .then(|| Type::Named(enum_name.clone()))
                }),
            Expr::Try { value, .. } => {
                let Type::Named(name) =
                    self.infer_expr_type_inner(value, variables, visiting_globals)?
                else {
                    return None;
                };
                self.enum_declarations
                    .get(&name)?
                    .variants
                    .iter()
                    .find(|variant| variant.name == "Ok")?
                    .payload
                    .first()
                    .cloned()
            }
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
        Type::Function(params, result) => format!(
            "fn({})->{}",
            params.iter().map(type_key).collect::<Vec<_>>().join(","),
            type_key(result)
        ),
        Type::Vector(element) => format!("Vec<{}>", type_key(element)),
        Type::Reference(element, _) => format!("&{}", type_key(element)),
        Type::MutableReference(element, _) => format!("&mut {}", type_key(element)),
        Type::RawPointer(element, mutable) => format!(
            "*{} {}",
            if *mutable { "mut" } else { "const" },
            type_key(element)
        ),
    }
}
