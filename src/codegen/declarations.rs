use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn for_program(context: &'ctx Context, program: &Program) -> Result<Self, String> {
        let mut codegen = Self::new(context, "apex");
        codegen.declare_enums(program)?;
        codegen.declare_structs(program)?;
        codegen.define_enums()?;
        codegen.declare_functions(program)?;
        codegen.emit_print_builtins()?;
        codegen.emit_read_i64_builtin()?;
        codegen.emit_string_concat_builtin()?;
        for function in &program.functions {
            codegen.module_context = function.module.clone();
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
            enum_payload_types: Vec::new(),
            structs: Vec::new(),
            struct_ids: HashMap::new(),
            struct_types: Vec::new(),
            array_shapes: Vec::new(),
            array_shape_ids: HashMap::new(),
            scopes: Vec::new(),
            string_literals: HashMap::new(),
            loop_targets: Vec::new(),
            current_function: None,
            return_type: ValueType::I64,
            module_context: String::new(),
            function_visibility: HashMap::new(),
            enum_visibility: HashMap::new(),
            struct_visibility: HashMap::new(),
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
            ValueType::Array(index) => Some(self.llvm_array_type(index)?.into()),
            ValueType::Enum(index) => self.enum_types.get(index).copied().map(Into::into),
            ValueType::Struct(index) => self.struct_types.get(index).copied().map(Into::into),
            ValueType::Unit => None,
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

    pub(super) fn resolve_type(&mut self, ty: &Type, span: Span) -> Result<ValueType, String> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Array(element, length) => {
                let element = match element.as_ref() {
                    Type::I64 => ArrayElementType::I64,
                    Type::Bool => ArrayElementType::Bool,
                    Type::Str => ArrayElementType::Str,
                    Type::Named(name) if self.enum_ids.contains_key(name) => {
                        ArrayElementType::Enum(self.enum_ids[name])
                    }
                    Type::Named(name) if self.struct_ids.contains_key(name) => {
                        ArrayElementType::Struct(self.struct_ids[name])
                    }
                    Type::Named(name) => {
                        return Err(format!(
                            "line {}:{}: unknown type `{name}`",
                            span.line, span.column
                        ));
                    }
                    Type::Array(_, _) => self.resolve_array_element(element, span)?,
                    Type::Unit => {
                        return Err(format!(
                            "line {}:{}: array elements cannot have type `unit`",
                            span.line, span.column
                        ));
                    }
                };
                self.array_type(element, *length)
            }
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

    pub(super) fn array_type(&mut self, element: ArrayElementType, length: u32) -> ValueType {
        let key = (element, length);
        let index = if let Some(index) = self.array_shape_ids.get(&key) {
            *index
        } else {
            let index = self.array_shapes.len();
            self.array_shapes.push(ArrayShape { element, length });
            self.array_shape_ids.insert(key, index);
            index
        };
        ValueType::Array(index)
    }

    fn resolve_array_element(
        &mut self,
        element: &Type,
        span: Span,
    ) -> Result<ArrayElementType, String> {
        match element {
            Type::I64 => Ok(ArrayElementType::I64),
            Type::Bool => Ok(ArrayElementType::Bool),
            Type::Str => Ok(ArrayElementType::Str),
            Type::Array(_, _) => {
                let ValueType::Array(index) = self.resolve_type(element, span)? else {
                    unreachable!()
                };
                Ok(ArrayElementType::Array(index))
            }
            Type::Unit => Err(format!(
                "line {}:{}: array elements cannot have type `unit`",
                span.line, span.column
            )),
            Type::Named(name) if self.enum_ids.contains_key(name) => {
                Ok(ArrayElementType::Enum(self.enum_ids[name]))
            }
            Type::Named(name) if self.struct_ids.contains_key(name) => {
                Ok(ArrayElementType::Struct(self.struct_ids[name]))
            }
            Type::Named(name) => Err(format!(
                "line {}:{}: unknown type `{name}`",
                span.line, span.column
            )),
        }
    }

    pub(super) fn llvm_array_type(&self, index: usize) -> Option<inkwell::types::ArrayType<'ctx>> {
        let shape = self.array_shapes[index];
        Some(
            self.llvm_element_type(shape.element)?
                .array_type(shape.length),
        )
    }

    pub(super) fn llvm_element_type(
        &self,
        element: ArrayElementType,
    ) -> Option<BasicTypeEnum<'ctx>> {
        match element {
            ArrayElementType::Array(index) => Some(self.llvm_array_type(index)?.into()),
            _ => self.llvm_type(ValueType::from(element)),
        }
    }

    pub(super) fn declare_enums(&mut self, program: &Program) -> Result<(), String> {
        self.enums = program.enums.clone();
        for (index, declaration) in program.enums.iter().enumerate() {
            self.enum_visibility.insert(
                declaration.name.clone(),
                (declaration.module.clone(), declaration.is_public),
            );
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
            self.enum_types.push(
                self.context
                    .opaque_struct_type(&format!("apex.enum.{}", declaration.name)),
            );
            self.enum_payload_types.push(Vec::new());
            self.enum_visibility.insert(
                declaration.name.clone(),
                (declaration.module.clone(), declaration.is_public),
            );
        }
        Ok(())
    }

    pub(super) fn define_enums(&mut self) -> Result<(), String> {
        for enum_id in 0..self.enums.len() {
            let declaration = self.enums[enum_id].clone();
            let mut fields: Vec<BasicTypeEnum<'ctx>> = vec![self.context.i32_type().into()];
            let mut payload_types = Vec::with_capacity(declaration.variants.len());
            for variant in &declaration.variants {
                let mut payload_fields = Vec::with_capacity(variant.payload.len());
                for ty in &variant.payload {
                    let value_type = self.resolve_type(ty, variant.span)?;
                    if value_type == ValueType::Unit {
                        return Err(format!(
                            "line {}:{}: enum payloads cannot have type `unit`",
                            variant.span.line, variant.span.column
                        ));
                    }
                    payload_fields.push(
                        self.llvm_type(value_type)
                            .ok_or("enum payload has no LLVM type")?,
                    );
                }
                let payload_type = self.context.struct_type(&payload_fields, false);
                fields.push(payload_type.into());
                payload_types.push(payload_type);
            }
            if !self.enum_types[enum_id].set_body(&fields, false) {
                return Err(format!("enum `{}` was already defined", declaration.name));
            }
            self.enum_payload_types[enum_id] = payload_types;
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
            self.struct_visibility.insert(
                declaration.name.clone(),
                (declaration.module.clone(), declaration.is_public),
            );
        }
        for declaration in &self.structs {
            self.module_context = declaration.module.clone();
            for field in &declaration.fields {
                match &field.ty {
                    Type::I64 | Type::Bool | Type::Str | Type::Array(_, _) => {}
                    Type::Named(name) if self.struct_ids.contains_key(name) => {}
                    Type::Named(name) if self.enum_ids.contains_key(name) => {}
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
        crate::aggregate_validation::validate_aggregate_cycles(&self.enums, &self.structs)
            .map_err(|error| error.to_string())?;
        for declaration in program.structs.iter() {
            self.module_context = declaration.module.clone();
            self.module_context = declaration.module.clone();
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
            self.module_context = function.module.clone();
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
                    | "strlen"
                    | "malloc"
                    | "memcpy"
                    | "apex_str_concat"
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
            } else if program.functions_are_qualified {
                &function.name
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
            self.function_visibility.insert(
                function.name.clone(),
                (function.module.clone(), function.is_public),
            );
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
