use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn for_program(
        context: &'ctx Context,
        program: &Program,
    ) -> Result<Self, Diagnostic> {
        let mut codegen = Self::new(context, "vaca");
        codegen.lambda_captures = program.lambda_captures.clone();
        codegen.declare_enums(program)?;
        codegen.declare_structs(program)?;
        codegen.define_enums()?;
        codegen.declare_functions(program)?;
        codegen.declare_globals(program).map_err(|error| {
            if error.span.is_some() {
                error
            } else if let Some(declaration) = program.globals.first() {
                Diagnostic::at(declaration.span.into(), error.message)
            } else {
                error
            }
        })?;
        codegen.emit_print_builtins()?;
        codegen.emit_read_i64_builtin()?;
        codegen.emit_allocator_builtin()?;
        codegen.emit_string_concat_builtin()?;
        codegen.emit_vector_runtime()?;
        codegen.emit_function_adapters()?;
        codegen.define_globals(program)?;
        for function in &program.functions {
            codegen.module_context = function.module.clone();
            codegen.emit_function(function).map_err(|error| {
                if error.span.is_some() {
                    error
                } else {
                    Diagnostic::at(function.span.into(), error.message)
                }
            })?;
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
            reference_types: Vec::new(),
            reference_type_ids: HashMap::new(),
            mutable_reference_types: Vec::new(),
            mutable_reference_type_ids: HashMap::new(),
            raw_pointer_types: Vec::new(),
            raw_pointer_type_ids: HashMap::new(),
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
            next_binding_id: 0,
            lambda_captures: HashMap::new(),
            lambda_capture_types: HashMap::new(),
            function_adapters: HashMap::new(),
        }
    }

    pub(super) fn next_binding_id(&mut self) -> usize {
        let id = self.next_binding_id;
        self.next_binding_id += 1;
        id
    }

    pub(super) fn argument_origin(&self, expression: &Expr) -> Option<MutableReferenceOrigin> {
        let Expr::Variable(name, _) = expression else {
            return None;
        };
        let Some(local) = self.lookup_local(name) else {
            return None;
        };
        if !matches!(local.ty, ValueType::MutableReference(_)) {
            return None;
        }
        Some(
            local
                .mutable_reference_origin
                .clone()
                .unwrap_or(MutableReferenceOrigin::Unknown),
        )
    }

    pub(super) fn reject_overlapping_mutable_arguments(
        &self,
        args: &[Expr],
        params: &[ValueType],
    ) -> Result<(), Diagnostic> {
        let mutable_count = args
            .iter()
            .zip(params)
            .filter(|(_, parameter)| matches!(parameter, ValueType::MutableReference(_)))
            .count();
        if mutable_count > 1
            && args.iter().zip(params).any(|(argument, parameter)| {
                matches!(parameter, ValueType::MutableReference(_))
                    && self.argument_origin(argument).is_none()
            })
        {
            return Err(Diagnostic::at(
                args.iter()
                    .zip(params)
                    .find(|(argument, parameter)| {
                        matches!(parameter, ValueType::MutableReference(_))
                            && self.argument_origin(argument).is_none()
                    })
                    .map(|(argument, _)| codegen_span_of_expr(argument))
                    .unwrap_or_else(|| codegen_span_of_expr(&args[0]))
                    .into(),
                "overlapping mutable references cannot be passed to the same call",
            ));
        }
        let mut origins: Vec<MutableReferenceOrigin> = Vec::new();
        for (argument, parameter) in args.iter().zip(params) {
            if matches!(parameter, ValueType::MutableReference(_)) {
                let Some(origin) = self.argument_origin(argument) else {
                    continue;
                };
                if origins.iter().any(|prior| prior.overlaps(&origin)) {
                    return Err(Diagnostic::at(
                        codegen_span_of_expr(argument).into(),
                        "overlapping mutable references cannot be passed to the same call",
                    ));
                }
                origins.push(origin);
            }
        }
        Ok(())
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
            ValueType::Reference(_) | ValueType::MutableReference(_) | ValueType::RawPointer(_) => {
                Some(self.context.ptr_type(AddressSpace::default()).into())
            }
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
    ) -> Result<FunctionType<'ctx>, Diagnostic> {
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

    pub(super) fn resolve_type(&mut self, ty: &Type, span: Span) -> Result<ValueType, Diagnostic> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::F32 => ValueType::F32,
            Type::F64 => ValueType::F64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Reference(element, _) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references cannot target `unit`"),
                    ));
                }
                if matches!(
                    element,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references to references are not supported"),
                    ));
                }
                if !self
                    .reference_target_is_supported(element, &mut std::collections::HashSet::new())?
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references to values containing vectors are not supported"),
                    ));
                }
                self.reference_type(element)
            }
            Type::MutableReference(element, _) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit
                    || matches!(
                        element,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    )
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("invalid mutable reference target"),
                    ));
                }
                if !self
                    .reference_target_is_supported(element, &mut std::collections::HashSet::new())?
                {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("references to values containing vectors are not supported"),
                    ));
                }
                self.mutable_reference_type(element)
            }
            Type::RawPointer(element, mutable) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("raw pointers cannot target `unit`"),
                    ));
                }
                if matches!(
                    element,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("raw pointers cannot target safe references"),
                    ));
                }
                self.raw_pointer_type(element, *mutable)
            }
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("tuple types must contain at least one element"),
                    ));
                }
                let mut types = Vec::with_capacity(elements.len());
                for element in elements {
                    let ty = self.resolve_type(element, span)?;
                    if ty == ValueType::Unit {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("tuple elements cannot have type `unit`"),
                        ));
                    }
                    if matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_)) {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("references cannot be stored in tuples"),
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
                        return Err(Diagnostic::at(
                            span.into(),
                            format!("function type parameters cannot have type `unit`"),
                        ));
                    }
                    param_types.push(param_type);
                }
                let return_type = self.resolve_type(result, span)?;
                if matches!(
                    return_type,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("function types cannot return references"),
                    ));
                }
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
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("unknown type `{name}`"),
                    ));
                }
            }
            Type::Applied(name, _) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic type `{name}` was not specialized"),
                ));
            }
            Type::Parameter(name) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic parameter `{name}` was not specialized"),
                ));
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

    pub(super) fn reference_type(&mut self, element: ValueType) -> ValueType {
        let index = if let Some(index) = self.reference_type_ids.get(&element) {
            *index
        } else {
            let index = self.reference_types.len();
            self.reference_types.push(element);
            self.reference_type_ids.insert(element, index);
            index
        };
        ValueType::Reference(index)
    }

    pub(super) fn mutable_reference_type(&mut self, element: ValueType) -> ValueType {
        let index = if let Some(index) = self.mutable_reference_type_ids.get(&element) {
            *index
        } else {
            let index = self.mutable_reference_types.len();
            self.mutable_reference_types.push(element);
            self.mutable_reference_type_ids.insert(element, index);
            index
        };
        ValueType::MutableReference(index)
    }

    pub(super) fn raw_pointer_type(&mut self, element: ValueType, mutable: bool) -> ValueType {
        let key = (element, mutable);
        let index = if let Some(index) = self.raw_pointer_type_ids.get(&key) {
            *index
        } else {
            let index = self.raw_pointer_types.len();
            self.raw_pointer_types.push(key);
            self.raw_pointer_type_ids.insert(key, index);
            index
        };
        ValueType::RawPointer(index)
    }

    pub(super) fn reference_target_is_supported(
        &mut self,
        ty: ValueType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        if !visited.insert(ty) {
            return Ok(true);
        }
        match ty {
            ValueType::Vector(_) => Ok(false),
            ValueType::Array(index) => {
                self.reference_element_is_supported(self.array_shapes[index].element, visited)
            }
            ValueType::Tuple(index) => {
                for element in self.tuple_shapes[index].clone() {
                    if !self.reference_target_is_supported(element, visited)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ValueType::Struct(index) => {
                for field in self.structs[index].fields.clone() {
                    let field_type = self.resolve_type(&field.ty, field.span)?;
                    if !self.reference_target_is_supported(field_type, visited)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ValueType::Enum(index) => {
                for variant in self.enums[index].variants.clone() {
                    for payload in variant.payload {
                        let payload_type = self.resolve_type(&payload, variant.span)?;
                        if !self.reference_target_is_supported(payload_type, visited)? {
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            }
            ValueType::I64
            | ValueType::F32
            | ValueType::F64
            | ValueType::Bool
            | ValueType::Str
            | ValueType::Unit
            | ValueType::Function(_)
            | ValueType::Reference(_)
            | ValueType::MutableReference(_)
            | ValueType::RawPointer(_) => Ok(true),
        }
    }

    fn reference_element_is_supported(
        &mut self,
        element: ArrayElementType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        self.reference_target_is_supported(ValueType::from(element), visited)
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
    ) -> Result<FunctionType<'ctx>, Diagnostic> {
        let shape = &self.function_shapes[function_id];
        self.closure_function_type(&shape.params, shape.return_type)
    }

    pub(super) fn closure_function_type(
        &self,
        params: &[ValueType],
        ret: ValueType,
    ) -> Result<FunctionType<'ctx>, Diagnostic> {
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

    fn emit_function_adapters(&mut self) -> Result<(), Diagnostic> {
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
    ) -> Result<StructType<'ctx>, Diagnostic> {
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
    ) -> Result<TypedValue<'ctx>, Diagnostic> {
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

    pub(super) fn declare_enums(&mut self, program: &Program) -> Result<(), Diagnostic> {
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
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("enum `{}` is defined more than once", declaration.name),
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

    pub(super) fn define_enums(&mut self) -> Result<(), Diagnostic> {
        for enum_id in 0..self.enums.len() {
            let declaration = self.enums[enum_id].clone();
            let mut fields: Vec<BasicTypeEnum<'ctx>> = vec![self.context.i32_type().into()];
            let mut payload_types = Vec::with_capacity(declaration.variants.len());
            for variant in &declaration.variants {
                let mut payload_fields = Vec::with_capacity(variant.payload.len());
                for ty in &variant.payload {
                    let value_type = self.resolve_type(ty, variant.span)?;
                    if matches!(
                        value_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(Diagnostic::at(
                            variant.span.into(),
                            format!("references cannot be stored in enums"),
                        ));
                    }
                    if value_type == ValueType::Unit {
                        return Err(Diagnostic::at(
                            variant.span.into(),
                            format!("enum payloads cannot have type `unit`"),
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
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("enum `{}` was already defined", declaration.name),
                ));
            }
            self.enum_payload_types[enum_id] = payload_types;
        }
        Ok(())
    }

    pub(super) fn declare_structs(&mut self, program: &Program) -> Result<(), Diagnostic> {
        self.structs = program.structs.clone();
        for (index, declaration) in program.structs.iter().enumerate() {
            if self.enum_ids.contains_key(&declaration.name)
                || self
                    .struct_ids
                    .insert(declaration.name.clone(), index)
                    .is_some()
            {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("type `{}` is defined more than once", declaration.name),
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
                    Type::I64
                    | Type::Bool
                    | Type::Str
                    | Type::Array(_, _)
                    | Type::Vector(_)
                    | Type::RawPointer(_, _) => {}
                    Type::Tuple(_) => {}
                    Type::F32 | Type::F64 => {}
                    Type::Function(_, _) => {}
                    Type::Named(name) if self.struct_ids.contains_key(name) => {}
                    Type::Named(name) if self.enum_ids.contains_key(name) => {}
                    Type::Named(name) => {
                        return Err(Diagnostic::at(
                            field.span.into(),
                            format!("unknown type `{name}`"),
                        ));
                    }
                    Type::Unit => {
                        return Err(Diagnostic::at(
                            field.span.into(),
                            format!("struct fields cannot have type `unit`"),
                        ));
                    }
                    Type::Reference(_, _) | Type::MutableReference(_, _) => {
                        return Err(Diagnostic::at(
                            field.span.into(),
                            format!("references cannot be stored in structs"),
                        ));
                    }
                    Type::Applied(name, _) => {
                        return Err(Diagnostic::at(
                            field.span.into(),
                            format!("generic type `{name}` was not specialized"),
                        ));
                    }
                    Type::Parameter(name) => {
                        return Err(Diagnostic::at(
                            field.span.into(),
                            format!("generic parameter `{name}` was not specialized"),
                        ));
                    }
                }
            }
        }
        crate::aggregate_validation::validate_aggregate_cycles(&self.enums, &self.structs)?;
        for declaration in program.structs.iter() {
            self.module_context = declaration.module.clone();
            self.module_context = declaration.module.clone();
            let mut field_types = Vec::with_capacity(declaration.fields.len());
            let mut names = std::collections::HashSet::new();
            for field in &declaration.fields {
                if !names.insert(field.name.as_str()) {
                    return Err(Diagnostic::at(
                        field.span.into(),
                        format!("struct field `{}` is defined more than once", field.name),
                    ));
                }
                let ty = self.resolve_type(&field.ty, field.span)?;
                let llvm_type = self.llvm_type(ty).ok_or("struct field has no LLVM type")?;
                field_types.push(llvm_type);
            }
            if !self.struct_types[self.struct_ids[&declaration.name]].set_body(&field_types, false)
            {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("struct `{}` was already defined", declaration.name),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn declare_functions(&mut self, program: &Program) -> Result<(), Diagnostic> {
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
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!("`{}` is a reserved compiler/runtime name", function.name),
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!("function `{}` is defined more than once", function.name),
                ));
            }
            let mut names = std::collections::HashSet::new();
            for param in &function.params {
                if param.ty == Type::Unit {
                    return Err(Diagnostic::at(
                        param.span.into(),
                        format!("parameters cannot have type `unit`"),
                    ));
                }
                if !names.insert(param.name.as_str()) {
                    return Err(Diagnostic::at(
                        param.span.into(),
                        format!("duplicate parameter `{}`", param.name),
                    ));
                }
            }
            let params: Vec<ValueType> = function
                .params
                .iter()
                .map(|p| self.resolve_type(&p.ty, p.span))
                .collect::<Result<_, _>>()?;
            let ret: ValueType = self.resolve_type(&function.return_type, function.span)?;
            if matches!(
                ret,
                ValueType::Reference(_) | ValueType::MutableReference(_)
            ) {
                if matches!(ret, ValueType::MutableReference(_)) {
                    return Err(Diagnostic::at(
                        function.span.into(),
                        "functions cannot return references",
                    ));
                }
                let lifetime =
                    codegen_reference_lifetime(&function.return_type).ok_or_else(|| {
                        Diagnostic::at(
                            function.span.into(),
                            "functions returning references must specify a named return lifetime",
                        )
                    })?;
                if !function.lifetime_params.contains(&lifetime)
                    || !function.params.iter().any(|param| {
                        codegen_reference_lifetime(&param.ty).as_deref() == Some(&lifetime)
                    })
                {
                    return Err(Diagnostic::at(
                        function.span.into(),
                        "returned reference lifetime must be declared and appear on a reference parameter",
                    ));
                }
            }
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
                    return Err(Diagnostic::at(
                        function.span.into(),
                        "entry point must have signature `fn main() -> i64`",
                    ));
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
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!(
                        "unsupported C ABI type in extern function `{}`",
                        function.name
                    ),
                ));
            }
            if let Some(previous) = c_symbols.get(&function.link_name) {
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!(
                        "C symbol `{}` conflicts with another LLVM function declared at {}:{}:{}",
                        function.link_name,
                        previous
                            .file
                            .and_then(|source_id| program.source_files.get(source_id))
                            .map(String::as_str)
                            .unwrap_or("unknown source"),
                        previous.line,
                        previous.column
                    ),
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!("function `{}` is defined more than once", function.name),
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
                return Err(Diagnostic::at(
                    function.span.into(),
                    format!(
                        "unsupported C ABI type in extern function `{}`",
                        function.name
                    ),
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
            let message = "program must define the required entry point `fn main() -> i64`";
            return Err(program
                .functions
                .iter()
                .find(|function| function.module.is_empty())
                .map(|function| Diagnostic::at(function.span.into(), message))
                .unwrap_or_else(|| {
                    if program.source_files.is_empty() {
                        Diagnostic::new(message)
                    } else {
                        Diagnostic::at(
                            crate::diagnostic::SourceSpan {
                                line: 1,
                                column: 1,
                                end_column: 1,
                                file: Some(0),
                            },
                            message,
                        )
                    }
                }));
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

fn codegen_reference_lifetime(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(_, lifetime) | Type::MutableReference(_, lifetime) => lifetime.clone(),
        _ => None,
    }
}
