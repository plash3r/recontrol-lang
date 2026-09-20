use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
struct FunctionSig { params: Vec<Type> }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BorrowKind { Shared, Mutable }

#[derive(Debug, Clone)]
struct ActiveBorrow {
    kind: BorrowKind,
    scope: usize,
    holder: Option<String>,
}

#[derive(Debug, Default)]
struct Env {
    mutable: Vec<HashMap<String, bool>>,
    reference: Vec<HashMap<String, bool>>,
}

impl Env {
    fn push(&mut self) {
        self.mutable.push(HashMap::new());
        self.reference.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.mutable.pop();
        self.reference.pop();
    }

    fn define(&mut self, name: String, is_mut: bool, is_reference: bool) {
        self.mutable.last_mut().unwrap().insert(name.clone(), is_mut);
        self.reference.last_mut().unwrap().insert(name, is_reference);
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutable.iter().rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(false)
    }

    fn is_reference(&self, name: &str) -> bool {
        self.reference.iter().rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(false)
    }
}

pub struct BorrowChecker {
    functions: HashMap<String, FunctionSig>,
    methods: HashMap<(String, String), FunctionSig>,
    errors: Vec<BorrowError>,
    active: HashMap<String, Vec<ActiveBorrow>>,
    scope: usize,
}

impl BorrowChecker {
    pub fn check(program: &Program) -> Result<(), Vec<BorrowError>> {
        let mut checker = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
            active: HashMap::new(),
            scope: 0,
        };
        checker.collect(program);

        for item in &program.items {
            match item {
                Item::Function(function) => checker.check_function(function, None),
                Item::Impl(implementation) => {
                    for method in &implementation.methods {
                        checker.check_function(method, Some(&implementation.type_name));
                    }
                }
                Item::Struct(_) => {}
            }
        }

