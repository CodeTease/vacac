use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, VecDeque};

type Substitution = HashMap<String, Type>;

struct PendingFunction {
    function: Function,
    substitutions: Substitution,
}

struct Specializer {
    generic_functions: HashMap<String, Function>,
    generic_structs: HashMap<String, StructDecl>,
    enum_names: std::collections::HashSet<String>,
    enum_declarations: HashMap<String, EnumDecl>,
    concrete_structs: HashMap<String, StructDecl>,
    function_instances: HashMap<(String, Vec<String>), String>,
    struct_instances: HashMap<String, (String, Vec<Type>)>,
    signatures: HashMap<String, (Vec<Type>, Type)>,
    function_queue: VecDeque<PendingFunction>,
    generated_structs: Vec<StructDecl>,
    instance_count: usize,
}

const MAX_INSTANCES: usize = 512;

pub(super) fn monomorphize(program: &mut Program) -> Result<(), Diagnostic> {
    validate_templates(program)?;
    const RESERVED_FUNCTIONS: &[&str] = &[
        "print_i64",
        "print_f32",
        "print_f64",
        "print",
        "println",
        "read_i64",
        "len",
        "vec_new",
        "vec_push",
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
        "realloc",
        "vaca_vec_new",
        "vaca_vec_push",
    ];
    for function in program
        .functions
        .iter()
        .filter(|function| !function.generic_params.is_empty())
    {
        if function.name == "main" {
            return Err(Diagnostic::at(
                function.span.into(),
                "the entry point `main` cannot be generic",
            ));
        }
        if RESERVED_FUNCTIONS.contains(&function.name.as_str()) {
            return Err(Diagnostic::at(
                function.span.into(),
                format!("`{}` is a reserved compiler/runtime name", function.name),
            ));
        }
    }
    let mut specializer = Specializer {
        generic_functions: program
            .functions
            .iter()
            .filter(|function| !function.generic_params.is_empty())
            .map(|function| (function.name.clone(), function.clone()))
            .collect(),
        generic_structs: program
            .structs
            .iter()
            .filter(|declaration| !declaration.generic_params.is_empty())
            .map(|declaration| (declaration.name.clone(), declaration.clone()))
            .collect(),
        enum_names: program
            .enums
            .iter()
            .map(|declaration| declaration.name.clone())
            .collect(),
        enum_declarations: program
            .enums
            .iter()
            .map(|declaration| (declaration.name.clone(), declaration.clone()))
            .collect(),
        concrete_structs: program
            .structs
            .iter()
            .filter(|declaration| declaration.generic_params.is_empty())
            .map(|declaration| (declaration.name.clone(), declaration.clone()))
            .collect(),
        function_instances: HashMap::new(),
        struct_instances: HashMap::new(),
        signatures: HashMap::new(),
        function_queue: VecDeque::new(),
        generated_structs: Vec::new(),
        instance_count: 0,
    };

    program
        .functions
        .retain(|function| function.generic_params.is_empty());
    program
        .structs
        .retain(|declaration| declaration.generic_params.is_empty());

    for declaration in &mut program.structs {
        for field in &mut declaration.fields {
            field.ty =
                specializer.normalize_type(field.ty.clone(), &Substitution::new(), field.span)?;
        }
    }
    for declaration in &mut program.enums {
        for variant in &mut declaration.variants {
            for payload in &mut variant.payload {
                *payload = specializer.normalize_type(
                    payload.clone(),
                    &Substitution::new(),
                    variant.span,
                )?;
            }
        }
    }

    // Register every concrete signature before visiting any body, permitting forward calls.
    for function in &mut program.functions {
        for param in &mut function.params {
            param.ty =
                specializer.normalize_type(param.ty.clone(), &Substitution::new(), param.span)?;
        }
        function.return_type = specializer.normalize_type(
            function.return_type.clone(),
            &Substitution::new(),
            function.span,
        )?;
        specializer.signatures.insert(
            function.name.clone(),
            (
                function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                function.return_type.clone(),
            ),
        );
        specializer.function_queue.push_back(PendingFunction {
            function: function.clone(),
            substitutions: Substitution::new(),
        });
    }

    while let Some(mut pending) = specializer.function_queue.pop_front() {
        specializer.process_function(&mut pending.function, &pending.substitutions)?;
        if let Some(concrete) = program
            .functions
            .iter_mut()
            .find(|function| function.name == pending.function.name)
        {
            concrete.body = pending.function.body;
        } else {
            program.functions.push(pending.function);
        }
    }

    program.structs.append(&mut specializer.generated_structs);
    Ok(())
}

