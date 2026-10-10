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

impl<'ctx> Codegen<'ctx> {
    fn for_program(context: &'ctx Context, program: &Program) -> Result<Self, String> {
        let mut codegen = Self::new(context, "apex");
        codegen.declare_enums(program)?;
        codegen.declare_structs(program)?;
        codegen.declare_functions(program)?;
        codegen.emit_print_builtins()?;
        codegen.emit_read_i64_builtin()?;
        for function in &program.functions {
            codegen.emit_function(function)?;
        }
        codegen.emit_c_main_wrapper()?;
        codegen
            .module
            .verify()
            .map_err(|e| format!("invalid LLVM module: {e}"))?;
        Ok(codegen)
    }

    fn new(context: &'ctx Context, name: &str) -> Self {
        Self {
            context,
            module: context.create_module(name),
            builder: context.create_builder(),
            functions: HashMap::new(),
            signatures: HashMap::new(),
            enums: Vec::new(),
            enum_ids: HashMap::new(),
            enum_types: Vec::new(),
            structs: Vec::new(),
            struct_ids: HashMap::new(),
            struct_types: Vec::new(),
            scopes: Vec::new(),
            string_literals: HashMap::new(),
            loop_targets: Vec::new(),
            current_function: None,
            return_type: ValueType::I64,
        }
    }

