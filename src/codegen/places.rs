use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn emit_place_pointer(
        &mut self,
        expr: &Expr,
        root_name: &str,
        root_pointer: PointerValue<'ctx>,
        root_type: ValueType,
    ) -> Result<(PointerValue<'ctx>, ValueType), String> {
        match expr {
            Expr::Variable(name, _) if name == root_name => Ok((root_pointer, root_type)),
            Expr::Field { value, name, span } => {
                let (base_pointer, base_type) =
                    self.emit_place_pointer(value, root_name, root_pointer, root_type)?;
                let ValueType::Struct(struct_id) = base_type else {
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
                let field = &self.structs[struct_id].fields[field_index];
                let field_declaration_type = field.ty.clone();
                let field_span = field.span;
                let field_type = self.resolve_type(&field_declaration_type, field_span)?;
                let field_pointer = self
                    .builder
                    .build_struct_gep(
                        self.struct_types[struct_id],
                        base_pointer,
                        field_index as u32,
                        "struct.field.ptr",
                    )
                    .map_err(|error| error.to_string())?;
                Ok((field_pointer, field_type))
            }
            Expr::Index { array, index, span } => {
                let (array_pointer, array_type) =
                    self.emit_place_pointer(array, root_name, root_pointer, root_type)?;
                let ValueType::Array(array_id) = array_type else {
                    return Err(format!(
                        "line {}:{}: indexing requires an array",
                        span.line, span.column
                    ));
                };
                let index_value = self.emit_expression(index)?;
                let index_value = self.expect_i64(index_value, "array index")?;
                let shape = self.array_shapes[array_id];
                self.emit_bounds_check(index_value, shape.length)?;
                let element_pointer =
                    self.array_element_pointer(array_pointer, array_type, index_value)?;
                Ok((element_pointer, ValueType::from(shape.element)))
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    pub(super) fn emit_assignment_statement(&mut self, statement: &Stmt) -> Result<bool, String> {
        match statement {
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
                let value = self.emit_expression_expected(value, local.ty)?;
                if local.ty != value.ty {
                    return Err(format!(
                        "line {}:{}: cannot assign {} to `{name}` of type {}",
                        span.line,
                        span.column,
                        self.type_name(value.ty),
                        self.type_name(local.ty)
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
            Stmt::AssignPlace {
                target,
                value,
                span,
            } => {
                let name = codegen_assignment_root(target).ok_or_else(|| {
                    format!(
                        "line {}:{}: assignment target must start with a local variable",
                        span.line, span.column
                    )
                })?;
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
                let (target_pointer, target_type) =
                    self.emit_place_pointer(target, name, local.pointer, local.ty)?;
                let value = self.emit_expression_expected(value, target_type)?;
                if value.ty != target_type {
                    return Err(format!(
                        "line {}:{}: assignment target has type {}, expected {}",
                        span.line,
                        span.column,
                        self.type_name(value.ty),
                        self.type_name(target_type)
                    ));
                }
                self.builder
                    .build_store(
                        target_pointer,
                        value.value.ok_or("assignment target value is missing")?,
                    )
                    .map_err(|error| error.to_string())?;
                Ok(false)
            }
            _ => unreachable!("non-assignment passed to assignment emitter"),
        }
    }
}

fn codegen_assignment_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name, _) => Some(name),
        Expr::Field { value, .. } => codegen_assignment_root(value),
        Expr::Index { array, .. } => codegen_assignment_root(array),
        _ => None,
    }
}
