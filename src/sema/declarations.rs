use super::*;

impl Checker {
    pub(super) fn collect_signatures(&mut self, program: &Program) -> Result<(), Diagnostic> {
        for global in &program.globals {
            if matches!(
                global.name.as_str(),
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
                return Err(at(
                    global.span,
                    format!("`{}` is a reserved compiler/runtime name", global.name),
                ));
            }
            self.global_declarations
                .insert(global.name.clone(), global.clone());
            self.global_states.insert(global.name.clone(), 0);
        }
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
                    match self.resolve_type(payload_type, variant.span)? {
                        ValueType::Unit => {
                            return Err(at(variant.span, "enum payloads cannot have type `unit`"));
                        }
                        ValueType::Reference(_) | ValueType::MutableReference(_) => {
                            return Err(at(variant.span, "references cannot be stored in enums"));
                        }
                        _ => {}
                    }
                }
            }
        }
        let struct_fields = self.structs.clone();
        for declaration in &struct_fields {
            for field in &declaration.fields {
                match self.resolve_type(&field.ty, field.span)? {
                    ValueType::Unit => {
                        return Err(at(field.span, "struct fields cannot have type `unit`"));
                    }
                    ValueType::Reference(_) | ValueType::MutableReference(_) => {
                        return Err(at(field.span, "references cannot be stored in structs"));
                    }
                    _ => {}
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
            if matches!(
                return_type,
                ValueType::Reference(_) | ValueType::MutableReference(_)
            ) {
                return Err(at(function.span, "functions cannot return references"));
            }
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
        let mut c_symbols = std::collections::HashMap::<&str, Span>::new();
        for function in &program.functions {
            let symbol = if function.name == "main" {
                "vaca_main"
            } else {
                function.name.as_str()
            };
            c_symbols.insert(symbol, function.span);
        }
        const RESERVED_LINK_SYMBOLS: &[&str] = &[
            "print_i64",
            "print_f32",
            "print_f64",
            "print",
            "println",
            "read_i64",
            "printf",
            "strcmp",
            "vaca_main",
            "getchar",
            "exit",
            "strlen",
            "malloc",
            "vaca_alloc",
            "memcpy",
            "vaca_str_concat",
            "len",
            "vec_new",
            "vec_push",
            "realloc",
            "vaca_vec_new",
            "vaca_vec_push",
            "main",
        ];
        for function in &program.extern_functions {
            self.module_context = function.module.clone();
            let mut params_seen = std::collections::HashSet::new();
            for param in &function.params {
                if !params_seen.insert(param.name.as_str()) {
                    return Err(at(
                        param.span,
                        format!("duplicate parameter `{}`", param.name),
                    ));
                }
            }
            if RESERVED_LINK_SYMBOLS.contains(&function.link_name.as_str()) {
                return Err(at(
                    function.span,
                    format!(
                        "C symbol `{}` conflicts with a reserved compiler/runtime symbol",
                        function.link_name
                    ),
                ));
            }
            if let Some(previous) = c_symbols.insert(&function.link_name, function.span) {
                return Err(at(
                    function.span,
                    format!(
                        "C symbol `{}` is declared more than once (previous declaration at line {}:{})",
                        function.link_name, previous.line, previous.column
                    ),
                ));
            }
            if function
                .params
                .iter()
                .any(|param| !matches!(param.ty, Type::I64 | Type::F32 | Type::F64))
            {
                let param = function
                    .params
                    .iter()
                    .find(|param| !matches!(param.ty, Type::I64 | Type::F32 | Type::F64))
                    .unwrap();
                return Err(at(
                    param.span,
                    "C extern parameters support only `i64`, `f32`, and `f64`",
                ));
            }
            if !matches!(
                function.return_type,
                Type::I64 | Type::F32 | Type::F64 | Type::Unit
            ) {
                return Err(at(
                    function.span,
                    "C extern return types support only `i64`, `f32`, `f64`, and `unit`",
                ));
            }
            if self.signatures.contains_key(&function.name) {
                return Err(at(
                    function.span,
                    format!("function `{}` is defined more than once", function.name),
                ));
            }
            let params = function
                .params
                .iter()
                .map(|param| self.resolve_type(&param.ty, param.span))
                .collect::<Result<_, _>>()?;
            let return_type = self.resolve_type(&function.return_type, function.span)?;
            if matches!(
                return_type,
                ValueType::Reference(_) | ValueType::MutableReference(_)
            ) {
                return Err(at(
                    function.span,
                    "extern functions cannot return references",
                ));
            }
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
        self.signatures.insert(
            "print_f64".into(),
            Signature {
                params: vec![ValueType::F64],
                return_type: ValueType::Unit,
            },
        );
        self.signatures.insert(
            "print_f32".into(),
            Signature {
                params: vec![ValueType::F32],
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

    pub(super) fn check_global(
        &mut self,
        name: &str,
        reference_span: Span,
    ) -> Result<ValueType, Diagnostic> {
        if let Some(ty) = self.global_types.get(name) {
            return Ok(*ty);
        }
        let declaration = self
            .global_declarations
            .get(name)
            .cloned()
            .ok_or_else(|| at(reference_span, format!("undefined variable `{name}`")))?;
        match self.global_states.get(name).copied().unwrap_or(0) {
            1 => {
                return Err(at(
                    declaration.span,
                    format!("global initializer cycle involving `{name}`"),
                ));
            }
            2 => {
                return self.global_types.get(name).copied().ok_or_else(|| {
                    at(
                        declaration.span,
                        format!("could not infer type of global `{name}`"),
                    )
                });
            }
            _ => {}
        }
        self.global_states.insert(name.to_owned(), 1);
        let previous_module = self.module_context.clone();
        self.module_context = declaration.module.clone();
        let result = (|| {
            if let Some(ty) = &declaration.ty {
                let expected = self.resolve_type(ty, declaration.span)?;
                if expected == ValueType::Unit
                    || matches!(expected, ValueType::Vector(_) | ValueType::Reference(_))
                {
                    return Err(at(
                        declaration.span,
                        "global initializer type cannot be `unit`, a vector, or a reference",
                    ));
                }
            }
            self.check_constant_initializer(&declaration.value)?;
            let value_type = if let Some(ty) = &declaration.ty {
                let expected = self.resolve_type(ty, declaration.span)?;
                self.check_expr_expected(&declaration.value, expected)?
            } else if matches!(&declaration.value, Expr::Array(elements, _) if elements.is_empty())
            {
                return Err(at(
                    span_of_expr(&declaration.value),
                    "empty array literals require an array type annotation",
                ));
            } else {
                self.check_expr(&declaration.value, false)?
            };
            let ty = match &declaration.ty {
                Some(ty) => self.resolve_type(ty, declaration.span)?,
                None => value_type,
            };
            self.expect_type(
                value_type,
                ty,
                span_of_expr(&declaration.value),
                &format!("initializer for global `{name}`"),
            )?;
            if self.contains_raw_pointer(ty, &mut std::collections::HashSet::new())? {
                return Err(at(
                    declaration.span,
                    "global initializers cannot contain raw pointers",
                ));
            }
            if !self.global_type_is_static(ty, &mut std::collections::HashSet::new())? {
                return Err(at(
                    declaration.span,
                    "global type must be statically storable (vectors and `unit` are not supported)",
                ));
            }
            Ok(ty)
        })();
        self.module_context = previous_module;
        match result {
            Ok(ty) => {
                self.global_types.insert(name.to_owned(), ty);
                self.global_states.insert(name.to_owned(), 2);
                Ok(ty)
            }
            Err(error) => {
                self.global_states.insert(name.to_owned(), 0);
                Err(error)
            }
        }
    }

    fn check_constant_initializer(&mut self, expr: &Expr) -> Result<(), Diagnostic> {
        match expr {
            Expr::Integer(..)
            | Expr::Float32(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::String(..) => {}
            Expr::Variable(name, span) => {
                if self.lookup_local(name).is_some() {
                    return Err(at(
                        *span,
                        "global initializers cannot refer to local variables",
                    ));
                }
                let global = self.find_global_name(name);
                let Some(global) = global else {
                    if self.find_function_name(name).is_some() {
                        return Err(at(
                            *span,
                            "global initializers must be compile-time constant expressions",
                        ));
                    }
                    return Err(at(*span, format!("undefined variable `{name}`")));
                };
                if self.global_declarations[&global].mutable {
                    return Err(at(
                        *span,
                        "global initializers may reference only immutable globals",
                    ));
                }
                self.check_global(&global, *span)?;
            }
            Expr::Array(elements, _) | Expr::Tuple(elements, _) => {
                for element in elements {
                    self.check_constant_initializer(element)?;
                }
            }
            Expr::Struct { fields, .. } => {
                for (_, value, _) in fields {
                    self.check_constant_initializer(value)?;
                }
            }
            Expr::Variant { payload, .. } => {
                if let Some(payload) = payload {
                    for value in payload {
                        self.check_constant_initializer(value)?;
                    }
                }
            }
            Expr::Unary { value, .. } => self.check_constant_initializer(value)?,
            Expr::Try { span, .. } => {
                return Err(at(
                    *span,
                    "global initializers must be compile-time constant expressions",
                ));
            }
            Expr::Binary { left, right, .. } => {
                self.check_constant_initializer(left)?;
                self.check_constant_initializer(right)?;
            }
            Expr::Field { value, .. } => self.check_constant_initializer(value)?,
            Expr::Index { array, index, .. } => {
                self.check_constant_initializer(array)?;
                self.check_constant_initializer(index)?;
            }
            Expr::Call { span, .. } | Expr::CallValue { span, .. } | Expr::Lambda { span, .. } => {
                return Err(at(
                    *span,
                    "global initializers must be compile-time constant expressions",
                ));
            }
            Expr::If { span, .. } | Expr::Match { span, .. } | Expr::Loop { span, .. } => {
                return Err(at(
                    *span,
                    "global initializers must be compile-time constant expressions",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn find_global_name(&self, name: &str) -> Option<String> {
        if self.global_declarations.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.global_declarations
                .contains_key(&qualified)
                .then_some(qualified)
        }
    }

    pub(super) fn find_function_name(&self, name: &str) -> Option<String> {
        if self.signatures.contains_key(name) {
            Some(name.to_owned())
        } else {
            let qualified = if self.module_context.is_empty() {
                name.to_owned()
            } else {
                format!("{}::{name}", self.module_context)
            };
            self.signatures
                .contains_key(&qualified)
                .then_some(qualified)
        }
    }

    pub(super) fn check_function(&mut self, function: &Function) -> Result<(), Diagnostic> {
        self.scopes.clear();
        self.immutable_scopes.clear();
        self.mutable_borrows.clear();
        self.loop_scopes.clear();
        self.loop_contexts.clear();
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        self.mutable_borrows.push(std::collections::HashSet::new());
        for param in &function.params {
            let ty = self.resolve_type(&param.ty, param.span)?;
            self.scopes[0].insert(param.name.clone(), ty);
            if matches!(ty, ValueType::Reference(_) | ValueType::MutableReference(_)) {
                self.immutable_scopes[0].insert(param.name.clone());
            }
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
