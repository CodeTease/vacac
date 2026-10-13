use super::super::*;

impl<'ctx> Codegen<'ctx> {
    pub(in crate::codegen) fn emit_vector_runtime(&mut self) -> Result<(), String> {
        let pointer = self.context.ptr_type(AddressSpace::default());
        let i64_type = self.i64_type();
        let i8_type = self.context.i8_type();
        let allocate = self
            .module
            .get_function("vaca_alloc")
            .ok_or("missing allocator runtime function")?;
        let exit = self
            .module
            .get_function("exit")
            .ok_or("missing exit function")?;
        let header_type = self
            .context
            .struct_type(&[pointer.into(), i64_type.into(), i64_type.into()], false);
        let vec_new = self
            .module
            .add_function("vaca_vec_new", pointer.fn_type(&[], false), None);
        let entry = self.context.append_basic_block(vec_new, "entry");
        let init = self.context.append_basic_block(vec_new, "initialize");
        self.builder.position_at_end(entry);
        let header_size = header_type.size_of().ok_or("vector header has no size")?;
        let header = self
            .builder
            .build_call(
                allocate,
                &[pointer.const_null().into(), header_size.into()],
                "vec.header",
            )
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("allocator returned void")?
            .into_pointer_value();
        self.builder
            .build_unconditional_branch(init)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(init);
        let data_field = self
            .builder
            .build_struct_gep(header_type, header, 0, "vec.data.field")
            .map_err(|e| e.to_string())?;
        let length_field = self
            .builder
            .build_struct_gep(header_type, header, 1, "vec.length.field")
            .map_err(|e| e.to_string())?;
        let capacity_field = self
            .builder
            .build_struct_gep(header_type, header, 2, "vec.capacity.field")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(data_field, pointer.const_null())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(length_field, i64_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(capacity_field, i64_type.const_zero())
            .map_err(|e| e.to_string())?;
        self.builder
            .build_return(Some(&header))
            .map_err(|e| e.to_string())?;

        let push = self.module.add_function(
            "vaca_vec_push",
            self.context.void_type().fn_type(
                &[
                    pointer.into(),
                    pointer.into(),
                    i64_type.into(),
                    i64_type.into(),
                ],
                false,
            ),
            None,
        );
        let entry = self.context.append_basic_block(push, "entry");
        let append = self.context.append_basic_block(push, "append");
        let failure = self.context.append_basic_block(push, "allocation.error");
        self.builder.position_at_end(entry);
        let header = push
            .get_nth_param(0)
            .ok_or("missing vector header")?
            .into_pointer_value();
        let data_value = push
            .get_nth_param(1)
            .ok_or("missing vector element")?
            .into_pointer_value();
        let storage_stride = push
            .get_nth_param(2)
            .ok_or("missing vector storage stride")?
            .into_int_value();
        let element_size = push
            .get_nth_param(3)
            .ok_or("missing vector element size")?
            .into_int_value();
        let len_ptr = self
            .builder
            .build_struct_gep(header_type, header, 1, "vec.len.ptr")
            .map_err(|e| e.to_string())?;
        let cap_ptr = self
            .builder
            .build_struct_gep(header_type, header, 2, "vec.cap.ptr")
            .map_err(|e| e.to_string())?;
        let data_ptr = self
            .builder
            .build_struct_gep(header_type, header, 0, "vec.data.ptr")
            .map_err(|e| e.to_string())?;
        let len = self
            .builder
            .build_load(i64_type, len_ptr, "vec.len")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let cap = self
            .builder
            .build_load(i64_type, cap_ptr, "vec.cap")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let enough = self
            .builder
            .build_int_compare(IntPredicate::ULT, len, cap, "vec.has.capacity")
            .map_err(|e| e.to_string())?;
        let length_overflow = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                len,
                i64_type.const_all_ones(),
                "vec.length.overflow",
            )
            .map_err(|e| e.to_string())?;
        let append_ready = self.context.append_basic_block(push, "append.ready");
        let grow = self.context.append_basic_block(push, "grow");
        self.builder
            .build_conditional_branch(length_overflow, failure, append_ready)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(append_ready);
        self.builder
            .build_conditional_branch(enough, append, grow)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(grow);
        let is_empty = self
            .builder
            .build_int_compare(IntPredicate::EQ, cap, i64_type.const_zero(), "vec.empty")
            .map_err(|e| e.to_string())?;
        let doubled = self
            .builder
            .build_int_mul(cap, i64_type.const_int(2, false), "vec.doubled")
            .map_err(|e| e.to_string())?;
        let new_cap = self
            .builder
            .build_select(
                is_empty,
                i64_type.const_int(4, false),
                doubled,
                "vec.new.capacity",
            )
            .map_err(|e| e.to_string())?
            .into_int_value();
        let too_large = self
            .builder
            .build_int_compare(IntPredicate::ULT, new_cap, cap, "vec.capacity.overflow")
            .map_err(|e| e.to_string())?;
        let safe_element_size = self
            .builder
            .build_select(
                self.builder
                    .build_int_compare(
                        IntPredicate::EQ,
                        storage_stride,
                        i64_type.const_zero(),
                        "vec.zero.size",
                    )
                    .map_err(|e| e.to_string())?,
                i64_type.const_int(1, false),
                storage_stride,
                "vec.safe.size",
            )
            .map_err(|e| e.to_string())?
            .into_int_value();
        let maximum_count = self
            .builder
            .build_int_unsigned_div(
                i64_type.const_all_ones(),
                safe_element_size,
                "vec.max.count",
            )
            .map_err(|e| e.to_string())?;
        let size_overflow = self
            .builder
            .build_int_compare(
                IntPredicate::UGT,
                new_cap,
                maximum_count,
                "vec.size.overflow",
            )
            .map_err(|e| e.to_string())?;
        let overflow = self
            .builder
            .build_or(too_large, size_overflow, "vec.overflow")
            .map_err(|e| e.to_string())?;
        let allocation_size = self
            .builder
            .build_int_mul(new_cap, safe_element_size, "vec.allocation.size")
            .map_err(|e| e.to_string())?;
        let resize = self.context.append_basic_block(push, "resize");
        self.builder
            .build_conditional_branch(overflow, failure, resize)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(resize);
        let old_data = self
            .builder
            .build_load(pointer, data_ptr, "vec.old.data")
            .map_err(|e| e.to_string())?
            .into_pointer_value();
        let new_data = self
            .builder
            .build_call(
                allocate,
                &[old_data.into(), allocation_size.into()],
                "vec.allocation",
            )
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or("allocator returned void")?
            .into_pointer_value();
        self.builder
            .build_store(data_ptr, new_data)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(cap_ptr, new_cap)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(append)
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(failure);
        self.builder
            .build_call(
                exit,
                &[self.context.i32_type().const_int(1, false).into()],
                "vec.realloc.exit",
            )
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;
        self.builder.position_at_end(append);
        self.emit_vector_append(
            data_ptr,
            len_ptr,
            data_value,
            storage_stride,
            element_size,
            pointer,
            i8_type,
            i64_type,
        )?;
        Ok(())
    }

