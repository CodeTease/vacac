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
    validation::validate_templates(program)?;
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

mod inference;
mod specialization;
mod traversal;
mod validation;
