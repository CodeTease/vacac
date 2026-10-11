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

struct LoopTarget<'ctx> {
    continue_target: BasicBlock<'ctx>,
    break_target: BasicBlock<'ctx>,
    value_loop: bool,
    result_type: Option<ValueType>,
    incoming: Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)>,
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
    enum_payload_types: Vec<Vec<StructType<'ctx>>>,
    structs: Vec<StructDecl>,
    struct_ids: HashMap<String, usize>,
    struct_types: Vec<StructType<'ctx>>,
    array_shapes: Vec<ArrayShape>,
    array_shape_ids: HashMap<(ArrayElementType, u32), usize>,
    vector_shapes: Vec<VectorShape>,
    vector_shape_ids: HashMap<ArrayElementType, usize>,
    scopes: Vec<HashMap<String, Local<'ctx>>>,
    string_literals: HashMap<String, PointerValue<'ctx>>,
    loop_targets: Vec<LoopTarget<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
    return_type: ValueType,
    module_context: String,
    function_visibility: HashMap<String, (String, bool)>,
    enum_visibility: HashMap<String, (String, bool)>,
    struct_visibility: HashMap<String, (String, bool)>,
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
mod equality;
mod expressions;
mod loops;
mod operators;
mod output;
mod places;
mod runtime;
mod statements;

impl<'ctx> Codegen<'ctx> {
    fn type_name(&self, ty: ValueType) -> String {
        match ty {
            ValueType::I64 => "i64".into(),
            ValueType::Bool => "bool".into(),
            ValueType::Str => "str".into(),
            ValueType::Unit => "unit".into(),
            ValueType::Array(index) => self.array_shape_name(index),
            ValueType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
            ValueType::Enum(index) => self.enums[index].name.clone(),
            ValueType::Struct(index) => self.structs[index].name.clone(),
        }
    }

    fn array_element_type_name(&self, element: ArrayElementType) -> String {
        match element {
            ArrayElementType::I64 => "i64".into(),
            ArrayElementType::Bool => "bool".into(),
            ArrayElementType::Str => "str".into(),
            ArrayElementType::Enum(index) => self.enums[index].name.clone(),
            ArrayElementType::Struct(index) => self.structs[index].name.clone(),
            ArrayElementType::Array(index) => self.array_shape_name(index),
            ArrayElementType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
        }
    }

    fn array_shape_name(&self, index: usize) -> String {
        let shape = self.array_shapes[index];
        let element = self.array_element_type_name(shape.element);
        format!("[{element}; {}]", shape.length)
    }
}
