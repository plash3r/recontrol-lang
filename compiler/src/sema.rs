use std::collections::HashMap;

use crate::ast::*;
use crate::types::Type;
use crate::lexer::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
    return_type: Type,
}

#[derive(Debug, Clone)]
struct StructInfo {
    fields: HashMap<String, Type>,
}

#[derive(Debug, Default)]
struct Env {
    variables: Vec<HashMap<String, Type>>,
}

impl Env {
    fn push(&mut self) { self.variables.push(HashMap::new()); }
    fn pop(&mut self) { self.variables.pop(); }

    fn define(&mut self, name: String, ty: Type) -> bool {
        let scope = self.variables.last_mut().expect("scope exists");
        scope.insert(name, ty).is_none()
    }

    fn get(&self, name: &str) -> Option<Type> {
        self.variables.iter().rev().find_map(|scope| scope.get(name).cloned())
    }
}

pub struct SemanticAnalyzer {
    functions: HashMap<String, FunctionSig>,
    structs: HashMap<String, StructInfo>,
    methods: HashMap<(String, String), FunctionSig>,
    errors: Vec<SemanticError>,
}

impl SemanticAnalyzer {
    pub fn check(program: &Program) -> Result<(), Vec<SemanticError>> {
        let mut analyzer = Self {
            functions: HashMap::new(),
            structs: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
        };

        analyzer.collect_items(program);
        analyzer.check_items(program);

        if analyzer.errors.is_empty() { Ok(()) } else { Err(analyzer.errors) }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(SemanticError {
            message: message.into(),
            span: Span { line: 1, column: 1, length: 0 },
        });
    }

