use crate::diagnostic::Diagnostic;
use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, GlobalDecl, MatchArm,
    MatchPattern, Program, Span, Stmt, StructDecl, Type, UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ValueType {
    I64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    Array(usize),
    Enum(usize),
    Struct(usize),
    Tuple(usize),
    Vector(usize),
    Function(usize),
    Reference(usize),
    MutableReference(usize),
    RawPointer(usize),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FunctionShape {
    params: Vec<ValueType>,
    return_type: ValueType,
}

#[derive(Clone, Copy)]
struct ArrayShape {
    element: ArrayElementType,
    length: u32,
}

#[derive(Clone, Copy)]
struct VectorShape {
    element: ArrayElementType,
}

#[derive(Clone)]
struct Signature {
    params: Vec<ValueType>,
    return_type: ValueType,
}

/// A place currently held by a mutable safe reference. The root identifies a
/// particular lexical binding, not just its spelling, and projections retain
/// enough structure to support precise overlap checks as borrowing expands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BorrowPlace {
    root: BorrowRoot,
    projections: Vec<BorrowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MutableReferenceOrigin {
    Place(BorrowPlace),
    Unknown,
}

impl MutableReferenceOrigin {
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Place(left), Self::Place(right)) => left.overlaps(right),
            // Function parameters may alias caller-provided places. Keep those
            // origins conservative even when their parameter names differ.
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BorrowRoot {
    scope_index: usize,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BorrowProjection {
    Field(String),
    Index(Option<i64>),
}

impl BorrowPlace {
    fn overlaps(&self, other: &Self) -> bool {
        if self.root != other.root {
            return false;
        }

        for (left, right) in self.projections.iter().zip(&other.projections) {
            match (left, right) {
                (BorrowProjection::Field(left), BorrowProjection::Field(right))
                    if left == right => {}
                (BorrowProjection::Index(Some(left)), BorrowProjection::Index(Some(right)))
                    if left == right => {}
                (BorrowProjection::Field(_), BorrowProjection::Field(_))
                | (BorrowProjection::Index(Some(_)), BorrowProjection::Index(Some(_))) => {
                    return false;
                }
                // An unknown index, or different projection kinds, may refer to
                // the same memory. Remain conservative in those cases.
                _ => return true,
            }
        }

        // A place overlaps any of its descendants (for example `point` and
        // `point.x`).
        true
    }
}

struct Checker {
    signatures: HashMap<String, Signature>,
    scopes: Vec<HashMap<String, ValueType>>,
    immutable_scopes: Vec<std::collections::HashSet<String>>,
    mutable_borrows: Vec<Vec<BorrowPlace>>,
    shared_borrows: Vec<Vec<BorrowPlace>>,
    mutable_reference_origins: Vec<HashMap<String, MutableReferenceOrigin>>,
    loop_scopes: Vec<std::collections::HashSet<String>>,
    loop_contexts: Vec<LoopContext>,
    current_return_type: ValueType,
    enum_ids: HashMap<String, usize>,
    enums: Vec<EnumDecl>,
    struct_ids: HashMap<String, usize>,
    structs: Vec<StructDecl>,
    tuple_shapes: Vec<Vec<ValueType>>,
    tuple_shape_ids: HashMap<Vec<ValueType>, usize>,
    array_shapes: Vec<ArrayShape>,
    array_shape_ids: HashMap<(ArrayElementType, u32), usize>,
    vector_shapes: Vec<VectorShape>,
    vector_shape_ids: HashMap<ArrayElementType, usize>,
    function_shapes: Vec<FunctionShape>,
    function_shape_ids: HashMap<FunctionShape, usize>,
    reference_types: Vec<ValueType>,
    reference_type_ids: HashMap<ValueType, usize>,
    mutable_reference_types: Vec<ValueType>,
    mutable_reference_type_ids: HashMap<ValueType, usize>,
    raw_pointer_types: Vec<(ValueType, bool)>,
    raw_pointer_type_ids: HashMap<(ValueType, bool), usize>,
    allow_shared_borrow: bool,
    allow_mutable_borrow_initializer: bool,
    lambda_scope_boundaries: Vec<usize>,
    lambda_capture_scopes: Vec<Vec<String>>,
    lambda_captures: HashMap<(Option<usize>, usize, usize), Vec<String>>,
    module_context: String,
    function_visibility: HashMap<String, (String, bool)>,
    global_declarations: HashMap<String, GlobalDecl>,
    global_types: HashMap<String, ValueType>,
    global_states: HashMap<String, u8>,
}

struct LoopContext {
    value_loop: bool,
    result_type: Option<ValueType>,
}

pub fn check(program: &mut Program) -> Result<(), Diagnostic> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        scopes: Vec::new(),
        immutable_scopes: Vec::new(),
        mutable_borrows: Vec::new(),
        shared_borrows: Vec::new(),
        mutable_reference_origins: Vec::new(),
        loop_scopes: Vec::new(),
        loop_contexts: Vec::new(),
        current_return_type: ValueType::Unit,
        enum_ids: HashMap::new(),
        enums: Vec::new(),
        struct_ids: HashMap::new(),
        structs: Vec::new(),
        tuple_shapes: Vec::new(),
        tuple_shape_ids: HashMap::new(),
        array_shapes: Vec::new(),
        array_shape_ids: HashMap::new(),
        vector_shapes: Vec::new(),
        vector_shape_ids: HashMap::new(),
        function_shapes: Vec::new(),
        function_shape_ids: HashMap::new(),
        reference_types: Vec::new(),
        reference_type_ids: HashMap::new(),
        mutable_reference_types: Vec::new(),
        mutable_reference_type_ids: HashMap::new(),
        raw_pointer_types: Vec::new(),
        raw_pointer_type_ids: HashMap::new(),
        allow_shared_borrow: false,
        allow_mutable_borrow_initializer: false,
        lambda_scope_boundaries: Vec::new(),
        lambda_capture_scopes: Vec::new(),
        lambda_captures: HashMap::new(),
        module_context: String::new(),
        function_visibility: HashMap::new(),
        global_declarations: HashMap::new(),
        global_types: HashMap::new(),
        global_states: HashMap::new(),
    };
    checker.collect_signatures(program)?;
    for global in &program.globals {
        checker.check_global(&global.name, global.span)?;
    }
    for global in &mut program.globals {
        if global.ty.is_none() {
            let ty = checker.global_types[&global.name];
            global.ty = Some(checker.type_from_value(ty));
        }
    }
    for function in &program.functions {
        checker.module_context = function.module.clone();
        checker.check_function(function)?;
    }
    program.lambda_captures = checker.lambda_captures;
    Ok(())
}

