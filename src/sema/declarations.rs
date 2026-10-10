use super::*;

impl Checker {
    pub(super) fn collect_signatures(&mut self, program: &Program) -> Result<(), String> {
        for (enum_id, declaration) in program.enums.iter().enumerate() {
            if enum_id > u32::MAX as usize {
                return Err(at(
                    declaration.span,
                    "program contains too many enum declarations",
                ));
            }
            if self
                .enum_ids
                .insert(declaration.name.clone(), enum_id)
                .is_some()
            {
                return Err(at(
                    declaration.span,
                    format!("enum `{}` is defined more than once", declaration.name),
                ));
            }
            let mut seen = std::collections::HashSet::new();
            for variant in &declaration.variants {
                if !seen.insert(variant.name.as_str()) {
                    return Err(at(
                        variant.span,
                        format!("enum variant `{}` is defined more than once", variant.name),
                    ));
                }
                if matches!(
                    &variant.payload,
                    Some(Type::Unit | Type::Array(_, _) | Type::Named(_))
                ) {
                    return Err(at(
                        variant.span,
                        "enum payloads must have type `i64`, `bool`, or `str`",
                    ));
                }
            }
        }
        self.enums = program.enums.clone();
        for (struct_id, declaration) in program.structs.iter().enumerate() {
            if self.enum_ids.contains_key(&declaration.name)
                || self
                    .struct_ids
                    .insert(declaration.name.clone(), struct_id)
                    .is_some()
            {
                return Err(at(
                    declaration.span,
                    format!("type `{}` is defined more than once", declaration.name),
                ));
            }
            let mut seen = std::collections::HashSet::new();
            for field in &declaration.fields {
                if !seen.insert(field.name.as_str()) {
                    return Err(at(
                        field.span,
                        format!("struct field `{}` is defined more than once", field.name),
                    ));
                }
            }
        }
        for declaration in &program.enums {
            if self.struct_ids.contains_key(&declaration.name) {
                return Err(at(
                    declaration.span,
                    format!("type `{}` is defined more than once", declaration.name),
                ));
            }
        }
        self.structs = program.structs.clone();
        for declaration in &self.structs {
            for field in &declaration.fields {
                if self.resolve_type(&field.ty, field.span)? == ValueType::Unit {
                    return Err(at(field.span, "struct fields cannot have type `unit`"));
                }
            }
        }
        validate_struct_cycles(&self.structs, &self.struct_ids)?;
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
                return Err(at(
                    function.span,
                    format!("`{}` is a reserved compiler/runtime name", function.name),
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(at(
                    function.span,
                    format!("function `{}` is defined more than once", function.name),
                ));
            }

            let mut params_seen = std::collections::HashSet::new();
            for param in &function.params {
                if param.ty == Type::Unit {
                    return Err(at(param.span, "parameters cannot have type `unit`"));
                }
                if !params_seen.insert(param.name.as_str()) {
                    return Err(at(
                        param.span,
                        format!("duplicate parameter `{}`", param.name),
                    ));
                }
            }

            let params = function
                .params
                .iter()
                .map(|param| self.resolve_type(&param.ty, param.span))
                .collect::<Result<_, _>>()?;
            let return_type = self.resolve_type(&function.return_type, function.span)?;
            self.signatures.insert(
                function.name.clone(),
                Signature {
                    params,
                    return_type,
                },
            );
            if function.name == "main" {
                if !function.params.is_empty() || return_type != ValueType::I64 {
                    return Err("entry point must have signature `fn main() -> i64`".into());
                }
                saw_main = true;
            }
        }
        if !saw_main {
            return Err("program must define the required entry point `fn main() -> i64`".into());
        }
        self.signatures.insert(
            "print_i64".into(),
            Signature {
                params: vec![ValueType::I64],
                return_type: ValueType::Unit,
            },
        );
        for name in ["print", "println"] {
            self.signatures.insert(
                name.into(),
                Signature {
                    params: vec![ValueType::Str],
                    return_type: ValueType::Unit,
                },
            );
        }
        self.signatures.insert(
            "read_i64".into(),
            Signature {
                params: vec![],
                return_type: ValueType::I64,
            },
        );
        Ok(())
    }

    pub(super) fn check_function(&mut self, function: &Function) -> Result<(), String> {
        self.scopes.clear();
        self.immutable_scopes.clear();
        self.loop_scopes.clear();
        self.loop_depth = 0;
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        for param in &function.params {
            let ty = self.resolve_type(&param.ty, param.span)?;
            self.scopes[0].insert(param.name.clone(), ty);
        }
        let return_type = self.resolve_type(&function.return_type, function.span)?;
        let returns = self.check_block(&function.body, return_type, true)?;
        if return_type != ValueType::Unit && !returns {
            return Err(at(
                function.span,
                format!(
                    "function `{}` can reach its end without returning a value",
                    function.name
                ),
            ));
        }
        Ok(())
    }

    // Returns true if every path through this block returns from the function.
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
        fn named_struct_id(ty: &Type, struct_ids: &HashMap<String, usize>) -> Option<usize> {
            match ty {
                Type::Named(name) => struct_ids.get(name).copied(),
                Type::Array(element, _) => named_struct_id(element, struct_ids),
                _ => None,
            }
        }

        states[id] = 1;
        for field in &structs[id].fields {
            let Some(child_id) = named_struct_id(&field.ty, struct_ids) else {
                continue;
            };
            if states[child_id] == 1 {
                return Err(at(
                    field.span,
                    format!(
                        "struct `{}` has a recursive by-value field `{}`",
                        structs[id].name, field.name
                    ),
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
