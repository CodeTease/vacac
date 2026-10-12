use crate::diagnostic::Diagnostic;
use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, MatchArm, MatchPattern, Program,
    Span, Stmt, StructDecl, Type, UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    I64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    Array(usize),
    Enum(usize),
    Struct(usize),
    Vector(usize),
}

#[derive(Clone, Copy)]
struct ArrayShape {
    element: ArrayElementType,
    length: u32,
}

#[derive(Clone, Copy)]
struct VectorShape {
    element: ArrayElementType,
}

#[derive(Clone)]
struct Signature {
    params: Vec<ValueType>,
    return_type: ValueType,
}

struct Checker {
    signatures: HashMap<String, Signature>,
    scopes: Vec<HashMap<String, ValueType>>,
    immutable_scopes: Vec<std::collections::HashSet<String>>,
    loop_scopes: Vec<std::collections::HashSet<String>>,
    loop_contexts: Vec<LoopContext>,
    current_return_type: ValueType,
    enum_ids: HashMap<String, usize>,
    enums: Vec<EnumDecl>,
    struct_ids: HashMap<String, usize>,
    structs: Vec<StructDecl>,
    array_shapes: Vec<ArrayShape>,
    array_shape_ids: HashMap<(ArrayElementType, u32), usize>,
    vector_shapes: Vec<VectorShape>,
    vector_shape_ids: HashMap<ArrayElementType, usize>,
    module_context: String,
    function_visibility: HashMap<String, (String, bool)>,
}

struct LoopContext {
    value_loop: bool,
    result_type: Option<ValueType>,
}

pub fn check(program: &Program) -> Result<(), Diagnostic> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        scopes: Vec::new(),
        immutable_scopes: Vec::new(),
        loop_scopes: Vec::new(),
        loop_contexts: Vec::new(),
        current_return_type: ValueType::Unit,
        enum_ids: HashMap::new(),
        enums: Vec::new(),
        struct_ids: HashMap::new(),
        structs: Vec::new(),
        array_shapes: Vec::new(),
        array_shape_ids: HashMap::new(),
        vector_shapes: Vec::new(),
        vector_shape_ids: HashMap::new(),
        module_context: String::new(),
        function_visibility: HashMap::new(),
    };
    checker.collect_signatures(program)?;
    for function in &program.functions {
        checker.module_context = function.module.clone();
        checker.check_function(function)?;
    }
    Ok(())
}

mod declarations;
mod expressions;
mod matching;
mod statements;
mod types;

use types::{assignment_root, at, span_of_expr};
