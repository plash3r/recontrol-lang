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
struct FunctionSig {
    params: Vec<Type>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BorrowKind {
    Shared,
    Mutable,
}

#[derive(Debug, Default)]
struct Env {
    mutable: Vec<HashMap<String, bool>>,
}

impl Env {
    fn push(&mut self) { self.mutable.push(HashMap::new()); }
    fn pop(&mut self) { self.mutable.pop(); }

    fn define(&mut self, name: String, is_mut: bool) {
        self.mutable.last_mut().unwrap().insert(name, is_mut);
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutable.iter().rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(false)
    }
}

pub struct BorrowChecker {
    functions: HashMap<String, FunctionSig>,
    methods: HashMap<(String, String), FunctionSig>,
    errors: Vec<BorrowError>,
}

impl BorrowChecker {
    pub fn check(program: &Program) -> Result<(), Vec<BorrowError>> {
        let mut checker = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
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

        if checker.errors.is_empty() {
            Ok(())
        } else {
            Err(checker.errors)
        }
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
                    self.functions.insert(
                        function.name.clone(),
                        FunctionSig {
                            params: function.params.iter()
                                .map(|parameter| Type::from_ref(&parameter.ty))
                                .collect(),
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

        let mut env = Env::default();
        env.push();

        for (index, parameter) in function.params.iter().enumerate() {
            let ty = &signature.params[index];
            env.define(
                parameter.name.clone(),
                matches!(ty, Type::Reference { mutable: true, .. }),
            );
        }

        self.check_block(&function.body, &mut env);
        env.pop();
    }

    fn check_block(&mut self, block: &Block, env: &mut Env) {
        env.push();

        for statement in &block.statements {
            self.check_statement(statement, env);
        }

        env.pop();
    }

    fn check_statement(&mut self, statement: &Stmt, env: &mut Env) {
        match statement {
            Stmt::Let { name, mutable, initializer, .. } => {
                env.define(name.clone(), *mutable);
                if let Some(expression) = initializer {
                    self.check_expression(expression, env);
                }
            }
            Stmt::Expr(expression) => self.check_expression(expression, env),
            Stmt::Return(expression) => {
                if let Some(expression) = expression {
                    self.check_expression(expression, env);
                }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.check_expression(condition, env);
                self.check_block(then_branch, env);
                if let Some(branch) = else_branch {
                    self.check_statement(branch, env);
                }
            }
            Stmt::While { condition, body } => {
                self.check_expression(condition, env);
                self.check_block(body, env);
            }
            Stmt::DoWhile { body, condition } => {
                self.check_block(body, env);
                self.check_expression(condition, env);
            }
            Stmt::For { initializer, condition, update, body } => {
                env.push();
                if let Some(initializer) = initializer {
                    self.check_statement(initializer, env);
                }
                if let Some(condition) = condition {
                    self.check_expression(condition, env);
                }
                if let Some(update) = update {
                    self.check_expression(update, env);
                }
                self.check_block(body, env);
                env.pop();
            }
            Stmt::Block(block) => self.check_block(block, env),
        }
    }

    fn check_expression(&mut self, expression: &Expr, env: &Env) {
        match expression {
            Expr::Assignment { target, value, .. } => {
                self.check_expression(target, env);
                self.check_expression(value, env);
            }
            Expr::Call { callee, args } => self.check_call(callee, args, env),
            Expr::Member { object, .. } => self.check_expression(object, env),
            Expr::Postfix { expr, .. } => self.check_expression(expr, env),
            Expr::Unary { expr, .. } | Expr::Grouping(expr) => self.check_expression(expr, env),
            Expr::Binary { left, right, .. } => {
                self.check_expression(left, env);
                self.check_expression(right, env);
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.check_expression(value, env);
                }
            }
            Expr::Array(values) => {
                for value in values {
                    self.check_expression(value, env);
                }
            }
            Expr::Index { object, index } => {
                self.check_expression(object, env);
                self.check_expression(index, env);
            }
            Expr::Identifier(_) | Expr::Literal(_) => {}
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], env: &Env) {
        if let Expr::Identifier(name) = callee {
            if name == "print" || name == "println" {
                for argument in args {
                    self.check_expression(argument, env);
                }
                return;
            }

            if let Some(signature) = self.functions.get(name).cloned() {
                self.check_signature(&signature, args, env);
                return;
            }
        }

        if let Expr::Member { object, name } = callee {
            self.check_expression(object, env);

            let type_name = self.object_type_name(object);
            if let Some(signature) = self.methods.get(&(type_name.clone(), name.clone())).cloned() {
                let mut all_args = vec![object.as_ref().clone()];
                all_args.extend_from_slice(args);
                self.check_signature(&signature, &all_args, env);
                return;
            }

            for argument in args {
                self.check_expression(argument, env);
            }
            return;
        }

        self.check_expression(callee, env);
        for argument in args {
            self.check_expression(argument, env);
        }
    }

    fn check_signature(&mut self, signature: &FunctionSig, args: &[Expr], env: &Env) {
        let mut active: HashMap<String, BorrowKind> = HashMap::new();

        for (index, argument) in args.iter().enumerate() {
            self.check_expression(argument, env);

            let Some(expected) = signature.params.get(index) else { continue };
            let Type::Reference { mutable, .. } = expected else { continue };

            let Some(name) = self.root_identifier(argument) else {
                self.error("references currently require a local variable or direct field/index expression");
                continue;
            };

            let kind = if *mutable { BorrowKind::Mutable } else { BorrowKind::Shared };

            if *mutable && !env.is_mutable(&name) {
                self.error(format!("cannot mutably borrow immutable variable '{}'", name));
            }

            if let Some(previous) = active.get(&name) {
                match (previous, kind) {
                    (BorrowKind::Shared, BorrowKind::Mutable) |
                    (BorrowKind::Mutable, BorrowKind::Shared) |
                    (BorrowKind::Mutable, BorrowKind::Mutable) => {
                        self.error(format!("conflicting borrows of '{}'", name));
                    }
                    (BorrowKind::Shared, BorrowKind::Shared) => {}
                }
            } else {
                active.insert(name, kind);
            }
        }
    }

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
    fn mutable_reference_requires_mut_variable() {
        let source = r#"
            fn touch(value: &mut i32) {}
            fn main() {
                let value = 10
                touch(value)
            }
        "#;

        assert!(check(source).is_err());
    }

    #[test]
    fn mutable_reference_accepts_mut_variable() {
        let source = r#"
            fn touch(value: &mut i32) {}
            fn main() {
                let mut value = 10
                touch(value)
            }
        "#;

        assert!(check(source).is_ok());
    }

    #[test]
    fn detects_conflicting_borrows() {
        let source = r#"
            fn mix(a: &mut i32, b: &i32) {}
            fn main() {
                let mut value = 10
                mix(value, value)
            }
        "#;

        assert!(check(source).is_err());
    }

    #[test]
    fn allows_multiple_shared_borrows() {
        let source = r#"
            fn read(a: &i32, b: &i32) {}
            fn main() {
                let value = 10
                read(value, value)
            }
        "#;

        assert!(check(source).is_ok());
    }
}
