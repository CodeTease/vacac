use crate::diagnostic::Diagnostic;
use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, GlobalDecl, MatchArm,
    MatchPattern, Program, Span, Stmt, StructDecl, Type, UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Tuple(usize),
    Vector(usize),
    Function(usize),
    Reference(usize),
    MutableReference(usize),
    RawPointer(usize),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FunctionShape {
    params: Vec<ValueType>,
    return_type: ValueType,
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
    mutable_borrows: Vec<std::collections::HashSet<String>>,
    loop_scopes: Vec<std::collections::HashSet<String>>,
    loop_contexts: Vec<LoopContext>,
    current_return_type: ValueType,
    enum_ids: HashMap<String, usize>,
    enums: Vec<EnumDecl>,
    struct_ids: HashMap<String, usize>,
    structs: Vec<StructDecl>,
    tuple_shapes: Vec<Vec<ValueType>>,
    tuple_shape_ids: HashMap<Vec<ValueType>, usize>,
    array_shapes: Vec<ArrayShape>,
    array_shape_ids: HashMap<(ArrayElementType, u32), usize>,
    vector_shapes: Vec<VectorShape>,
    vector_shape_ids: HashMap<ArrayElementType, usize>,
    function_shapes: Vec<FunctionShape>,
    function_shape_ids: HashMap<FunctionShape, usize>,
    reference_types: Vec<ValueType>,
    reference_type_ids: HashMap<ValueType, usize>,
    mutable_reference_types: Vec<ValueType>,
    mutable_reference_type_ids: HashMap<ValueType, usize>,
    raw_pointer_types: Vec<(ValueType, bool)>,
    raw_pointer_type_ids: HashMap<(ValueType, bool), usize>,
    allow_mutable_borrow_initializer: bool,
    lambda_scope_boundaries: Vec<usize>,
    lambda_capture_scopes: Vec<Vec<String>>,
    lambda_captures: HashMap<(Option<usize>, usize, usize), Vec<String>>,
    module_context: String,
    function_visibility: HashMap<String, (String, bool)>,
    global_declarations: HashMap<String, GlobalDecl>,
    global_types: HashMap<String, ValueType>,
    global_states: HashMap<String, u8>,
}

struct LoopContext {
    value_loop: bool,
    result_type: Option<ValueType>,
}

pub fn check(program: &mut Program) -> Result<(), Diagnostic> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        scopes: Vec::new(),
        immutable_scopes: Vec::new(),
        mutable_borrows: Vec::new(),
        loop_scopes: Vec::new(),
        loop_contexts: Vec::new(),
        current_return_type: ValueType::Unit,
        enum_ids: HashMap::new(),
        enums: Vec::new(),
        struct_ids: HashMap::new(),
        structs: Vec::new(),
        tuple_shapes: Vec::new(),
        tuple_shape_ids: HashMap::new(),
        array_shapes: Vec::new(),
        array_shape_ids: HashMap::new(),
        vector_shapes: Vec::new(),
        vector_shape_ids: HashMap::new(),
        function_shapes: Vec::new(),
        function_shape_ids: HashMap::new(),
        reference_types: Vec::new(),
        reference_type_ids: HashMap::new(),
        mutable_reference_types: Vec::new(),
        mutable_reference_type_ids: HashMap::new(),
        raw_pointer_types: Vec::new(),
        raw_pointer_type_ids: HashMap::new(),
        allow_mutable_borrow_initializer: false,
        lambda_scope_boundaries: Vec::new(),
        lambda_capture_scopes: Vec::new(),
        lambda_captures: HashMap::new(),
        module_context: String::new(),
        function_visibility: HashMap::new(),
        global_declarations: HashMap::new(),
        global_types: HashMap::new(),
        global_states: HashMap::new(),
    };
    checker.collect_signatures(program)?;
    for global in &program.globals {
        checker.check_global(&global.name, global.span)?;
    }
    for global in &mut program.globals {
        if global.ty.is_none() {
            let ty = checker.global_types[&global.name];
            global.ty = Some(checker.type_from_value(ty));
        }
    }
    for function in &program.functions {
        checker.module_context = function.module.clone();
        checker.check_function(function)?;
    }
    program.lambda_captures = checker.lambda_captures;
    Ok(())
}

mod declarations;
mod expressions;
mod matching;
mod statements;
mod types;

use types::{assignment_root, at, span_of_expr};
