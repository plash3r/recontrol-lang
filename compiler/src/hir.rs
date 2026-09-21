use crate::ast::{self, AssignOp, BinaryOp, Literal, PostfixOp, UnaryOp};
use crate::types::Type;
use std::collections::HashMap;

pub type FunctionId = usize;
pub type LocalId = usize;
pub type ExprId = usize;

// Reserved IDs are outside the range of real user functions.
pub const BUILTIN_PRINTLN_ID: FunctionId = usize::MAX;
pub const BUILTIN_PRINT_ID: FunctionId = usize::MAX - 1;
pub const BUILTIN_TYPEOF_ID: FunctionId = usize::MAX - 2;
pub const BUILTIN_LEN_ID: FunctionId = usize::MAX - 3;

#[derive(Debug, Clone, PartialEq)]
pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirStruct {
    pub name: String,
    pub fields: Vec<HirField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirField {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<HirParam>,
    pub return_type: Type,
    pub body: HirBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirParam {
    pub local: LocalId,
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirLocal {
    pub id: LocalId,
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirBlock {
    pub locals: Vec<HirLocal>,
    pub statements: Vec<HirStmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Let { local: LocalId, initializer: Option<HirExpr> },
    Expr(HirExpr),
    Return(Option<HirExpr>),
    If { condition: HirExpr, then_branch: HirBlock, else_branch: Option<Box<HirStmt>> },
    While { condition: HirExpr, body: HirBlock },
    DoWhile { body: HirBlock, condition: HirExpr },
    For { initializer: Option<Box<HirStmt>>, condition: Option<HirExpr>, update: Option<HirExpr>, body: HirBlock },
    Block(HirBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirExpr {
    pub ty: Type,
    pub kind: HirExprKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExprKind {
    Literal(Literal),
    Local(LocalId),
    Unary { op: UnaryOp, expr: Box<HirExpr> },
    Binary { left: Box<HirExpr>, op: BinaryOp, right: Box<HirExpr> },
    Assignment { target: Box<HirExpr>, op: AssignOp, value: Box<HirExpr> },
    Call { callee: Box<HirExpr>, args: Vec<HirExpr> },
    Member { object: Box<HirExpr>, name: String },
    Postfix { expr: Box<HirExpr>, op: PostfixOp },
    StructLiteral { name: String, fields: Vec<(String, HirExpr)> },
    Array(Vec<HirExpr>),
    Index { object: Box<HirExpr>, index: Box<HirExpr> },
    Function(FunctionId),
}

#[derive(Debug)]
pub struct HirLowerer {
    next_function: FunctionId,
    next_local: LocalId,
    functions: Vec<HirFunction>,
    structs: Vec<HirStruct>,
    scopes: Vec<HashMap<String, LocalId>>,
    local_types: HashMap<LocalId, Type>,
    function_ids: HashMap<String, FunctionId>,
    function_returns: HashMap<FunctionId, Type>,
    method_ids: HashMap<(String, String), FunctionId>,
}

impl HirLowerer {
    pub fn lower(program: &ast::Program) -> HirProgram {
        let mut lowerer = Self {
            next_function: 0,
            next_local: 0,
            functions: Vec::new(),
            structs: Vec::new(),
            scopes: Vec::new(),
            local_types: HashMap::new(),
            function_ids: HashMap::new(),
            function_returns: HashMap::new(),
            method_ids: HashMap::new(),
        };

        for item in &program.items {
            match item {
                ast::Item::Function(f) => {
                    let id = lowerer.next_function;
                    lowerer.function_ids.insert(f.name.clone(), id);
                    lowerer.function_returns.insert(id, f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit));
                    lowerer.next_function += 1;
                }
                ast::Item::Impl(i) => {
                    for f in &i.methods {
                        let id = lowerer.next_function;
                        lowerer.function_ids.insert(f.name.clone(), id);
                        lowerer.function_returns.insert(id, f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit));
                        lowerer.method_ids.insert((i.type_name.clone(), f.name.clone()), id);
                        lowerer.next_function += 1;
                    }
                }
                _ => {}
            }
        }
        lowerer.next_function = 0;

        for item in &program.items {
            match item {
                ast::Item::Struct(s) => lowerer.structs.push(HirStruct {
                    name: s.name.clone(),
                    fields: s.fields.iter().map(|f| HirField {
                        name: f.name.clone(),
                        ty: Type::from_ref(&f.ty),
                    }).collect(),
                }),
                ast::Item::Function(f) => lowerer.lower_function(f, None),
                ast::Item::Impl(i) => {
                    for f in &i.methods {
                        lowerer.lower_function(f, Some(&i.type_name));
                    }
                }
                ast::Item::Import(_) => {}
            }
        }

        HirProgram {
            functions: lowerer.functions,
            structs: lowerer.structs,
        }
    }

    fn lower_function(&mut self, function: &ast::Function, impl_type: Option<&str>) {
        let id = self.next_function;
        self.next_function += 1;
        let mut locals = Vec::new();
        let mut params = Vec::new();
        self.scopes.push(HashMap::new());

        for parameter in &function.params {
            let local = self.new_local();
            let ty = if parameter.name == "self" {
                Type::Reference {
                    mutable: parameter.ty.reference == ast::ReferenceKind::Mutable,
                    inner: Box::new(Type::Named(impl_type.unwrap_or("Self").to_string())),
                }
            } else {
                Type::from_ref(&parameter.ty)
            };
            self.scopes.last_mut().unwrap().insert(parameter.name.clone(), local);
            self.local_types.insert(local, ty.clone());
            locals.push(HirLocal {
                id: local,
                name: parameter.name.clone(),
                ty: ty.clone(),
                mutable: matches!(ty, Type::Reference { mutable: true, .. }),
            });
            params.push(HirParam {
                local,
                name: parameter.name.clone(),
                ty,
            });
        }

        let mut body = self.lower_block(&function.body);
        body.locals.splice(0..0, locals);

        self.scopes.pop();
        self.functions.push(HirFunction {
            id,
            name: function.name.clone(),
            params,
            return_type: function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
            body,
        });
    }

    fn new_local(&mut self) -> LocalId {
        let id = self.next_local;
        self.next_local += 1;
        id
    }

    fn lower_block(&mut self, block: &ast::Block) -> HirBlock {
        self.scopes.push(HashMap::new());
        let mut result = HirBlock { locals: Vec::new(), statements: Vec::new() };
        for statement in &block.statements {
            result.statements.push(self.lower_stmt(statement, &mut result.locals));
        }
        self.scopes.pop();
        result
    }

    fn lower_stmt(&mut self, statement: &ast::Stmt, locals: &mut Vec<HirLocal>) -> HirStmt {
        match &statement.kind {
            ast::StmtKind::Let { name, mutable, ty, initializer } => {
                let local = self.new_local();
                let initializer_hir = initializer.as_ref().map(|e| self.lower_expr(e));
                let inferred = initializer_hir.as_ref().map(|e| e.ty.clone());
                let local_ty = ty.as_ref().map(Type::from_ref).or(inferred).unwrap_or(Type::Unknown);
                locals.push(HirLocal { id: local, name: name.clone(), ty: local_ty, mutable: *mutable });
                self.local_types.insert(local, locals.last().unwrap().ty.clone());
                self.scopes.last_mut().unwrap().insert(name.clone(), local);
                HirStmt::Let {
                    local,
                    initializer: initializer_hir,
                }
            }
            ast::StmtKind::Expr(e) => HirStmt::Expr(self.lower_expr(e)),
            ast::StmtKind::Return(e) => HirStmt::Return(e.as_ref().map(|e| self.lower_expr(e))),
            ast::StmtKind::If { condition, then_branch, else_branch } => HirStmt::If {
                condition: self.lower_expr(condition),
                then_branch: self.lower_block(then_branch),
                else_branch: else_branch.as_ref().map(|s| Box::new(self.lower_stmt(s, &mut Vec::new()))),
            },
            ast::StmtKind::While { condition, body } => HirStmt::While {
                condition: self.lower_expr(condition),
                body: self.lower_block(body),
            },
            ast::StmtKind::DoWhile { body, condition } => HirStmt::DoWhile {
                body: self.lower_block(body),
                condition: self.lower_expr(condition),
            },
            ast::StmtKind::For { initializer, condition, update, body } => HirStmt::For {
                initializer: initializer.as_ref().map(|s| Box::new(self.lower_stmt(s, locals))),
                condition: condition.as_ref().map(|e| self.lower_expr(e)),
                update: update.as_ref().map(|e| self.lower_expr(e)),
                body: self.lower_block(body),
            },
            ast::StmtKind::Block(b) => HirStmt::Block(self.lower_block(b)),
        }
    }

    fn lower_expr(&mut self, expr: &ast::Expr) -> HirExpr {
        match &expr.kind {
            ast::ExprKind::Literal(lit) => HirExpr { ty: self.literal_type(lit), kind: HirExprKind::Literal(lit.clone()) },
            ast::ExprKind::Identifier(name) => {
                if name == "println" {
                    HirExpr { ty: Type::Unit, kind: HirExprKind::Function(BUILTIN_PRINTLN_ID) }
                } else if name == "print" {
                    HirExpr { ty: Type::Unit, kind: HirExprKind::Function(BUILTIN_PRINT_ID) }
                } else if name == "typeof" {
                    HirExpr { ty: Type::Str, kind: HirExprKind::Function(BUILTIN_TYPEOF_ID) }
                } else if name == "len" {
                    HirExpr { ty: Type::I32, kind: HirExprKind::Function(BUILTIN_LEN_ID) }
                } else if let Some(id) = self.function_ids.get(name).copied() {
                    HirExpr { ty: Type::Unknown, kind: HirExprKind::Function(id) }
                } else {
                    let local = self.local_id(name);
                    let ty = self.local_types.get(&local).cloned().unwrap_or(Type::Unknown);
                    HirExpr { ty, kind: HirExprKind::Local(local) }
                }
            },
            ast::ExprKind::Unary { op, expr } => {
                let inner = self.lower_expr(expr);
                let ty = match op {
                    UnaryOp::BorrowShared => Type::Reference { mutable: false, inner: Box::new(inner.ty.clone()) },
                    UnaryOp::BorrowMutable => Type::Reference { mutable: true, inner: Box::new(inner.ty.clone()) },
                    _ => inner.ty.clone(),
                };
                HirExpr { ty, kind: HirExprKind::Unary { op: *op, expr: Box::new(inner) } }
            }
            ast::ExprKind::Binary { left, op, right } => {
                let l = self.lower_expr(left);
                let r = self.lower_expr(right);
                let ty = match op {
                    BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessEqual |
                    BinaryOp::Greater | BinaryOp::GreaterEqual | BinaryOp::And | BinaryOp::Or => Type::Bool,
                    _ => l.ty.clone(),
                };
                HirExpr { ty, kind: HirExprKind::Binary { left: Box::new(l), op: *op, right: Box::new(r) } }
            }
            ast::ExprKind::Assignment { target, op, value } => {
                let target = self.lower_expr(target);
                let value = self.lower_expr(value);
                HirExpr { ty: target.ty.clone(), kind: HirExprKind::Assignment { target: Box::new(target), op: *op, value: Box::new(value) } }
            }
            ast::ExprKind::Call { callee, args } => {
                let (callee, args) = if let ast::ExprKind::Member { object, name } = &callee.kind {
                    let object = self.lower_expr(object);
                    if let Type::Named(type_name) = &object.ty {
                        if let Some(id) = self.method_ids.get(&(type_name.clone(), name.clone())).copied() {
                            let self_ref = HirExpr {
                                ty: Type::Reference { mutable: false, inner: Box::new(object.ty.clone()) },
                                kind: HirExprKind::Unary { op: UnaryOp::BorrowShared, expr: Box::new(object) },
                            };
                            let mut method_args = vec![self_ref];
                            method_args.extend(args.iter().map(|a| self.lower_expr(a)));
                            (HirExpr { ty: Type::Unknown, kind: HirExprKind::Function(id) }, method_args)
                        } else {
                            (self.lower_expr(callee), args.iter().map(|a| self.lower_expr(a)).collect())
                        }
                    } else {
                        (self.lower_expr(callee), args.iter().map(|a| self.lower_expr(a)).collect())
                    }
                } else {
                    (self.lower_expr(callee), args.iter().map(|a| self.lower_expr(a)).collect())
                };
                let ty = match &callee.kind {
                    HirExprKind::Function(id) if *id == BUILTIN_TYPEOF_ID => Type::Str,
                    HirExprKind::Function(id) if *id == BUILTIN_LEN_ID => Type::I32,
                    HirExprKind::Function(id) => self.function_returns.get(id).cloned().unwrap_or(Type::Unit),
                    _ => Type::Unknown,
                };
                HirExpr { ty, kind: HirExprKind::Call { callee: Box::new(callee), args } }
            }
            ast::ExprKind::Member { object, name } => {
                let object = self.lower_expr(object);
                let ty = match &object.ty {
                    Type::Named(type_name) => self.structs.iter().find(|structure| &structure.name == type_name)
                        .and_then(|structure| structure.fields.iter().find(|field| field.name == *name).map(|field| field.ty.clone()))
                        .unwrap_or(Type::Unknown),
                    Type::Reference { inner, .. } => match inner.as_ref() {
                        Type::Named(type_name) => self.structs.iter().find(|structure| &structure.name == type_name)
                            .and_then(|structure| structure.fields.iter().find(|field| field.name == *name).map(|field| field.ty.clone()))
                            .unwrap_or(Type::Unknown),
                        _ => Type::Unknown,
                    },
                    _ => Type::Unknown,
                };
                HirExpr { ty, kind: HirExprKind::Member { object: Box::new(object), name: name.clone() } }
            }
            ast::ExprKind::Postfix { expr, op } => {
                let expr = self.lower_expr(expr);
                HirExpr { ty: expr.ty.clone(), kind: HirExprKind::Postfix { expr: Box::new(expr), op: *op } }
            }
            ast::ExprKind::Grouping(inner) => self.lower_expr(inner),
            ast::ExprKind::StructLiteral { name, fields } => HirExpr {
                ty: Type::Named(name.clone()),
                kind: HirExprKind::StructLiteral {
                    name: name.clone(),
                    fields: fields.iter().map(|(n, e)| (n.clone(), self.lower_expr(e))).collect(),
                },
            },
            ast::ExprKind::Array(values) => {
                let values: Vec<_> = values.iter().map(|e| self.lower_expr(e)).collect();
                let ty = values.first().map(|e| Type::Array { element: Box::new(e.ty.clone()), len: values.len() }).unwrap_or(Type::Array { element: Box::new(Type::Unknown), len: 0 });
                HirExpr { ty, kind: HirExprKind::Array(values) }
            }
            ast::ExprKind::Index { object, index } => {
                let object = self.lower_expr(object);
                let index = self.lower_expr(index);
                let ty = match &object.ty {
                    Type::Array { element, .. } => (**element).clone(),
                    Type::Str => Type::Char,
                    _ => Type::Unknown,
                };
                HirExpr { ty, kind: HirExprKind::Index { object: Box::new(object), index: Box::new(index) } }
            }
        }
    }

    fn local_id(&self, name: &str) -> LocalId {
        self.scopes.iter().rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(usize::MAX)
    }

    fn literal_type(&self, literal: &Literal) -> Type {
        match literal {
            Literal::Bool(_) => Type::Bool,
            Literal::String(_) => Type::Str,
            Literal::Number(n) => self.number_type(n),
        }
    }

    fn number_type(&self, n: &str) -> Type {
        let l = n.to_ascii_lowercase();
        for s in ["u8","u16","u32","u64","u128","u256","i8","i16","i32","i64","i128","i256","f32","f64","f128"] {
            if l.ends_with(s) {
                return match s {
                    "u8"=>Type::U8,"u16"=>Type::U16,"u32"=>Type::U32,"u64"=>Type::U64,"u128"=>Type::U128,"u256"=>Type::U256,
                    "i8"=>Type::I8,"i16"=>Type::I16,"i32"=>Type::I32,"i64"=>Type::I64,"i128"=>Type::I128,"i256"=>Type::I256,
                    "f32"=>Type::F32,"f64"=>Type::F64,"f128"=>Type::F128,_=>Type::Unknown
                };
            }
        }
        if l.contains('.') { Type::F64 } else { Type::I32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser, sema::SemanticAnalyzer};

    #[test]
    fn lowers_basic_program() {
        let source = "fn main(){let x:i32=10 println(x)}";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        assert_eq!(hir.functions.len(), 1);
        assert_eq!(hir.functions[0].name, "main");
        assert_eq!(hir.functions[0].body.statements.len(), 2);
        assert_eq!(hir.functions[0].body.locals.len(), 1);
    }
}
