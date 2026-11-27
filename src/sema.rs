use crate::diagnostic::Diagnostic;
use crate::parser::{
    ArrayElementType, BinaryOp, Block, EnumDecl, Expr, Function, GlobalDecl, LambdaCapture,
    LambdaKey, MatchArm, MatchPattern, Program, Span, Stmt, StructDecl, Type, UnaryOp,
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
    return_lifetime: Option<String>,
    return_parameter_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceProvenance {
    sources: Vec<ReferenceSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BorrowKind {
    Shared,
    Mutable,
}

impl BorrowKind {
    fn conflicts_with(self, access: AccessMode) -> bool {
        match self {
            Self::Mutable => true,
            Self::Shared => matches!(access, AccessMode::Write | AccessMode::BorrowMutable),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessMode {
    Read,
    Write,
    BorrowShared,
    BorrowMutable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveLoan {
    id: usize,
    place: BorrowPlace,
    kind: BorrowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceSource {
    Parameter {
        index: usize,
        lifetime: Option<String>,
        place: BorrowPlace,
        loans: Vec<usize>,
    },
    Borrowed {
        place: BorrowPlace,
        loans: Vec<usize>,
        /// True when the value came from an existing reference or a call that
        /// promises to preserve one. Fresh owner borrows stay false so they
        /// cannot escape through an `if`/`match` expression.
        derived: bool,
    },
    Unknown,
}

impl ReferenceSource {
    fn loans(&self) -> &[usize] {
        match self {
            Self::Parameter { loans, .. } | Self::Borrowed { loans, .. } => loans,
            Self::Unknown => &[],
        }
    }

    fn loans_mut(&mut self) -> Option<&mut Vec<usize>> {
        match self {
            Self::Parameter { loans, .. } | Self::Borrowed { loans, .. } => Some(loans),
            Self::Unknown => None,
        }
    }

    fn place(&self) -> Option<&BorrowPlace> {
        match self {
            Self::Parameter { place, .. } | Self::Borrowed { place, .. } => Some(place),
            Self::Unknown => None,
        }
    }

    fn as_derived(&mut self) {
        if let Self::Borrowed { derived, .. } = self {
            *derived = true;
        }
    }
}

impl ReferenceProvenance {
    fn parameter(index: usize, lifetime: Option<String>, loan_id: usize, callable: usize) -> Self {
        Self {
            sources: vec![ReferenceSource::Parameter {
                index,
                lifetime,
                place: BorrowPlace::parameter(callable, index),
                loans: vec![loan_id],
            }],
        }
    }

    fn borrowed(place: BorrowPlace) -> Self {
        Self {
            sources: vec![ReferenceSource::Borrowed {
                place,
                loans: Vec::new(),
                derived: false,
            }],
        }
    }

    fn unknown() -> Self {
        Self {
            sources: vec![ReferenceSource::Unknown],
        }
    }

    fn extend(&mut self, other: Self) {
        for source in other.sources {
            if !self.sources.contains(&source) {
                self.sources.push(source);
            }
        }
    }

    fn map_places(&mut self, mut map: impl FnMut(&mut BorrowPlace)) {
        for source in &mut self.sources {
            match source {
                ReferenceSource::Parameter { place, .. }
                | ReferenceSource::Borrowed { place, .. } => map(place),
                ReferenceSource::Unknown => {}
            }
        }
    }

    /// Forget loans that ended with a block scope. Keep every source place and
    /// the loans inherited from enclosing scopes, so origins that escape an
    /// `if`/`match` still block their actual ancestors.
    fn remove_loans(&mut self, exited: &[usize]) {
        for source in &mut self.sources {
            if let Some(loans) = source.loans_mut() {
                loans.retain(|loan| !exited.contains(loan));
            }
        }
    }

    fn into_derived(mut self) -> Self {
        for source in &mut self.sources {
            source.as_derived();
        }
        self
    }

    fn is_unknown(&self) -> bool {
        self.sources
            .iter()
            .any(|source| matches!(source, ReferenceSource::Unknown))
    }
}

/// A place borrowed by a safe reference. The root identifies a particular
/// lexical binding, not just its spelling, and projections retain enough
/// structure for precise overlap checks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BorrowPlace {
    root: BorrowRoot,
    projections: Vec<BorrowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BorrowRoot {
    Local { scope_index: usize, name: String },
    Parameter { callable: usize, index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BorrowProjection {
    Field(String),
    Index(Option<i64>),
    /// A subplace that is not known at the use site, such as the pointee of a
    /// reference returned from a call. Later projections must not narrow it.
    Unknown,
}

impl BorrowPlace {
    fn parameter(callable: usize, index: usize) -> Self {
        Self {
            root: BorrowRoot::Parameter { callable, index },
            projections: Vec::new(),
        }
    }

    fn local(scope_index: usize, name: impl Into<String>) -> Self {
        Self {
            root: BorrowRoot::Local {
                scope_index,
                name: name.into(),
            },
            projections: Vec::new(),
        }
    }

    fn root_name(&self) -> &str {
        match &self.root {
            BorrowRoot::Local { name, .. } => name,
            BorrowRoot::Parameter { .. } => "reference parameter",
        }
    }

    fn with_projections(mut self, projections: Vec<BorrowProjection>) -> Self {
        self.projections = projections;
        self
    }

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
                // An unknown index or projection, or different projection
                // kinds, may refer to the same memory. Remain conservative.
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
    loans: Vec<Vec<ActiveLoan>>,
    next_loan_id: usize,
    current_callable_id: usize,
    next_callable_id: usize,
    reference_origins: HashMap<(usize, String), ReferenceProvenance>,
    expression_reference_origins: HashMap<(Option<usize>, usize, usize), ReferenceProvenance>,
    loop_scopes: Vec<std::collections::HashSet<String>>,
    loop_contexts: Vec<LoopContext>,
    current_return_type: ValueType,
    return_lifetime: Option<String>,
    return_parameter_indices: Vec<usize>,
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
    lambda_capture_scopes: Vec<Vec<LambdaCapture>>,
    lambda_capture_borrow_snapshots: Vec<Vec<Vec<ActiveLoan>>>,
    lambda_captures: HashMap<LambdaKey, Vec<LambdaCapture>>,
    current_function_owner: String,
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
        loans: Vec::new(),
        next_loan_id: 0,
        current_callable_id: 0,
        next_callable_id: 0,
        reference_origins: HashMap::new(),
        expression_reference_origins: HashMap::new(),
        loop_scopes: Vec::new(),
        loop_contexts: Vec::new(),
        current_return_type: ValueType::Unit,
        return_lifetime: None,
        return_parameter_indices: Vec::new(),
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
        lambda_capture_borrow_snapshots: Vec::new(),
        lambda_captures: HashMap::new(),
        current_function_owner: String::new(),
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

mod borrows;
mod declarations;
mod expressions;
mod matching;
mod places;
mod statements;
mod types;

use types::{assignment_root, at, span_of_expr};

#[cfg(test)]
mod pointer_mutability_tests {
    use super::*;

    #[test]
    fn vector_element_store_through_const_pointer_is_rejected() {
        let source = "struct H { v: Vec<i64> } fn main() -> i64 { let v: Vec<i64> = vec_new(); vec_push(v, 1); let h = H { v: v }; let p: *const H = &raw const h; (*p).v[0] = 2; 0 }";
        let tokens = crate::lexer::lex(source).expect("source lexes");
        let (mut program, diagnostics) = crate::parser::Parser::new(tokens).parse_program();
        assert!(diagnostics.is_empty(), "parse diagnostics: {diagnostics:?}");
        let error = check(&mut program).expect_err("sema must reject the store");
        assert_eq!(error.message, "cannot assign through an immutable pointer");
    }
}

#[cfg(test)]
mod borrow_place_tests {
    use super::{BorrowPlace, BorrowProjection};

    fn place(name: &str, projections: Vec<BorrowProjection>) -> BorrowPlace {
        BorrowPlace::local(0, name).with_projections(projections)
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
        let inner = BorrowPlace::local(1, "value");
        assert!(!outer.overlaps(&inner));
    }
}
