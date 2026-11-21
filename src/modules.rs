use crate::ast::Program;
use crate::diagnostic::{Diagnostic, DiagnosticSet};
use std::path::Path;

mod generics;
mod loader;
mod resolve;
mod traits;

pub(crate) fn load(root: &Path) -> Result<Program, DiagnosticSet> {
    let mut program = loader::load(root)?;
    program.modules.clear();
    resolve::resolve_names(&mut program).map_err(|error| with_source(&program, error))?;
    let traits = traits::prepare(&mut program).map_err(|error| with_source(&program, error))?;
    generics::monomorphize(&mut program, &traits).map_err(|error| with_source(&program, error))?;
    program.functions_are_qualified = true;
    Ok(program)
}

/// Attach the originating file and source excerpt to diagnostics that were
/// created with a span but never mapped back through `program.source_files`.
fn with_source(program: &Program, error: Diagnostic) -> Diagnostic {
    let source_id = error
        .source_id
        .or_else(|| error.span.and_then(|span| span.file));
    match source_id.and_then(|source_id| {
        program
            .source_files
            .get(source_id)
            .map(|file| (source_id, file, &program.source_texts[source_id]))
    }) {
        Some((source_id, file, text)) => error.with_source_id(source_id, file.clone(), text),
        None => error,
    }
}
