#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    Tuple(Vec<Type>),
    Function(Vec<Type>, Box<Type>),
    Array(Box<Type>, u32),
    Vector(Box<Type>),
    Reference(Box<Type>, Option<String>),
    MutableReference(Box<Type>, Option<String>),
    RawPointer(Box<Type>, bool),
    Named(String),
    Applied(String, Vec<Type>),
    Parameter(String),
    SelfType,
}

#[derive(Debug, Clone)]
pub struct TraitRef {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<TraitRef>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrayElementType {
    I64,
    F32,
    F64,
    Bool,
    Str,
    Enum(usize),
    Struct(usize),
    Tuple(usize),
    Function(usize),
    Array(usize),
    Vector(usize),
    RawPointer(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub file: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub enums: Vec<EnumDecl>,
    pub structs: Vec<StructDecl>,
    pub globals: Vec<GlobalDecl>,
    pub functions: Vec<Function>,
    pub extern_functions: Vec<ExternFunction>,
    pub traits: Vec<TraitDecl>,
    pub impls: Vec<ImplDecl>,
    pub modules: Vec<ModuleDecl>,
    pub uses: Vec<UseDecl>,
    pub source_files: Vec<String>,
    pub source_texts: Vec<String>,
    pub functions_are_qualified: bool,
    /// Local bindings copied into each lambda's heap-backed closure environment.
    pub lambda_captures: std::collections::HashMap<LambdaKey, Vec<LambdaCapture>>,
}

/// Identifies a lambda by the concrete function that contains it and its source
/// location. Generic specializations and inherited trait bodies can share a source
/// span while producing different concrete closures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LambdaKey {
    pub owner: String,
    pub file: Option<usize>,
    pub line: usize,
    pub column: usize,
}

impl LambdaKey {
    pub fn new(owner: &str, span: Span) -> Self {
        Self {
            owner: owner.to_owned(),
            file: span.file,
            line: span.line,
            column: span.column,
        }
    }
}

/// One value copied into a closure environment when the lambda is evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaCapture {
    pub name: String,
    /// Lexical scope that owns the source binding, used to validate the snapshot.
    pub scope_index: usize,
    /// `const` bindings and loop variables cannot be assigned through the slot.
    pub immutable: bool,
    pub loop_variable: bool,
}

#[derive(Debug, Clone)]
pub struct GlobalDecl {
    pub name: String,
    pub ty: Option<Type>,
    pub value: Expr,
    pub mutable: bool,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub name: String,
    pub span: Span,
}

/// A module-level import: `use path::name [as alias];`.
#[derive(Debug, Clone)]
pub struct UseDecl {
    /// Root-relative path of the imported item or module.
    pub path: Vec<String>,
    /// Local binding chosen by the importer, defaulting to the last path segment.
    pub alias: Option<String>,
    pub span: Span,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub functions: Vec<TraitFunction>,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct TraitFunction {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    /// Default body inherited by implementations that omit this function.
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    pub trait_name: String,
    pub target: Type,
    pub generic_params: Vec<GenericParam>,
    pub functions: Vec<Function>,
    pub span: Span,
    pub trait_span: Span,
    pub target_span: Span,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub fields: Vec<StructField>,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub lifetime_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Block,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct ExternFunction {
    /// Vaca-visible name, qualified during module name resolution.
    pub name: String,
    /// C linker symbol, retained as written in the source declaration.
    pub link_name: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub span: Span,
    pub is_public: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub tail: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        pattern: MatchPattern,
        ty: Option<Type>,
        value: Expr,
        immutable: bool,
        span: Span,
    },
    Assign {
        name: String,
        value: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    For {
        name: String,
        start: Expr,
        end: Expr,
        body: Block,
    },
    ForArray {
        name: String,
        array: Expr,
        body: Block,
        span: Span,
    },
    Break {
        value: Option<Expr>,
        span: Span,
    },
    Continue {
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    AssignPlace {
        target: Expr,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Integer(u64, Span),
    Float32(f32, Span),
    Float(f64, Span),
    Bool(bool, Span),
    String(String, Span),
    Unit(Span),
    Tuple(Vec<Expr>, Span),
    Array(Vec<Expr>, Span),
    Variable(String, Span),
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Field {
        value: Box<Expr>,
        name: String,
        span: Span,
    },
    Struct {
        name: String,
        type_arguments: Vec<Type>,
        fields: Vec<(String, Expr, Span)>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        name: String,
        type_arguments: Vec<Type>,
        args: Vec<Expr>,
        span: Span,
    },
    TraitCall {
        trait_name: String,
        self_type: Option<Type>,
        function_name: String,
        args: Vec<Expr>,
        span: Span,
    },
    CallValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Lambda {
        params: Vec<Param>,
        return_type: Type,
        body: Box<Block>,
        span: Span,
    },
    If {
        condition: Box<Expr>,
        then_block: Box<Block>,
        else_block: Box<Block>,
        span: Span,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Try {
        value: Box<Expr>,
        span: Span,
    },
    Loop {
        body: Box<Block>,
        span: Span,
    },
    Variant {
        enum_name: String,
        type_arguments: Vec<Type>,
        variant_name: String,
        payload: Option<Vec<Expr>>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub span: Span,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Wildcard,
    Binding(String),
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Tuple(Vec<MatchPattern>),
    Array(Vec<MatchPattern>),
    Struct {
        name: String,
        fields: Vec<(String, MatchPattern, Span)>,
        /// Parallel to `fields`: true when the field was written without a
        /// `: pattern` and so is always a binding, even in a match arm.
        shorthand: Vec<bool>,
        rest: bool,
    },
    Variant {
        enum_name: String,
        variant_name: String,
        bindings: Option<Vec<MatchPattern>>,
    },
    /// A payload pattern written with a single name, e.g. `Circle(radius)`,
    /// where the name may be a directly imported enum variant. Module
    /// resolution replaces it with `Variant` or reports a diagnostic.
    ImportedVariant {
        name: String,
        bindings: Vec<MatchPattern>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Negate,
    Not,
    Borrow,
    BorrowMut,
    RawBorrowConst,
    RawBorrowMut,
    Dereference,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    BitAnd,
    BitXor,
    BitOr,
    ShiftLeft,
    ShiftRight,
    And,
    Or,
}
