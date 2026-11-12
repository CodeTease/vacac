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

    /// Collect every named type reachable by value from `ty`. All tuple elements
    /// and type arguments must be inspected: a recursive element hidden behind an
    /// earlier acyclic one still makes the layout infinitely sized.
    fn named_type_ids(ty: &Type, type_ids: &HashMap<&str, usize>, out: &mut Vec<usize>) {
        match ty {
            Type::Named(name) => {
                if let Some(id) = type_ids.get(name.as_str()).copied() {
                    out.push(id);
                }
            }
            Type::Array(element, _) => named_type_ids(element, type_ids, out),
            Type::Tuple(elements) => {
                for element in elements {
                    named_type_ids(element, type_ids, out);
                }
            }
            Type::Applied(_, arguments) => {
                for argument in arguments {
                    named_type_ids(argument, type_ids, out);
                }
            }
            // A vector stores its elements behind a heap allocation, so it breaks
            // any otherwise by-value aggregate cycle.
            Type::Vector(_)
            | Type::Reference(_, _)
            | Type::MutableReference(_, _)
            | Type::RawPointer(_, _)
            | Type::Function(_, _)
            | Type::I64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str
            | Type::Unit => {}
            Type::Parameter(_) => {}
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
        let mut dependencies: Vec<(usize, Span, String)> = Vec::new();
        if id < enums.len() {
            let declaration = &enums[id];
            for variant in &declaration.variants {
                for ty in &variant.payload {
                    let mut targets = Vec::new();
                    named_type_ids(ty, type_ids, &mut targets);
                    for target in targets {
                        dependencies.push((
                            target,
                            variant.span,
                            format!(
                                "enum `{}` has a recursive by-value payload in variant `{}`",
                                declaration.name, variant.name
                            ),
                        ));
                    }
                }
            }
        } else {
            let declaration = &structs[id - enums.len()];
            for field in &declaration.fields {
                let mut targets = Vec::new();
                named_type_ids(&field.ty, type_ids, &mut targets);
                for target in targets {
                    dependencies.push((
                        target,
                        field.span,
                        format!(
                            "struct `{}` has a recursive by-value field `{}`",
                            declaration.name, field.name
                        ),
                    ));
                }
            }
        }

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