fn validate_templates(program: &Program) -> Result<(), Diagnostic> {
    let generic_structs = program
        .structs
        .iter()
        .filter(|declaration| !declaration.generic_params.is_empty())
        .map(|declaration| (declaration.name.as_str(), declaration.generic_params.len()))
        .collect::<HashMap<_, _>>();
    let known_types = program
        .structs
        .iter()
        .map(|declaration| declaration.name.as_str())
        .chain(
            program
                .enums
                .iter()
                .map(|declaration| declaration.name.as_str()),
        )
        .collect::<std::collections::HashSet<_>>();

    fn check_type(
        ty: &Type,
        parameters: &std::collections::HashSet<&str>,
        generic_structs: &HashMap<&str, usize>,
        known_types: &std::collections::HashSet<&str>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match ty {
            Type::Parameter(name) if !parameters.contains(name.as_str()) => Err(Diagnostic::at(
                span.into(),
                format!("unknown generic type parameter `{name}`"),
            )),
            Type::Applied(name, arguments) => {
                let Some(expected) = generic_structs.get(name.as_str()) else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!("`{name}` is not a generic struct"),
                    ));
                };
                if arguments.len() != *expected {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "generic struct `{name}` expects {expected} type argument(s), got {}",
                            arguments.len()
                        ),
                    ));
                }
                for argument in arguments {
                    check_type(argument, parameters, generic_structs, known_types, span)?;
                }
                Ok(())
            }
            Type::Named(name) if generic_structs.contains_key(name.as_str()) => {
                Err(Diagnostic::at(
                    span.into(),
                    format!("generic struct `{name}` requires explicit type arguments"),
                ))
            }
            Type::Named(name) if !known_types.contains(name.as_str()) => Err(Diagnostic::at(
                span.into(),
                format!("unknown type `{name}`"),
            )),
            Type::Array(element, _) | Type::Vector(element) => {
                check_type(element, parameters, generic_structs, known_types, span)
            }
            Type::Parameter(_)
            | Type::Named(_)
            | Type::I64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str
            | Type::Unit => Ok(()),
        }
    }

    for declaration in program
        .structs
        .iter()
        .filter(|declaration| !declaration.generic_params.is_empty())
    {
        let parameters = declaration
            .generic_params
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for field in &declaration.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::at(
                    field.span.into(),
                    format!("struct field `{}` is defined more than once", field.name),
                ));
            }
            if field.ty == Type::Unit {
                return Err(Diagnostic::at(
                    field.span.into(),
                    "struct fields cannot have type `unit`",
                ));
            }
            check_type(
                &field.ty,
                &parameters,
                &generic_structs,
                &known_types,
                field.span,
            )?;
        }
    }
    for function in program
        .functions
        .iter()
        .filter(|function| !function.generic_params.is_empty())
    {
        let parameters = function
            .generic_params
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for param in &function.params {
            if !seen.insert(param.name.as_str()) {
                return Err(Diagnostic::at(
                    param.span.into(),
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            if param.ty == Type::Unit {
                return Err(Diagnostic::at(
                    param.span.into(),
                    "parameters cannot have type `unit`",
                ));
            }
            check_type(
                &param.ty,
                &parameters,
                &generic_structs,
                &known_types,
                param.span,
            )?;
        }
        check_type(
            &function.return_type,
            &parameters,
            &generic_structs,
            &known_types,
            function.span,
        )?;
    }
    Ok(())
}

impl Specializer {
    fn new_instance(&mut self, span: Span) -> Result<(), Diagnostic> {
        self.instance_count += 1;
        if self.instance_count > MAX_INSTANCES {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic specialization exceeded the limit of {MAX_INSTANCES} concrete instances"
                ),
            ));
        }
        Ok(())
    }

    fn normalize_type(
        &mut self,
        ty: Type,
        substitutions: &Substitution,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        Ok(match ty {
            Type::Parameter(name) => {
                let replacement = substitutions.get(&name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        span.into(),
                        format!("unresolved generic type parameter `{name}`"),
                    )
                })?;
                self.normalize_type(replacement, substitutions, span)?
            }
            Type::Applied(name, arguments) => {
                let arguments = arguments
                    .into_iter()
                    .map(|argument| self.normalize_type(argument, substitutions, span))
                    .collect::<Result<Vec<_>, _>>()?;
                self.instantiate_struct(&name, arguments, span)?
            }
            Type::Named(name) if self.generic_structs.contains_key(&name) => {
                return Err(Diagnostic::at(
                    span.into(),
                    format!("generic struct `{name}` requires explicit type arguments"),
                ));
            }
            Type::Array(element, length) => Type::Array(
                Box::new(self.normalize_type(*element, substitutions, span)?),
                length,
            ),
            Type::Vector(element) => Type::Vector(Box::new(self.normalize_type(
                *element,
                substitutions,
                span,
            )?)),
            other => other,
        })
    }

    fn instantiate_struct(
        &mut self,
        template_name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        let Some(template) = self.generic_structs.get(template_name).cloned() else {
            return Err(Diagnostic::at(
                span.into(),
                format!("`{template_name}` is not a generic struct"),
            ));
        };
        if arguments.len() != template.generic_params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic struct `{template_name}` expects {} type argument(s), got {}",
                    template.generic_params.len(),
                    arguments.len()
                ),
            ));
        }
        let concrete_name = format!(
            "{template_name}::<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        );
        if self.struct_instances.contains_key(&concrete_name) {
            return Ok(Type::Named(concrete_name));
        }
        self.new_instance(span)?;
        self.struct_instances.insert(
            concrete_name.clone(),
            (template_name.to_owned(), arguments.clone()),
        );
        let substitutions = template
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.clone())
            .collect::<Substitution>();
        let mut concrete = template;
        concrete.name = concrete_name.clone();
        concrete.generic_params.clear();
        for field in &mut concrete.fields {
            field.ty = self.normalize_type(field.ty.clone(), &substitutions, field.span)?;
        }
        self.generated_structs.push(concrete);
        Ok(Type::Named(concrete_name))
    }

    fn instantiate_function(
        &mut self,
        template_name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let Some(template) = self.generic_functions.get(template_name).cloned() else {
            return Err(Diagnostic::at(
                span.into(),
                format!("`{template_name}` is not a generic function"),
            ));
        };
        if arguments.len() != template.generic_params.len() {
            return Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic function `{template_name}` expects {} type argument(s), got {}",
                    template.generic_params.len(),
                    arguments.len()
                ),
            ));
        }
        let key = (
            template_name.to_owned(),
            arguments.iter().map(type_key).collect::<Vec<_>>(),
        );
        if let Some(name) = self.function_instances.get(&key) {
            return Ok(name.clone());
        }
        self.new_instance(span)?;
        let concrete_name = format!(
            "{template_name}::<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        );
        self.function_instances.insert(key, concrete_name.clone());
        let substitutions = template
            .generic_params
            .iter()
            .cloned()
            .zip(arguments)
            .collect::<Substitution>();
        let mut concrete = template;
        concrete.name = concrete_name.clone();
        concrete.generic_params.clear();
        for param in &mut concrete.params {
            param.ty = self.normalize_type(param.ty.clone(), &substitutions, param.span)?;
        }
        concrete.return_type =
            self.normalize_type(concrete.return_type.clone(), &substitutions, concrete.span)?;
        self.signatures.insert(
            concrete_name.clone(),
            (
                concrete
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                concrete.return_type.clone(),
            ),
        );
        self.function_queue.push_back(PendingFunction {
            function: concrete,
            substitutions,
        });
        Ok(concrete_name)
    }

    fn process_function(
        &mut self,
        function: &mut Function,
        substitutions: &Substitution,
    ) -> Result<(), Diagnostic> {
        let mut variables = function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect::<HashMap<_, _>>();
        self.process_block(
            &mut function.body,
            substitutions,
            &mut variables,
            function.span,
        )
    }

    fn process_block(
        &mut self,
        block: &mut Block,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        for statement in &mut block.statements {
            match statement {
                Stmt::Let {
                    name, ty, value, ..
                } => {
                    if let Some(ty) = ty {
                        *ty = self.normalize_type(ty.clone(), substitutions, span)?;
                    }
                    self.process_expr(value, substitutions, variables)?;
                    if let Some(ty) = ty
                        .clone()
                        .or_else(|| self.infer_expr_type(value, variables))
                    {
                        variables.insert(name.clone(), ty);
                    }
                }
                Stmt::Assign { name, value, .. } => {
                    self.process_expr(value, substitutions, variables)?;
                    if let Some(ty) = self.infer_expr_type(value, variables) {
                        variables.insert(name.clone(), ty);
                    }
                }
                Stmt::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.process_expr(condition, substitutions, variables)?;
                    let mut then_variables = variables.clone();
                    self.process_block(then_block, substitutions, &mut then_variables, span)?;
                    if let Some(else_block) = else_block {
                        let mut else_variables = variables.clone();
                        self.process_block(else_block, substitutions, &mut else_variables, span)?;
                    }
                }
                Stmt::While { condition, body } => {
                    self.process_expr(condition, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::For {
                    name,
                    start,
                    end,
                    body,
                } => {
                    self.process_expr(start, substitutions, variables)?;
                    self.process_expr(end, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    body_variables.insert(name.clone(), Type::I64);
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::ForArray {
                    name, array, body, ..
                } => {
                    self.process_expr(array, substitutions, variables)?;
                    let mut body_variables = variables.clone();
                    if let Some(element) =
                        self.infer_expr_type(array, variables)
                            .and_then(|ty| match ty {
                                Type::Array(element, _) | Type::Vector(element) => Some(*element),
                                _ => None,
                            })
                    {
                        body_variables.insert(name.clone(), element);
                    }
                    self.process_block(body, substitutions, &mut body_variables, span)?;
                }
                Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                    if let Some(value) = value {
                        self.process_expr(value, substitutions, variables)?;
                    }
                }
                Stmt::Continue { .. } => {}
                Stmt::AssignPlace { target, value, .. } => {
                    self.process_expr(target, substitutions, variables)?;
                    self.process_expr(value, substitutions, variables)?;
                }
                Stmt::Expr(value) => self.process_expr(value, substitutions, variables)?,
            }
        }
        if let Some(tail) = &mut block.tail {
            self.process_expr(tail, substitutions, variables)?;
        }
        Ok(())
    }

    fn process_expr(
        &mut self,
        expr: &mut Expr,
        substitutions: &Substitution,
        variables: &mut HashMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        match expr {
            Expr::Array(values, _) => {
                for value in values {
                    self.process_expr(value, substitutions, variables)?;
                }
            }
            Expr::Index { array, index, .. } => {
                self.process_expr(array, substitutions, variables)?;
                self.process_expr(index, substitutions, variables)?;
            }
            Expr::Field { value, .. } | Expr::Unary { value, .. } => {
                self.process_expr(value, substitutions, variables)?;
            }
            Expr::Struct {
                name,
                type_arguments,
                fields,
                span,
            } => {
                for argument in type_arguments.iter_mut() {
                    *argument = self.normalize_type(argument.clone(), substitutions, *span)?;
                }
                if self.generic_structs.contains_key(name) {
                    let concrete_type =
                        self.instantiate_struct(name, type_arguments.clone(), *span)?;
                    let Type::Named(concrete_name) = concrete_type else {
                        unreachable!()
                    };
                    *name = concrete_name;
                    type_arguments.clear();
                } else if !type_arguments.is_empty() {
                    return Err(Diagnostic::at(
                        (*span).into(),
                        format!("struct `{name}` does not accept generic type arguments"),
                    ));
                }
                for (_, value, _) in fields {
                    self.process_expr(value, substitutions, variables)?;
                }
            }
            Expr::Binary { left, right, .. } => {
                self.process_expr(left, substitutions, variables)?;
                self.process_expr(right, substitutions, variables)?;
            }
            Expr::Call {
                name,
                type_arguments,
                args,
                span,
            } => {
                for argument in args.iter_mut() {
                    self.process_expr(argument, substitutions, variables)?;
                }
                for argument in type_arguments.iter_mut() {
                    *argument = self.normalize_type(argument.clone(), substitutions, *span)?;
                }
                if self.generic_functions.contains_key(name) {
                    let template = self.generic_functions.get(name).unwrap().clone();
                    if args.len() != template.params.len() {
                        return Err(Diagnostic::at(
                            (*span).into(),
                            format!(
                                "function `{name}` expects {} argument(s), got {}",
                                template.params.len(),
                                args.len()
                            ),
                        ));
                    }
                    let mut inferred = HashMap::new();
                    for (parameter, argument) in template.params.iter().zip(args.iter()) {
                        if let Some(actual) = self.infer_expr_type(argument, variables) {
                            self.unify(&parameter.ty, &actual, &mut inferred, *span)?;
                        }
                    }
                    if !type_arguments.is_empty() {
                        if type_arguments.len() != template.generic_params.len() {
                            return Err(Diagnostic::at(
                                (*span).into(),
                                format!(
                                    "generic function `{name}` expects {} type argument(s), got {}",
                                    template.generic_params.len(),
                                    type_arguments.len()
                                ),
                            ));
                        }
                        let explicit_arguments = type_arguments
                            .iter()
                            .map(|argument| {
                                self.normalize_type(argument.clone(), substitutions, *span)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        for (parameter, argument) in template
                            .generic_params
                            .iter()
                            .zip(explicit_arguments.iter())
                        {
                            if let Some(inferred_type) = inferred.get(parameter)
                                && inferred_type != argument
                            {
                                return Err(Diagnostic::at(
                                    (*span).into(),
                                    format!(
                                        "explicit type argument `{}` conflicts with inferred type `{}`",
                                        type_key(argument),
                                        type_key(inferred_type)
                                    ),
                                ));
                            }
                            inferred.insert(parameter.clone(), argument.clone());
                        }
                    }
                    let mut resolved_arguments = Vec::new();
                    for parameter in &template.generic_params {
                        resolved_arguments.push(inferred.get(parameter).cloned().ok_or_else(|| {
                            Diagnostic::at(
                                (*span).into(),
                                format!(
                                    "cannot infer generic type parameter `{parameter}` for `{name}`; provide explicit type arguments with `::<...>`"
                                ),
                            )
                        })?);
                    }
                    for (parameter, argument) in template.params.iter().zip(args.iter()) {
                        if let Some(actual) = self.infer_expr_type(argument, variables) {
                            let mut checked = inferred.clone();
                            self.unify(&parameter.ty, &actual, &mut checked, *span)?;
                        }
                    }
                    *name = self.instantiate_function(name, resolved_arguments, *span)?;
                    type_arguments.clear();
                } else if !type_arguments.is_empty() {
                    return Err(Diagnostic::at(
                        (*span).into(),
                        format!("function `{name}` is not generic"),
                    ));
                }
            }
            Expr::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                self.process_expr(condition, substitutions, variables)?;
                let mut then_variables = variables.clone();
                self.process_block(then_block, substitutions, &mut then_variables, *span)?;
                let mut else_variables = variables.clone();
                self.process_block(else_block, substitutions, &mut else_variables, *span)?;
            }
            Expr::Match { value, arms, span } => {
                self.process_expr(value, substitutions, variables)?;
                let matched_type = self.infer_expr_type(value, variables);
                for arm in arms {
                    let mut arm_variables = variables.clone();
                    if let (
                        Some(Type::Named(enum_name)),
                        MatchPattern::Variant {
                            enum_name: pattern_enum,
                            variant_name,
                            bindings: Some(bindings),
                        },
                    ) = (&matched_type, &arm.pattern)
                        && enum_name == pattern_enum
                        && let Some(variant) =
                            self.enum_declarations
                                .get(enum_name)
                                .and_then(|declaration| {
                                    declaration
                                        .variants
                                        .iter()
                                        .find(|variant| variant.name == *variant_name)
                                })
                    {
                        for (binding, payload_type) in bindings.iter().zip(&variant.payload) {
                            if let Some(binding) = binding {
                                arm_variables.insert(binding.clone(), payload_type.clone());
                            }
                        }
                    }
                    self.process_block(&mut arm.body, substitutions, &mut arm_variables, *span)?;
                }
            }
            Expr::Loop { body, span } => {
                let mut body_variables = variables.clone();
                self.process_block(body, substitutions, &mut body_variables, *span)?;
            }
            Expr::Variant { payload, .. } => {
                if let Some(payload) = payload {
                    for value in payload {
                        self.process_expr(value, substitutions, variables)?;
                    }
                }
            }
            Expr::Integer(..)
            | Expr::Float32(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::String(..)
            | Expr::Variable(..) => {}
        }
        Ok(())
    }

    fn unify(
        &self,
        pattern: &Type,
        actual: &Type,
        substitutions: &mut Substitution,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match (pattern, actual) {
            (Type::Parameter(name), actual) => {
                if let Some(previous) = substitutions.get(name) {
                    if previous != actual {
                        return Err(Diagnostic::at(
                            span.into(),
                            format!(
                                "generic type parameter `{name}` is inferred as both `{}` and `{}`",
                                type_key(previous),
                                type_key(actual)
                            ),
                        ));
                    }
                } else {
                    substitutions.insert(name.clone(), actual.clone());
                }
                Ok(())
            }
            (Type::Vector(pattern), Type::Vector(actual)) => {
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::Array(pattern, pattern_length), Type::Array(actual, actual_length)) => {
                if pattern_length != actual_length {
                    return Err(Diagnostic::at(
                        span.into(),
                        "generic argument array lengths do not match",
                    ));
                }
                self.unify(pattern, actual, substitutions, span)
            }
            (Type::Applied(pattern_name, pattern_args), Type::Named(actual_name)) => {
                let Some((actual_template, actual_args)) = self.struct_instances.get(actual_name)
                else {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                };
                if pattern_name != actual_template || pattern_args.len() != actual_args.len() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                }
                for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args) {
                    self.unify(pattern_arg, actual_arg, substitutions, span)?;
                }
                Ok(())
            }
            (
                Type::Applied(pattern_name, pattern_args),
                Type::Applied(actual_name, actual_args),
            ) => {
                if pattern_name != actual_name || pattern_args.len() != actual_args.len() {
                    return Err(Diagnostic::at(
                        span.into(),
                        format!(
                            "type `{}` does not match generic type `{pattern_name}`",
                            type_key(actual)
                        ),
                    ));
                }
                for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args) {
                    self.unify(pattern_arg, actual_arg, substitutions, span)?;
                }
                Ok(())
            }
            (Type::Named(pattern), Type::Named(actual)) if pattern == actual => Ok(()),
            (Type::I64, Type::I64)
            | (Type::F32, Type::F32)
            | (Type::F64, Type::F64)
            | (Type::Bool, Type::Bool)
            | (Type::Str, Type::Str)
            | (Type::Unit, Type::Unit) => Ok(()),
            _ if pattern == actual => Ok(()),
            _ => Err(Diagnostic::at(
                span.into(),
                format!(
                    "generic argument has type `{}`, expected `{}`",
                    type_key(actual),
                    type_key(pattern)
                ),
            )),
        }
    }

    fn infer_expr_type(&self, expr: &Expr, variables: &HashMap<String, Type>) -> Option<Type> {
        match expr {
            Expr::Integer(..) => Some(Type::I64),
            Expr::Float32(..) => Some(Type::F32),
            Expr::Float(..) => Some(Type::F64),
            Expr::Bool(..) => Some(Type::Bool),
            Expr::String(..) => Some(Type::Str),
            Expr::Variable(name, _) => variables.get(name).cloned(),
            Expr::Array(values, _) => {
                let first = self.infer_expr_type(values.first()?, variables)?;
                values
                    .iter()
                    .all(|value| self.infer_expr_type(value, variables).as_ref() == Some(&first))
                    .then(|| Type::Array(Box::new(first), values.len() as u32))
            }
            Expr::Struct {
                name,
                type_arguments,
                ..
            } => {
                if type_arguments.is_empty() {
                    Some(Type::Named(name.clone()))
                } else {
                    let concrete_name = format!(
                        "{name}::<{}>",
                        type_arguments
                            .iter()
                            .map(type_key)
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    Some(Type::Named(concrete_name))
                }
            }
            Expr::Call { name, .. } => self.signatures.get(name).map(|(_, result)| result.clone()),
            Expr::Unary { value, .. } => self.infer_expr_type(value, variables),
            Expr::Binary { op, left, .. } => match op {
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::And
                | BinaryOp::Or => Some(Type::Bool),
                _ => self.infer_expr_type(left, variables),
            },
            Expr::Index { array, .. } => match self.infer_expr_type(array, variables)? {
                Type::Array(element, _) | Type::Vector(element) => Some(*element),
                _ => None,
            },
            Expr::Field { value, name, .. } => {
                let Type::Named(struct_name) = self.infer_expr_type(value, variables)? else {
                    return None;
                };
                self.concrete_structs
                    .get(&struct_name)
                    .or_else(|| {
                        self.generated_structs
                            .iter()
                            .find(|declaration| declaration.name == struct_name)
                    })?
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .map(|field| field.ty.clone())
            }
            Expr::If { then_block, .. } => then_block
                .tail
                .as_ref()
                .and_then(|value| self.infer_expr_type(value, variables)),
            Expr::Match { arms, .. } => arms
                .first()
                .and_then(|arm| arm.body.tail.as_ref())
                .and_then(|value| self.infer_expr_type(value, variables)),
            Expr::Variant { enum_name, .. } => self
                .enum_names
                .contains(enum_name)
                .then(|| Type::Named(enum_name.clone())),
            Expr::Loop { .. } => None,
        }
    }
}

fn type_key(ty: &Type) -> String {
    match ty {
        Type::I64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Unit => "unit".into(),
        Type::Named(name) | Type::Parameter(name) => name.clone(),
        Type::Applied(name, arguments) => format!(
            "{name}<{}>",
            arguments.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Type::Array(element, length) => format!("[{};{length}]", type_key(element)),
        Type::Vector(element) => format!("Vec<{}>", type_key(element)),
    }
}
