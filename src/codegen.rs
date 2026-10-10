use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, MatchPattern, Program, Span, Stmt,
    StructDecl, Type, UnaryOp,
};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};
use std::collections::HashMap;
use std::path::Path;

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

#[derive(Clone, Copy)]
struct Local<'ctx> {
    pointer: PointerValue<'ctx>,
    ty: ValueType,
    immutable: bool,
    loop_variable: bool,
}

struct TypedValue<'ctx> {
    value: Option<BasicValueEnum<'ctx>>,
    ty: ValueType,
}

struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    signatures: HashMap<String, (Vec<ValueType>, ValueType)>,
    enums: Vec<EnumDecl>,
    enum_ids: HashMap<String, usize>,
    enum_types: Vec<StructType<'ctx>>,
    structs: Vec<StructDecl>,
    struct_ids: HashMap<String, usize>,
    struct_types: Vec<StructType<'ctx>>,
    scopes: Vec<HashMap<String, Local<'ctx>>>,
    string_literals: HashMap<String, PointerValue<'ctx>>,
    loop_targets: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    current_function: Option<FunctionValue<'ctx>>,
    return_type: ValueType,
}

pub fn emit_object(program: &Program, path: &Path) -> Result<(), String> {
    let context = Context::create();
    let mut codegen = Codegen::for_program(&context, program)?;
    codegen.write_object(path)
}

pub fn check(program: &Program) -> Result<(), String> {
    let context = Context::create();
    Codegen::for_program(&context, program).map(|_| ())
}

pub fn emit_llvm(program: &Program, path: &Path) -> Result<(), String> {
    let context = Context::create();
    let mut codegen = Codegen::for_program(&context, program)?;
    codegen.write_llvm(path)
}

mod aggregates;
mod arrays;
mod calls;
mod declarations;
mod expressions;
mod operators;
mod output;
mod runtime;
mod statements;

fn type_name(ty: ValueType) -> String {
    match ty {
        ValueType::I64 => "i64".into(),
        ValueType::Bool => "bool".into(),
        ValueType::Str => "str".into(),
        ValueType::Unit => "unit".into(),
        ValueType::Array(element, length) => {
            let element = match element {
                ArrayElementType::I64 => "i64",
                ArrayElementType::Bool => "bool",
                ArrayElementType::Str => "str",
            };
            format!("[{element}; {length}]")
        }
        ValueType::Enum(index) => format!("enum#{index}"),
        ValueType::Struct(index) => format!("struct#{index}"),
    }
}