    fn collect_items(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(function) => {
                    if self.functions.contains_key(&function.name) {
                        self.error(format!("duplicate function '{}'", function.name));
                        continue;
                    }
                    let params = function.params.iter().map(|p| Type::from_ref(&p.ty)).collect();
                    let return_type = function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit);
                    self.functions.insert(function.name.clone(), FunctionSig { params, return_type });
                }
                Item::Struct(def) => {
                    if self.structs.contains_key(&def.name) {
                        self.error(format!("duplicate struct '{}'", def.name));
                        continue;
                    }
                    let mut fields = HashMap::new();
                    for field in &def.fields {
                        if fields.insert(field.name.clone(), Type::from_ref(&field.ty)).is_some() {
                            self.error(format!("duplicate field '{}.{}'", def.name, field.name));
                        }
                    }
                    self.structs.insert(def.name.clone(), StructInfo { fields });
                }
                Item::Impl(imp) => {
                    if !self.structs.contains_key(&imp.type_name) {
                        self.error(format!("unknown type '{}' in impl", imp.type_name));
                    }
                    for method in &imp.methods {
                        let params = method.params.iter().map(|p| {
                            let mut ty = Type::from_ref(&p.ty);
                            if p.name == "self" {
                                ty = Type::Reference {
                                    mutable: matches!(ty, Type::Reference { mutable: true, .. }),
                                    inner: Box::new(Type::Named(imp.type_name.clone())),
                                };
                            }
                            ty
                        }).collect();
                        let return_type = method.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit);
                        let key = (imp.type_name.clone(), method.name.clone());
                        if self.methods.insert(key.clone(), FunctionSig { params, return_type }).is_some() {
                            self.error(format!("duplicate method '{}.{}'", key.0, key.1));
                        }
                    }
                }
            }
        }
    }

    fn check_items(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(function) => self.check_function(function, None),
                Item::Impl(imp) => {
                    for method in &imp.methods {
                        self.check_function(method, Some(&imp.type_name));
                    }
                }
                Item::Struct(_) => {}
            }
        }
    }

    fn check_function(&mut self, function: &Function, impl_type: Option<&str>) {
        let sig = if let Some(type_name) = impl_type {
            self.methods.get(&(type_name.to_string(), function.name.clone())).cloned()
        } else {
            self.functions.get(&function.name).cloned()
        };
        let Some(sig) = sig else { return };

        let mut env = Env::default();
        env.push();

        for (index, param) in function.params.iter().enumerate() {
            let mut ty = sig.params[index].clone();
            if param.name == "self" && impl_type.is_some() {
                ty = Type::Reference {
                    mutable: matches!(ty, Type::Reference { mutable: true, .. }),
                    inner: Box::new(Type::Named(impl_type.unwrap().into())),
                };
            }
            if !env.define(param.name.clone(), ty) {
                self.error(format!("duplicate parameter '{}'", param.name));
            }
        }

        self.check_block(&function.body, &mut env, &sig.return_type);
        env.pop();
    }

    fn check_block(&mut self, block: &Block, env: &mut Env, return_type: &Type) {
        env.push();
        for stmt in &block.statements {
            self.check_stmt(stmt, env, return_type);
        }
        env.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt, env: &mut Env, return_type: &Type) {
        match stmt {
            Stmt::Let { name, ty, initializer, .. } => {
                let inferred = initializer.as_ref().map(|e| self.check_expr(e, env));
                let declared = ty.as_ref().map(Type::from_ref);
                let final_ty = match (declared, inferred) {
                    (Some(declared), Some(actual)) => {
                        if !self.compatible(&declared, &actual) {
                            self.error(format!("type mismatch: expected {}, found {}", declared.display_name(), actual.display_name()));
                        }
                        declared
                    }
                    (Some(declared), None) => {
                        if matches!(declared, Type::Unknown) { self.error("unknown variable type"); }
                        declared
                    }
                    (None, Some(actual)) => actual,
                    (None, None) => Type::Unknown,
                };
                if !env.define(name.clone(), final_ty) {
                    self.error(format!("duplicate variable '{}'", name));
                }
            }
            Stmt::Expr(expr) => { self.check_expr(expr, env); }
            Stmt::Return(expr) => {
                let actual = expr.as_ref().map(|e| self.check_expr(e, env)).unwrap_or(Type::Unit);
                if !self.compatible(return_type, &actual) {
                    self.error(format!("return type mismatch: expected {}, found {}", return_type.display_name(), actual.display_name()));
                }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                let ty = self.check_expr(condition, env);
                if ty != Type::Bool { self.error(format!("if condition must be bool, found {}", ty.display_name())); }
                self.check_block(then_branch, env, return_type);
                if let Some(block) = else_branch { self.check_block(block, env, return_type); }
            }
            Stmt::While { condition, body } => {
                let ty = self.check_expr(condition, env);
                if ty != Type::Bool { self.error(format!("while condition must be bool, found {}", ty.display_name())); }
                self.check_block(body, env, return_type);
            }
            Stmt::DoWhile { body, condition } => {
                self.check_block(body, env, return_type);
                let ty = self.check_expr(condition, env);
                if ty != Type::Bool { self.error(format!("do while condition must be bool, found {}", ty.display_name())); }
            }
            Stmt::For { initializer, condition, update, body } => {
                env.push();
                if let Some(init) = initializer { self.check_stmt(init, env, return_type); }
                if let Some(cond) = condition {
                    let ty = self.check_expr(cond, env);
                    if ty != Type::Bool { self.error(format!("for condition must be bool, found {}", ty.display_name())); }
                }
                if let Some(update) = update { self.check_expr(update, env); }
                self.check_block(body, env, return_type);
                env.pop();
            }
            Stmt::Block(block) => self.check_block(block, env, return_type),
        }
    }

    fn check_expr(&mut self, expr: &Expr, env: &Env) -> Type {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Bool(_) => Type::Bool,
                Literal::String(_) => Type::Str,
                Literal::Number(text) => self.number_type(text),
            },
            Expr::Identifier(name) => {
                if let Some(ty) = env.get(name) { ty }
                else if self.functions.contains_key(name) { Type::Named(format!("fn {}", name)) }
                else if self.structs.contains_key(name) { Type::Named(name.clone()) }
                else {
                    self.error(format!("unknown identifier '{}'", name));
                    Type::Unknown
                }
            }
            Expr::Unary { op, expr } => {
                let ty = self.check_expr(expr, env);
                match op {
                    UnaryOp::Not if ty == Type::Bool => Type::Bool,
                    UnaryOp::Not => { self.error(format!("operator ! requires bool, found {}", ty.display_name())); Type::Unknown }
                    UnaryOp::Plus | UnaryOp::Minus if ty.is_numeric() => ty,
                    UnaryOp::Plus | UnaryOp::Minus => { self.error(format!("unary numeric operator requires number, found {}", ty.display_name())); Type::Unknown }
                }
            }
            Expr::Binary { left, op, right } => {
                let l = self.check_expr(left, env);
                let r = self.check_expr(right, env);
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        if l != Type::Bool || r != Type::Bool { self.error("logical operators require bool operands"); }
                        Type::Bool
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessEqual |
                    BinaryOp::Greater | BinaryOp::GreaterEqual => {
                        if !self.compatible(&l, &r) { self.error(format!("incompatible comparison types: {} and {}", l.display_name(), r.display_name())); }
                        Type::Bool
                    }
                    _ => {
                        if !l.is_numeric() || !r.is_numeric() || !self.compatible(&l, &r) {
                            self.error(format!("incompatible operands: {} and {}", l.display_name(), r.display_name()));
                            Type::Unknown
                        } else { l }
                    }
                }
            }
            Expr::Assignment { target, value } => {
                let target_ty = self.check_expr(target, env);
                let value_ty = self.check_expr(value, env);
                if !self.compatible(&target_ty, &value_ty) {
                    self.error(format!("assignment type mismatch: expected {}, found {}", target_ty.display_name(), value_ty.display_name()));
                }
                target_ty
            }
            Expr::Call { callee, args } => self.check_call(callee, args, env),
            Expr::Member { object, name } => self.check_member(object, name, env),
            Expr::Postfix { expr, .. } => {
                let ty = self.check_expr(expr, env);
                if !ty.is_numeric() { self.error("increment/decrement requires a numeric value"); }
                ty
            }
            Expr::Grouping(inner) => self.check_expr(inner, env),
            Expr::StructLiteral { name, fields } => self.check_struct_literal(name, fields, env),
            Expr::Array(elements) => {
                if elements.is_empty() { return Type::Array(Box::new(Type::Unknown)); }
                let first = self.check_expr(&elements[0], env);
                for element in &elements[1..] {
                    let ty = self.check_expr(element, env);
                    if !self.compatible(&first, &ty) { self.error("array elements must have compatible types"); }
                }
                Type::Array(Box::new(first))
            }
            Expr::Index { object, index } => {
                let object_ty = self.check_expr(object, env);
                let index_ty = self.check_expr(index, env);
                if !index_ty.is_integer() { self.error("array index must be an integer"); }
                match object_ty {
                    Type::Array(inner) => *inner,
                    Type::Str => Type::Char,
                    _ => { self.error("indexing requires an array or str"); Type::Unknown }
                }
            }
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], env: &Env) -> Type {
        if let Expr::Identifier(name) = callee {
            if name == "println" || name == "print" {
                for arg in args { self.check_expr(arg, env); }
                return Type::Unit;
            }
            if let Some(sig) = self.functions.get(name).cloned() {
                return self.check_signature(&sig, args, env);
            }
            self.error(format!("unknown function '{}'", name));
            return Type::Unknown;
        }

        if let Expr::Member { object, name } = callee {
            let object_ty = self.check_expr(object, env);
            let type_name = match object_ty {
                Type::Named(name) => name,
                Type::Reference { inner, .. } => match *inner { Type::Named(name) => name, _ => String::new() },
                _ => String::new(),
            };
            if let Some(sig) = self.methods.get(&(type_name.clone(), name.clone())).cloned() {
                let mut all_args = vec![object.as_ref().clone()];
                all_args.extend_from_slice(args);
                return self.check_signature(&sig, &all_args, env);
            }
            self.error(format!("unknown method '{}.{}'", type_name, name));
            return Type::Unknown;
        }

        self.error("expression is not callable");
        Type::Unknown
    }

    fn check_signature(&mut self, sig: &FunctionSig, args: &[Expr], env: &Env) -> Type {
        if args.len() != sig.params.len() {
            self.error(format!("wrong argument count: expected {}, found {}", sig.params.len(), args.len()));
        }
        for (index, arg) in args.iter().enumerate() {
            let actual = self.check_expr(arg, env);
            if let Some(expected) = sig.params.get(index) {
                if !self.compatible(expected, &actual) {
                    self.error(format!("argument {} type mismatch: expected {}, found {}", index + 1, expected.display_name(), actual.display_name()));
                }
            }
        }
        sig.return_type.clone()
    }

    fn check_member(&mut self, object: &Expr, name: &str, env: &Env) -> Type {
        let object_ty = self.check_expr(object, env);
        let type_name = match object_ty {
            Type::Named(name) => name,
            Type::Reference { inner, .. } => match *inner { Type::Named(name) => name, _ => String::new() },
            _ => String::new(),
        };
        if let Some(info) = self.structs.get(&type_name) {
            if let Some(ty) = info.fields.get(name) { return ty.clone(); }
            if self.methods.contains_key(&(type_name.clone(), name.to_string())) {
                return Type::Named(format!("method {}.{}", type_name, name));
            }
        }
        self.error(format!("unknown member '{}.{}'", type_name, name));
        Type::Unknown
    }

    fn check_struct_literal(&mut self, name: &str, fields: &[(String, Expr)], env: &Env) -> Type {
        let Some(info) = self.structs.get(name).cloned() else {
            self.error(format!("unknown struct '{}'", name));
            for (_, value) in fields { self.check_expr(value, env); }
            return Type::Unknown;
        };

        let mut seen = HashMap::new();
        for (field, value) in fields {
            let actual = self.check_expr(value, env);
            if seen.insert(field, true).is_some() {
                self.error(format!("duplicate field '{}' in {}", field, name));
            }
            match info.fields.get(field) {
                Some(expected) => {
                    if !self.compatible(expected, &actual) {
                        self.error(format!("field '{}.{}' type mismatch: expected {}, found {}", name, field, expected.display_name(), actual.display_name()));
                    }
                }
                None => self.error(format!("unknown field '{}.{}'", name, field)),
            }
        }
        for field in info.fields.keys() {
            if !seen.contains_key(field) {
                self.error(format!("missing field '{}.{}'", name, field));
            }
        }
        Type::Named(name.into())
    }

    fn number_type(&self, text: &str) -> Type {
        let lower = text.to_ascii_lowercase();
        for suffix in ["u8","u16","u32","u64","u128","u256","i8","i16","i32","i64","i128","i256","f32","f64","f128"] {
            if lower.ends_with(suffix) {
                return match suffix {
                    "u8" => Type::U8, "u16" => Type::U16, "u32" => Type::U32, "u64" => Type::U64,
                    "u128" => Type::U128, "u256" => Type::U256, "i8" => Type::I8, "i16" => Type::I16,
                    "i32" => Type::I32, "i64" => Type::I64, "i128" => Type::I128, "i256" => Type::I256,
                    "f32" => Type::F32, "f64" => Type::F64, "f128" => Type::F128, _ => Type::Unknown,
                };
            }
        }
        if lower.contains('.') { Type::F64 } else { Type::I32 }
    }

    fn compatible(&self, expected: &Type, actual: &Type) -> bool {
        expected == actual || matches!(expected, Type::Unknown) || matches!(actual, Type::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    fn check(source: &str) -> Result<(), Vec<SemanticError>> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program)
    }

    #[test]
    fn accepts_inferred_and_explicit_types() {
        assert!(check("fn main() { let a: i32 = 10 let b = 20u64 }").is_ok());
    }

    #[test]
    fn rejects_type_mismatch() {
        assert!(check("fn main() { let a: i32 = true }").is_err());
    }

    #[test]
    fn rejects_unknown_variable() {
        assert!(check("fn main() { println(missing) }").is_err());
    }

    #[test]
    fn rejects_wrong_call_arity() {
        assert!(check("fn add(a: i32, b: i32) { return } fn main() { add(1) }").is_err());
    }

    #[test]
    fn checks_struct_literals() {
        let source = "struct Player { name: str health: i32 alive: bool } fn main() { let p = Player { name: "x" health: 100 alive: true } println(p.health) }";
        assert!(check(source).is_ok());
    }

    #[test]
    fn checks_conditions() {
        assert!(check("fn main() { if true { println(1) } while false { } }").is_ok());
        assert!(check("fn main() { if 10 { } }").is_err());
    }
}
