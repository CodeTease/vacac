use super::*;

mod constants;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn declare_globals(&mut self, program: &Program) -> Result<(), Diagnostic> {
        for declaration in &program.globals {
            self.module_context = declaration.module.clone();
            let ty = declaration.ty.as_ref().ok_or_else(|| {
                Diagnostic::at(
                    declaration.span.into(),
                    format!("global `{}` has no resolved type", declaration.name),
                )
            })?;
            let ty = self.resolve_type(ty, declaration.span)?;
            if ty == ValueType::Unit
                || matches!(
                    ty,
                    ValueType::Vector(_) | ValueType::Reference(_) | ValueType::MutableReference(_)
                )
            {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("global initializer type cannot be `unit`, a vector, or a reference"),
                ));
            }
            if self.global_type_contains_raw_pointer(ty, &mut std::collections::HashSet::new())? {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!("global initializers cannot contain raw pointers"),
                ));
            }
            let llvm_type = self.llvm_type(ty).ok_or("global has no LLVM type")?;
            let symbol = llvm_global_name(&declaration.name);
            let global = self
                .module
                .add_global(llvm_type, Some(AddressSpace::default()), &symbol);
            global.set_linkage(if declaration.is_public {
                Linkage::External
            } else {
                Linkage::Internal
            });
            global.set_constant(!declaration.mutable);
            self.global_types.insert(declaration.name.clone(), ty);
            self.globals.insert(declaration.name.clone(), global);
            self.global_declarations
                .insert(declaration.name.clone(), declaration.clone());
            self.global_states.insert(declaration.name.clone(), 0);
        }
        Ok(())
    }

    fn global_type_contains_raw_pointer(
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
            ValueType::Array(index) => {
                self.global_element_contains_raw_pointer(self.array_shapes[index].element, visited)
            }
            ValueType::Vector(index) => {
                self.global_element_contains_raw_pointer(self.vector_shapes[index].element, visited)
            }
            ValueType::Tuple(index) => {
                for element in self.tuple_shapes[index].clone() {
                    if self.global_type_contains_raw_pointer(element, visited)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ValueType::Struct(index) => {
                for field in self.structs[index].fields.clone() {
                    let field_type = self.resolve_type(&field.ty, field.span)?;
                    if self.global_type_contains_raw_pointer(field_type, visited)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ValueType::Enum(index) => {
                for variant in self.enums[index].variants.clone() {
                    for payload in variant.payload {
                        let payload_type = self.resolve_type(&payload, variant.span)?;
                        if self.global_type_contains_raw_pointer(payload_type, visited)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            ValueType::Function(_) => Ok(false),
            ValueType::I64
            | ValueType::F32
            | ValueType::F64
            | ValueType::Bool
            | ValueType::Str
            | ValueType::Unit
            | ValueType::Reference(_)
            | ValueType::MutableReference(_)
            | ValueType::RawPointer(_) => Ok(false),
        }
    }

    fn global_element_contains_raw_pointer(
        &mut self,
        element: ArrayElementType,
        visited: &mut std::collections::HashSet<ValueType>,
    ) -> Result<bool, Diagnostic> {
        self.global_type_contains_raw_pointer(ValueType::from(element), visited)
    }

    pub(super) fn define_globals(&mut self, program: &Program) -> Result<(), Diagnostic> {
        for declaration in &program.globals {
            let value = self.evaluate_global(&declaration.name)?;
            let initializer = self.lower_constant(&value)?;
            let ty = self.global_types[&declaration.name];
            if value.ty != ty {
                return Err(Diagnostic::at(
                    declaration.span.into(),
                    format!(
                        "initializer for global `{}` has type {}, expected {}",
                        declaration.name,
                        self.type_name(value.ty),
                        self.type_name(ty)
                    ),
                ));
            }
            self.globals[&declaration.name].set_initializer(&initializer);
        }
        Ok(())
    }

    pub(super) fn resolve_global_name(&self, name: &str) -> Option<String> {
        if self.globals.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.globals.contains_key(&qualified).then_some(qualified)
        }
    }

    fn array_element_type(&self, ty: ValueType) -> Option<ArrayElementType> {
        match ty {
            ValueType::I64 => Some(ArrayElementType::I64),
            ValueType::F32 => Some(ArrayElementType::F32),
            ValueType::F64 => Some(ArrayElementType::F64),
            ValueType::Bool => Some(ArrayElementType::Bool),
            ValueType::Str => Some(ArrayElementType::Str),
            ValueType::Array(index) => Some(ArrayElementType::Array(index)),
            ValueType::Vector(index) => Some(ArrayElementType::Vector(index)),
            ValueType::Enum(index) => Some(ArrayElementType::Enum(index)),
            ValueType::Struct(index) => Some(ArrayElementType::Struct(index)),
            ValueType::Tuple(index) => Some(ArrayElementType::Tuple(index)),
            ValueType::Function(index) => Some(ArrayElementType::Function(index)),
            ValueType::Unit | ValueType::Reference(_) | ValueType::MutableReference(_) => None,
            ValueType::RawPointer(index) => Some(ArrayElementType::RawPointer(index)),
        }
    }
}

fn llvm_global_name(name: &str) -> String {
    format!("vaca.global.{}", name.replace("::", "."))
}