    fn i64_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.i64_type()
    }
    fn bool_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.bool_type()
    }

    fn llvm_type(&self, ty: ValueType) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            ValueType::I64 => Some(self.i64_type().into()),
            ValueType::Bool => Some(self.bool_type().into()),
            ValueType::Str => Some(self.context.ptr_type(AddressSpace::default()).into()),
            ValueType::Array(element, length) => {
                Some(self.llvm_element_type(element).array_type(length).into())
            }
            ValueType::Enum(index) => self.enum_types.get(index).copied().map(Into::into),
            ValueType::Struct(index) => self.struct_types.get(index).copied().map(Into::into),
            ValueType::Unit => None,
        }
    }

    fn llvm_element_type(&self, element: ArrayElementType) -> inkwell::types::BasicTypeEnum<'ctx> {
        match element {
            ArrayElementType::I64 => self.i64_type().into(),
            ArrayElementType::Bool => self.bool_type().into(),
            ArrayElementType::Str => self.context.ptr_type(AddressSpace::default()).into(),
        }
    }

    fn llvm_fn_type(
        &self,
        params: &[ValueType],
        ret: ValueType,
    ) -> Result<FunctionType<'ctx>, String> {
        let params: Vec<BasicMetadataTypeEnum<'ctx>> = params
            .iter()
            .map(|ty| {
                self.llvm_type(*ty)
                    .ok_or_else(|| "unit is not a valid parameter type".to_owned())
                    .map(Into::into)
            })
            .collect::<Result<_, _>>()?;
        match self.llvm_type(ret) {
            Some(ret) => Ok(ret.fn_type(&params, false)),
            None => Ok(self.context.void_type().fn_type(&params, false)),
        }
    }

    fn resolve_type(&self, ty: &Type, span: Span) -> Result<ValueType, String> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Array(element, length) => ValueType::Array(*element, *length),
            Type::Named(name) => {
                if let Some(index) = self.enum_ids.get(name) {
                    ValueType::Enum(*index)
                } else if let Some(index) = self.struct_ids.get(name) {
                    ValueType::Struct(*index)
                } else {
                    return Err(format!(
                        "line {}:{}: unknown type `{name}`",
                        span.line, span.column
                    ));
                }
            }
        })
    }

    fn declare_enums(&mut self, program: &Program) -> Result<(), String> {
        self.enums = program.enums.clone();
        for (index, declaration) in program.enums.iter().enumerate() {
            if self
                .enum_ids
                .insert(declaration.name.clone(), index)
                .is_some()
            {
                return Err(format!(
                    "enum `{}` is defined more than once",
                    declaration.name
                ));
            }
            self.enum_types.push(self.context.struct_type(
                &[
                    self.context.i32_type().into(),
                    self.i64_type().into(),
                    self.context.ptr_type(AddressSpace::default()).into(),
                ],
                false,
            ));
        }
        Ok(())
    }

    fn declare_structs(&mut self, program: &Program) -> Result<(), String> {
        self.structs = program.structs.clone();
        for (index, declaration) in program.structs.iter().enumerate() {
            if self.enum_ids.contains_key(&declaration.name)
                || self
                    .struct_ids
                    .insert(declaration.name.clone(), index)
                    .is_some()
            {
                return Err(format!(
                    "type `{}` is defined more than once",
                    declaration.name
                ));
            }
            let mut field_types = Vec::with_capacity(declaration.fields.len());
            let mut names = std::collections::HashSet::new();
            for field in &declaration.fields {
                if !names.insert(field.name.as_str()) {
                    return Err(format!(
                        "line {}:{}: struct field `{}` is defined more than once",
                        field.span.line, field.span.column, field.name
                    ));
                }
                let ty = self.resolve_type(&field.ty, field.span)?;
                let llvm_type = self.llvm_type(ty).ok_or("struct field has no LLVM type")?;
                field_types.push(llvm_type);
            }
            self.struct_types
                .push(self.context.struct_type(&field_types, false));
        }
        Ok(())
    }

    fn declare_functions(&mut self, program: &Program) -> Result<(), String> {
        let mut saw_main = false;
        for function in &program.functions {
            if matches!(
                function.name.as_str(),
                "print_i64"
                    | "print"
                    | "println"
                    | "read_i64"
                    | "printf"
                    | "strcmp"
                    | "apex_main"
                    | "getchar"
                    | "exit"
                    | "len"
            ) {
                return Err(format!(
                    "`{}` is a reserved compiler/runtime name",
                    function.name
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(format!(
                    "line {}:{}: function `{}` is defined more than once",
                    function.span.line, function.span.column, function.name
                ));
            }
            let mut names = std::collections::HashSet::new();
            for param in &function.params {
                if param.ty == Type::Unit {
                    return Err(format!(
                        "line {}:{}: parameters cannot have type `unit`",
                        param.span.line, param.span.column
                    ));
                }
                if !names.insert(param.name.as_str()) {
                    return Err(format!(
                        "line {}:{}: duplicate parameter `{}`",
                        param.span.line, param.span.column, param.name
                    ));
                }
            }
            let params: Vec<ValueType> = function
                .params
                .iter()
                .map(|p| self.resolve_type(&p.ty, p.span))
                .collect::<Result<_, _>>()?;
            let ret: ValueType = self.resolve_type(&function.return_type, function.span)?;
            let llvm_name = if function.name == "main" {
                "apex_main"
            } else {
                &function.name
            };
            let llvm_fn =
                self.module
                    .add_function(llvm_name, self.llvm_fn_type(&params, ret)?, None);
            self.functions.insert(function.name.clone(), llvm_fn);
            self.signatures.insert(function.name.clone(), (params, ret));
            if function.name == "main" {
                if !function.params.is_empty() || ret != ValueType::I64 {
                    return Err("entry point must have signature `fn main() -> i64`".into());
                }
                saw_main = true;
            }
        }
        if !saw_main {
            return Err("program must define the required entry point `fn main() -> i64`".into());
        }
        self.signatures
            .insert("print_i64".into(), (vec![ValueType::I64], ValueType::Unit));
        for name in ["print", "println"] {
            self.signatures
                .insert(name.into(), (vec![ValueType::Str], ValueType::Unit));
        }
        self.signatures
            .insert("read_i64".into(), (vec![], ValueType::I64));
        Ok(())
    }

    fn emit_print_builtins(&mut self) -> Result<(), String> {
        let i8_ptr = self.context.ptr_type(AddressSpace::default());
        let printf_type = self.context.i32_type().fn_type(&[i8_ptr.into()], true);
        let printf = self.module.add_function("printf", printf_type, None);
        self.emit_printf_wrapper(printf, "print_i64", ValueType::I64, "%lld\n")?;
        self.emit_printf_wrapper(printf, "print", ValueType::Str, "%s")?;
        self.emit_printf_wrapper(printf, "println", ValueType::Str, "%s\n")?;
        Ok(())
    }

    fn emit_read_i64_builtin(&mut self) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.i64_type();
        let bool_type = self.bool_type();
        let getchar = self
            .module
            .add_function("getchar", i32_type.fn_type(&[], false), None);
        let exit = self.module.add_function(
            "exit",
            self.context.void_type().fn_type(&[i32_type.into()], false),
            None,
        );
        let function = self
            .module
            .add_function("read_i64", i64_type.fn_type(&[], false), None);
        self.functions.insert("read_i64".into(), function);

        let entry = self.context.append_basic_block(function, "entry");
        let skip_test = self.context.append_basic_block(function, "input.skip.test");
        let skip = self.context.append_basic_block(function, "input.skip");
        let after_whitespace = self
            .context
            .append_basic_block(function, "input.after.whitespace");
        let sign_check = self
            .context
            .append_basic_block(function, "input.sign.check");
        let plus = self.context.append_basic_block(function, "input.plus");
        let minus = self.context.append_basic_block(function, "input.minus");
        let no_sign = self.context.append_basic_block(function, "input.no.sign");
        let parse = self.context.append_basic_block(function, "input.parse");
        let digit = self.context.append_basic_block(function, "input.digit");
        let digit_accumulate = self
            .context
            .append_basic_block(function, "input.digit.accumulate");
        let not_digit = self.context.append_basic_block(function, "input.not.digit");
        let success = self.context.append_basic_block(function, "input.success");
        let invalid = self.context.append_basic_block(function, "input.invalid");

        self.builder.position_at_end(entry);
        let character_slot = self
            .builder
            .build_alloca(i32_type, "input.character")
            .map_err(|e| e.to_string())?;
        let negative_slot = self
            .builder
            .build_alloca(bool_type, "input.negative")
            .map_err(|e| e.to_string())?;
        let accumulator_slot = self
            .builder
            .build_alloca(i64_type, "input.magnitude")
            .map_err(|e| e.to_string())?;
        let digit_seen_slot = self
            .builder
            .build_alloca(bool_type, "input.digit.seen")
            .map_err(|e| e.to_string())?;
        let first_character = self
            .builder
            .build_call(getchar, &[], "input.first")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?
            .into_int_value();
        self.builder
            .build_store(character_slot, first_character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(accumulator_slot, i64_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(digit_seen_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(skip_test)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(skip_test);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let whitespace = self.is_ascii_whitespace(character)?;
        self.builder
            .build_conditional_branch(whitespace, skip, after_whitespace)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(skip);
        let character = self
            .builder
            .build_call(getchar, &[], "input.whitespace")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(skip_test)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(after_whitespace);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let at_eof = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(u64::MAX, false),
                "input.eof",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(at_eof, invalid, sign_check)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(sign_check);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_plus = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b'+' as u64, false),
                "input.is.plus",
            )
            .map_err(|e| e.to_string())?;
        let sign_check_minus = self
            .context
            .append_basic_block(function, "input.sign.check.minus");
        self.builder
            .build_conditional_branch(is_plus, plus, sign_check_minus)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(sign_check_minus);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_minus = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b'-' as u64, false),
                "input.is.minus",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(is_minus, minus, no_sign)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(plus);
        self.builder
            .build_store(negative_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.after.plus")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(minus);
        self.builder
            .build_store(negative_slot, bool_type.const_all_ones())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.after.minus")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(no_sign);
        self.builder
            .build_store(negative_slot, bool_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(parse);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let is_digit = self.is_ascii_digit(character)?;
        self.builder
            .build_conditional_branch(is_digit, digit, not_digit)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(digit);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let digit_i32 = self
            .builder
            .build_int_sub(
                character,
                i32_type.const_int(b'0' as u64, false),
                "input.digit.i32",
            )
            .map_err(|e| e.to_string())?;
        let digit_i64 = self
            .builder
            .build_int_z_extend(digit_i32, i64_type, "input.digit")
            .map_err(|e| e.to_string())?;
        let negative = self
            .builder
            .build_load(bool_type, negative_slot, "input.negative")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negative_limit = i64_type.const_int(1u64 << 63, false);
        let positive_limit = i64_type.const_int(i64::MAX as u64, false);
        let limit = self
            .builder
            .build_select(negative, negative_limit, positive_limit, "input.limit")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let remaining = self
            .builder
            .build_int_sub(limit, digit_i64, "input.remaining")
            .map_err(|e| e.to_string())?;
        let threshold = self
            .builder
            .build_int_unsigned_div(remaining, i64_type.const_int(10, false), "input.threshold")
            .map_err(|e| e.to_string())?;
        let accumulator = self
            .builder
            .build_load(i64_type, accumulator_slot, "input.magnitude")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let overflows = self
            .builder
            .build_int_compare(IntPredicate::UGT, accumulator, threshold, "input.overflow")
            .map_err(|e| e.to_string())?;
        let overflow = self
            .context
            .append_basic_block(function, "input.overflow.error");
        self.builder
            .build_conditional_branch(overflows, overflow, digit_accumulate)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(overflow);
        self.builder
            .build_unconditional_branch(invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(digit_accumulate);
        let accumulator = self
            .builder
            .build_int_mul(accumulator, i64_type.const_int(10, false), "input.multiply")
            .map_err(|e| e.to_string())?;
        let accumulator = self
            .builder
            .build_int_add(accumulator, digit_i64, "input.add")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(accumulator_slot, accumulator)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(digit_seen_slot, bool_type.const_all_ones())
            .map_err(|e| e.to_string())?;
        let character = self
            .builder
            .build_call(getchar, &[], "input.next")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: getchar returned void")?;
        self.builder
            .build_store(character_slot, character)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(parse)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(not_digit);
        let digit_seen = self
            .builder
            .build_load(bool_type, digit_seen_slot, "input.digit.seen")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let delimiter_check = self
            .context
            .append_basic_block(function, "input.delimiter.check");
        self.builder
            .build_conditional_branch(digit_seen, delimiter_check, invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(delimiter_check);
        let character = self
            .builder
            .build_load(i32_type, character_slot, "input.character")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let at_eof = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(u64::MAX, false),
                "input.eof",
            )
            .map_err(|e| e.to_string())?;
        let whitespace = self.is_ascii_whitespace(character)?;
        let valid_delimiter = self
            .builder
            .build_or(at_eof, whitespace, "input.valid.delimiter")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(valid_delimiter, success, invalid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(success);
        let accumulator = self
            .builder
            .build_load(i64_type, accumulator_slot, "input.magnitude")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negative = self
            .builder
            .build_load(bool_type, negative_slot, "input.negative")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let negated = self
            .builder
            .build_int_neg(accumulator, "input.negated")
            .map_err(|e| e.to_string())?;
        let result = self
            .builder
            .build_select(negative, negated, accumulator, "input.result")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&result))
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(invalid);
        self.builder
            .build_call(exit, &[i32_type.const_int(1, false).into()], "input.exit")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    fn is_ascii_whitespace(&self, character: IntValue<'ctx>) -> Result<IntValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let low = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                character,
                i32_type.const_int(9, false),
                "input.whitespace.low",
            )
            .map_err(|e| e.to_string())?;
        let high = self
            .builder
            .build_int_compare(
                IntPredicate::SLE,
                character,
                i32_type.const_int(13, false),
                "input.whitespace.high",
            )
            .map_err(|e| e.to_string())?;
        let control_whitespace = self
            .builder
            .build_and(low, high, "input.whitespace.control")
            .map_err(|e| e.to_string())?;
        let space = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                character,
                i32_type.const_int(b' ' as u64, false),
                "input.whitespace.space",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_or(control_whitespace, space, "input.whitespace")
            .map_err(|e| e.to_string())
    }

    fn is_ascii_digit(&self, character: IntValue<'ctx>) -> Result<IntValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let low = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                character,
                i32_type.const_int(b'0' as u64, false),
                "input.digit.low",
            )
            .map_err(|e| e.to_string())?;
        let high = self
            .builder
            .build_int_compare(
                IntPredicate::SLE,
                character,
                i32_type.const_int(b'9' as u64, false),
                "input.digit.high",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_and(low, high, "input.is.digit")
            .map_err(|e| e.to_string())
    }

    // Defines `name(value) -> unit` as `printf(format, value)`.
    fn emit_printf_wrapper(
        &mut self,
        printf: FunctionValue<'ctx>,
        name: &str,
        param: ValueType,
        format: &str,
    ) -> Result<(), String> {
        let function_type = self.llvm_fn_type(&[param], ValueType::Unit)?;
        let function = self.module.add_function(name, function_type, None);
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        let fmt = self
            .builder
            .build_global_string_ptr(format, &format!("apex_{name}_fmt"))
            .map_err(|e| e.to_string())?;
        let value = function
            .get_nth_param(0)
            .ok_or_else(|| format!("missing {name} parameter"))?;
        self.builder
            .build_call(
                printf,
                &[fmt.as_pointer_value().into(), value.into()],
                "printf_result",
            )
            .map_err(|e| e.to_string())?;
        self.builder.build_return(None).map_err(|e| e.to_string())?;
        self.functions.insert(name.into(), function);
        Ok(())
    }

    fn strcmp_function(&self) -> FunctionValue<'ctx> {
        self.module.get_function("strcmp").unwrap_or_else(|| {
            let ptr = self.context.ptr_type(AddressSpace::default());
            let strcmp_type = self
                .context
                .i32_type()
                .fn_type(&[ptr.into(), ptr.into()], false);
            self.module.add_function("strcmp", strcmp_type, None)
        })
    }

    fn string_literal(&mut self, value: &str) -> Result<PointerValue<'ctx>, String> {
        if let Some(pointer) = self.string_literals.get(value) {
            return Ok(*pointer);
        }
        let name = format!("apex_str.{}", self.string_literals.len());
        let pointer = self
            .builder
            .build_global_string_ptr(value, &name)
            .map_err(|e| e.to_string())?
            .as_pointer_value();
        self.string_literals.insert(value.to_owned(), pointer);
        Ok(pointer)
    }

    fn emit_c_main_wrapper(&mut self) -> Result<(), String> {
        let apex_main = *self.functions.get("main").ok_or("missing main function")?;
        let wrapper_type = self.context.i32_type().fn_type(&[], false);
        let wrapper = self.module.add_function("main", wrapper_type, None);
        let entry = self.context.append_basic_block(wrapper, "entry");
        self.builder.position_at_end(entry);
        let result = self
            .builder
            .build_call(apex_main, &[], "apex_result")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: Apex main returned void")?
            .into_int_value();
        let process_status = self
            .builder
            .build_int_truncate(result, self.context.i32_type(), "process.status")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&process_status))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn emit_function(&mut self, function: &Function) -> Result<(), String> {
        let function_value = *self
            .functions
            .get(&function.name)
            .ok_or("internal error: function was not declared")?;
        let entry = self.context.append_basic_block(function_value, "entry");
        self.builder.position_at_end(entry);
        self.current_function = Some(function_value);
        self.return_type = self.resolve_type(&function.return_type, function.span)?;
        self.scopes.clear();
        self.loop_targets.clear();
        self.scopes.push(HashMap::new());

        for (index, param) in function.params.iter().enumerate() {
            let param_value = function_value
                .get_nth_param(index as u32)
                .ok_or("internal error: missing LLVM parameter")?;
            param_value.set_name(&param.name);
            let ty: ValueType = self.resolve_type(&param.ty, param.span)?;
            let alloca = self.create_entry_alloca(ty, &param.name)?;
            self.builder
                .build_store(alloca, param_value)
                .map_err(|e| e.to_string())?;
            self.scopes[0].insert(
                param.name.clone(),
                Local {
                    pointer: alloca,
                    ty,
                    immutable: false,
                    loop_variable: false,
                },
            );
        }

        let (terminated, tail) = self.emit_block(&function.body, true)?;
        if !terminated {
            if let Some(value) = tail {
                if value.ty != self.return_type {
                    return Err(format!(
                        "line {}:{}: function `{}` returns {}, but its declared return type is {}",
                        function.span.line,
                        function.span.column,
                        function.name,
                        type_name(value.ty),
                        type_name(self.return_type)
                    ));
                }
                if let Some(returned) = value.value {
                    self.builder
                        .build_return(Some(&returned))
                        .map_err(|e| e.to_string())?;
                } else if self.return_type == ValueType::Unit {
                    self.builder.build_return(None).map_err(|e| e.to_string())?;
                } else {
                    return Err("internal error: return value missing".into());
                }
            } else if self.return_type == ValueType::Unit {
                self.builder.build_return(None).map_err(|e| e.to_string())?;
            } else {
                return Err(format!(
                    "line {}:{}: function `{}` can reach its end without returning a value",
                    function.span.line, function.span.column, function.name
                ));
            }
        }
        self.current_function = None;
        self.scopes.clear();
        Ok(())
    }

    // The function body's final expression is its implicit return value. Nested blocks
    // evaluate, but do not return, their final expression.
    fn emit_block(
        &mut self,
        block: &Block,
        function_body: bool,
    ) -> Result<(bool, Option<TypedValue<'ctx>>), String> {
        self.scopes.push(HashMap::new());
        let mut terminated = false;
        for statement in &block.statements {
            if self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some()
            {
                terminated = true;
                break;
            }
            if self.emit_statement(statement)? {
                terminated = true;
                break;
            }
        }
        let mut tail_value = None;
        if !terminated && let Some(expr) = &block.tail {
            let value = self.emit_expression(expr)?;
            if function_body {
                tail_value = Some(value);
            }
        }
        self.scopes.pop();
        Ok((terminated, tail_value))
    }

    fn emit_statement(&mut self, statement: &Stmt) -> Result<bool, String> {
        match statement {
            Stmt::Let {
                name,
                ty,
                value,
                immutable,
                span,
            } => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(format!(
                        "line {}:{}: `{name}` is already declared in this scope",
                        span.line, span.column
                    ));
                }
                let value = if let (Some(ty), Expr::Array(elements, _)) = (ty.as_ref(), value) {
                    if elements.is_empty() {
                        let array_type = self.resolve_type(ty, *span)?;
                        if !matches!(array_type, ValueType::Array(_, 0)) {
                            return Err(format!(
                                "line {}:{}: empty array literal must have array length 0",
                                span.line, span.column
                            ));
                        }
                        let llvm_type =
                            self.llvm_type(array_type).ok_or("array has no LLVM type")?;
                        TypedValue {
                            value: Some(llvm_type.const_zero()),
                            ty: array_type,
                        }
                    } else {
                        self.emit_expression(value)?
                    }
                } else {
                    self.emit_expression(value)?
                };
                if value.ty == ValueType::Unit {
                    return Err(format!(
                        "line {}:{}: cannot bind a value of type `unit`",
                        span.line, span.column
                    ));
                }
                let ty = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => value.ty,
                };
                if ty != value.ty {
                    return Err(format!(
                        "line {}:{}: binding `{name}` has type {}, but its initializer has type {}",
                        span.line,
                        span.column,
                        type_name(ty),
                        type_name(value.ty)
                    ));
                }
                let alloca = self.create_entry_alloca(ty, name)?;
                self.builder
                    .build_store(alloca, value.value.ok_or("missing initializer value")?)
                    .map_err(|e| e.to_string())?;
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        pointer: alloca,
                        ty,
                        immutable: *immutable,
                        loop_variable: false,
                    },
                );
                Ok(false)
            }
            Stmt::Assign { name, value, span } => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                if local.immutable {
                    let reason = if local.loop_variable {
                        format!("cannot assign to immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign to immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column));
                }
                let value = self.emit_expression(value)?;
                if local.ty != value.ty {
                    return Err(format!(
                        "line {}:{}: cannot assign {} to `{name}` of type {}",
                        span.line,
                        span.column,
                        type_name(value.ty),
                        type_name(local.ty)
                    ));
                }
                self.builder
                    .build_store(
                        local.pointer,
                        value.value.ok_or("missing assignment value")?,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            Stmt::AssignIndex {
                name,
                index,
                value,
                span,
            } => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                let ValueType::Array(element, length) = local.ty else {
                    return Err(format!(
                        "line {}:{}: `{name}` is not an array",
                        span.line, span.column
                    ));
                };
                if local.immutable {
                    let reason = if local.loop_variable {
                        format!("cannot assign through immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign through immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column));
                }
                let index = self.emit_expression(index)?;
                let index = self.expect_i64(index, "array index")?;
                self.emit_bounds_check(index, length)?;
                let value = self.emit_expression(value)?;
                let expected = ValueType::from(element);
                if value.ty != expected {
                    return Err(format!(
                        "line {}:{}: array element assignment expects {}, got {}",
                        span.line,
                        span.column,
                        type_name(expected),
                        type_name(value.ty)
                    ));
                }
                let element_pointer = self.array_element_pointer(local.pointer, local.ty, index)?;
                self.builder
                    .build_store(
                        element_pointer,
                        value.value.ok_or("missing array element value")?,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            Stmt::AssignField {
                name,
                field,
                value,
                span,
            } => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                if local.immutable {
                    let reason = if local.loop_variable {
                        format!("cannot assign through immutable loop variable `{name}`")
                    } else {
                        format!("cannot assign through immutable binding `{name}`")
                    };
                    return Err(format!("line {}:{}: {reason}", span.line, span.column,));
                }
                let ValueType::Struct(struct_id) = local.ty else {
                    return Err(format!(
                        "line {}:{}: `{name}` is not a struct",
                        span.line, span.column
                    ));
                };
                let field_index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|declaration| declaration.name == *field)
                    .ok_or_else(|| {
                        format!(
                            "line {}:{}: struct `{}` has no field `{field}`",
                            span.line, span.column, self.structs[struct_id].name
                        )
                    })?;
                let field_type = self.resolve_type(
                    &self.structs[struct_id].fields[field_index].ty,
                    self.structs[struct_id].fields[field_index].span,
                )?;
                let field_pointer = self
                    .builder
                    .build_struct_gep(
                        self.struct_types[struct_id],
                        local.pointer,
                        field_index as u32,
                        "struct.field.ptr",
                    )
                    .map_err(|error| error.to_string())?;
                let value = self.emit_expression(value)?;
                if value.ty != field_type {
                    return Err(format!(
                        "line {}:{}: assignment to field `{field}` has type {}, expected {}",
                        span.line,
                        span.column,
                        type_name(value.ty),
                        type_name(field_type)
                    ));
                }
                self.builder
                    .build_store(
                        field_pointer,
                        value.value.ok_or("struct field assignment has no value")?,
                    )
                    .map_err(|error| error.to_string())?;
                Ok(false)
            }
            Stmt::Expr(expr) => {
                let value = self.emit_expression(expr)?;
                if value.ty != ValueType::Unit { /* expression value intentionally discarded */ }
                Ok(false)
            }
            Stmt::Return { value, .. } => {
                let value = if let Some(expr) = value {
                    let value = self.emit_expression(expr)?;
                    if value.ty != self.return_type {
                        return Err(format!(
                            "return type mismatch: expected {}, got {}",
                            type_name(self.return_type),
                            type_name(value.ty)
                        ));
                    }
                    value.value
                } else {
                    if self.return_type != ValueType::Unit {
                        return Err("`return;` is only valid in a function returning `unit`".into());
                    }
                    None
                };
                if let Some(value) = value {
                    self.builder
                        .build_return(Some(&value))
                        .map_err(|e| e.to_string())?;
                } else {
                    self.builder.build_return(None).map_err(|e| e.to_string())?;
                }
                Ok(true)
            }
            Stmt::Break { span } => {
                let (_, break_target) = self.loop_targets.last().copied().ok_or_else(|| {
                    format!(
                        "line {}:{}: `break` is only valid inside a loop",
                        span.line, span.column
                    )
                })?;
                self.builder
                    .build_unconditional_branch(break_target)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Stmt::Continue { span } => {
                let (continue_target, _) = self.loop_targets.last().copied().ok_or_else(|| {
                    format!(
                        "line {}:{}: `continue` is only valid inside a loop",
                        span.line, span.column
                    )
                })?;
                self.builder
                    .build_unconditional_branch(continue_target)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => self.emit_if(condition, then_block, else_block.as_ref()),
            Stmt::While {
                condition, body, ..
            } => self.emit_while(condition, body),
            Stmt::For {
                name,
                start,
                end,
                body,
                ..
            } => self.emit_for(name, start, end, body),
            Stmt::ForArray {
                name,
                array,
                body,
                span,
            } => self.emit_for_array(name, array, body, *span),
        }
    }

    fn emit_if(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<bool, String> {
        let cond = self.emit_expression(condition)?;
        let cond_value = self.expect_bool(cond, "if condition")?;
        let function = self.current_function.ok_or("if outside function")?;
        let then_bb = self.context.append_basic_block(function, "if.then");
        let merge_bb = self.context.append_basic_block(function, "if.end");
        let else_bb = else_block
            .map(|_| self.context.append_basic_block(function, "if.else"))
            .unwrap_or(merge_bb);
        self.builder
            .build_conditional_branch(cond_value, then_bb, else_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        let (then_terminated, _) = self.emit_block(then_block, false)?;
        let then_terminated = then_terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some();
        if !then_terminated {
            self.builder
                .build_unconditional_branch(merge_bb)
                .map_err(|e| e.to_string())?;
        }

        let mut else_terminated = false;
        if let Some(else_block) = else_block {
            self.builder.position_at_end(else_bb);
            let (terminated, _) = self.emit_block(else_block, false)?;
            else_terminated = terminated
                || self
                    .builder
                    .get_insert_block()
                    .and_then(|b| b.get_terminator())
                    .is_some();
            if !else_terminated {
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(|e| e.to_string())?;
            }
        }

        if else_block.is_some() && then_terminated && else_terminated {
            self.builder.position_at_end(merge_bb);
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
            Ok(true)
        } else {
            self.builder.position_at_end(merge_bb);
            Ok(false)
        }
    }

    fn emit_while(&mut self, condition: &Expr, body: &Block) -> Result<bool, String> {
        let function = self.current_function.ok_or("while outside function")?;
        let cond_bb = self.context.append_basic_block(function, "while.cond");
        let body_bb = self.context.append_basic_block(function, "while.body");
        let end_bb = self.context.append_basic_block(function, "while.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(cond_bb);
        let cond = self.emit_expression(condition)?;
        let cond_value = self.expect_bool(cond, "while condition")?;
        self.builder
            .build_conditional_branch(cond_value, body_bb, end_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(body_bb);
        self.loop_targets.push((cond_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(cond_bb)
                .map_err(|e| e.to_string())?;
        }
        self.builder.position_at_end(end_bb);
        Ok(false)
    }

    fn emit_for(
        &mut self,
        name: &str,
        start_expr: &Expr,
        end_expr: &Expr,
        body: &Block,
    ) -> Result<bool, String> {
        // Evaluate the bounds once, before the loop variable enters scope.
        let start = self.emit_expression(start_expr)?;
        let end = self.emit_expression(end_expr)?;
        if start.ty != ValueType::I64 || end.ty != ValueType::I64 {
            return Err("for range bounds must be i64".into());
        }
        let start = start
            .value
            .ok_or("for range start has no value")?
            .into_int_value();
        let end = end
            .value
            .ok_or("for range end has no value")?
            .into_int_value();

        let function = self.current_function.ok_or("for loop outside function")?;
        let counter = self.create_entry_alloca(ValueType::I64, name)?;
        self.builder
            .build_store(counter, start)
            .map_err(|e| e.to_string())?;

        self.scopes.push(HashMap::new());
        self.scopes.last_mut().unwrap().insert(
            name.to_owned(),
            Local {
                pointer: counter,
                ty: ValueType::I64,
                immutable: true,
                loop_variable: true,
            },
        );

        let cond_bb = self.context.append_basic_block(function, "for.cond");
        let body_bb = self.context.append_basic_block(function, "for.body");
        let step_bb = self.context.append_basic_block(function, "for.step");
        let end_bb = self.context.append_basic_block(function, "for.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_bb);
        let current = self
            .builder
            .build_load(self.i64_type(), counter, "for.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let in_range = self
            .builder
            .build_int_compare(IntPredicate::SLT, current, end, "for.test")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(in_range, body_bb, end_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_bb);
        self.loop_targets.push((step_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|block| block.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(step_bb)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(step_bb);
        let current = self
            .builder
            .build_load(self.i64_type(), counter, "for.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(current, self.i64_type().const_int(1, false), "for.next")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(counter, next)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(end_bb);
        self.scopes.pop();
        Ok(false)
    }

    fn emit_for_array(
        &mut self,
        name: &str,
        array_expr: &Expr,
        body: &Block,
        span: Span,
    ) -> Result<bool, String> {
        let array = self.emit_expression(array_expr)?;
        let ValueType::Array(element, length) = array.ty else {
            return Err(format!(
                "line {}:{}: array iteration requires an array",
                span.line, span.column
            ));
        };
        let array_type = array.ty;
        let array_slot = self.create_entry_alloca(array_type, "for.array")?;
        self.builder
            .build_store(array_slot, array.value.ok_or("array has no value")?)
            .map_err(|e| e.to_string())?;
        let index_slot = self.create_entry_alloca(ValueType::I64, "for.array.index")?;
        self.builder
            .build_store(index_slot, self.i64_type().const_zero())
            .map_err(|e| e.to_string())?;
        let value_slot = self.create_entry_alloca(ValueType::from(element), name)?;

        self.scopes.push(HashMap::from([(
            name.to_owned(),
            Local {
                pointer: value_slot,
                ty: ValueType::from(element),
                immutable: true,
                loop_variable: true,
            },
        )]));

        let function = self.current_function.ok_or("array loop outside function")?;
        let cond_bb = self.context.append_basic_block(function, "for.array.cond");
        let body_bb = self.context.append_basic_block(function, "for.array.body");
        let step_bb = self.context.append_basic_block(function, "for.array.step");
        let end_bb = self.context.append_basic_block(function, "for.array.end");
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let in_bounds = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                index,
                self.i64_type().const_int(u64::from(length), false),
                "for.array.test",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(in_bounds, body_bb, end_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let element_pointer = self.array_element_pointer(array_slot, array_type, index)?;
        let element_value = self
            .builder
            .build_load(
                self.llvm_element_type(element),
                element_pointer,
                "for.array.value",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(value_slot, element_value)
            .map_err(|e| e.to_string())?;
        self.loop_targets.push((step_bb, end_bb));
        let body_result = self.emit_block(body, false);
        self.loop_targets.pop();
        let (terminated, _) = body_result?;
        let terminated = terminated
            || self
                .builder
                .get_insert_block()
                .and_then(|block| block.get_terminator())
                .is_some();
        if !terminated {
            self.builder
                .build_unconditional_branch(step_bb)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(step_bb);
        let index = self
            .builder
            .build_load(self.i64_type(), index_slot, "for.array.index")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(index, self.i64_type().const_int(1, false), "for.array.next")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(index_slot, next)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(end_bb);
        self.scopes.pop();
        Ok(false)
    }

    fn emit_expression(&mut self, expr: &Expr) -> Result<TypedValue<'ctx>, String> {
        match expr {
            Expr::Integer(n, _) => Ok(TypedValue {
                value: Some(self.i64_type().const_int(*n, false).into()),
                ty: ValueType::I64,
            }),
            Expr::Bool(value, _) => Ok(TypedValue {
                value: Some(self.bool_type().const_int(u64::from(*value), false).into()),
                ty: ValueType::Bool,
            }),
            Expr::String(value, _) => Ok(TypedValue {
                value: Some(self.string_literal(value)?.into()),
                ty: ValueType::Str,
            }),
            Expr::Array(elements, span) => self.emit_array_literal(elements, *span),
            Expr::Variable(name, span) => {
                let local = self.lookup_local(name).ok_or_else(|| {
                    format!(
                        "line {}:{}: undefined variable `{name}`",
                        span.line, span.column
                    )
                })?;
                let value = self
                    .builder
                    .build_load(self.llvm_type(local.ty).unwrap(), local.pointer, name)
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: local.ty,
                })
            }
            Expr::Index { array, index, span } => {
                let array_value = self.emit_expression(array)?;
                let ValueType::Array(element, length) = array_value.ty else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array",
                        span.line, span.column
                    ));
                };
                let index = self.emit_expression(index)?;
                let index = self.expect_i64(index, "array index")?;
                self.emit_bounds_check(index, length)?;
                let array_type = ValueType::Array(element, length);
                let scratch = self.create_entry_alloca(array_type, "array.index.base")?;
                self.builder
                    .build_store(scratch, array_value.value.ok_or("array has no value")?)
                    .map_err(|e| e.to_string())?;
                let pointer = self.array_element_pointer(scratch, array_type, index)?;
                let value = self
                    .builder
                    .build_load(self.llvm_element_type(element), pointer, "array.element")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: ValueType::from(element),
                })
            }
            Expr::Field { value, name, span } => {
                let value = self.emit_expression(value)?;
                let ValueType::Struct(struct_id) = value.ty else {
                    return Err(format!(
                        "line {}:{}: field access requires a struct value",
                        span.line, span.column
                    ));
                };
                let field_index = self.structs[struct_id]
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| {
                        format!(
                            "line {}:{}: struct `{}` has no field `{name}`",
                            span.line, span.column, self.structs[struct_id].name
                        )
                    })?;
                let field_ty = self.resolve_type(
                    &self.structs[struct_id].fields[field_index].ty,
                    self.structs[struct_id].fields[field_index].span,
                )?;
                let value = self
                    .builder
                    .build_extract_value(
                        value
                            .value
                            .ok_or("struct value has no LLVM value")?
                            .into_struct_value(),
                        field_index as u32,
                        "struct.field",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue {
                    value: Some(value),
                    ty: field_ty,
                })
            }
            Expr::Struct { name, fields, span } => self.emit_struct_literal(name, fields, *span),
            Expr::Unary { op, value, .. } => {
                let value = self.emit_expression(value)?;
                match op {
                    UnaryOp::Negate => {
                        if value.ty != ValueType::I64 {
                            return Err(format!(
                                "unary `-` requires i64, got {}",
                                type_name(value.ty)
                            ));
                        }
                        let result = self
                            .builder
                            .build_int_neg(value.value.unwrap().into_int_value(), "neg")
                            .map_err(|e| e.to_string())?;
                        Ok(TypedValue {
                            value: Some(result.into()),
                            ty: ValueType::I64,
                        })
                    }
                    UnaryOp::Not => {
                        if value.ty != ValueType::Bool {
                            return Err(format!(
                                "unary `!` requires bool, got {}",
                                type_name(value.ty)
                            ));
                        }
                        let result = self
                            .builder
                            .build_not(value.value.unwrap().into_int_value(), "not")
                            .map_err(|e| e.to_string())?;
                        Ok(TypedValue {
                            value: Some(result.into()),
                            ty: ValueType::Bool,
                        })
                    }
                }
            }
            Expr::Binary {
                op, left, right, ..
            } => self.emit_binary(*op, left, right),
            Expr::Call { name, args, span } => self.emit_call(name, args, *span),
            Expr::If {
                condition,
                then_block,
                else_block,
                ..
            } => self.emit_if_expression(condition, then_block, else_block),
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
            } => self.emit_enum_variant(enum_name, variant_name, payload.as_deref(), *span),
            Expr::Match { value, arms, span } => self.emit_match(value, arms, *span),
        }
    }

    fn emit_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payload_expr: Option<&Expr>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let enum_id = *self.enum_ids.get(enum_name).ok_or_else(|| {
            format!(
                "line {}:{}: unknown enum `{enum_name}`",
                span.line, span.column
            )
        })?;
        let variant_index = self.enums[enum_id]
            .variants
            .iter()
            .position(|variant| variant.name == variant_name)
            .ok_or_else(|| {
                format!(
                    "line {}:{}: enum `{enum_name}` has no variant `{variant_name}`",
                    span.line, span.column
                )
            })?;
        let variant = self.enums[enum_id].variants[variant_index].clone();
        let (integer_payload, string_payload) = match (&variant.payload, payload_expr) {
            (None, None) => (
                self.i64_type().const_zero(),
                self.context.ptr_type(AddressSpace::default()).const_null(),
            ),
            (Some(payload_type), Some(expression)) => {
                let payload = self.emit_expression(expression)?;
                let expected = self.resolve_type(payload_type, variant.span)?;
                if payload.ty != expected {
                    return Err(format!(
                        "enum payload has type {}, expected {}",
                        type_name(payload.ty),
                        type_name(expected)
                    ));
                }
                match payload.ty {
                    ValueType::I64 => (
                        payload
                            .value
                            .ok_or("enum payload has no value")?
                            .into_int_value(),
                        self.context.ptr_type(AddressSpace::default()).const_null(),
                    ),
                    ValueType::Bool => {
                        let value = self
                            .builder
                            .build_int_z_extend(
                                payload
                                    .value
                                    .ok_or("enum payload has no value")?
                                    .into_int_value(),
                                self.i64_type(),
                                "enum.bool.payload",
                            )
                            .map_err(|e| e.to_string())?;
                        (
                            value,
                            self.context.ptr_type(AddressSpace::default()).const_null(),
                        )
                    }
                    ValueType::Str => (
                        self.i64_type().const_zero(),
                        payload
                            .value
                            .ok_or("enum payload has no value")?
                            .into_pointer_value(),
                    ),
                    _ => return Err("enum payloads must have type `i64`, `bool`, or `str`".into()),
                }
            }
            (Some(_), None) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` requires a payload"
                ));
            }
            (None, Some(_)) => {
                return Err(format!(
                    "variant `{enum_name}::{variant_name}` does not take a payload"
                ));
            }
        };
        let enum_type = self.enum_types[enum_id];
        let mut value = enum_type.const_zero().as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                self.context
                    .i32_type()
                    .const_int(variant_index as u64, false),
                0,
                "enum.tag",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                integer_payload,
                1,
                "enum.integer.payload",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        value = self
            .builder
            .build_insert_value(
                value.into_struct_value(),
                string_payload,
                2,
                "enum.string.payload",
            )
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        Ok(TypedValue {
            value: Some(value),
            ty: ValueType::Enum(enum_id),
        })
    }

    fn emit_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, Expr, Span)],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let struct_id = *self.struct_ids.get(name).ok_or_else(|| {
            format!(
                "line {}:{}: unknown struct `{name}`",
                span.line, span.column
            )
        })?;
        let declaration = self.structs[struct_id].clone();
        let struct_type = self.struct_types[struct_id];
        let mut aggregate = struct_type.const_zero().as_basic_value_enum();
        let mut initialized = std::collections::HashSet::new();
        for (field_name, expression, field_span) in fields {
            if !initialized.insert(field_name.as_str()) {
                return Err(format!(
                    "line {}:{}: struct field `{field_name}` is initialized more than once",
                    field_span.line, field_span.column
                ));
            }
            let Some(index) = declaration
                .fields
                .iter()
                .position(|field| field.name == *field_name)
            else {
                return Err(format!(
                    "line {}:{}: struct `{name}` has no field `{field_name}`",
                    field_span.line, field_span.column
                ));
            };
            let value = self.emit_expression(expression)?;
            let expected = self.resolve_type(
                &declaration.fields[index].ty,
                declaration.fields[index].span,
            )?;
            if value.ty != expected {
                return Err(format!(
                    "line {}:{}: initializer for field `{field_name}` has type {}, expected {}",
                    field_span.line,
                    field_span.column,
                    type_name(value.ty),
                    type_name(expected)
                ));
            }
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_struct_value(),
                    value.value.ok_or("struct field has no value")?,
                    index as u32,
                    "struct.field.init",
                )
                .map_err(|e| e.to_string())?
                .as_basic_value_enum();
        }
        if fields.len() != declaration.fields.len() {
            let missing = declaration
                .fields
                .iter()
                .filter(|field| !initialized.contains(field.name.as_str()))
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "line {}:{}: struct `{name}` is missing field(s): {missing}",
                span.line, span.column
            ));
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: ValueType::Struct(struct_id),
        })
    }

    fn emit_match(
        &mut self,
        value_expr: &Expr,
        arms: &[crate::parser::MatchArm],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        let value = self.emit_expression(value_expr)?;
        let ValueType::Enum(enum_id) = value.ty else {
            return Err(format!(
                "line {}:{}: match requires an enum value",
                span.line, span.column
            ));
        };
        let enum_value = value
            .value
            .ok_or("match value has no LLVM value")?
            .into_struct_value();
        let tag = self
            .builder
            .build_extract_value(enum_value, 0, "enum.match.tag")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let function = self.current_function.ok_or("match outside function")?;
        let end_bb = self.context.append_basic_block(function, "match.end");
        let arm_blocks: Vec<_> = arms
            .iter()
            .map(|_| self.context.append_basic_block(function, "match.arm"))
            .collect();
        let fallback_bb = self.context.append_basic_block(function, "match.invalid");
        for (index, arm) in arms.iter().enumerate() {
            if matches!(arm.pattern, MatchPattern::Wildcard) {
                self.builder
                    .build_unconditional_branch(arm_blocks[index])
                    .map_err(|e| e.to_string())?;
                break;
            }
            let MatchPattern::Variant { variant_name, .. } = &arm.pattern else {
                return Err("internal error: invalid match pattern".into());
            };
            let variant_index = self.enums[enum_id]
                .variants
                .iter()
                .position(|variant| variant.name == *variant_name)
                .ok_or_else(|| {
                    format!(
                        "line {}:{}: unknown variant `{variant_name}`",
                        arm.span.line, arm.span.column
                    )
                })?;
            let test = self
                .builder
                .build_int_compare(
                    IntPredicate::EQ,
                    tag,
                    self.context
                        .i32_type()
                        .const_int(variant_index as u64, false),
                    "enum.match.test",
                )
                .map_err(|e| e.to_string())?;
            let next = if index + 1 == arms.len() {
                fallback_bb
            } else {
                self.context.append_basic_block(function, "match.next")
            };
            self.builder
                .build_conditional_branch(test, arm_blocks[index], next)
                .map_err(|e| e.to_string())?;
            if index + 1 != arms.len() {
                self.builder.position_at_end(next);
            }
        }
        self.builder.position_at_end(fallback_bb);
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        let mut incoming = Vec::new();
        let mut result_type = None;
        for (index, arm) in arms.iter().enumerate() {
            self.builder.position_at_end(arm_blocks[index]);
            self.scopes.push(HashMap::new());
            let binding = match &arm.pattern {
                MatchPattern::Variant {
                    binding: Some(binding),
                    ..
                } => Some(binding),
                _ => None,
            };
            if let Some(binding) = binding {
                let MatchPattern::Variant { variant_name, .. } = &arm.pattern else {
                    return Err("internal error: wildcard match arm cannot bind a payload".into());
                };
                let variant = self.enums[enum_id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                    .ok_or("unknown match variant")?;
                let payload_type = variant
                    .payload
                    .as_ref()
                    .ok_or("payload binding on unit variant")?;
                let ty = self.resolve_type(payload_type, arm.span)?;
                let field = if ty == ValueType::Str { 2 } else { 1 };
                let mut payload = self
                    .builder
                    .build_extract_value(enum_value, field, "enum.match.payload")
                    .map_err(|e| e.to_string())?;
                if ty == ValueType::Bool {
                    payload = self
                        .builder
                        .build_int_truncate(
                            payload.into_int_value(),
                            self.bool_type(),
                            "enum.match.bool",
                        )
                        .map_err(|e| e.to_string())?
                        .into();
                }
                let alloca = self.create_entry_alloca(ty, binding)?;
                self.builder
                    .build_store(alloca, payload)
                    .map_err(|e| e.to_string())?;
                self.scopes.last_mut().unwrap().insert(
                    binding.clone(),
                    Local {
                        pointer: alloca,
                        ty,
                        immutable: false,
                        loop_variable: false,
                    },
                );
            }
            let (terminated, result) = self.emit_block(&arm.body, true)?;
            self.scopes.pop();
            if terminated {
                return Err("match arm unexpectedly terminated".into());
            }
            let result = result.ok_or("match arm must produce a value")?;
            if result.ty == ValueType::Unit {
                return Err("match arms must produce a value".into());
            }
            if let Some(expected) = result_type {
                if expected != result.ty {
                    return Err(format!(
                        "match arms have different types: {} and {}",
                        type_name(expected),
                        type_name(result.ty)
                    ));
                }
            } else {
                result_type = Some(result.ty);
            }
            let result_value = result.value.ok_or("match arm has no LLVM value")?;
            let result_end = self
                .builder
                .get_insert_block()
                .ok_or("match arm has no insertion block")?;
            self.builder
                .build_unconditional_branch(end_bb)
                .map_err(|e| e.to_string())?;
            incoming.push((result_value, result_end));
        }
        self.builder.position_at_end(end_bb);
        let ty = result_type.ok_or("match expression has no arms")?;
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(ty).ok_or("unit match result")?,
                "match.value",
            )
            .map_err(|e| e.to_string())?;
        let incoming_refs: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&incoming_refs);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty,
        })
    }

    fn emit_array_literal(
        &mut self,
        elements: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if elements.is_empty() {
            return Err(format!(
                "line {}:{}: empty array literals require an array type annotation",
                span.line, span.column
            ));
        }
        let Some(first) = elements.first() else {
            return Err(format!(
                "line {}:{}: array literals must contain at least one element",
                span.line, span.column
            ));
        };
        if elements.len() > u32::MAX as usize {
            return Err(format!(
                "line {}:{}: array literal is too large",
                span.line, span.column
            ));
        }
        let first = self.emit_expression(first)?;
        let element = match first.ty {
            ValueType::I64 => ArrayElementType::I64,
            ValueType::Bool => ArrayElementType::Bool,
            ValueType::Str => ArrayElementType::Str,
            _ => return Err("array elements must have type `i64`, `bool`, or `str`".into()),
        };
        let array_type = ValueType::Array(element, elements.len() as u32);
        let llvm_array_type = self
            .llvm_type(array_type)
            .ok_or("array has no LLVM type")?
            .into_array_type();
        let mut aggregate = llvm_array_type.const_zero().as_basic_value_enum();
        let first_value = first.value.ok_or("array element has no value")?;
        aggregate = self
            .builder
            .build_insert_value(aggregate.into_array_value(), first_value, 0, "array.init")
            .map_err(|e| e.to_string())?
            .as_basic_value_enum();
        for (index, expression) in elements.iter().enumerate().skip(1) {
            let value = self.emit_expression(expression)?;
            if value.ty != ValueType::from(element) {
                return Err(format!(
                    "array element has type {}, expected {}",
                    type_name(value.ty),
                    type_name(ValueType::from(element))
                ));
            }
            aggregate = self
                .builder
                .build_insert_value(
                    aggregate.into_array_value(),
                    value.value.ok_or("array element has no value")?,
                    index as u32,
                    "array.insert",
                )
                .map_err(|e| e.to_string())?
                .as_basic_value_enum();
        }
        Ok(TypedValue {
            value: Some(aggregate),
            ty: array_type,
        })
    }

    fn array_element_pointer(
        &self,
        pointer: PointerValue<'ctx>,
        array_type: ValueType,
        index: IntValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, String> {
        let zero = self.i64_type().const_zero();
        let llvm_array_type = self.llvm_type(array_type).ok_or("array has no LLVM type")?;
        unsafe {
            self.builder
                .build_in_bounds_gep(
                    llvm_array_type,
                    pointer,
                    &[zero, index],
                    "array.element.ptr",
                )
                .map_err(|e| e.to_string())
        }
    }

    fn emit_bounds_check(&mut self, index: IntValue<'ctx>, length: u32) -> Result<(), String> {
        let is_negative = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                index,
                self.i64_type().const_zero(),
                "index.negative",
            )
            .map_err(|e| e.to_string())?;
        let too_large = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                index,
                self.i64_type().const_int(u64::from(length), false),
                "index.too.large",
            )
            .map_err(|e| e.to_string())?;
        let invalid = self
            .builder
            .build_or(is_negative, too_large, "index.invalid")
            .map_err(|e| e.to_string())?;
        let function = self
            .current_function
            .ok_or("array index outside function")?;
        let failure = self.context.append_basic_block(function, "bounds.error");
        let valid = self.context.append_basic_block(function, "bounds.valid");
        self.builder
            .build_conditional_branch(invalid, failure, valid)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(failure);
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing runtime exit declaration")?;
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "bounds.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }

    fn expect_i64(&self, value: TypedValue<'ctx>, context: &str) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::I64 {
            return Err(format!(
                "{context} must be i64, got {}",
                type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    fn emit_if_expression(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: &Block,
    ) -> Result<TypedValue<'ctx>, String> {
        let condition = self.emit_expression(condition)?;
        let condition = self.expect_bool(condition, "if condition")?;
        let function = self
            .current_function
            .ok_or("if expression outside function")?;
        let then_bb = self.context.append_basic_block(function, "if.expr.then");
        let else_bb = self.context.append_basic_block(function, "if.expr.else");
        let merge_bb = self.context.append_basic_block(function, "if.expr.end");
        self.builder
            .build_conditional_branch(condition, then_bb, else_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        let (then_terminated, then_value) = self.emit_block(then_block, true)?;
        if then_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let then_value = then_value.ok_or("if expression branch has no value")?;
        let then_llvm_value = then_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let then_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression then branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(else_bb);
        let (else_terminated, else_value) = self.emit_block(else_block, true)?;
        if else_terminated {
            return Err("if expression branch unexpectedly terminated".into());
        }
        let else_value = else_value.ok_or("if expression branch has no value")?;
        if else_value.ty != then_value.ty {
            return Err(format!(
                "if expression branches have different types: {} and {}",
                type_name(then_value.ty),
                type_name(else_value.ty)
            ));
        }
        let else_llvm_value = else_value
            .value
            .ok_or("if expression branch has no LLVM value")?;
        let else_end = self
            .builder
            .get_insert_block()
            .ok_or("if expression else branch has no insertion block")?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(
                self.llvm_type(then_value.ty).ok_or("unit if expression")?,
                "if.value",
            )
            .map_err(|e| e.to_string())?;
        phi.add_incoming(&[(&then_llvm_value, then_end), (&else_llvm_value, else_end)]);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty: then_value.ty,
        })
    }

    fn emit_binary(
        &mut self,
        op: BinaryOp,
        left_expr: &Expr,
        right_expr: &Expr,
    ) -> Result<TypedValue<'ctx>, String> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.emit_logical(op, left_expr, right_expr);
        }
        let left = self.emit_expression(left_expr)?;
        let right = self.emit_expression(right_expr)?;
        if left.ty != right.ty {
            return Err(format!(
                "operator operands have different types: {} and {}",
                type_name(left.ty),
                type_name(right.ty)
            ));
        }
        if left.ty == ValueType::Unit {
            return Err("void expressions cannot be used as operands".into());
        }
        if matches!(left.ty, ValueType::Array(_, _))
            && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
        {
            return Err("array values cannot be compared".into());
        }
        if matches!(left.ty, ValueType::Enum(_))
            && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
        {
            return Err("enum values cannot be compared".into());
        }
        if left.ty == ValueType::Str {
            return self.emit_string_equality(op, left, right);
        }
        let lhs = left
            .value
            .ok_or("left operand has no value")?
            .into_int_value();
        let rhs = right
            .value
            .ok_or("right operand has no value")?
            .into_int_value();
        let result = match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "arithmetic operators require i64, got {}",
                        type_name(left.ty)
                    ));
                }
                if matches!(op, BinaryOp::Divide | BinaryOp::Remainder) {
                    self.guard_signed_division_operands(lhs, rhs)?;
                }
                let value = match op {
                    BinaryOp::Add => self.builder.build_int_add(lhs, rhs, "add"),
                    BinaryOp::Subtract => self.builder.build_int_sub(lhs, rhs, "sub"),
                    BinaryOp::Multiply => self.builder.build_int_mul(lhs, rhs, "mul"),
                    BinaryOp::Divide => self.builder.build_int_signed_div(lhs, rhs, "div"),
                    BinaryOp::Remainder => self.builder.build_int_signed_rem(lhs, rhs, "rem"),
                    _ => unreachable!(),
                }
                .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::I64,
                }
            }
            BinaryOp::And | BinaryOp::Or => unreachable!(),
            BinaryOp::Equal | BinaryOp::NotEqual => {
                let pred = if matches!(op, BinaryOp::Equal) {
                    IntPredicate::EQ
                } else {
                    IntPredicate::NE
                };
                let value = self
                    .builder
                    .build_int_compare(pred, lhs, rhs, "eq")
                    .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::Bool,
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                if left.ty != ValueType::I64 {
                    return Err(format!(
                        "ordering comparisons require i64, got {}",
                        type_name(left.ty)
                    ));
                }
                let pred = match op {
                    BinaryOp::Less => IntPredicate::SLT,
                    BinaryOp::LessEqual => IntPredicate::SLE,
                    BinaryOp::Greater => IntPredicate::SGT,
                    BinaryOp::GreaterEqual => IntPredicate::SGE,
                    _ => unreachable!(),
                };
                let value = self
                    .builder
                    .build_int_compare(pred, lhs, rhs, "cmp")
                    .map_err(|e| e.to_string())?;
                TypedValue {
                    value: Some(value.into()),
                    ty: ValueType::Bool,
                }
            }
        };
        Ok(result)
    }

    fn guard_signed_division_operands(
        &mut self,
        lhs: IntValue<'ctx>,
        rhs: IntValue<'ctx>,
    ) -> Result<(), String> {
        let i64_type = self.i64_type();
        let zero_divisor = self
            .builder
            .build_int_compare(IntPredicate::EQ, rhs, i64_type.const_zero(), "div.by.zero")
            .map_err(|e| e.to_string())?;
        let is_min = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                lhs,
                i64_type.const_int(1u64 << 63, false),
                "div.is.min",
            )
            .map_err(|e| e.to_string())?;
        let is_negative_one = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                rhs,
                i64_type.const_all_ones(),
                "div.is.negative.one",
            )
            .map_err(|e| e.to_string())?;
        let min_overflow = self
            .builder
            .build_and(is_min, is_negative_one, "div.overflow")
            .map_err(|e| e.to_string())?;
        let invalid = self
            .builder
            .build_or(zero_divisor, min_overflow, "div.invalid")
            .map_err(|e| e.to_string())?;
        let function = self.current_function.ok_or("division outside function")?;
        let failure = self
            .context
            .append_basic_block(function, "arithmetic.error");
        let valid = self
            .context
            .append_basic_block(function, "arithmetic.valid");
        self.builder
            .build_conditional_branch(invalid, failure, valid)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(failure);
        let exit = self
            .module
            .get_function("exit")
            .ok_or("internal error: missing runtime exit declaration")?;
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "arithmetic.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }

    fn emit_string_equality(
        &mut self,
        op: BinaryOp,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, String> {
        let pred = match op {
            BinaryOp::Equal => IntPredicate::EQ,
            BinaryOp::NotEqual => IntPredicate::NE,
            _ => {
                return Err(format!(
                    "operator is not supported for {}",
                    type_name(ValueType::Str)
                ));
            }
        };
        let lhs = left.value.ok_or("left operand has no value")?;
        let rhs = right.value.ok_or("right operand has no value")?;
        let strcmp = self.strcmp_function();
        let ordering = self
            .builder
            .build_call(strcmp, &[lhs.into(), rhs.into()], "strcmp")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("internal error: strcmp returned void")?
            .into_int_value();
        let value = self
            .builder
            .build_int_compare(
                pred,
                ordering,
                self.context.i32_type().const_zero(),
                "str.eq",
            )
            .map_err(|e| e.to_string())?;
        Ok(TypedValue {
            value: Some(value.into()),
            ty: ValueType::Bool,
        })
    }

    fn emit_logical(
        &mut self,
        op: BinaryOp,
        left_expr: &Expr,
        right_expr: &Expr,
    ) -> Result<TypedValue<'ctx>, String> {
        let left = self.emit_expression(left_expr)?;
        let lhs = self.expect_bool(left, "logical operator operand")?;
        let function = self
            .current_function
            .ok_or("logical expression outside function")?;
        let left_block = self
            .builder
            .get_insert_block()
            .ok_or("logical expression has no insertion block")?;
        let rhs_block = self.context.append_basic_block(function, "logic.rhs");
        let end_block = self.context.append_basic_block(function, "logic.end");
        let is_and = matches!(op, BinaryOp::And);
        if is_and {
            self.builder
                .build_conditional_branch(lhs, rhs_block, end_block)
                .map_err(|e| e.to_string())?;
        } else {
            self.builder
                .build_conditional_branch(lhs, end_block, rhs_block)
                .map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(rhs_block);
        let right = self.emit_expression(right_expr)?;
        let rhs = self.expect_bool(right, "logical operator operand")?;
        let rhs_end = self
            .builder
            .get_insert_block()
            .ok_or("logical right operand has no insertion block")?;
        self.builder
            .build_unconditional_branch(end_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(end_block);
        let phi = self
            .builder
            .build_phi(self.bool_type(), "logic")
            .map_err(|e| e.to_string())?;
        let short_value = self.bool_type().const_int(u64::from(!is_and), false);
        phi.add_incoming(&[(&short_value, left_block), (&rhs, rhs_end)]);
        Ok(TypedValue {
            value: Some(phi.as_basic_value()),
            ty: ValueType::Bool,
        })
    }

    fn emit_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, String> {
        if name == "len" {
            if args.len() != 1 {
                return Err(format!(
                    "line {}:{}: function `len` expects 1 argument(s), got {}",
                    span.line,
                    span.column,
                    args.len()
                ));
            }
            let value = self.emit_expression(&args[0])?;
            let ValueType::Array(_, length) = value.ty else {
                return Err(format!(
                    "line {}:{}: argument 1 to `len` must be an array",
                    span.line, span.column
                ));
            };
            return Ok(TypedValue {
                value: Some(self.i64_type().const_int(u64::from(length), false).into()),
                ty: ValueType::I64,
            });
        }
        let (params, ret) = self.signatures.get(name).cloned().ok_or_else(|| {
            format!(
                "line {}:{}: unknown function `{name}`",
                span.line, span.column
            )
        })?;
        if args.len() != params.len() {
            return Err(format!(
                "line {}:{}: function `{name}` expects {} argument(s), got {}",
                span.line,
                span.column,
                params.len(),
                args.len()
            ));
        }
        let mut values: Vec<BasicMetadataValueEnum<'ctx>> = Vec::with_capacity(args.len());
        for (index, (arg, expected)) in args.iter().zip(params.iter()).enumerate() {
            let value = self.emit_expression(arg)?;
            if value.ty != *expected {
                return Err(format!(
                    "line {}:{}: argument {} to `{name}` expects {}, got {}",
                    span.line,
                    span.column,
                    index + 1,
                    type_name(*expected),
                    type_name(value.ty)
                ));
            }
            values.push(value.value.ok_or("call argument has no value")?.into());
        }
        let function = *self.functions.get(name).ok_or_else(|| {
            format!(
                "line {}:{}: missing function `{name}`",
                span.line, span.column
            )
        })?;
        let call = self
            .builder
            .build_call(function, &values, "call")
            .map_err(|e| e.to_string())?;
        let value = call.try_as_basic_value().basic();
        if ret == ValueType::Unit && value.is_some() {
            return Err("internal error: void call returned a value".into());
        }
        if ret != ValueType::Unit && value.is_none() {
            return Err("internal error: value call returned void".into());
        }
        Ok(TypedValue { value, ty: ret })
    }

    fn expect_bool(
        &self,
        value: TypedValue<'ctx>,
        context: &str,
    ) -> Result<IntValue<'ctx>, String> {
        if value.ty != ValueType::Bool {
            return Err(format!(
                "{context} must be bool, got {}",
                type_name(value.ty)
            ));
        }
        Ok(value
            .value
            .ok_or_else(|| format!("{context} has no value"))?
            .into_int_value())
    }

    fn lookup_local(&self, name: &str) -> Option<Local<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn create_entry_alloca(&self, ty: ValueType, name: &str) -> Result<PointerValue<'ctx>, String> {
        let function = self.current_function.ok_or("alloca outside function")?;
        let entry = function
            .get_first_basic_block()
            .ok_or("function has no entry block")?;
        let alloca_builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            alloca_builder.position_before(&first);
        } else {
            alloca_builder.position_at_end(entry);
        }
        alloca_builder
            .build_alloca(self.llvm_type(ty).ok_or("cannot allocate unit")?, name)
            .map_err(|e| e.to_string())
    }

    fn target_machine(&mut self) -> Result<TargetMachine, String> {
        Target::initialize_x86(&InitializationConfig::default());
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("could not create native LLVM target machine")?;
        self.module.set_triple(&triple);
        self.module
            .set_data_layout(&machine.get_target_data().get_data_layout());
        Ok(machine)
    }

    fn write_object(&mut self, path: &Path) -> Result<(), String> {
        let machine = self.target_machine()?;
        machine
            .write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| e.to_string())
    }

    fn write_llvm(&mut self, path: &Path) -> Result<(), String> {
        self.target_machine()?;
        self.module.print_to_file(path).map_err(|e| e.to_string())
    }
}

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

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
        }
    }
}
