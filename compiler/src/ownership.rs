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
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut VarState> {
        self.scopes.iter_mut().rev().find_map(|scope| scope.get_mut(name))
    }
}

pub struct OwnershipChecker {
    functions: HashMap<(usize, String), FunctionSig>,
    methods: HashMap<(usize, String, String), FunctionSig>,
    enums: HashMap<(usize, String), Vec<(String, Vec<Type>)>>,
    imports: HashMap<usize, Vec<(Option<String>, usize)>>,
    errors: Vec<OwnershipError>,
}

impl OwnershipChecker {
    pub fn check(program: &Program) -> Result<(), Vec<OwnershipError>> {
        let mut checker = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
            enums: HashMap::new(),
            imports: HashMap::new(),
            errors: Vec::new(),
        };
        checker.collect(program);

        for item in &program.items {
            match item {
                Item::Function(function) => checker.check_function(function, None),
                Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        checker.check_function(function, Some(&implementation.type_name));
                    }
                }
                Item::Struct(_) | Item::Enum(_) | Item::Import(_) => {}
            }
        }

        if checker.errors.is_empty() { Ok(()) } else { Err(checker.errors) }
    }

    fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(OwnershipError { message: message.into(), span });
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
                            params: function.params.iter().map(|parameter| Type::from_ref(&parameter.ty)).collect(),
                            return_type: function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                        },
                    );
                }
                Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        self.methods.insert(
                            (
                                implementation.span.source_id,
                                implementation.type_name.clone(),
                                function.name.clone(),
                            ),
                            FunctionSig {
                                params: function.params.iter().map(|parameter| {
                                    if parameter.name == "self" {
                                        Type::Reference {
                                            mutable: parameter.ty.reference == ReferenceKind::Mutable,
                                            inner: Box::new(Type::Named(implementation.type_name.clone())),
                                        }
                                    } else {
                                        Type::from_ref(&parameter.ty)
                                    }
                                }).collect(),
                                return_type: function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                            },
                        );
                    }
                }
                Item::Enum(definition) => {
                    self.enums.insert(
                        (definition.span.source_id, definition.name.clone()),
                        definition.variants.iter().map(|variant| {
                            (
                                variant.name.clone(),
                                variant.payload.iter().map(Type::from_ref).collect(),
                            )
                        }).collect(),
                    );
                }
                Item::Struct(_) => {}
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

        let mut env = Env::default();
        env.push();
        for (index, parameter) in function.params.iter().enumerate() {
            env.define(parameter.name.clone(), signature.params[index].clone());
        }
        self.check_block(&function.body, &mut env);
        env.pop();
    }

    fn check_block(&mut self, block: &Block, env: &mut Env) {
        env.push();
        for statement in &block.statements {
            self.check_stmt(statement, env);
        }
        env.pop();
    }

    fn check_stmt(&mut self, statement: &Stmt, env: &mut Env) {
        match &statement.kind {
            StmtKind::Let { name, initializer, ty, .. } => {
                if let Some(expression) = initializer {
                    let value_ty = self.expr(expression, env, true);
                    let final_ty = ty.as_ref().map(Type::from_ref).unwrap_or(value_ty);
                    env.define(name.clone(), final_ty);
                } else {
                    env.define(name.clone(), ty.as_ref().map(Type::from_ref).unwrap_or(Type::Unknown));
                }
            }
            StmtKind::Expr(expression) => { self.expr(expression, env, false); }
            StmtKind::Return(expression) => {
                if let Some(expression) = expression {
                    self.expr(expression, env, true);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::If { condition, then_branch, else_branch } => {
                self.expr(condition, env, false);
                self.check_block(then_branch, env);
                if let Some(branch) = else_branch {
                    self.check_stmt(branch, env);
                }
            }
            StmtKind::While { condition, body } => {
                self.expr(condition, env, false);
                self.check_block(body, env);
            }
            StmtKind::DoWhile { body, condition } => {
                self.check_block(body, env);
                self.expr(condition, env, false);
            }
            StmtKind::For { initializer, condition, update, body } => {
                env.push();
                if let Some(initializer) = initializer { self.check_stmt(initializer, env); }
                if let Some(condition) = condition { self.expr(condition, env, false); }
                if let Some(update) = update { self.expr(update, env, false); }
                self.check_block(body, env);
                env.pop();
            }
            StmtKind::Match { value, arms } => {
                self.expr(value, env, true);
                for arm in arms {
                    env.push();
                    if let Some(payload) = self.resolve_enum_variant(
                        arm.span.source_id,
                        &arm.enum_name,
                        &arm.variant,
                    ) {
                        for (index, binding) in arm.bindings.iter().enumerate() {
                            if binding == "_" { continue; }
                            env.define(
                                binding.clone(),
                                payload.get(index).cloned().unwrap_or(Type::Unknown),
                            );
                        }
                    }
                    for statement in &arm.body.statements {
                        self.check_stmt(statement, env);
                    }
                    env.pop();
                }
            }
            StmtKind::Block(block) => self.check_block(block, env),
        }
    }

    fn expr(&mut self, expression: &Expr, env: &mut Env, consume: bool) -> Type {
        match &expression.kind {
            ExprKind::Literal(Literal::Bool(_)) => Type::Bool,
            ExprKind::Literal(Literal::String(_)) => Type::Str,
            ExprKind::Literal(Literal::Number(number)) => self.number_type(number),
            ExprKind::Identifier(name) => {
                let ty = match env.get(name) {
                    Some(state) => state.ty.clone(),
                    None => {
                        if self.resolve_function(expression.span.source_id, name).is_some() {
                            return Type::Named(format!("fn {}", name));
                        }
                        self.error_at(expression.span, format!("unknown identifier '{}'", name));
                        return Type::Unknown;
                    }
                };
                self.ensure_available(name, env, expression.span);
                if consume && !ty.is_copy() {
                    self.move_value(name, env);
                }
                ty
            }
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::BorrowShared | UnaryOp::BorrowMutable => {
                    self.expr(expr, env, false);
                    let inner = self.expr_type(expr, env);
                    Type::Reference { mutable: *op == UnaryOp::BorrowMutable, inner: Box::new(inner) }
                }
                _ => self.expr(expr, env, false),
            },
            ExprKind::Grouping(inner) => self.expr(inner, env, consume),
            ExprKind::Binary { left, right, .. } => {
                self.expr(left, env, false);
                self.expr(right, env, false);
                self.expr_type(left, env)
            }
            ExprKind::Assignment { target, op, value } => {
                let value_ty = self.expr(value, env, true);

                let direct_reinitialization =
                    *op == AssignOp::Assign && matches!(target.kind, ExprKind::Identifier(_));

                if !direct_reinitialization {
                    self.expr(target, env, false);
                }

                if let Some(name) = self.root_identifier(target) {
                    if let Some(state) = env.get_mut(&name) {
                        state.moved = false;
                    }
                }
                value_ty
            }
            ExprKind::Call { callee, args } => self.call(callee, args, env),
            ExprKind::Member { object, .. } => {
                if let ExprKind::Identifier(name) = &object.kind {
                    if self.resolve_enum(expression.span.source_id, name) {
                        return Type::Named(name.clone());
                    }
                }
                self.expr(object, env, false)
            },
            ExprKind::Postfix { expr, .. } => self.expr(expr, env, false),
            ExprKind::StructLiteral { name, fields } => {
                for (_, value) in fields {
                    self.expr(value, env, true);
                }
                Type::Named(name.clone())
            }
            ExprKind::Array(values) => {
                for value in values {
                    self.expr(value, env, true);
                }
                if values.is_empty() {
                    Type::Array { element: Box::new(Type::Unknown), len: 0 }
                } else {
                    Type::Array {
                        element: Box::new(self.expr_type(&values[0], env)),
                        len: values.len(),
                    }
                }
            }
            ExprKind::Index { object, index } => {
                self.expr(object, env, false);
                self.expr(index, env, false);
                match self.expr_type(object, env) {
                    Type::Array { element, .. } => *element,
                    Type::Str => Type::Char,
                    _ => Type::Unknown,
                }
            }
        }
    }

    fn resolve_enum(&self, source_id: usize, name: &str) -> bool {
        if self.enums.contains_key(&(source_id, name.to_string())) {
            return true;
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .any(|(_, target)| self.enums.contains_key(&(*target, name.to_string())))
    }

    fn resolve_enum_variant(&self, source_id: usize, enum_name: &str, variant: &str) -> Option<Vec<Type>> {
        if let Some(variants) = self.enums.get(&(source_id, enum_name.to_string())) {
            if let Some((_, payload)) = variants.iter().find(|(name, _)| name == variant) {
                return Some(payload.clone());
            }
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .find_map(|(_, target)| self.enums
                .get(&(*target, enum_name.to_string()))
                .and_then(|variants| variants.iter()
                    .find(|(name, _)| name == variant)
                    .map(|(_, payload)| payload.clone())))
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

    fn call(&mut self, callee: &Expr, args: &[Expr], env: &mut Env) -> Type {
        if let ExprKind::Identifier(name) = &callee.kind {
            if name == "print" || name == "println" || name == "typeof" || name == "len" {
                for argument in args {
                    self.expr(argument, env, false);
                }
                return Type::Unit;
            }
            if let Some(signature) = self.resolve_function(callee.span.source_id, name) {
                return self.check_signature(&signature, args, env);
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
                        return self.check_signature(&signature, args, env);
                    }
                }

                if let Some(payload) =
                    self.resolve_enum_variant(callee.span.source_id, namespace, name)
                {
                    for (index, argument) in args.iter().enumerate() {
                        let by_ref = matches!(payload.get(index), Some(Type::Reference { .. }));
                        self.expr(argument, env, !by_ref);
                    }
                    return Type::Named(namespace.clone());
                }
            }

            self.expr(object, env, false);
            let type_name = self.expr_type(object, env).display_name();
            if let Some(signature) = self.methods
                .get(&(callee.span.source_id, type_name, name.clone()))
                .cloned()
            {
                let mut all = vec![object.as_ref().clone()];
                all.extend_from_slice(args);
                return self.check_signature(&signature, &all, env);
            }
        }

        for argument in args {
            self.expr(argument, env, false);
        }
        Type::Unknown
    }

    fn check_signature(&mut self, signature: &FunctionSig, args: &[Expr], env: &mut Env) -> Type {
        for (index, argument) in args.iter().enumerate() {
            if let Some(expected) = signature.params.get(index) {
                let by_ref = matches!(expected, Type::Reference { .. });
                self.expr(argument, env, !by_ref);
            } else {
                self.expr(argument, env, false);
            }
        }
        signature.return_type.clone()
    }

    fn expr_type(&self, expression: &Expr, env: &Env) -> Type {
        match &expression.kind {
            ExprKind::Identifier(name) => env.get(name).map(|state| state.ty.clone()).unwrap_or(Type::Unknown),
            ExprKind::Literal(Literal::Bool(_)) => Type::Bool,
            ExprKind::Literal(Literal::String(_)) => Type::Str,
            ExprKind::Literal(Literal::Number(number)) => self.number_type(number),
            ExprKind::Grouping(inner) => self.expr_type(inner, env),
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::BorrowShared => Type::Reference {
                    mutable: false,
                    inner: Box::new(self.expr_type(expr, env)),
                },
                UnaryOp::BorrowMutable => Type::Reference {
                    mutable: true,
                    inner: Box::new(self.expr_type(expr, env)),
                },
                _ => self.expr_type(expr, env),
            },
            ExprKind::Array(values) => values.first()
                .map(|value| Type::Array {
                    element: Box::new(self.expr_type(value, env)),
                    len: values.len(),
                })
                .unwrap_or(Type::Array { element: Box::new(Type::Unknown), len: 0 }),
            ExprKind::Index { object, .. } => match self.expr_type(object, env) {
                Type::Array { element, .. } => *element,
                Type::Str => Type::Char,
                _ => Type::Unknown,
            },
            ExprKind::StructLiteral { name, .. } => Type::Named(name.clone()),
            _ => Type::Unknown,
        }
    }

    fn ensure_available(&mut self, name: &str, env: &Env, span: Span) {
        if let Some(state) = env.get(name) {
            if state.moved {
                self.error_at(span, format!("use of moved value '{}'", name));
            }
        }
    }

    fn move_value(&mut self, name: &str, env: &mut Env) {
        if let Some(state) = env.get_mut(name) {
            state.moved = true;
        }
    }

    fn root_identifier(&self, expression: &Expr) -> Option<String> {
        match &expression.kind {
            ExprKind::Identifier(name) => Some(name.clone()),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => self.root_identifier(object),
            ExprKind::Grouping(inner) => self.root_identifier(inner),
            _ => None,
        }
    }

    fn number_type(&self, number: &str) -> Type {
        let lower = number.to_ascii_lowercase();
        for suffix in ["u8","u16","u32","u64","u128","u256","i8","i16","i32","i64","i128","i256","f32","f64","f128"] {
            if lower.ends_with(suffix) {
                return match suffix {
                    "u8"=>Type::U8, "u16"=>Type::U16, "u32"=>Type::U32, "u64"=>Type::U64,
                    "u128"=>Type::U128, "u256"=>Type::U256, "i8"=>Type::I8, "i16"=>Type::I16,
                    "i32"=>Type::I32, "i64"=>Type::I64, "i128"=>Type::I128, "i256"=>Type::I256,
                    "f32"=>Type::F32, "f64"=>Type::F64, "f128"=>Type::F128, _=>Type::Unknown,
                };
            }
        }
        if lower.contains('.') { Type::F64 } else { Type::I32 }
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
        let error = check(source).unwrap_err().remove(0);
        assert!(error.span.line >= 3);
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

    #[test]
    fn mutable_reference_binding_moves() {
        let source = r#"fn main() {
            let mut value = 1
            let first = &mut value
            let second = first
            let third = first
        }"#;
        assert!(check(source).is_err());
    }
}
