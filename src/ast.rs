#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    Tuple(Vec<Type>),
    Array(Box<Type>, u32),
    Vector(Box<Type>),
    Named(String),
    Applied(String, Vec<Type>),
    Parameter(String),
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
    Array(usize),
    Vector(usize),
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
    pub functions: Vec<Function>,
    pub modules: Vec<ModuleDecl>,
    pub source_files: Vec<String>,
    pub source_texts: Vec<String>,
    pub functions_are_qualified: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub generic_params: Vec<String>,
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
    pub generic_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Block,
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
    Loop {
        body: Box<Block>,
        span: Span,
    },
    Variant {
        enum_name: String,
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
        rest: bool,
    },
    Variant {
        enum_name: String,
        variant_name: String,
        bindings: Option<Vec<MatchPattern>>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Negate,
    Not,
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
