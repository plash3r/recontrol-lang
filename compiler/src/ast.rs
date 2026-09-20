#[derive(Debug, Clone, PartialEq)]
pub struct Program { pub items: Vec<Item> }

#[derive(Debug, Clone, PartialEq)]
pub enum Item { Function(Function), Struct(StructDef), Impl(ImplBlock) }

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock { pub type_name: String, pub methods: Vec<Function> }

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter { pub name: String, pub ty: TypeRef }

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef { pub name: String, pub fields: Vec<Field> }

#[derive(Debug, Clone, PartialEq)]
pub struct Field { pub name: String, pub ty: TypeRef }

#[derive(Debug, Clone, PartialEq)]
pub struct Block { pub statements: Vec<Stmt> }

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, mutable: bool, ty: Option<TypeRef>, initializer: Option<Expr> },
    Expr(Expr),
    Return(Option<Expr>),
    If { condition: Expr, then_branch: Block, else_branch: Option<Box<Stmt>> },
    While { condition: Expr, body: Block },
    DoWhile { body: Block, condition: Expr },
    For { initializer: Option<Box<Stmt>>, condition: Option<Expr>, update: Option<Expr>, body: Block },
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
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
pub enum UnaryOp { Plus, Minus, Not }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Subtract, Multiply, Divide, Modulo,
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual,
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOp { Increment, Decrement }\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum AssignOp { Assign, Add, Subtract, Multiply, Divide, Modulo }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef { pub name: String, pub reference: ReferenceKind }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind { Value, Shared, Mutable }