mod declarations;
mod expressions;
mod matching;
mod places;
mod statements;
mod types;

use types::{assignment_root, at, span_of_expr};

#[cfg(test)]
mod borrow_place_tests {
    use super::{BorrowPlace, BorrowProjection, BorrowRoot};

    fn place(name: &str, projections: Vec<BorrowProjection>) -> BorrowPlace {
        BorrowPlace {
            root: BorrowRoot {
                scope_index: 0,
                name: name.to_owned(),
            },
            projections,
        }
    }

    #[test]
    fn place_overlap_distinguishes_disjoint_fields_and_constant_indices() {
        assert!(
            !place("point", vec![BorrowProjection::Field("x".into())])
                .overlaps(&place("point", vec![BorrowProjection::Field("y".into())]))
        );
        assert!(
            !place("values", vec![BorrowProjection::Index(Some(0))])
                .overlaps(&place("values", vec![BorrowProjection::Index(Some(1))]))
        );
    }

    #[test]
    fn place_overlap_is_conservative_for_unknown_or_parent_projections() {
        assert!(
            place("values", vec![BorrowProjection::Index(None)])
                .overlaps(&place("values", vec![BorrowProjection::Index(Some(1))]))
        );
        assert!(
            place("point", Vec::new())
                .overlaps(&place("point", vec![BorrowProjection::Field("x".into())]))
        );
        assert!(!place("point", Vec::new()).overlaps(&place("other", Vec::new())));
    }

    #[test]
    fn shadowed_names_have_distinct_borrow_roots() {
        let outer = place("value", Vec::new());
        let inner = BorrowPlace {
            root: BorrowRoot {
                scope_index: 1,
                name: "value".into(),
            },
            projections: Vec::new(),
        };
        assert!(!outer.overlaps(&inner));
    }
}
