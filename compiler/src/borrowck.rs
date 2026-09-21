use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowError {
    pub message: String,
    pub span: Span,
    pub secondary: Option<(Span, String)>,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BorrowKind {
    Shared,
    Mutable,
}

#[derive(Debug, Clone)]
struct ActiveBorrow {
    kind: BorrowKind,
    scope: usize,
    holder: Option<String>,
    origin: Span,
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
    functions: HashMap<(usize, String), FunctionSig>,
    methods: HashMap<(usize, String, String), FunctionSig>,
    imports: HashMap<usize, Vec<(Option<String>, usize)>>,
    errors: Vec<BorrowError>,
    active: HashMap<String, Vec<ActiveBorrow>>,
    scope: usize,
}

impl BorrowChecker {
    pub fn check(program: &Program) -> Result<(), Vec<BorrowError>> {
        let mut checker = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
            imports: HashMap::new(),
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
                Item::Struct(_) | Item::Enum(_) | Item::Import(_) => {}
            }
        }

        if checker.errors.is_empty() { Ok(()) } else { Err(checker.errors) }
    }

    fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(BorrowError {
            message: message.into(),
            span,
            secondary: None,
        });
    }

    fn conflict_at(&mut self, span: Span, target: &str, kind: BorrowKind, message: String) {
        let secondary = self.active.get(target)
            .and_then(|borrows| {
                borrows.iter().find(|borrow| match kind {
                    BorrowKind::Shared => borrow.kind == BorrowKind::Mutable,
                    BorrowKind::Mutable => true,
                })
            })
            .map(|borrow| {
                let label = match borrow.kind {
                    BorrowKind::Shared => "existing shared borrow starts here",
                    BorrowKind::Mutable => "existing mutable borrow starts here",
                };
                (borrow.origin, label.to_string())
            });
        self.errors.push(BorrowError { message, span, secondary });
    }

    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Import(import) => {
                    if let Some(target_source_id) = import.target_source_id {
                        self.imports.entry(import.span.source_id).or_default()
                            .push((import.alias.clone(), target_source_id));
                    }
                }
                Item::Function(function) => {
                    self.functions.insert(
                        (function.span.source_id, function.name.clone()),
                        FunctionSig {
                            params: function.params.iter().map(|p| Type::from_ref(&p.ty)).collect(),
                        },
                    );
                }
                Item::Impl(implementation) => {
                    for method in &implementation.methods {
                        let params = method.params.iter().map(|parameter| {
                            if parameter.name == "self" {
                                Type::Reference {
                                    mutable: parameter.ty.reference == ReferenceKind::Mutable,
                                    inner: Box::new(Type::Named(implementation.type_name.clone())),
                                }
                            } else {
                                Type::from_ref(&parameter.ty)
                            }
                        }).collect();
                        self.methods.insert(
                            (
                                implementation.span.source_id,
                                implementation.type_name.clone(),
                                method.name.clone(),
                            ),
                            FunctionSig { params },
                        );
                    }
                }
                Item::Struct(_) | Item::Enum(_) => {}
            }
        }
    }

    fn check_function(&mut self, function: &Function, impl_type: Option<&str>) {
        let source_id = function.span.source_id;
        let signature = if let Some(type_name) = impl_type {
            self.methods.get(&(source_id, type_name.to_string(), function.name.clone())).cloned()
        } else {
            self.functions.get(&(source_id, function.name.clone())).cloned()
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
        match &statement.kind {
            StmtKind::Let { name, mutable, initializer, .. } => {
                if let Some(expression) = initializer {
                    if let Some((target, kind)) = self.borrow_expression(expression) {
                        if kind == BorrowKind::Mutable && !self.is_mutable_target(&target, env) {
                            self.error_at(
                                expression.span,
                                format!("cannot mutably borrow immutable variable '{}'", target),
                            );
                        }
                        self.create_borrow(target, kind, Some(name.clone()), expression.span, env);
                        env.define(name.clone(), *mutable, true);
                        return;
                    }
                    self.check_expression(expression, env);
                }
                env.define(name.clone(), *mutable, false);
            }
            StmtKind::Expr(expression) => self.check_expression(expression, env),
            StmtKind::Return(expression) => {
                if let Some(expression) = expression {
                    self.check_expression(expression, env);
                }
            }
            StmtKind::If { condition, then_branch, else_branch } => {
                self.check_expression(condition, env);
                self.check_block_body(then_branch, env);
                if let Some(branch) = else_branch {
                    self.check_statement(branch, env);
                }
            }
            StmtKind::While { condition, body } => {
                self.check_expression(condition, env);
                self.check_block_body(body, env);
            }
            StmtKind::DoWhile { body, condition } => {
                self.check_block_body(body, env);
                self.check_expression(condition, env);
            }
            StmtKind::For { initializer, condition, update, body } => {
                self.enter_scope(env);
                if let Some(initializer) = initializer { self.check_statement(initializer, env); }
                if let Some(condition) = condition { self.check_expression(condition, env); }
                if let Some(update) = update { self.check_expression(update, env); }
                self.check_block_body(body, env);
                self.leave_scope(env);
            }
            StmtKind::Match { value, arms } => {
                self.check_expression(value, env);
                for arm in arms {
                    self.check_block_body(&arm.body, env);
                }
            }
            StmtKind::Block(block) => self.check_block_body(block, env),
        }
    }

    fn check_expression(&mut self, expression: &Expr, env: &Env) {
        match &expression.kind {
            ExprKind::Identifier(name) => self.check_read(name, expression.span),
            ExprKind::Assignment { target, value, .. } => {
                if let Some(name) = self.root_identifier(target) {
                    self.check_mutation(&name, target.span);
                }
                self.check_expression(value, env);
            }
            ExprKind::Call { callee, args } => self.check_call(callee, args, env),
            ExprKind::Member { object, .. } => self.check_expression(object, env),
            ExprKind::Postfix { expr, .. } => {
                if let Some(name) = self.root_identifier(expr) {
                    self.check_mutation(&name, expr.span);
                }
                self.check_expression(expr, env);
            }
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::BorrowShared => {
                    if let Some(target) = self.root_identifier(expr) {
                        self.create_borrow(target, BorrowKind::Shared, None, expression.span, env);
                    } else {
                        self.error_at(expression.span, "cannot borrow temporary expression");
                    }
                }
                UnaryOp::BorrowMutable => {
                    if let Some(target) = self.root_identifier(expr) {
                        if !self.is_mutable_target(&target, env) {
                            self.error_at(
                                expression.span,
                                format!("cannot mutably borrow immutable variable '{}'", target),
                            );
                        }
                        self.create_borrow(target, BorrowKind::Mutable, None, expression.span, env);
                    } else {
                        self.error_at(expression.span, "cannot mutably borrow temporary expression");
                    }
                }
                _ => self.check_expression(expr, env),
            },
            ExprKind::Binary { left, right, .. } => {
                self.check_expression(left, env);
                self.check_expression(right, env);
            }
            ExprKind::Grouping(inner) => self.check_expression(inner, env),
            ExprKind::StructLiteral { fields, .. } => {
                for (_, value) in fields { self.check_expression(value, env); }
            }
            ExprKind::Array(values) => {
                for value in values { self.check_expression(value, env); }
            }
            ExprKind::Index { object, index } => {
                self.check_expression(object, env);
                self.check_expression(index, env);
            }
            ExprKind::Literal(_) => {}
        }
    }

    fn namespace_target(&self, source_id: usize, alias: &str) -> Option<usize> {
        self.imports.get(&source_id)
            .and_then(|imports| imports.iter()
                .find(|(candidate, _)| candidate.as_deref() == Some(alias))
                .map(|(_, target)| *target))
    }

    fn resolve_function(&self, source_id: usize, name: &str) -> Option<FunctionSig> {
        if let Some(signature) = self.functions.get(&(source_id, name.to_string())).cloned() {
            return Some(signature);
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .find_map(|(_, target)| self.functions.get(&(*target, name.to_string())).cloned())
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], env: &Env) {
        if let ExprKind::Identifier(name) = &callee.kind {
            if name == "print" || name == "println" || name == "typeof" || name == "len" {
                for argument in args { self.check_expression(argument, env); }
                return;
            }
            if let Some(signature) = self.resolve_function(callee.span.source_id, name) {
                self.check_signature(&signature, args, env);
                return;
            }
        }

        if let ExprKind::Member { object, name } = &callee.kind {
            if let ExprKind::Identifier(namespace) = &object.kind {
                if let Some(target_source_id) =
                    self.namespace_target(callee.span.source_id, namespace)
                {
                    if let Some(signature) = self.functions
                        .get(&(target_source_id, name.clone()))
                        .cloned()
                    {
                        self.check_signature(&signature, args, env);
                        return;
                    }
                }
            }

            self.check_expression(object, env);
            let type_name = self.object_type_name(object);
            if let Some(signature) = self.methods
                .get(&(callee.span.source_id, type_name, name.clone()))
                .cloned()
            {
                let mut all = vec![object.as_ref().clone()];
                all.extend_from_slice(args);
                self.check_signature(&signature, &all, env);
                return;
            }
        }

        self.check_expression(callee, env);
        for argument in args { self.check_expression(argument, env); }
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
                    self.error_at(argument.span, "reference arguments require a variable, field, or index expression");
                    continue;
                };
                let kind = if *mutable { BorrowKind::Mutable } else { BorrowKind::Shared };

                if *mutable && !self.is_mutable_target(&name, env) {
                    self.error_at(
                        argument.span,
                        format!("cannot mutably borrow immutable variable '{}'", name),
                    );
                }
                if self.can_borrow(&name, kind) {
                    self.create_borrow(name.clone(), kind, None, argument.span, env);
                    temporary.push((name, kind));
                } else {
                    self.report_conflict(&name, kind, argument.span);
                }
            } else {
                self.check_expression(argument, env);
            }
        }

        for (name, kind) in temporary {
            self.remove_temporary_borrow(&name, kind);
        }
    }

    fn borrow_target(&self, expression: &Expr) -> Option<String> {
        match &expression.kind {
            ExprKind::Unary {
                op: UnaryOp::BorrowShared | UnaryOp::BorrowMutable,
                expr,
            } => self.root_identifier(expr),
            _ => self.root_identifier(expression),
        }
    }

    fn borrow_expression(&self, expression: &Expr) -> Option<(String, BorrowKind)> {
        match &expression.kind {
            ExprKind::Unary { op: UnaryOp::BorrowShared, expr } =>
                self.root_identifier(expr).map(|name| (name, BorrowKind::Shared)),
            ExprKind::Unary { op: UnaryOp::BorrowMutable, expr } =>
                self.root_identifier(expr).map(|name| (name, BorrowKind::Mutable)),
            _ => None,
        }
    }

    fn create_borrow(
        &mut self,
        target: String,
        kind: BorrowKind,
        holder: Option<String>,
        origin: Span,
        env: &Env,
    ) {
        if !self.can_borrow(&target, kind) {
            self.report_conflict(&target, kind, origin);
            return;
        }
        self.active.entry(target).or_default().push(ActiveBorrow {
            kind,
            scope: self.scope,
            holder: holder.clone(),
            origin,
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
            if borrows.is_empty() {
                self.active.remove(target);
            }
        }
    }

    fn can_borrow(&self, target: &str, kind: BorrowKind) -> bool {
        match self.active.get(target) {
            None => true,
            Some(borrows) => match kind {
                BorrowKind::Shared => borrows.iter().all(|borrow| borrow.kind == BorrowKind::Shared),
                BorrowKind::Mutable => borrows.is_empty(),
            },
        }
    }

    fn report_conflict(&mut self, target: &str, kind: BorrowKind, span: Span) {
        let message = match kind {
            BorrowKind::Shared => {
                format!("cannot borrow '{}' as shared because it is mutably borrowed", target)
            }
            BorrowKind::Mutable => {
                format!("cannot mutably borrow '{}' because it is already borrowed", target)
            }
        };
        self.conflict_at(span, target, kind, message);
    }

    fn check_read(&mut self, name: &str, span: Span) {
        if self.active.get(name)
            .map(|borrows| borrows.iter().any(|borrow| borrow.kind == BorrowKind::Mutable))
            .unwrap_or(false)
        {
            self.conflict_at(
                span,
                name,
                BorrowKind::Shared,
                format!("cannot read '{}' because it is mutably borrowed", name),
            );
        }
    }

    fn check_mutation(&mut self, name: &str, span: Span) {
        if self.active.get(name).map(|borrows| !borrows.is_empty()).unwrap_or(false) {
            self.conflict_at(
                span,
                name,
                BorrowKind::Mutable,
                format!("cannot modify '{}' because it is borrowed", name),
            );
        }
    }

    fn is_mutable_target(&self, name: &str, env: &Env) -> bool {
        env.is_mutable(name)
    }

    fn root_identifier(&self, expression: &Expr) -> Option<String> {
        match &expression.kind {
            ExprKind::Identifier(name) => Some(name.clone()),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => self.root_identifier(object),
            ExprKind::Grouping(inner) => self.root_identifier(inner),
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
        assert!(check("fn main(){let x=10\nlet a=&x\nlet b=&x}").is_ok());
    }

    #[test]
    fn mutable_borrow_blocks_mutation() {
        let errors = check("fn main(){let mut x=10\nlet r=&mut x\nx=20}").unwrap_err();
        assert!(errors[0].span.line >= 2);
        assert!(errors.iter().any(|error| error.secondary.is_some()));
    }

    #[test]
    fn shared_borrow_blocks_mutation() {
        assert!(check("fn main(){let mut x=10\nlet r=&x\nx=20}").is_err());
    }

    #[test]
    fn mutable_borrows_are_exclusive() {
        assert!(check("fn main(){let mut x=10\nlet a=&mut x\nlet b=&mut x}").is_err());
    }

    #[test]
    fn mutable_borrow_blocks_read() {
        assert!(check("fn main(){let mut x=10\nlet r=&mut x\nprintln(x)}").is_err());
    }

    #[test]
    fn borrow_ends_with_scope() {
        assert!(check("fn main(){let mut x=10\n{let r=&x}\nx=20}").is_ok());
    }

    #[test]
    fn mutable_reference_requires_mut_variable() {
        assert!(check("fn touch(value: &mut i32){} fn main(){\nlet value=10\ntouch(value)\n}").is_err());
    }

    #[test]
    fn mutable_reference_accepts_mut_variable() {
        assert!(check("fn touch(value: &mut i32){} fn main(){\nlet mut value=10\ntouch(value)\n}").is_ok());
    }
}
