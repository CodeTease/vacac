use crate::ast::Program;
use crate::diagnostic::DiagnosticSet;
use std::path::Path;

mod generics;
mod loader;
mod resolve;

pub(crate) fn load(root: &Path) -> Result<Program, DiagnosticSet> {
    let mut program = loader::load(root)?;
    program.modules.clear();
    resolve::resolve_names(&mut program)?;
    generics::monomorphize(&mut program)?;
    program.functions_are_qualified = true;
    Ok(program)
}
