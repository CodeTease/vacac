use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn for_program(context: &'ctx Context, program: &Program) -> Result<Self, String> {
        let mut codegen = Self::new(context, "vaca");
        codegen.lambda_captures = program.lambda_captures.clone();
        codegen.declare_enums(program)?;
        codegen.declare_structs(program)?;
        codegen.define_enums()?;
        codegen.declare_functions(program)?;
        codegen.declare_globals(program)?;
        codegen.emit_print_builtins()?;
        codegen.emit_read_i64_builtin()?;
        codegen.emit_allocator_builtin()?;
        codegen.emit_string_concat_builtin()?;
        codegen.emit_vector_runtime()?;
        codegen.emit_function_adapters()?;
        codegen.define_globals(program)?;
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
            tuple_shapes: Vec::new(),
            tuple_shape_ids: HashMap::new(),
            array_shapes: Vec::new(),
            array_shape_ids: HashMap::new(),
            vector_shapes: Vec::new(),
            vector_shape_ids: HashMap::new(),
            function_shapes: Vec::new(),
            function_shape_ids: HashMap::new(),
            scopes: Vec::new(),
            string_literals: HashMap::new(),
            loop_targets: Vec::new(),
            current_function: None,
            return_type: ValueType::I64,
            module_context: String::new(),
            function_visibility: HashMap::new(),
            enum_visibility: HashMap::new(),
            struct_visibility: HashMap::new(),
            global_declarations: HashMap::new(),
            global_types: HashMap::new(),
            globals: HashMap::new(),
            global_constant_values: HashMap::new(),
            global_states: HashMap::new(),
            next_lambda_id: 0,
            lambda_captures: HashMap::new(),
            lambda_capture_types: HashMap::new(),
            function_adapters: HashMap::new(),
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
            ValueType::F32 => Some(self.context.f32_type().into()),
            ValueType::F64 => Some(self.context.f64_type().into()),
            ValueType::Bool => Some(self.bool_type().into()),
            ValueType::Str => Some(self.context.ptr_type(AddressSpace::default()).into()),
            ValueType::Function(_) => Some(
                self.context
                    .struct_type(
                        &[
                            self.context.ptr_type(AddressSpace::default()).into(),
                            self.context.ptr_type(AddressSpace::default()).into(),
                        ],
                        false,
                    )
                    .into(),
            ),
            ValueType::Array(index) => Some(self.llvm_array_type(index)?.into()),
            ValueType::Tuple(index) => {
                let elements = self.tuple_shapes[index].clone();
                let types = elements
                    .into_iter()
                    .map(|ty| self.llvm_type(ty))
                    .collect::<Option<Vec<_>>>()?;
                Some(self.context.struct_type(&types, false).into())
            }
            ValueType::Vector(_) => Some(self.context.ptr_type(AddressSpace::default()).into()),
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
            Type::F32 => ValueType::F32,
            Type::F64 => ValueType::F64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    return Err(format!(
                        "line {}:{}: tuple types must contain at least one element",
                        span.line, span.column
                    ));
                }
                let mut types = Vec::with_capacity(elements.len());
                for element in elements {
                    let ty = self.resolve_type(element, span)?;
                    if ty == ValueType::Unit {
                        return Err(format!(
                            "line {}:{}: tuple elements cannot have type `unit`",
                            span.line, span.column
                        ));
                    }
                    types.push(ty);
                }
                self.tuple_type(types)
            }
            Type::Function(params, result) => {
                let mut param_types = Vec::with_capacity(params.len());
                for param in params {
                    let param_type = self.resolve_type(param, span)?;
                    if param_type == ValueType::Unit {
                        return Err(format!(
                            "line {}:{}: function type parameters cannot have type `unit`",
                            span.line, span.column
                        ));
                    }
                    param_types.push(param_type);
                }
                let return_type = self.resolve_type(result, span)?;
                self.function_type(param_types, return_type)
            }
            Type::Array(element, length) => {
                let element = self.resolve_array_element(element, span)?;
                self.array_type(element, *length)
            }
            Type::Vector(element) => {
                let element = self.resolve_array_element(element, span)?;
                self.vector_type(element)
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
            Type::Applied(name, _) => {
                return Err(format!("generic type `{name}` was not specialized"));
            }
            Type::Parameter(name) => {
                return Err(format!("generic parameter `{name}` was not specialized"));
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

    pub(super) fn tuple_type(&mut self, elements: Vec<ValueType>) -> ValueType {
        let index = if let Some(index) = self.tuple_shape_ids.get(&elements) {
            *index
        } else {
            let index = self.tuple_shapes.len();
            self.tuple_shapes.push(elements.clone());
            self.tuple_shape_ids.insert(elements, index);
            index
        };
        ValueType::Tuple(index)
    }

    pub(super) fn function_type(
        &mut self,
        params: Vec<ValueType>,
        return_type: ValueType,
    ) -> ValueType {
        let shape = FunctionShape {
            params,
            return_type,
        };
        let index = if let Some(index) = self.function_shape_ids.get(&shape) {
            *index
        } else {
            let index = self.function_shapes.len();
            self.function_shapes.push(shape.clone());
            self.function_shape_ids.insert(shape, index);
            index
        };
        ValueType::Function(index)
    }

    pub(super) fn llvm_function_type(
        &self,
        function_id: usize,
    ) -> Result<FunctionType<'ctx>, String> {
        let shape = &self.function_shapes[function_id];
        self.closure_function_type(&shape.params, shape.return_type)
    }

    pub(super) fn closure_function_type(
        &self,
        params: &[ValueType],
        ret: ValueType,
    ) -> Result<FunctionType<'ctx>, String> {
        let mut closure_params = vec![self.context.ptr_type(AddressSpace::default()).into()];
        for ty in params {
            closure_params.push(
                self.llvm_type(*ty)
                    .ok_or("unit is not a valid parameter type")?
                    .into(),
            );
        }
        match self.llvm_type(ret) {
            Some(ret) => Ok(ret.fn_type(&closure_params, false)),
            None => Ok(self.context.void_type().fn_type(&closure_params, false)),
        }
    }

    fn emit_function_adapters(&mut self) -> Result<(), String> {
        let functions = self
            .functions
            .iter()
            .filter(|(name, _)| {
                !name.starts_with("vaca.lambda.") && !name.starts_with("vaca.adapter.")
            })
            .map(|(name, function)| (name.clone(), *function, self.signatures[name].clone()))
            .collect::<Vec<_>>();
        for (index, (name, target, (params, ret))) in functions.into_iter().enumerate() {
            let adapter_name = format!("vaca.adapter.{index}");
            let adapter = self.module.add_function(
                &adapter_name,
                self.closure_function_type(&params, ret)?,
                Some(Linkage::Internal),
            );
            let entry = self.context.append_basic_block(adapter, "entry");
            let builder = &self.builder;
            builder.position_at_end(entry);
            let mut arguments = Vec::with_capacity(params.len());
            for (param_index, _) in params.iter().enumerate() {
                arguments.push(
                    adapter
                        .get_nth_param(param_index as u32 + 1)
                        .ok_or("adapter parameter missing")?
                        .into(),
                );
            }
            let _environment = adapter
                .get_nth_param(0)
                .ok_or("adapter environment parameter missing")?
                .into_pointer_value();
            let call = builder
                .build_call(target, &arguments, "adapter.call")
                .map_err(|error| error.to_string())?;
            let value = call.try_as_basic_value().basic();
            if ret == ValueType::Unit {
                builder
                    .build_return(None)
                    .map_err(|error| error.to_string())?;
            } else {
                builder
                    .build_return(Some(&value.ok_or("adapter call returned void")?))
                    .map_err(|error| error.to_string())?;
            }
            self.function_adapters.insert(name, adapter);
        }
        Ok(())
    }

    pub(super) fn closure_environment_type(
        &self,
        captures: &[(String, ValueType)],
    ) -> Result<StructType<'ctx>, String> {
        let fields = captures
            .iter()
            .map(|(_, ty)| self.llvm_type(*ty).ok_or("capture has no LLVM type"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.context.struct_type(&fields, false))
    }

    pub(super) fn make_closure(
        &mut self,
        code: PointerValue<'ctx>,
        environment: PointerValue<'ctx>,
        function_type: ValueType,
    ) -> Result<TypedValue<'ctx>, String> {
        let ValueType::Function(_) = function_type else {
            return Err("internal error: closure requires a function type".into());
        };
        let mut result = self
            .llvm_type(function_type)
            .ok_or("function has no LLVM type")?
            .into_struct_type()
            .const_zero()
            .as_basic_value_enum();
        result = self
            .builder
            .build_insert_value(result.into_struct_value(), code, 0, "closure.code")
            .map_err(|error| error.to_string())?
            .as_basic_value_enum();
        result = self
            .builder
            .build_insert_value(result.into_struct_value(), environment, 1, "closure.env")
            .map_err(|error| error.to_string())?
            .as_basic_value_enum();
        Ok(TypedValue {
            value: Some(result),
            ty: function_type,
        })
    }

    pub(super) fn vector_type(&mut self, element: ArrayElementType) -> ValueType {
        let index = if let Some(index) = self.vector_shape_ids.get(&element) {
            *index
        } else {
            let index = self.vector_shapes.len();
            self.vector_shapes.push(VectorShape { element });
            self.vector_shape_ids.insert(element, index);
            index
        };
        ValueType::Vector(index)
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
                    .opaque_struct_type(&format!("vaca.enum.{}", declaration.name)),
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
                    .opaque_struct_type(&format!("vaca.struct.{}", declaration.name)),
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
                    Type::I64 | Type::Bool | Type::Str | Type::Array(_, _) | Type::Vector(_) => {}
                    Type::Tuple(_) => {}
                    Type::F32 | Type::F64 => {}
                    Type::Function(_, _) => {}
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
                    Type::Applied(name, _) => {
                        return Err(format!("generic type `{name}` was not specialized"));
                    }
                    Type::Parameter(name) => {
                        return Err(format!("generic parameter `{name}` was not specialized"));
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
        let mut c_symbols = std::collections::HashMap::<String, Span>::new();
        for function in &program.functions {
            self.module_context = function.module.clone();
            if matches!(
                function.name.as_str(),
                "print_i64"
                    | "print_f32"
                    | "print_f64"
                    | "print"
                    | "println"
                    | "read_i64"
                    | "printf"
                    | "strcmp"
                    | "vaca_main"
                    | "getchar"
                    | "exit"
                    | "strlen"
                    | "malloc"
                    | "vaca_alloc"
                    | "memcpy"
                    | "vaca_str_concat"
                    | "len"
                    | "vec_new"
                    | "vec_push"
                    | "realloc"
                    | "vaca_vec_new"
                    | "vaca_vec_push"
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
                "vaca_main"
            } else if program.functions_are_qualified {
                &function.name
            } else {
                &function.name
            };
            let llvm_fn =
                self.module
                    .add_function(llvm_name, self.llvm_fn_type(&params, ret)?, None);
            c_symbols.insert(llvm_name.to_owned(), function.span);
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
        for function in &program.extern_functions {
            self.module_context = function.module.clone();
            if function
                .params
                .iter()
                .any(|param| !matches!(param.ty, Type::I64 | Type::F32 | Type::F64))
                || !matches!(
                    function.return_type,
                    Type::I64 | Type::F32 | Type::F64 | Type::Unit
                )
            {
                return Err(format!(
                    "line {}:{}: unsupported C ABI type in extern function `{}`",
                    function.span.line, function.span.column, function.name
                ));
            }
            if let Some(previous) = c_symbols.get(&function.link_name) {
                return Err(format!(
                    "line {}:{}: C symbol `{}` conflicts with another LLVM function declared at line {}:{}",
                    function.span.line,
                    function.span.column,
                    function.link_name,
                    previous.line,
                    previous.column
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(format!(
                    "line {}:{}: function `{}` is defined more than once",
                    function.span.line, function.span.column, function.name
                ));
            }
            let params = function
                .params
                .iter()
                .map(|param| self.resolve_type(&param.ty, param.span))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = self.resolve_type(&function.return_type, function.span)?;
            if params
                .iter()
                .any(|ty| !matches!(ty, ValueType::I64 | ValueType::F32 | ValueType::F64))
                || !matches!(
                    ret,
                    ValueType::I64 | ValueType::F32 | ValueType::F64 | ValueType::Unit
                )
            {
                return Err(format!(
                    "line {}:{}: unsupported C ABI type in extern function `{}`",
                    function.span.line, function.span.column, function.name
                ));
            }
            let external = self.module.add_function(
                &function.link_name,
                self.llvm_fn_type(&params, ret)?,
                None,
            );
            c_symbols.insert(function.link_name.clone(), function.span);
            self.functions.insert(function.name.clone(), external);
            self.signatures.insert(function.name.clone(), (params, ret));
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
        self.signatures
            .insert("print_f64".into(), (vec![ValueType::F64], ValueType::Unit));
        self.signatures
            .insert("print_f32".into(), (vec![ValueType::F32], ValueType::Unit));
        for name in ["print", "println"] {
            self.signatures
                .insert(name.into(), (vec![ValueType::Str], ValueType::Unit));
        }
        self.signatures
            .insert("read_i64".into(), (vec![], ValueType::I64));
        Ok(())
    }
}
