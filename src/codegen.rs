use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, GlobalDecl, MatchPattern, Program,
    Span, Stmt, StructDecl, Type, UnaryOp,
};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{
    ArrayValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue,
    GlobalValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};
use std::collections::HashMap;
use std::path::Path;

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
    scopes: Vec<HashMap<String, Local<'ctx>>>,
    string_literals: HashMap<String, PointerValue<'ctx>>,
    loop_targets: Vec<LoopTarget<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
    return_type: ValueType,
    module_context: String,
    function_visibility: HashMap<String, (String, bool)>,
    enum_visibility: HashMap<String, (String, bool)>,
    struct_visibility: HashMap<String, (String, bool)>,
    global_declarations: HashMap<String, GlobalDecl>,
    global_types: HashMap<String, ValueType>,
    globals: HashMap<String, GlobalValue<'ctx>>,
    global_constant_values: HashMap<String, ConstantValue>,
    global_states: HashMap<String, u8>,
    next_lambda_id: usize,
    lambda_captures: HashMap<(Option<usize>, usize, usize), Vec<String>>,
    lambda_capture_types: HashMap<String, Vec<(String, ValueType)>>,
    function_adapters: HashMap<String, FunctionValue<'ctx>>,
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
mod globals;
mod loops;
mod matching;
mod operators;
mod output;
mod places;
mod runtime;
mod statements;

impl<'ctx> Codegen<'ctx> {
    fn type_name(&self, ty: ValueType) -> String {
        match ty {
            ValueType::I64 => "i64".into(),
            ValueType::F32 => "f32".into(),
            ValueType::F64 => "f64".into(),
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
            ValueType::Tuple(index) => {
                let elements = self.tuple_shapes[index]
                    .iter()
                    .map(|ty| self.type_name(*ty))
                    .collect::<Vec<_>>();
                let trailing_comma = if elements.len() == 1 { "," } else { "" };
                format!("({}{trailing_comma})", elements.join(", "))
            }
            ValueType::Function(index) => {
                let shape = &self.function_shapes[index];
                format!(
                    "fn({}) -> {}",
                    shape
                        .params
                        .iter()
                        .map(|ty| self.type_name(*ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                    self.type_name(shape.return_type)
                )
            }
            ValueType::Reference(index) => {
                format!("&{}", self.type_name(self.reference_types[index]))
            }
        }
    }

    fn array_element_type_name(&self, element: ArrayElementType) -> String {
        match element {
            ArrayElementType::I64 => "i64".into(),
            ArrayElementType::F32 => "f32".into(),
            ArrayElementType::F64 => "f64".into(),
            ArrayElementType::Bool => "bool".into(),
            ArrayElementType::Str => "str".into(),
            ArrayElementType::Enum(index) => self.enums[index].name.clone(),
            ArrayElementType::Struct(index) => self.structs[index].name.clone(),
            ArrayElementType::Array(index) => self.array_shape_name(index),
            ArrayElementType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
            ArrayElementType::Tuple(index) => format!(
                "({})",
                self.tuple_shapes[index]
                    .iter()
                    .map(|ty| self.type_name(*ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ArrayElementType::Function(index) => {
                let shape = &self.function_shapes[index];
                format!(
                    "fn({}) -> {}",
                    shape
                        .params
                        .iter()
                        .map(|ty| self.type_name(*ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                    self.type_name(shape.return_type)
                )
            }
        }
    }

    fn array_shape_name(&self, index: usize) -> String {
        let shape = self.array_shapes[index];
        let element = self.array_element_type_name(shape.element);
        format!("[{element}; {}]", shape.length)
    }
}

#[derive(Clone)]
struct ConstantValue {
    ty: ValueType,
    data: ConstantData,
}

#[derive(Clone)]
enum ConstantData {
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Str(String),
    Array(Vec<ConstantValue>),
    Tuple(Vec<ConstantValue>),
    Struct(Vec<ConstantValue>),
    Enum {
        variant: usize,
        payload: Vec<ConstantValue>,
    },
}
