use super::*;

impl Checker {
    pub(super) fn resolve_type(&mut self, ty: &Type, span: Span) -> Result<ValueType, Diagnostic> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::F32 => ValueType::F32,
            Type::F64 => ValueType::F64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Reference(element) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit {
                    return Err(at(span, "references cannot target `unit`"));
                }
                if matches!(
                    element,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(span, "references to references are not supported"));
                }
                if !self
                    .reference_target_is_supported(element, &mut std::collections::HashSet::new())?
                {
                    return Err(at(
                        span,
                        "references to values containing vectors are not supported",
                    ));
                }
                self.reference_type(element)
            }
            Type::MutableReference(element) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit {
                    return Err(at(span, "references cannot target `unit`"));
                }
                if matches!(
                    element,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(span, "references to references are not supported"));
                }
                if !self
                    .reference_target_is_supported(element, &mut std::collections::HashSet::new())?
                {
                    return Err(at(
                        span,
                        "references to values containing vectors are not supported",
                    ));
                }
                self.mutable_reference_type(element)
            }
            Type::RawPointer(element, mutable) => {
                let element = self.resolve_type(element, span)?;
                if element == ValueType::Unit {
                    return Err(at(span, "raw pointers cannot target `unit`"));
                }
                if matches!(
                    element,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(span, "raw pointers cannot target safe references"));
                }
                self.raw_pointer_type(element, *mutable)
            }
            Type::Tuple(elements) => {
                let mut types = Vec::with_capacity(elements.len());
                for element in elements {
                    let element_type = self.resolve_type(element, span)?;
                    if element_type == ValueType::Unit {
                        return Err(at(span, "tuple elements cannot have type `unit`"));
                    }
                    if matches!(
                        element_type,
                        ValueType::Reference(_) | ValueType::MutableReference(_)
                    ) {
                        return Err(at(span, "references cannot be stored in tuples"));
                    }
                    types.push(element_type);
                }
                if types.is_empty() {
                    return Err(at(span, "tuple types must contain at least one element"));
                }
                self.tuple_type(types)
            }
            Type::Function(params, result) => {
                let mut param_types = Vec::with_capacity(params.len());
                for param in params {
                    let param_type = self.resolve_type(param, span)?;
                    if param_type == ValueType::Unit {
                        return Err(at(span, "function type parameters cannot have type `unit`"));
                    }
                    param_types.push(param_type);
                }
                let return_type = self.resolve_type(result, span)?;
                if matches!(
                    return_type,
                    ValueType::Reference(_) | ValueType::MutableReference(_)
                ) {
                    return Err(at(span, "function types cannot return references"));
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
                    return Err(at(span, format!("unknown type `{name}`")));
                }
            }
            Type::Applied(name, _) => {
                return Err(at(
                    span,
                    format!("generic type `{name}` was not specialized"),
                ));
            }
            Type::Parameter(name) => {
                return Err(at(
                    span,
                    format!("generic parameter `{name}` was not specialized"),
                ));
            }
        })
    }

    pub(super) fn type_name(&self, ty: ValueType) -> String {
        match ty {
            ValueType::Array(index) => self.array_shape_name(index),
            ValueType::Vector(index) => format!(
                "Vec<{}>",
                self.array_element_type_name(self.vector_shapes[index].element)
            ),
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
            ValueType::MutableReference(index) => {
                format!(
                    "&mut {}",
                    self.type_name(self.mutable_reference_types[index])
                )
            }
            ValueType::RawPointer(index) => {
                let (target, mutable) = self.raw_pointer_types[index];
                format!(
                    "*{} {}",
                    if mutable { "mut" } else { "const" },
                    self.type_name(target)
                )
            }
            ValueType::Enum(index) => self
                .enums
                .get(index)
                .map(|declaration| declaration.name.clone())
                .unwrap_or_else(|| "<invalid enum>".into()),
            ValueType::Struct(index) => self
                .structs
                .get(index)
                .map(|declaration| declaration.name.clone())
                .unwrap_or_else(|| "<invalid struct>".into()),
            ValueType::I64 => "i64".into(),
            ValueType::F32 => "f32".into(),
            ValueType::F64 => "f64".into(),
            ValueType::Bool => "bool".into(),
            ValueType::Str => "str".into(),
            ValueType::Unit => "unit".into(),
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
            ArrayElementType::RawPointer(index) => {
                let (target, mutable) = self.raw_pointer_types[index];
                format!(
                    "*{} {}",
                    if mutable { "mut" } else { "const" },
                    self.type_name(target)
                )
            }
        }
    }

    fn array_shape_name(&self, index: usize) -> String {
        let shape = self.array_shapes[index];
        format!(
            "[{}; {}]",
            self.array_element_type_name(shape.element),
            shape.length
        )
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

    fn resolve_array_element(
        &mut self,
        element: &Type,
        span: Span,
    ) -> Result<ArrayElementType, Diagnostic> {
        Ok(match element {
            Type::I64 => ArrayElementType::I64,
            Type::F32 => ArrayElementType::F32,
            Type::F64 => ArrayElementType::F64,
            Type::Bool => ArrayElementType::Bool,
            Type::Str => ArrayElementType::Str,
            Type::Named(name) => {
                if let Some(index) = self.enum_ids.get(name) {
                    ArrayElementType::Enum(*index)
                } else if let Some(index) = self.struct_ids.get(name) {
                    ArrayElementType::Struct(*index)
                } else {
                    return Err(at(span, format!("unknown type `{name}`")));
                }
            }
            Type::Array(nested_element, length) => {
                let nested_element = self.resolve_array_element(nested_element, span)?;
                let ValueType::Array(index) = self.array_type(nested_element, *length) else {
                    unreachable!()
                };
                ArrayElementType::Array(index)
            }
            Type::Vector(nested_element) => {
                let nested_element = self.resolve_array_element(nested_element, span)?;
                let ValueType::Vector(index) = self.vector_type(nested_element) else {
                    unreachable!()
                };
                ArrayElementType::Vector(index)
            }
            Type::Tuple(elements) => {
                let types = elements
                    .iter()
                    .map(|element| self.resolve_type(element, span))
                    .collect::<Result<Vec<_>, _>>()?;
                if types.contains(&ValueType::Unit) {
                    return Err(at(span, "tuple elements cannot have type `unit`"));
                }
                if types.iter().any(|ty| {
                    matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_))
                }) {
                    return Err(at(span, "references cannot be stored in tuples"));
                }
                if types.is_empty() {
                    return Err(at(span, "tuple types must contain at least one element"));
                }
                ArrayElementType::Tuple(self.tuple_shape(types))
            }
            Type::Reference(_) => {
                return Err(at(span, "references cannot be stored in arrays or vectors"));
            }
            Type::MutableReference(_) => {
                return Err(at(span, "references cannot be stored in arrays or vectors"));
            }
            Type::RawPointer(element, mutable) => {
                let ValueType::RawPointer(index) =
                    self.resolve_type(&Type::RawPointer(element.clone(), *mutable), span)?
                else {
                    unreachable!()
                };
                ArrayElementType::RawPointer(index)
            }
            Type::Function(_, _) => {
                let ValueType::Function(index) = self.resolve_type(element, span)? else {
                    unreachable!()
                };
                ArrayElementType::Function(index)
            }
            Type::Unit => return Err(at(span, "array elements cannot have type `unit`")),
            Type::Applied(name, _) => {
                return Err(at(
                    span,
                    format!("generic type `{name}` was not specialized"),
                ));
            }
            Type::Parameter(name) => {
                return Err(at(
                    span,
                    format!("generic parameter `{name}` was not specialized"),
                ));
            }
        })
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

    fn tuple_shape(&mut self, elements: Vec<ValueType>) -> usize {
        if let Some(index) = self.tuple_shape_ids.get(&elements) {
            *index
        } else {
            let index = self.tuple_shapes.len();
            self.tuple_shapes.push(elements.clone());
            self.tuple_shape_ids.insert(elements, index);
            index
        }
    }

    pub(super) fn tuple_type(&mut self, elements: Vec<ValueType>) -> ValueType {
        ValueType::Tuple(self.tuple_shape(elements))
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

    pub(super) fn check_block_result(
        &mut self,
        block: &Block,
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        self.mutable_borrows.push(std::collections::HashSet::new());
        let result = (|| {
            for statement in &block.statements {
                match statement {
                    Stmt::Let { .. }
                    | Stmt::Assign { .. }
                    | Stmt::AssignPlace { .. }
                    | Stmt::Expr(_) => {
                        self.check_statement(statement, ValueType::Unit)?;
                    }
                    _ => {
                        return Err(at(
                            span,
                            "`if` expression branches do not allow control-flow statements",
                        ));
                    }
                }
            }
            let Some(tail) = &block.tail else {
                return Err(at(
                    span,
                    "`if` expression branches must end with a value expression",
                ));
            };
            self.check_expr(tail, false)
        })();
        self.scopes.pop();
        self.immutable_scopes.pop();
        self.loop_scopes.pop();
        self.mutable_borrows.pop();
        result
    }

    pub(super) fn expect_type(
        &self,
        actual: ValueType,
        expected: ValueType,
        span: Span,
        context: &str,
    ) -> Result<(), Diagnostic> {
        if actual == expected {
            Ok(())
        } else {
            Err(at(
                span,
                format!(
                    "{context} has type {}, expected {}",
                    self.type_name(actual),
                    self.type_name(expected)
                ),
            ))
        }
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<ValueType> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub(super) fn lookup_local_with_scope(&self, name: &str) -> Option<(usize, ValueType)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, scope)| scope.get(name).copied().map(|ty| (index, ty)))
    }

    pub(super) fn reject_lambda_capture(&self, name: &str, span: Span) -> Result<(), Diagnostic> {
        if self
            .lookup_local_with_scope(name)
            .is_some_and(|(scope_index, _)| {
                self.lambda_scope_boundaries
                    .last()
                    .is_some_and(|boundary| scope_index < *boundary)
            })
        {
            Err(at(
                span,
                format!("cannot assign to captured binding `{name}`"),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn record_lambda_capture(&mut self, name: &str, scope_index: usize) {
        for (boundary, captures) in self
            .lambda_scope_boundaries
            .iter()
            .copied()
            .zip(self.lambda_capture_scopes.iter_mut())
        {
            if scope_index < boundary && !captures.iter().any(|capture| capture == name) {
                captures.push(name.to_owned());
            }
        }
    }

    pub(super) fn is_immutable_binding(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .zip(&self.immutable_scopes)
            .rev()
            .find_map(|(scope, immutable)| {
                scope.contains_key(name).then(|| immutable.contains(name))
            })
            .unwrap_or(false)
    }

    pub(super) fn is_loop_variable(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .zip(&self.loop_scopes)
            .rev()
            .find_map(|(scope, loop_variables)| {
                scope
                    .contains_key(name)
                    .then(|| loop_variables.contains(name))
            })
            .unwrap_or(false)
    }

    pub(super) fn is_mutably_borrowed(&self, name: &str) -> bool {
        self.mutable_borrows
            .iter()
            .any(|scope| scope.contains(name))
    }

    pub(super) fn reject_mutably_borrowed(&self, name: &str, span: Span) -> Result<(), Diagnostic> {
        if self.is_mutably_borrowed(name) {
            Err(at(
                span,
                format!("cannot access `{name}` while it is mutably borrowed"),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn place_is_mutable(&mut self, expr: &Expr) -> Result<bool, Diagnostic> {
        match expr {
            Expr::Unary {
                op: UnaryOp::Dereference,
                value,
                span,
            } => match self.check_expr(value, false)? {
                ValueType::MutableReference(_) => Ok(true),
                ValueType::Reference(_) => Ok(false),
                ValueType::RawPointer(index) => Ok(self.raw_pointer_types[index].1),
                other => Err(at(
                    *span,
                    format!("cannot dereference {}", self.type_name(other)),
                )),
            },
            Expr::Field { value, .. } => self.place_is_mutable(value),
            Expr::Index { array, .. } => self.place_is_mutable(array),
            _ => Ok(false),
        }
    }

    pub(super) fn contains_raw_pointer(
        &mut self,
        ty: ValueType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        if matches!(ty, ValueType::RawPointer(_)) {
            return Ok(true);
        }
        if !visited.insert(ty) {
            return Ok(false);
        }
        match ty {
            ValueType::Array(index) => self
                .contains_raw_pointer(ValueType::from(self.array_shapes[index].element), visited),
            ValueType::Vector(index) => self
                .contains_raw_pointer(ValueType::from(self.vector_shapes[index].element), visited),
            ValueType::Tuple(index) => {
                for element in self.tuple_shapes[index].clone() {
                    if self.contains_raw_pointer(element, visited)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ValueType::Struct(index) => {
                for field in self.structs[index].fields.clone() {
                    let field_type = self.resolve_type(&field.ty, field.span)?;
                    if self.contains_raw_pointer(field_type, visited)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ValueType::Enum(index) => {
                for variant in self.enums[index].variants.clone() {
                    for payload in variant.payload {
                        let field_type = self.resolve_type(&payload, variant.span)?;
                        if self.contains_raw_pointer(field_type, visited)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            ValueType::RawPointer(_) => Ok(true),
            ValueType::I64
            | ValueType::F32
            | ValueType::F64
            | ValueType::Bool
            | ValueType::Str
            | ValueType::Unit
            | ValueType::Function(_)
            | ValueType::Reference(_)
            | ValueType::MutableReference(_) => Ok(false),
        }
    }

    pub(super) fn type_from_value(&self, ty: ValueType) -> Type {
        match ty {
            ValueType::I64 => Type::I64,
            ValueType::F32 => Type::F32,
            ValueType::F64 => Type::F64,
            ValueType::Bool => Type::Bool,
            ValueType::Str => Type::Str,
            ValueType::Unit => Type::Unit,
            ValueType::Array(index) => {
                let shape = self.array_shapes[index];
                Type::Array(
                    Box::new(self.type_from_element(shape.element)),
                    shape.length,
                )
            }
            ValueType::Vector(index) => Type::Vector(Box::new(
                self.type_from_element(self.vector_shapes[index].element),
            )),
            ValueType::Reference(index) => {
                Type::Reference(Box::new(self.type_from_value(self.reference_types[index])))
            }
            ValueType::MutableReference(index) => Type::MutableReference(Box::new(
                self.type_from_value(self.mutable_reference_types[index]),
            )),
            ValueType::RawPointer(index) => {
                let (target, mutable) = self.raw_pointer_types[index];
                Type::RawPointer(Box::new(self.type_from_value(target)), mutable)
            }
            ValueType::Tuple(index) => Type::Tuple(
                self.tuple_shapes[index]
                    .iter()
                    .map(|element| self.type_from_value(*element))
                    .collect(),
            ),
            ValueType::Function(index) => {
                let shape = &self.function_shapes[index];
                Type::Function(
                    shape
                        .params
                        .iter()
                        .map(|ty| self.type_from_value(*ty))
                        .collect(),
                    Box::new(self.type_from_value(shape.return_type)),
                )
            }
            ValueType::Enum(index) => Type::Named(self.enums[index].name.clone()),
            ValueType::Struct(index) => Type::Named(self.structs[index].name.clone()),
        }
    }

    fn type_from_element(&self, element: ArrayElementType) -> Type {
        match element {
            ArrayElementType::I64 => Type::I64,
            ArrayElementType::F32 => Type::F32,
            ArrayElementType::F64 => Type::F64,
            ArrayElementType::Bool => Type::Bool,
            ArrayElementType::Str => Type::Str,
            ArrayElementType::Enum(index) => Type::Named(self.enums[index].name.clone()),
            ArrayElementType::Struct(index) => Type::Named(self.structs[index].name.clone()),
            ArrayElementType::Array(index) => self.type_from_value(ValueType::Array(index)),
            ArrayElementType::Vector(index) => self.type_from_value(ValueType::Vector(index)),
            ArrayElementType::Tuple(index) => self.type_from_value(ValueType::Tuple(index)),
            ArrayElementType::Function(index) => self.type_from_value(ValueType::Function(index)),
            ArrayElementType::RawPointer(index) => {
                let (target, mutable) = self.raw_pointer_types[index];
                Type::RawPointer(Box::new(self.type_from_value(target)), mutable)
            }
        }
    }

    pub(super) fn global_type_is_static(
        &mut self,
        ty: ValueType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        if !visited.insert(ty) {
            return Ok(true);
        }
        match ty {
            ValueType::Unit
            | ValueType::Vector(_)
            | ValueType::Function(_)
            | ValueType::Reference(_)
            | ValueType::MutableReference(_) => Ok(false),
            ValueType::RawPointer(_) => Ok(false),
            ValueType::Array(index) => {
                self.global_element_is_static(self.array_shapes[index].element, visited)
            }
            ValueType::Tuple(index) => {
                let elements = self.tuple_shapes[index].clone();
                for element in elements {
                    if !self.global_type_is_static(element, visited)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ValueType::Struct(index) => {
                let fields = self.structs[index].fields.clone();
                for field in fields {
                    let field_type = self.resolve_type(&field.ty, field.span)?;
                    if !self.global_type_is_static(field_type, visited)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ValueType::Enum(index) => {
                let variants = self.enums[index].variants.clone();
                for variant in variants {
                    for field in variant.payload {
                        let field_type = self.resolve_type(&field, variant.span)?;
                        if !self.global_type_is_static(field_type, visited)? {
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            }
            ValueType::I64 | ValueType::F32 | ValueType::F64 | ValueType::Bool | ValueType::Str => {
                Ok(true)
            }
        }
    }

    fn global_element_is_static(
        &mut self,
        element: ArrayElementType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        self.global_type_is_static(ValueType::from(element), visited)
    }
}

pub(super) fn span_of_expr(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Float32(_, span)
        | Expr::Float(_, span)
        | Expr::Variable(_, span)
        | Expr::Call { span, .. }
        | Expr::CallValue { span, .. }
        | Expr::Lambda { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Array(_, span)
        | Expr::Tuple(_, span)
        | Expr::Index { span, .. }
        | Expr::Field { span, .. }
        | Expr::Struct { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Loop { span, .. }
        | Expr::Variant { span, .. } => *span,
        Expr::Bool(_, span) | Expr::String(_, span) => *span,
    }
}

pub(super) fn assignment_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name, _) => Some(name),
        Expr::Field { value, .. } => assignment_root(value),
        Expr::Index { array, .. } => assignment_root(array),
        Expr::Unary {
            op: UnaryOp::Dereference,
            value,
            ..
        } => assignment_root(value),
        _ => None,
    }
}

pub(super) fn at(span: Span, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::at(span.into(), message.to_string())
}

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::F32 => Self::F32,
            ArrayElementType::F64 => Self::F64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
            ArrayElementType::Enum(index) => Self::Enum(index),
            ArrayElementType::Struct(index) => Self::Struct(index),
            ArrayElementType::Array(index) => Self::Array(index),
            ArrayElementType::Vector(index) => Self::Vector(index),
            ArrayElementType::Tuple(index) => Self::Tuple(index),
            ArrayElementType::Function(index) => Self::Function(index),
            ArrayElementType::RawPointer(index) => Self::RawPointer(index),
        }
    }
}
