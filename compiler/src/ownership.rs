use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
    return_type: Type,
}

#[derive(Debug, Clone)]
struct VarState {
    ty: Type,
    moved: bool,
}

#[derive(Debug, Default)]
struct Env {
    scopes: Vec<HashMap<String, VarState>>,
}

impl Env {
    fn push(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop(&mut self) { self.scopes.pop(); }

    fn define(&mut self, name: String, ty: Type) {
        self.scopes.last_mut().unwrap().insert(name, VarState { ty, moved: false });
    }

    fn get(&self, name: &str) -> Option<&VarState> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut VarState> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }
}

pub struct OwnershipChecker {
    functions: HashMap<String, FunctionSig>,
    methods: HashMap<(String, String), FunctionSig>,
    errors: Vec<OwnershipError>,
}

impl OwnershipChecker {
    pub fn check(program: &Program) -> Result<(), Vec<OwnershipError>> {
        let mut checker = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
        };
        checker.collect(program);

        for item in &program.items {
            match item {
                Item::Function(f) => checker.check_function(f, None),
                Item::Impl(i) => for f in &i.methods {
                    checker.check_function(f, Some(&i.type_name));
                },
                Item::Struct(_) => {}
                Item::Import(_) => {}
            }
        }

        if checker.errors.is_empty() { Ok(()) } else { Err(checker.errors) }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(OwnershipError {
            message: message.into(),
            span: Span { line: 1, column: 1, length: 0 },
        });
    }

    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    self.functions.insert(f.name.clone(), FunctionSig {
                        params: f.params.iter().map(|p| Type::from_ref(&p.ty)).collect(),
                        return_type: f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                    });
                }
                Item::Impl(i) => for f in &i.methods {
                    self.methods.insert(
                        (i.type_name.clone(), f.name.clone()),
                        FunctionSig {
                            params: f.params.iter().map(|p| {
                                if p.name == "self" {
                                    Type::Reference {
                                        mutable: p.ty.reference == ReferenceKind::Mutable,
                                        inner: Box::new(Type::Named(i.type_name.clone())),
                                    }
                                } else {
                                    Type::from_ref(&p.ty)
                                }
                            }).collect(),
                            return_type: f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                        },
                    );
                },
                Item::Struct(_) => {}
                Item::Import(_) => {}
            }
        }
    }

    fn check_function(&mut self, f: &Function, impl_type: Option<&str>) {
        let sig = if let Some(t) = impl_type {
            self.methods.get(&(t.to_string(), f.name.clone())).cloned()
        } else {
            self.functions.get(&f.name).cloned()
        };
        let Some(sig) = sig else { return };

        let mut env = Env::default();
        env.push();
        for (i, p) in f.params.iter().enumerate() {
            env.define(p.name.clone(), sig.params[i].clone());
        }
        self.check_block(&f.body, &mut env);
        env.pop();
    }

    fn check_block(&mut self, block: &Block, env: &mut Env) {
        env.push();
        for stmt in &block.statements {
            self.check_stmt(stmt, env);
        }
        env.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt, env: &mut Env) {
        match stmt {
            Stmt::Let { name, initializer, ty, .. } => {
                if let Some(expr) = initializer {
                    let value_ty = self.expr(expr, env, true);
                    let final_ty = ty.as_ref().map(Type::from_ref).unwrap_or(value_ty);
                    env.define(name.clone(), final_ty);
                } else {
                    env.define(name.clone(), ty.as_ref().map(Type::from_ref).unwrap_or(Type::Unknown));
                }
            }
            Stmt::Expr(expr) => { self.expr(expr, env, false); }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    self.expr(expr, env, true);
                }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.expr(condition, env, false);
                self.check_block(then_branch, env);
                if let Some(branch) = else_branch { self.check_stmt(branch, env); }
            }
            Stmt::While { condition, body } => {
                self.expr(condition, env, false);
                self.check_block(body, env);
            }
            Stmt::DoWhile { body, condition } => {
                self.check_block(body, env);
                self.expr(condition, env, false);
            }
            Stmt::For { initializer, condition, update, body } => {
                env.push();
                if let Some(x) = initializer { self.check_stmt(x, env); }
                if let Some(x) = condition { self.expr(x, env, false); }
                if let Some(x) = update { self.expr(x, env, false); }
                self.check_block(body, env);
                env.pop();
            }
            Stmt::Block(block) => self.check_block(block, env),
        }
    }

    fn expr(&mut self, expr: &Expr, env: &mut Env, consume: bool) -> Type {
        match expr {
            Expr::Literal(Literal::Bool(_)) => Type::Bool,
            Expr::Literal(Literal::String(_)) => Type::Str,
            Expr::Literal(Literal::Number(n)) => self.number_type(n),
            Expr::Identifier(name) => {
                let ty = match env.get(name) {
                    Some(state) => state.ty.clone(),
                    None => {
                        if self.functions.contains_key(name) {
                            return Type::Named(format!("fn {}", name));
                        }
                        self.error(format!("unknown identifier '{}'", name));
                        return Type::Unknown;
                    }
                };
                self.ensure_available(name, env);
                if consume && !ty.is_copy() && !matches!(ty, Type::Reference { .. }) {
                    self.move_value(name, env);
                }
                ty
            }
            Expr::Unary { op, expr } => {
                match op {
                    UnaryOp::BorrowShared | UnaryOp::BorrowMutable => {
                        self.expr(expr, env, false);
                        let inner = self.expr_type(expr, env);
                        Type::Reference { mutable: *op == UnaryOp::BorrowMutable, inner: Box::new(inner) }
                    }
                    _ => self.expr(expr, env, false),
                }
            }
            Expr::Grouping(inner) => self.expr(inner, env, consume),
            Expr::Binary { left, right, .. } => {
                self.expr(left, env, false);
                self.expr(right, env, false);
                self.expr_type(left, env)
            }
            Expr::Assignment { target, value, .. } => {
                if let Some(name) = self.root_identifier(target) {
                    self.ensure_available(&name, env);
                }
                let value_ty = self.expr(value, env, true);
                self.expr(target, env, false);
                if let Some(name) = self.root_identifier(target) {
                    if let Some(state) = env.get_mut(&name) {
                        state.moved = false;
                    }
                }
                value_ty
            }
            Expr::Call { callee, args } => self.call(callee, args, env),
            Expr::Member { object, .. } => self.expr(object, env, false),
            Expr::Postfix { expr, .. } => self.expr(expr, env, false),
            Expr::StructLiteral { fields, .. } => {
                for (_, value) in fields { self.expr(value, env, true); }
                Type::Named(match expr {
                    Expr::StructLiteral { name, .. } => name.clone(),
                    _ => String::new(),
                })
            }
            Expr::Array(values) => {
                for value in values { self.expr(value, env, true); }
                if values.is_empty() {
                    Type::Array(Box::new(Type::Unknown))
                } else {
                    Type::Array(Box::new(self.expr_type(&values[0], env)))
                }
            }
            Expr::Index { object, index } => {
                self.expr(object, env, false);
                self.expr(index, env, false);
                match self.expr_type(object, env) {
                    Type::Array(inner) => *inner,
                    Type::Str => Type::Char,
                    _ => Type::Unknown,
                }
            }
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], env: &mut Env) -> Type {
        if let Expr::Identifier(name) = callee {
            if name == "print" || name == "println" || name == "typeof" || name == "len" {
                for arg in args { self.expr(arg, env, false); }
                return Type::Unit;
            }
            if let Some(sig) = self.functions.get(name).cloned() {
                return self.check_signature(&sig, args, env);
            }
        }

        if let Expr::Member { object, name } = callee {
            self.expr(object, env, false);
            let type_name = self.expr_type(object, env).display_name();
            if let Some(sig) = self.methods.get(&(type_name, name.clone())).cloned() {
                let mut all = vec![object.as_ref().clone()];
                all.extend_from_slice(args);
                return self.check_signature(&sig, &all, env);
            }
        }

        for arg in args { self.expr(arg, env, false); }
        Type::Unknown
    }

    fn check_signature(&mut self, sig: &FunctionSig, args: &[Expr], env: &mut Env) -> Type {
        for (i, arg) in args.iter().enumerate() {
            if let Some(expected) = sig.params.get(i) {
                let by_ref = matches!(expected, Type::Reference { .. });
                self.expr(arg, env, !by_ref);
            } else {
                self.expr(arg, env, false);
            }
        }
        sig.return_type.clone()
    }

    fn expr_type(&self, expr: &Expr, env: &Env) -> Type {
        match expr {
            Expr::Identifier(n) => env.get(n).map(|s| s.ty.clone()).unwrap_or(Type::Unknown),
            Expr::Literal(Literal::Bool(_)) => Type::Bool,
            Expr::Literal(Literal::String(_)) => Type::Str,
            Expr::Literal(Literal::Number(n)) => self.number_type(n),
            Expr::Grouping(x) => self.expr_type(x, env),
            Expr::Unary { op, expr } => match op {
                UnaryOp::BorrowShared => Type::Reference { mutable: false, inner: Box::new(self.expr_type(expr, env)) },
                UnaryOp::BorrowMutable => Type::Reference { mutable: true, inner: Box::new(self.expr_type(expr, env)) },
                _ => self.expr_type(expr, env),
            },
            Expr::Array(xs) => xs.first().map(|x| Type::Array(Box::new(self.expr_type(x, env)))).unwrap_or(Type::Array(Box::new(Type::Unknown))),
            Expr::Index { object, .. } => match self.expr_type(object, env) {
                Type::Array(inner) => *inner,
                Type::Str => Type::Char,
                _ => Type::Unknown,
            },
            Expr::StructLiteral { name, .. } => Type::Named(name.clone()),
            _ => Type::Unknown,
        }
    }

    fn ensure_available(&mut self, name: &str, env: &Env) {
        if let Some(state) = env.get(name) {
            if state.moved {
                self.error(format!("use of moved value '{}'", name));
            }
        }
    }

    fn move_value(&mut self, name: &str, env: &mut Env) {
        if let Some(state) = env.get_mut(name) {
            state.moved = true;
        }
    }

    fn root_identifier(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Identifier(n) => Some(n.clone()),
            Expr::Member { object, .. } | Expr::Index { object, .. } => self.root_identifier(object),
            Expr::Grouping(inner) => self.root_identifier(inner),
            _ => None,
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

    fn check(source: &str) -> Result<(), Vec<OwnershipError>> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        OwnershipChecker::check(&program)
    }

    #[test]
    fn moved_value_is_rejected() {
        let source = r#"fn main() {
            let a = "hello"
            let b = a
            println(a)
        }"#;
        assert!(check(source).is_err());
    }

    #[test]
    fn copy_values_are_not_moved() {
        let source = r#"fn main() {
            let a = 10
            let b = a
            println(a)
        }"#;
        assert!(check(source).is_ok());
    }

    #[test]
    fn function_argument_moves_value() {
        let source = r#"fn take(value: str) {}
        fn main() {
            let text = "hello"
            take(text)
            println(text)
        }"#;
        assert!(check(source).is_err());
    }

    #[test]
    fn reference_argument_does_not_move() {
        let source = r#"fn read(value: &str) {}
        fn main() {
            let text = "hello"
            read(&text)
            println(text)
        }"#;
        assert!(check(source).is_ok());
    }

    #[test]
    fn move_then_reassignment_is_allowed() {
        let source = r#"fn main() {
            let mut a = "hello"
            let b = a
            a = "world"
            println(a)
        }"#;
        assert!(check(source).is_ok());
    }
}
