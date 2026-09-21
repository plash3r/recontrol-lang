use crate::lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program { pub items: Vec<Item> }

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(Function),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(ImplBlock),
    Import(Import),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Function(value) => value.span,
            Item::Struct(value) => value.span,
            Item::Enum(value) => value.span,
            Item::Impl(value) => value.span,
            Item::Import(value) => value.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: String,
    pub alias: Option<String>,
    pub target_source_id: Option<usize>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub type_name: String,
    pub methods: Vec<Function>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub public: bool,
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub public: bool,
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub public: bool,
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub public: bool,
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub enum_name: String,
    pub variant: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    pub fn new(kind: StmtKind, span: Span) -> Self { Self { kind, span } }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let { name: String, mutable: bool, ty: Option<TypeRef>, initializer: Option<Expr> },
    Expr(Expr),
    Return(Option<Expr>),
    If { condition: Expr, then_branch: Block, else_branch: Option<Box<Stmt>> },
    While { condition: Expr, body: Block },
    DoWhile { body: Block, condition: Expr },
    For { initializer: Option<Box<Stmt>>, condition: Option<Expr>, update: Option<Expr>, body: Block },
    Match { value: Expr, arms: Vec<MatchArm> },
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self { Self { kind, span } }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    Identifier(String),
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    Assignment { target: Box<Expr>, op: AssignOp, value: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Member { object: Box<Expr>, name: String },
    Postfix { expr: Box<Expr>, op: PostfixOp },
    Grouping(Box<Expr>),
    StructLiteral { name: String, fields: Vec<(String, Expr)> },
    Array(Vec<Expr>),
    Index { object: Box<Expr>, index: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal { Number(String), String(String), Bool(bool) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp { Plus, Minus, Not, BorrowShared, BorrowMutable }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Subtract, Multiply, Divide, Modulo,
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual,
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOp { Increment, Decrement }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp { Assign, Add, Subtract, Multiply, Divide, Modulo }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: String,
    pub reference: ReferenceKind,
    pub array_len: Option<usize>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind { Value, Shared, Mutable }
