use super::*;

impl Checker {
    pub(super) fn collect_signatures(&mut self, program: &Program) -> Result<(), Diagnostic> {
        for (enum_id, declaration) in program.enums.iter().enumerate() {
            self.module_context = declaration.module.clone();
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
            }
        }
        self.enums = program.enums.clone();
        for (struct_id, declaration) in program.structs.iter().enumerate() {
            self.module_context = declaration.module.clone();
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
        let enum_declarations = self.enums.clone();
        for declaration in &enum_declarations {
            for variant in &declaration.variants {
                for payload_type in &variant.payload {
                    if self.resolve_type(payload_type, variant.span)? == ValueType::Unit {
                        return Err(at(variant.span, "enum payloads cannot have type `unit`"));
                    }
                }
            }
        }
        let struct_fields = self.structs.clone();
        for declaration in &struct_fields {
            for field in &declaration.fields {
                if self.resolve_type(&field.ty, field.span)? == ValueType::Unit {
                    return Err(at(field.span, "struct fields cannot have type `unit`"));
                }
            }
        }
        crate::aggregate_validation::validate_aggregate_cycles(&self.enums, &self.structs)?;
        let mut saw_main = false;
        let mut visibility = HashMap::<String, (String, bool)>::new();
        for function in &program.functions {
            self.module_context = function.module.clone();
            if program.functions_are_qualified
                && function.name == "main"
                && !function.module.is_empty()
            {
                return Err(at(
                    function.span,
                    "`main` may only be defined in the root source file",
                ));
            }
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
            visibility.insert(
                function.name.clone(),
                (function.module.clone(), function.is_public),
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
        self.function_visibility = visibility;
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

    pub(super) fn check_function(&mut self, function: &Function) -> Result<(), Diagnostic> {
        self.scopes.clear();
        self.immutable_scopes.clear();
        self.loop_scopes.clear();
        self.loop_contexts.clear();
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        for param in &function.params {
            let ty = self.resolve_type(&param.ty, param.span)?;
            self.scopes[0].insert(param.name.clone(), ty);
        }
        let return_type = self.resolve_type(&function.return_type, function.span)?;
        self.current_return_type = return_type;
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
