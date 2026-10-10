use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn for_program(context: &'ctx Context, program: &Program) -> Result<Self, String> {
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

    pub(super) fn new(context: &'ctx Context, name: &str) -> Self {
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

    pub(super) fn i64_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.i64_type()
    }

    pub(super) fn bool_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.bool_type()
    }

    pub(super) fn llvm_type(&self, ty: ValueType) -> Option<BasicTypeEnum<'ctx>> {
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

    pub(super) fn llvm_element_type(
        &self,
        element: ArrayElementType,
    ) -> inkwell::types::BasicTypeEnum<'ctx> {
        match element {
            ArrayElementType::I64 => self.i64_type().into(),
            ArrayElementType::Bool => self.bool_type().into(),
            ArrayElementType::Str => self.context.ptr_type(AddressSpace::default()).into(),
        }
    }

    pub(super) fn llvm_fn_type(
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

    pub(super) fn resolve_type(&self, ty: &Type, span: Span) -> Result<ValueType, String> {
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

    pub(super) fn declare_enums(&mut self, program: &Program) -> Result<(), String> {
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

    pub(super) fn declare_structs(&mut self, program: &Program) -> Result<(), String> {
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
            self.struct_types.push(
                self.context
                    .opaque_struct_type(&format!("apex.struct.{}", declaration.name)),
            );
        }
        for declaration in &self.structs {
            for field in &declaration.fields {
                match &field.ty {
                    Type::I64 | Type::Bool | Type::Str | Type::Array(_, _) => {}
                    Type::Named(name) if self.struct_ids.contains_key(name) => {}
                    Type::Named(name) if self.enum_ids.contains_key(name) => {
                        return Err(format!(
                            "line {}:{}: struct fields cannot have enum type `{name}`",
                            field.span.line, field.span.column
                        ));
                    }
                    Type::Named(name) => {
                        return Err(format!(
                            "line {}:{}: unknown type `{name}`",
                            field.span.line, field.span.column
                        ));
                    }
                    Type::Unit => {
                        return Err(format!(
                            "line {}:{}: struct fields cannot have type `unit`",
                            field.span.line, field.span.column
                        ));
                    }
                }
            }
        }
        validate_struct_cycles(&self.structs, &self.struct_ids)?;
        for declaration in program.structs.iter() {
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
            if !self.struct_types[self.struct_ids[&declaration.name]].set_body(&field_types, false)
            {
                return Err(format!("struct `{}` was already defined", declaration.name));
            }
        }
        Ok(())
    }

    pub(super) fn declare_functions(&mut self, program: &Program) -> Result<(), String> {
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
}

fn validate_struct_cycles(
    structs: &[StructDecl],
    struct_ids: &HashMap<String, usize>,
) -> Result<(), String> {
    fn visit(
        id: usize,
        structs: &[StructDecl],
        struct_ids: &HashMap<String, usize>,
        states: &mut [u8],
    ) -> Result<(), String> {
        states[id] = 1;
        for field in &structs[id].fields {
            let Type::Named(name) = &field.ty else {
                continue;
            };
            let Some(&child_id) = struct_ids.get(name) else {
                continue;
            };
            if states[child_id] == 1 {
                return Err(format!(
                    "line {}:{}: struct `{}` has a recursive by-value field `{}`",
                    field.span.line, field.span.column, structs[id].name, field.name
                ));
            }
            if states[child_id] == 0 {
                visit(child_id, structs, struct_ids, states)?;
            }
        }
        states[id] = 2;
        Ok(())
    }

    let mut states = vec![0; structs.len()];
    for id in 0..structs.len() {
        if states[id] == 0 {
            visit(id, structs, struct_ids, &mut states)?;
        }
    }
    Ok(())
}