    fn emit_vector_append(
        &mut self,
        data_ptr: PointerValue<'ctx>,
        len_ptr: PointerValue<'ctx>,
        data_value: PointerValue<'ctx>,
        storage_stride: IntValue<'ctx>,
        element_size: IntValue<'ctx>,
        pointer: inkwell::types::PointerType<'ctx>,
        i8_type: inkwell::types::IntType<'ctx>,
        i64_type: inkwell::types::IntType<'ctx>,
    ) -> Result<(), String> {
        let current_data = self
            .builder
            .build_load(pointer, data_ptr, "vec.current.data")
            .map_err(|e| e.to_string())?
            .into_pointer_value();
        let current_len = self
            .builder
            .build_load(i64_type, len_ptr, "vec.current.len")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let offset = self
            .builder
            .build_int_mul(current_len, storage_stride, "vec.element.offset")
            .map_err(|e| e.to_string())?;
        let element_address = unsafe {
            self.builder
                .build_gep(i8_type, current_data, &[offset], "vec.element.address")
        }
        .map_err(|e| e.to_string())?;
        let memcpy = self.module.get_function("memcpy").unwrap_or_else(|| {
            self.module.add_function(
                "memcpy",
                pointer.fn_type(&[pointer.into(), pointer.into(), i64_type.into()], false),
                None,
            )
        });
        if element_size.get_zero_extended_constant() != Some(0) {
            self.builder
                .build_call(
                    memcpy,
                    &[
                        element_address.into(),
                        data_value.into(),
                        element_size.into(),
                    ],
                    "vec.copy.element",
                )
                .map_err(|e| e.to_string())?;
        }
        let next_len = self
            .builder
            .build_int_add(current_len, i64_type.const_int(1, false), "vec.next.len")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(len_ptr, next_len)
            .map_err(|e| e.to_string())?;
        self.builder.build_return(None).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(in crate::codegen) fn resolve_array_element(
        &mut self,
        element: &Type,
        span: Span,
    ) -> Result<ArrayElementType, String> {
        match element {
            Type::I64 => Ok(ArrayElementType::I64),
            Type::F32 => Ok(ArrayElementType::F32),
            Type::F64 => Ok(ArrayElementType::F64),
            Type::Bool => Ok(ArrayElementType::Bool),
            Type::Str => Ok(ArrayElementType::Str),
            Type::Array(_, _) => {
                let ValueType::Array(index) = self.resolve_type(element, span)? else {
                    unreachable!()
                };
                Ok(ArrayElementType::Array(index))
            }
            Type::Tuple(_) => {
                let ValueType::Tuple(index) = self.resolve_type(element, span)? else {
                    unreachable!()
                };
                Ok(ArrayElementType::Tuple(index))
            }
            Type::Function(_, _) => Err("function values cannot be stored in vectors".into()),
            Type::Vector(_) => {
                let ValueType::Vector(index) = self.resolve_type(element, span)? else {
                    unreachable!()
                };
                Ok(ArrayElementType::Vector(index))
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
            Type::Applied(name, _) => Err(format!("generic type `{name}` was not specialized")),
            Type::Parameter(name) => Err(format!("generic parameter `{name}` was not specialized")),
        }
    }
}
