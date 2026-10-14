use super::*;
pub(super) fn validate_templates(program: &Program) -> Result<(), Diagnostic> {
    let generic_structs = program
        .structs
        .iter()
        .filter(|declaration| !declaration.generic_params.is_empty())
        .map(|declaration| (declaration.name.as_str(), declaration.generic_params.len()))
        .collect::<HashMap<_, _>>();
    let known_types = program
        .structs
        .iter()
        .map(|declaration| declaration.name.as_str())
        .chain(
            program
                .enums
                .iter()
                .map(|declaration| declaration.name.as_str()),
        )
        .collect::<std::collections::HashSet<_>>();

    fn check_type(
        ty: &Type,
        parameters: &std::collections::HashSet<&str>,
        generic_structs: &HashMap<&str, usize>,
        known_types: &std::collections::HashSet<&str>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match ty {
            Type::Parameter(name) if !parameters.contains(name.as_str()) => Err(Diagnostic::at(
                span.into(),
                format!("unknown generic type parameter `{name}`"),
            )),
            Type::Applied(name, arguments) => {
                let Some(expected) = generic_structs.get(name.as_str()) else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`{name}` is not a generic struct"),
                    ));
                };
                if arguments.len() != *expected {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "generic struct `{name}` expects {expected} type argument(s), got {}",
                            arguments.len()
                        ),
                    ));
                }
                for argument in arguments {
                    check_type(argument, parameters, generic_structs, known_types, span)?;
                }
                Ok(())
            }
            Type::Named(name) if generic_structs.contains_key(name.as_str()) => {
                Err(Diagnostic::at(
                    span.into(),
                    format!("generic struct `{name}` requires explicit type arguments"),
                ))
            }
            Type::Named(name) if !known_types.contains(name.as_str()) => Err(Diagnostic::at(
                span.into(),
                format!("unknown type `{name}`"),
            )),
            Type::Array(element, _) | Type::Vector(element) => {
                check_type(element, parameters, generic_structs, known_types, span)
            }
            Type::Reference(element) => {
                check_type(element, parameters, generic_structs, known_types, span)
            }
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    return Err(Diagnostic::at(
                        span.into(),
                        "tuple types must contain at least one element",
                    ));
                }
                for element in elements {
                    check_type(element, parameters, generic_structs, known_types, span)?;
                }
                Ok(())
            }
            Type::Function(params, result) => {
                for param in params {
                    if *param == Type::Unit {
                        return Err(Diagnostic::at(
                            span.into(),
                            "function type parameters cannot have type `unit`",
                        ));
                    }
                    check_type(param, parameters, generic_structs, known_types, span)?;
                }
                if matches!(result.as_ref(), Type::Reference(_)) {
                    return Err(Diagnostic::at(
                        span.into(),
                        "function types cannot return references",
                    ));
                }
                check_type(result, parameters, generic_structs, known_types, span)
            }
            Type::Parameter(_)
            | Type::Named(_)
            | Type::I64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str
            | Type::Unit => Ok(()),
        }
    }

    for declaration in program
        .structs
        .iter()
        .filter(|declaration| !declaration.generic_params.is_empty())
    {
        let parameters = declaration
            .generic_params
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for field in &declaration.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::at(
                    field.span.into(),
                    format!("struct field `{}` is defined more than once", field.name),
                ));
            }
            if field.ty == Type::Unit {
                return Err(Diagnostic::at(
                    field.span.into(),
                    "struct fields cannot have type `unit`",
                ));
            }
            check_type(
                &field.ty,
                &parameters,
                &generic_structs,
                &known_types,
                field.span,
            )?;
        }
    }
    for function in program
        .functions
        .iter()
        .filter(|function| !function.generic_params.is_empty())
    {
        let parameters = function
            .generic_params
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for param in &function.params {
            if !seen.insert(param.name.as_str()) {
                return Err(Diagnostic::at(
                    param.span.into(),
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            if param.ty == Type::Unit {
                return Err(Diagnostic::at(
                    param.span.into(),
                    "parameters cannot have type `unit`",
                ));
            }
            check_type(
                &param.ty,
                &parameters,
                &generic_structs,
                &known_types,
                param.span,
            )?;
        }
        check_type(
            &function.return_type,
            &parameters,
            &generic_structs,
            &known_types,
            function.span,
        )?;
    }
    Ok(())
}