        if checker.errors.is_empty() { Ok(()) } else { Err(checker.errors) }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(BorrowError {
            message: message.into(),
            span: Span { line: 1, column: 1, length: 0 },
        });
    }

    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(function) => {
                    self.functions.insert(function.name.clone(), FunctionSig {
                        params: function.params.iter().map(|p| Type::from_ref(&p.ty)).collect(),
                    });
                }
                Item::Impl(implementation) => {
                    for method in &implementation.methods {
                        let params = method.params.iter().map(|p| {
                            if p.name == "self" {
                                Type::Reference {
                                    mutable: p.ty.reference == ReferenceKind::Mutable,
                                    inner: Box::new(Type::Named(implementation.type_name.clone())),
                                }
                            } else {
                                Type::from_ref(&p.ty)
                            }
                        }).collect();
                        self.methods.insert(
                            (implementation.type_name.clone(), method.name.clone()),
                            FunctionSig { params },
                        );
                    }
                }
                Item::Struct(_) => {}
            }
        }
    }

    fn check_function(&mut self, function: &Function, impl_type: Option<&str>) {
        let signature = if let Some(type_name) = impl_type {
            self.methods.get(&(type_name.to_string(), function.name.clone())).cloned()
        } else {
            self.functions.get(&function.name).cloned()
        };
        let Some(signature) = signature else { return };

        self.active.clear();
        self.scope = 0;

        let mut env = Env::default();
        env.push();
        self.scope = 1;

        for (index, parameter) in function.params.iter().enumerate() {
            let ty = &signature.params[index];
            env.define(
                parameter.name.clone(),
                matches!(ty, Type::Reference { mutable: true, .. }),
                matches!(ty, Type::Reference { .. }),
            );
        }

        self.check_block_body(&function.body, &mut env);
        env.pop();
    }

    fn enter_scope(&mut self, env: &mut Env) {
        env.push();
        self.scope += 1;
    }

    fn leave_scope(&mut self, env: &mut Env) {
        let depth = self.scope;
        for borrows in self.active.values_mut() {
            borrows.retain(|borrow| borrow.scope < depth);
        }
        self.active.retain(|_, borrows| !borrows.is_empty());
        env.pop();
        self.scope -= 1;
    }

    fn check_block_body(&mut self, block: &Block, env: &mut Env) {
        self.enter_scope(env);
        for statement in &block.statements {
            self.check_statement(statement, env);
        }
        self.leave_scope(env);
    }

    fn check_statement(&mut self, statement: &Stmt, env: &mut Env) {
        match statement {
            Stmt::Let { name, mutable, initializer, .. } => {
                if let Some(expression) = initializer {
                    if let Some((target, kind)) = self.borrow_expression(expression) {
                        self.create_borrow(target, kind, Some(name.clone()), env);
                        env.define(name.clone(), *mutable, true);
                        return;
                    }
                    self.check_expression(expression, env);
                }
                env.define(name.clone(), *mutable, false);
            }
            Stmt::Expr(expression) => self.check_expression(expression, env),
            Stmt::Return(expression) => {
                if let Some(expression) = expression { self.check_expression(expression, env); }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.check_expression(condition, env);
                self.check_block_body(then_branch, env);
                if let Some(branch) = else_branch { self.check_statement(branch, env); }
            }
            Stmt::While { condition, body } => {
                self.check_expression(condition, env);
                self.check_block_body(body, env);
            }
            Stmt::DoWhile { body, condition } => {
                self.check_block_body(body, env);
                self.check_expression(condition, env);
            }
            Stmt::For { initializer, condition, update, body } => {
                self.enter_scope(env);
                if let Some(initializer) = initializer { self.check_statement(initializer, env); }
                if let Some(condition) = condition { self.check_expression(condition, env); }
                if let Some(update) = update { self.check_expression(update, env); }
                self.check_block_body(body, env);
                self.leave_scope(env);
            }
            Stmt::Block(block) => self.check_block_body(block, env),
        }
    }

    fn check_expression(&mut self, expression: &Expr, env: &Env) {
        match expression {
            Expr::Identifier(name) => {
                self.check_read(name);
            }
            Expr::Assignment { target, value, .. } => {
                if let Some(name) = self.root_identifier(target) {
                    self.check_mutation(&name);
                }
                self.check_expression(value, env);
            }
            Expr::Call { callee, args } => self.check_call(callee, args, env),
            Expr::Member { object, .. } => self.check_expression(object, env),
            Expr::Postfix { expr, .. } => {
                if let Some(name) = self.root_identifier(expr) { self.check_mutation(&name); }
                self.check_expression(expr, env);
            }
            Expr::Unary { op, expr } => match op {
                UnaryOp::BorrowShared => {
                    if let Some(target) = self.root_identifier(expr) {
                        self.create_borrow(target, BorrowKind::Shared, None, env);
                    } else {
                        self.error("cannot borrow temporary expression");
                    }
                }
                UnaryOp::BorrowMutable => {
                    if let Some(target) = self.root_identifier(expr) {
                        if !self.is_mutable_target(&target, env) {
                            self.error(format!("cannot mutably borrow immutable variable '{}'", target));
                        }
                        self.create_borrow(target, BorrowKind::Mutable, None, env);
                    } else {
                        self.error("cannot mutably borrow temporary expression");
                    }
                }
                _ => self.check_expression(expr, env),
            },
            Expr::Binary { left, right, .. } => {
                self.check_expression(left, env);
                self.check_expression(right, env);
            }
            Expr::Grouping(inner) => self.check_expression(inner, env),
            Expr::StructLiteral { fields, .. } => {
                for (_, value) in fields { self.check_expression(value, env); }
            }
            Expr::Array(values) => for value in values { self.check_expression(value, env); },
            Expr::Index { object, index } => {
                self.check_expression(object, env);
                self.check_expression(index, env);
            }
            Expr::Literal(_) => {}
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], env: &Env) {
        if let Expr::Identifier(name) = callee {
            if name == "print" || name == "println" {
                for arg in args { self.check_expression(arg, env); }
                return;
            }
            if let Some(sig) = self.functions.get(name).cloned() {
                self.check_signature(&sig, args, env);
                return;
            }
        }

        if let Expr::Member { object, name } = callee {
            self.check_expression(object, env);
            let type_name = self.object_type_name(object);
            if let Some(sig) = self.methods.get(&(type_name, name.clone())).cloned() {
                let mut all = vec![object.as_ref().clone()];
                all.extend_from_slice(args);
                self.check_signature(&sig, &all, env);
                return;
            }
        }

        self.check_expression(callee, env);
        for arg in args { self.check_expression(arg, env); }
    }

    fn check_signature(&mut self, signature: &FunctionSig, args: &[Expr], env: &Env) {
        let mut temporary: Vec<(String, BorrowKind)> = Vec::new();

        for (index, argument) in args.iter().enumerate() {
            let Some(expected) = signature.params.get(index) else {
                self.check_expression(argument, env);
                continue;
            };

            if let Type::Reference { mutable, .. } = expected {
                let Some(name) = self.borrow_target(argument) else {
                    self.error("reference arguments require a variable, field, or index expression");
                    continue;
                };
                let kind = if *mutable { BorrowKind::Mutable } else { BorrowKind::Shared };

                if *mutable && !self.is_mutable_target(&name, env) {
                    self.error(format!("cannot mutably borrow immutable variable '{}'", name));
                }
                if !self.can_borrow(&name, kind) {
                    self.report_conflict(&name, kind);
                }
                temporary.push((name, kind));
            } else {
                self.check_expression(argument, env);
            }
        }

        for (name, kind) in temporary {
            self.remove_temporary_borrow(&name, kind);
        }
    }

    fn borrow_target(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Unary {
                op: UnaryOp::BorrowShared | UnaryOp::BorrowMutable,
                expr,
            } => self.root_identifier(expr),
            _ => self.root_identifier(expression),
        }
    }

    fn borrow_expression(&self, expression: &Expr) -> Option<(String, BorrowKind)> {
        match expression {
            Expr::Unary { op: UnaryOp::BorrowShared, expr } =>
                self.root_identifier(expr).map(|name| (name, BorrowKind::Shared)),
            Expr::Unary { op: UnaryOp::BorrowMutable, expr } =>
                self.root_identifier(expr).map(|name| (name, BorrowKind::Mutable)),
            _ => None,
        }
    }

    fn create_borrow(&mut self, target: String, kind: BorrowKind, holder: Option<String>, env: &Env) {
        if !self.can_borrow(&target, kind) {
            self.report_conflict(&target, kind);
            return;
        }
        self.active.entry(target).or_default().push(ActiveBorrow {
            kind,
            scope: self.scope,
            holder: holder.clone(),
        });
        if let Some(holder) = holder {
            let _ = env.is_reference(&holder);
        }
    }

    fn remove_temporary_borrow(&mut self, target: &str, kind: BorrowKind) {
        if let Some(borrows) = self.active.get_mut(target) {
            if let Some(index) = borrows.iter().rposition(|borrow| borrow.kind == kind && borrow.holder.is_none()) {
                borrows.remove(index);
            }
            if borrows.is_empty() { self.active.remove(target); }
        }
    }

    fn can_borrow(&self, target: &str, kind: BorrowKind) -> bool {
        match self.active.get(target) {
            None => true,
            Some(borrows) => match kind {
                BorrowKind::Shared => borrows.iter().all(|b| b.kind == BorrowKind::Shared),
                BorrowKind::Mutable => borrows.is_empty(),
            }
        }
    }

    fn report_conflict(&mut self, target: &str, kind: BorrowKind) {
        let message = match kind {
            BorrowKind::Shared => format!("cannot borrow '{}' as shared because it is mutably borrowed", target),
            BorrowKind::Mutable => format!("cannot mutably borrow '{}' because it is already borrowed", target),
        };
        self.error(message);
    }

    fn check_read(&mut self, name: &str) {
        if let Some(borrows) = self.active.get(name) {
            if borrows.iter().any(|b| b.kind == BorrowKind::Mutable) {
                self.error(format!("cannot read '{}' because it is mutably borrowed", name));
            }
        }
    }

    fn check_mutation(&mut self, name: &str) {
        if let Some(borrows) = self.active.get(name) {
            self.error(format!("cannot modify '{}' because it is borrowed", name));
        }
    }

    fn is_mutable_target(&self, name: &str, env: &Env) -> bool { env.is_mutable(name) }

    fn root_identifier(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Identifier(name) => Some(name.clone()),
            Expr::Member { object, .. } |
            Expr::Index { object, .. } => self.root_identifier(object),
            Expr::Grouping(inner) => self.root_identifier(inner),
            _ => None,
        }
    }

    fn object_type_name(&self, expression: &Expr) -> String {
        self.root_identifier(expression).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser, sema::SemanticAnalyzer};

    fn check(source: &str) -> Result<(), Vec<BorrowError>> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        BorrowChecker::check(&program)
    }

    #[test]
    fn shared_borrows_live_until_scope_end() {
        let source = "fn main(){let x=10\nlet a=&x\nlet b=&x}";
        assert!(check(source).is_ok());
    }

    #[test]
    fn mutable_borrow_blocks_mutation() {
        let source = "fn main(){let mut x=10\nlet r=&mut x\nx=20}";
        assert!(check(source).is_err());
    }

    #[test]
    fn shared_borrow_blocks_mutation() {
        let source = "fn main(){let mut x=10\nlet r=&x\nx=20}";
        assert!(check(source).is_err());
    }

    #[test]
    fn mutable_borrows_are_exclusive() {
        let source = "fn main(){let mut x=10\nlet a=&mut x\nlet b=&mut x}";
        assert!(check(source).is_err());
    }

    #[test]
    fn mutable_borrow_blocks_read() {
        let source = "fn main(){let mut x=10\nlet r=&mut x\nprintln(x)}";
        assert!(check(source).is_err());
    }

    #[test]
    fn borrow_ends_with_scope() {
        let source = "fn main(){let mut x=10\n{let r=&x}\nx=20}";
        assert!(check(source).is_ok());
    }

    #[test]
    fn mutable_reference_requires_mut_variable() {
        let source = "fn touch(value: &mut i32){} fn main(){\nlet value=10\ntouch(value)\n}";
        assert!(check(source).is_err());
    }

    #[test]
    fn mutable_reference_accepts_mut_variable() {
        let source = "fn touch(value: &mut i32){} fn main(){\nlet mut value=10\ntouch(value)\n}";
        assert!(check(source).is_ok());
    }
}
