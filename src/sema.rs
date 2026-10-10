use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, MatchPattern, Program, Span, Stmt,
    Type, UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    I64,
    Bool,
    Str,
    Unit,
    Array(ArrayElementType, u32),
    Enum(usize),
}

#[derive(Clone)]
struct Signature {
    params: Vec<ValueType>,
    return_type: ValueType,
}

struct Checker {
    signatures: HashMap<String, Signature>,
    scopes: Vec<HashMap<String, ValueType>>,
    immutable_scopes: Vec<std::collections::HashSet<String>>,
    loop_depth: usize,
    enum_ids: HashMap<String, usize>,
    enums: Vec<EnumDecl>,
}

pub fn check(program: &Program) -> Result<(), String> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        scopes: Vec::new(),
        immutable_scopes: Vec::new(),
        loop_depth: 0,
        enum_ids: HashMap::new(),
        enums: Vec::new(),
    };
    checker.collect_signatures(program)?;
    for function in &program.functions {
        checker.check_function(function)?;
    }
    Ok(())
}

impl Checker {
    fn collect_signatures(&mut self, program: &Program) -> Result<(), String> {
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
                    Some(Type::Unit | Type::Array(_, _) | Type::Enum(_))
                ) {
                    return Err(at(
                        variant.span,
                        "enum payloads must have type `i64`, `bool`, or `str`",
                    ));
                }
            }
        }
        self.enums = program.enums.clone();
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

    fn check_function(&mut self, function: &Function) -> Result<(), String> {
        self.scopes.clear();
        self.immutable_scopes.clear();
        self.loop_depth = 0;
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
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
    fn check_block(
        &mut self,
        block: &Block,
        return_type: ValueType,
        function_body: bool,
    ) -> Result<bool, String> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        let mut returns = false;
        for statement in &block.statements {
            let statement_returns = self.check_statement(statement, return_type)?;
            returns |= statement_returns;
        }

        if let Some(tail) = &block.tail {
            let tail_type = self.check_expr(tail, false)?;
            if function_body {
                self.expect_type(
                    tail_type,
                    return_type,
                    span_of_expr(tail),
                    "function result",
                )?;
                returns = true;
            }
        } else if function_body && return_type == ValueType::Unit {
            // Unit functions may complete normally without an explicit return.
            returns = true;
        }
        self.scopes.pop();
        self.immutable_scopes.pop();
        Ok(returns)
    }

    fn check_statement(
        &mut self,
        statement: &Stmt,
        return_type: ValueType,
    ) -> Result<bool, String> {
        match statement {
            Stmt::Let {
                name,
                ty,
                value,
                span,
            } => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(at(
                        *span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                let value_type = self.check_expr(value, false)?;
                if value_type == ValueType::Unit {
                    return Err(at(*span, "cannot bind a value of type `unit`"));
                }
                let binding_type = match ty {
                    Some(ty) => self.resolve_type(ty, *span)?,
                    None => value_type,
                };
                self.expect_type(
                    value_type,
                    binding_type,
                    span_of_expr(value),
                    &format!("initializer for `{name}`"),
                )?;
                self.scopes
                    .last_mut()
                    .expect("block scope exists")
                    .insert(name.clone(), binding_type);
                Ok(false)
            }
            Stmt::Assign { name, value, span } => {
                let binding_type = self
                    .lookup_local(name)
                    .ok_or_else(|| at(*span, format!("undefined variable `{name}`")))?;
                if self.is_immutable_binding(name) {
                    return Err(at(
                        *span,
                        format!("cannot assign to immutable loop variable `{name}`"),
                    ));
                }
                let value_type = self.check_expr(value, false)?;
                self.expect_type(
                    value_type,
                    binding_type,
                    span_of_expr(value),
                    &format!("assignment to `{name}`"),
                )?;
                Ok(false)
            }
            Stmt::AssignIndex {
                name,
                index,
                value,
                span,
            } => {
                let binding_type = self
                    .lookup_local(name)
                    .ok_or_else(|| at(*span, format!("undefined variable `{name}`")))?;
                let ValueType::Array(element, _) = binding_type else {
                    return Err(at(*span, format!("`{name}` is not an array")));
                };
                let index_type = self.check_expr(index, false)?;
                self.expect_type(
                    index_type,
                    ValueType::I64,
                    span_of_expr(index),
                    "array index",
                )?;
                let value_type = self.check_expr(value, false)?;
                self.expect_type(
                    value_type,
                    ValueType::from(element),
                    span_of_expr(value),
                    &format!("array element assignment to `{name}`"),
                )?;
                Ok(false)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "if condition",
                )?;
                let then_returns = self.check_block(then_block, return_type, false)?;
                let else_returns = if let Some(block) = else_block {
                    self.check_block(block, return_type, false)?
                } else {
                    false
                };
                Ok(then_returns && else_returns)
            }
            Stmt::While {
                condition, body, ..
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "while condition",
                )?;
                self.loop_depth += 1;
                let body_result = self.check_block(body, return_type, false);
                self.loop_depth -= 1;
                body_result?;
                Ok(false)
            }
            Stmt::For {
                name,
                start,
                end,
                body,
                ..
            } => {
                let start_type = self.check_expr(start, false)?;
                self.expect_type(
                    start_type,
                    ValueType::I64,
                    span_of_expr(start),
                    "for range start",
                )?;
                let end_type = self.check_expr(end, false)?;
                self.expect_type(end_type, ValueType::I64, span_of_expr(end), "for range end")?;

                self.scopes
                    .push(HashMap::from([(name.clone(), ValueType::I64)]));
                self.immutable_scopes
                    .push(std::collections::HashSet::from([name.clone()]));
                self.loop_depth += 1;
                let body_result = self.check_block(body, return_type, false);
                self.loop_depth -= 1;
                body_result?;
                self.scopes.pop();
                self.immutable_scopes.pop();
                Ok(false)
            }
            Stmt::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(at(*span, "`break` is only valid inside a loop"));
                }
                Ok(false)
            }
            Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(at(*span, "`continue` is only valid inside a loop"));
                }
                Ok(false)
            }
            Stmt::Return { value, span } => {
                if let Some(value) = value {
                    let value_type = self.check_expr(value, false)?;
                    self.expect_type(value_type, return_type, span_of_expr(value), "return value")?;
                } else if return_type != ValueType::Unit {
                    return Err(at(
                        *span,
                        "`return;` is only valid in a function returning `unit`",
                    ));
                }
                Ok(true)
            }
            Stmt::Expr(expr) => {
                self.check_expr(expr, false)?;
                Ok(false)
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, allow_min_magnitude: bool) -> Result<ValueType, String> {
        match expr {
            Expr::Integer(value, span) => {
                if *value <= i64::MAX as u64
                    || (allow_min_magnitude && *value == (i64::MAX as u64) + 1)
                {
                    Ok(ValueType::I64)
                } else {
                    Err(at(*span, "integer literal is out of range for i64"))
                }
            }
            Expr::Bool(_, _) => Ok(ValueType::Bool),
            Expr::String(_, _) => Ok(ValueType::Str),
            Expr::Array(elements, span) => {
                let Some(first) = elements.first() else {
                    return Err(at(
                        *span,
                        "array literals must contain at least one element",
                    ));
                };
                if elements.len() > u32::MAX as usize {
                    return Err(at(*span, "array literal is too large"));
                }
                let first_type = self.check_expr(first, false)?;
                let element = match first_type {
                    ValueType::I64 => ArrayElementType::I64,
                    ValueType::Bool => ArrayElementType::Bool,
                    ValueType::Str => ArrayElementType::Str,
                    ValueType::Unit | ValueType::Array(_, _) | ValueType::Enum(_) => {
                        return Err(at(
                            *span,
                            "array elements must have type `i64`, `bool`, or `str`",
                        ));
                    }
                };
                for value in &elements[1..] {
                    let value_type = self.check_expr(value, false)?;
                    self.expect_type(value_type, first_type, span_of_expr(value), "array element")?;
                }
                Ok(ValueType::Array(element, elements.len() as u32))
            }
            Expr::Variable(name, span) => self
                .lookup_local(name)
                .ok_or_else(|| at(*span, format!("undefined variable `{name}`"))),
            Expr::Index { array, index, span } => {
                let array_type = self.check_expr(array, false)?;
                let index_type = self.check_expr(index, false)?;
                self.expect_type(
                    index_type,
                    ValueType::I64,
                    span_of_expr(index),
                    "array index",
                )?;
                match array_type {
                    ValueType::Array(element, _) => Ok(element.into()),
                    _ => Err(at(*span, "indexing requires an array")),
                }
            }
            Expr::Unary { op, value, span } => {
                let value_type = self.check_expr(
                    value,
                    matches!(op, UnaryOp::Negate) && matches!(value.as_ref(), Expr::Integer(_, _)),
                )?;
                let expected = match op {
                    UnaryOp::Negate => ValueType::I64,
                    UnaryOp::Not => ValueType::Bool,
                };
                self.expect_type(value_type, expected, *span, "unary operator operand")?;
                Ok(expected)
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left_type = self.check_expr(left, false)?;
                let right_type = self.check_expr(right, false)?;
                if left_type != right_type {
                    return Err(at(
                        *span,
                        format!(
                            "operator operands have different types: {} and {}",
                            type_name(left_type, &self.enums),
                            type_name(right_type, &self.enums)
                        ),
                    ));
                }
                let result_type = match op {
                    BinaryOp::Add
                    | BinaryOp::Subtract
                    | BinaryOp::Multiply
                    | BinaryOp::Divide
                    | BinaryOp::Remainder => {
                        self.expect_type(left_type, ValueType::I64, *span, "arithmetic operands")?;
                        ValueType::I64
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == ValueType::Unit {
                            return Err(at(*span, "void expressions cannot be compared"));
                        }
                        if matches!(left_type, ValueType::Array(_, _)) {
                            return Err(at(*span, "array values cannot be compared"));
                        }
                        if matches!(left_type, ValueType::Enum(_)) {
                            return Err(at(*span, "enum values cannot be compared"));
                        }
                        ValueType::Bool
                    }
                    BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => {
                        self.expect_type(left_type, ValueType::I64, *span, "comparison operands")?;
                        ValueType::Bool
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        self.expect_type(left_type, ValueType::Bool, *span, "logical operands")?;
                        ValueType::Bool
                    }
                };
                Ok(result_type)
            }
            Expr::Call { name, args, span } => {
                if name == "len" {
                    if args.len() != 1 {
                        return Err(at(
                            *span,
                            format!("function `len` expects 1 argument(s), got {}", args.len()),
                        ));
                    }
                    let array_type = self.check_expr(&args[0], false)?;
                    if !matches!(array_type, ValueType::Array(_, _)) {
                        return Err(at(
                            span_of_expr(&args[0]),
                            "argument 1 to `len` must be an array",
                        ));
                    }
                    return Ok(ValueType::I64);
                }
                let signature = self
                    .signatures
                    .get(name)
                    .ok_or_else(|| at(*span, format!("unknown function `{name}`")))?;
                if args.len() != signature.params.len() {
                    return Err(at(
                        *span,
                        format!(
                            "function `{name}` expects {} argument(s), got {}",
                            signature.params.len(),
                            args.len()
                        ),
                    ));
                }
                let params = signature.params.clone();
                let return_type = signature.return_type;
                for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
                    let actual = self.check_expr(arg, false)?;
                    self.expect_type(
                        actual,
                        expected,
                        span_of_expr(arg),
                        &format!("argument {} to `{name}`", index + 1),
                    )?;
                }
                Ok(return_type)
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                let condition_type = self.check_expr(condition, false)?;
                self.expect_type(
                    condition_type,
                    ValueType::Bool,
                    span_of_expr(condition),
                    "if condition",
                )?;
                let then_type = self.check_block_result(then_block, *span)?;
                let else_type = self.check_block_result(else_block, *span)?;
                if then_type != else_type {
                    return Err(at(
                        *span,
                        format!(
                            "`if` expression branches have different types: {} and {}",
                            type_name(then_type, &self.enums),
                            type_name(else_type, &self.enums)
                        ),
                    ));
                }
                if then_type == ValueType::Unit {
                    return Err(at(*span, "`if` expression branches must produce a value"));
                }
                Ok(then_type)
            }
            Expr::Variant {
                enum_name,
                variant_name,
                payload,
                span,
            } => {
                let enum_id = self
                    .enum_ids
                    .get(enum_name)
                    .copied()
                    .ok_or_else(|| at(*span, format!("unknown enum `{enum_name}`")))?;
                let variant = self.enums[enum_id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                    .ok_or_else(|| {
                        at(
                            *span,
                            format!("enum `{enum_name}` has no variant `{variant_name}`"),
                        )
                    })?;
                match (&variant.payload, payload) {
                    (None, None) => {}
                    (Some(expected), Some(value)) => {
                        let expected = self.resolve_type(expected, variant.span)?;
                        let actual = self.check_expr(value, false)?;
                        self.expect_type(actual, expected, span_of_expr(value), "enum payload")?;
                    }
                    (Some(_), None) => {
                        return Err(at(
                            *span,
                            format!("variant `{enum_name}::{variant_name}` requires a payload"),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(at(
                            *span,
                            format!(
                                "variant `{enum_name}::{variant_name}` does not take a payload"
                            ),
                        ));
                    }
                }
                Ok(ValueType::Enum(enum_id))
            }
            Expr::Match { value, arms, span } => {
                let enum_id = match self.check_expr(value, false)? {
                    ValueType::Enum(enum_id) => enum_id,
                    actual => {
                        return Err(at(
                            *span,
                            format!(
                                "match requires an enum value, got {}",
                                type_name(actual, &self.enums)
                            ),
                        ));
                    }
                };
                let declaration = self.enums[enum_id].clone();
                let mut seen = std::collections::HashSet::new();
                let mut wildcard_seen = false;
                let mut result_type = None;
                for arm in arms {
                    let variant = match &arm.pattern {
                        MatchPattern::Wildcard => {
                            if wildcard_seen {
                                return Err(at(arm.span, "duplicate wildcard match arm"));
                            }
                            if seen.len() == declaration.variants.len() {
                                return Err(at(
                                    arm.span,
                                    "unreachable wildcard match arm after all variants are covered",
                                ));
                            }
                            wildcard_seen = true;
                            None
                        }
                        MatchPattern::Variant {
                            enum_name,
                            variant_name,
                            binding,
                            ignore_payload,
                        } => {
                            if wildcard_seen {
                                return Err(at(arm.span, "unreachable match arm after wildcard"));
                            }
                            if enum_name != &declaration.name {
                                return Err(at(
                                    arm.span,
                                    format!(
                                        "match pattern uses enum `{enum_name}`, expected `{}`",
                                        declaration.name
                                    ),
                                ));
                            }
                            let variant_index = declaration
                                .variants
                                .iter()
                                .position(|variant| variant.name == *variant_name)
                                .ok_or_else(|| {
                                    at(
                                        arm.span,
                                        format!(
                                            "enum `{}` has no variant `{variant_name}`",
                                            declaration.name
                                        ),
                                    )
                                })?;
                            if !seen.insert(variant_index) {
                                return Err(at(
                                    arm.span,
                                    format!(
                                        "duplicate match arm for `{}::{variant_name}`",
                                        declaration.name
                                    ),
                                ));
                            }
                            let variant = &declaration.variants[variant_index];
                            match (&variant.payload, binding, ignore_payload) {
                                (Some(_), None, false) => {
                                    return Err(at(
                                        arm.span,
                                        format!(
                                            "match arm `{}::{}` must bind its payload or use `_`",
                                            declaration.name, variant.name
                                        ),
                                    ));
                                }
                                (None, Some(_), _) | (None, None, true) => {
                                    return Err(at(
                                        arm.span,
                                        format!(
                                            "match arm `{}::{}` has no payload to bind or ignore",
                                            declaration.name, variant.name
                                        ),
                                    ));
                                }
                                _ => {}
                            }
                            Some(variant)
                        }
                    };
                    self.scopes.push(HashMap::new());
                    self.immutable_scopes.push(std::collections::HashSet::new());
                    let binding = match &arm.pattern {
                        MatchPattern::Variant {
                            binding: Some(binding),
                            ..
                        } => Some(binding),
                        _ => None,
                    };
                    let arm_result = if let (Some(binding), Some(variant)) = (binding, variant) {
                        let payload_type = variant
                            .payload
                            .as_ref()
                            .expect("payload binding validated above");
                        let binding_type = self.resolve_type(payload_type, arm.span)?;
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(binding.clone(), binding_type);
                        self.check_block_result(&arm.body, arm.span)
                    } else {
                        self.check_block_result(&arm.body, arm.span)
                    };
                    self.scopes.pop();
                    self.immutable_scopes.pop();
                    let actual = arm_result?;
                    if let Some(expected) = result_type {
                        self.expect_type(actual, expected, arm.span, "match arm result")?;
                    } else {
                        result_type = Some(actual);
                    }
                }
                if !wildcard_seen && seen.len() != declaration.variants.len() {
                    let missing = declaration
                        .variants
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !seen.contains(index))
                        .map(|(_, variant)| variant.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(at(
                        *span,
                        format!(
                            "non-exhaustive match on `{}`; missing variant(s): {missing}",
                            declaration.name
                        ),
                    ));
                }
                result_type.ok_or_else(|| at(*span, "match expression must have at least one arm"))
            }
        }
    }

    fn resolve_type(&self, ty: &Type, span: Span) -> Result<ValueType, String> {
        Ok(match ty {
            Type::I64 => ValueType::I64,
            Type::Bool => ValueType::Bool,
            Type::Str => ValueType::Str,
            Type::Unit => ValueType::Unit,
            Type::Array(element, length) => ValueType::Array(*element, *length),
            Type::Enum(name) => ValueType::Enum(
                *self
                    .enum_ids
                    .get(name)
                    .ok_or_else(|| at(span, format!("unknown enum type `{name}`")))?,
            ),
        })
    }

    fn check_block_result(&mut self, block: &Block, span: Span) -> Result<ValueType, String> {
        self.scopes.push(HashMap::new());
        self.immutable_scopes.push(std::collections::HashSet::new());
        let result = (|| {
            for statement in &block.statements {
                match statement {
                    Stmt::Let { .. }
                    | Stmt::Assign { .. }
                    | Stmt::AssignIndex { .. }
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
        result
    }

    fn expect_type(
        &self,
        actual: ValueType,
        expected: ValueType,
        span: Span,
        context: &str,
    ) -> Result<(), String> {
        if actual == expected {
            Ok(())
        } else {
            Err(at(
                span,
                format!(
                    "{context} has type {}, expected {}",
                    type_name(actual, &self.enums),
                    type_name(expected, &self.enums)
                ),
            ))
        }
    }

    fn lookup_local(&self, name: &str) -> Option<ValueType> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn is_immutable_binding(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .zip(&self.immutable_scopes)
            .rev()
            .find_map(|(scope, immutable)| {
                scope.contains_key(name).then(|| immutable.contains(name))
            })
            .unwrap_or(false)
    }
}

fn span_of_expr(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Variable(_, span)
        | Expr::Call { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Array(_, span)
        | Expr::Index { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Variant { span, .. } => *span,
        Expr::Bool(_, span) | Expr::String(_, span) => *span,
    }
}

fn at(span: Span, message: impl std::fmt::Display) -> String {
    format!(
        "line {}:{}:{}: {message}",
        span.line, span.column, span.end_column
    )
}

fn type_name(ty: ValueType, enums: &[EnumDecl]) -> String {
    match ty {
        ValueType::I64 => "i64".into(),
        ValueType::Bool => "bool".into(),
        ValueType::Str => "str".into(),
        ValueType::Unit => "unit".into(),
        ValueType::Array(element, length) => {
            format!("[{}; {length}]", type_name(element.into(), enums))
        }
        ValueType::Enum(index) => enums
            .get(index)
            .map(|declaration| declaration.name.clone())
            .unwrap_or_else(|| "<invalid enum>".into()),
    }
}

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
        }
    }
}
