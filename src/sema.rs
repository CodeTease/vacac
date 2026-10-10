use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, MatchPattern, Program, Span, Stmt,
    StructDecl, Type, UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    I64,
    Bool,
    Str,
    Unit,
    Array(ArrayElementType, u32),
    Enum(usize),
    Struct(usize),
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
    loop_depth: usize,
    enum_ids: HashMap<String, usize>,
    enums: Vec<EnumDecl>,
    struct_ids: HashMap<String, usize>,
    structs: Vec<StructDecl>,
}

pub fn check(program: &Program) -> Result<(), String> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        scopes: Vec::new(),
        immutable_scopes: Vec::new(),
        loop_scopes: Vec::new(),
        loop_depth: 0,
        enum_ids: HashMap::new(),
        enums: Vec::new(),
        struct_ids: HashMap::new(),
        structs: Vec::new(),
    };
    checker.collect_signatures(program)?;
    for function in &program.functions {
        checker.check_function(function)?;
    }
    Ok(())
}

mod declarations;
mod expressions;
mod statements;
mod types;

use types::{assignment_root, at, span_of_expr};
