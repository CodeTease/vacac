use crate::ast::{EnumDecl, Span, StructDecl, Type};
use crate::diagnostic::Diagnostic;
use std::collections::HashMap;

/// Reject aggregate layouts that recursively contain themselves by value.
/// Arrays are also by-value, so their element types participate in the graph.
pub(crate) fn validate_aggregate_cycles(
    enums: &[EnumDecl],
    structs: &[StructDecl],
) -> Result<(), Diagnostic> {
    let mut type_ids = HashMap::new();
    for (index, declaration) in enums.iter().enumerate() {
        type_ids.insert(declaration.name.as_str(), index);
    }
    for (index, declaration) in structs.iter().enumerate() {
        type_ids.insert(declaration.name.as_str(), enums.len() + index);
    }

    fn named_type_id(ty: &Type, type_ids: &HashMap<&str, usize>) -> Option<usize> {
        match ty {
            Type::Named(name) => type_ids.get(name.as_str()).copied(),
            Type::Array(element, _) => named_type_id(element, type_ids),
            Type::Applied(_, arguments) => arguments
                .iter()
                .find_map(|argument| named_type_id(argument, type_ids)),
            // A vector stores its elements behind a heap allocation, so it breaks
            // any otherwise by-value aggregate cycle.
            Type::Vector(_)
            | Type::I64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str
            | Type::Unit => None,
            Type::Parameter(_) => None,
        }
    }

    fn visit(
        id: usize,
        enums: &[EnumDecl],
        structs: &[StructDecl],
        type_ids: &HashMap<&str, usize>,
        states: &mut [u8],
    ) -> Result<(), Diagnostic> {
        states[id] = 1;
        let dependencies: Vec<(usize, Span, String)> = if id < enums.len() {
            let declaration = &enums[id];
            declaration
                .variants
                .iter()
                .flat_map(|variant| {
                    variant.payload.iter().filter_map(|ty| {
                        named_type_id(ty, type_ids).map(|target| {
                            (
                                target,
                                variant.span,
                                format!(
                                    "enum `{}` has a recursive by-value payload in variant `{}`",
                                    declaration.name, variant.name
                                ),
                            )
                        })
                    })
                })
                .collect()
        } else {
            let declaration = &structs[id - enums.len()];
            declaration
                .fields
                .iter()
                .filter_map(|field| {
                    named_type_id(&field.ty, type_ids).map(|target| {
                        (
                            target,
                            field.span,
                            format!(
                                "struct `{}` has a recursive by-value field `{}`",
                                declaration.name, field.name
                            ),
                        )
                    })
                })
                .collect()
        };

        for (target, span, message) in dependencies {
            if states[target] == 1 {
                return Err(Diagnostic::at(span.into(), message));
            }
            if states[target] == 0 {
                visit(target, enums, structs, type_ids, states)?;
            }
        }
        states[id] = 2;
        Ok(())
    }

    let mut states = vec![0; enums.len() + structs.len()];
    for id in 0..states.len() {
        if states[id] == 0 {
            visit(id, enums, structs, &type_ids, &mut states)?;
        }
    }
    Ok(())
}
