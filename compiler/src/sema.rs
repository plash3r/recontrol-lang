use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
    return_type: Type,
    public: bool,
    source_id: usize,
}

#[derive(Debug, Clone)]
struct FieldInfo {
    ty: Type,
    public: bool,
    source_id: usize,
}

#[derive(Debug, Clone)]
struct StructInfo {
    fields: HashMap<String, FieldInfo>,
    public: bool,
    source_id: usize,
}

#[derive(Debug, Clone)]
struct ImportInfo {
    target_source_id: usize,
    alias: Option<String>,
}

#[derive(Debug, Default)]
struct Env {
    vars: Vec<HashMap<String, Type>>,
    mutable: Vec<HashMap<String, bool>>,
}

impl Env {
    fn push(&mut self) {
        self.vars.push(HashMap::new());
        self.mutable.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.vars.pop();
        self.mutable.pop();
    }

    fn define(&mut self, name: String, ty: Type, is_mut: bool) -> bool {
        let vars = self.vars.last_mut().unwrap();
        let muts = self.mutable.last_mut().unwrap();
        let fresh = vars.insert(name.clone(), ty).is_none();
        if fresh {
            muts.insert(name, is_mut);
        }
        fresh
    }

    fn get(&self, name: &str) -> Option<Type> {
        self.vars.iter().rev().find_map(|scope| scope.get(name).cloned())
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutable.iter().rev().find_map(|scope| scope.get(name).copied()).unwrap_or(false)
    }
}

pub struct SemanticAnalyzer {
    functions: HashMap<(usize, String), FunctionSig>,
    structs: HashMap<(usize, String), StructInfo>,
    methods: HashMap<(usize, String, String), FunctionSig>,
    imports: HashMap<usize, Vec<ImportInfo>>,
    errors: Vec<SemanticError>,
}

impl SemanticAnalyzer {
    pub fn check(program: &Program) -> Result<(), Vec<SemanticError>> {
        let mut analyzer = Self {
            functions: HashMap::new(),
            structs: HashMap::new(),
            methods: HashMap::new(),
            imports: HashMap::new(),
            errors: Vec::new(),
        };
        analyzer.collect(program);
        analyzer.check_items(program);
        if analyzer.errors.is_empty() { Ok(()) } else { Err(analyzer.errors) }
    }

    fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(SemanticError { message: message.into(), span });
    }

    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Import(import) => {
                    let Some(target_source_id) = import.target_source_id else { continue };
                    if let Some(alias) = &import.alias {
                        let duplicate = self.imports
                            .get(&import.span.source_id)
                            .map(|imports| imports.iter().any(|existing| existing.alias.as_ref() == Some(alias)))
                            .unwrap_or(false);
                        if duplicate {
                            self.error_at(import.span, format!("duplicate namespace '{}'", alias));
                            continue;
                        }
                    }
                    self.imports.entry(import.span.source_id).or_default().push(ImportInfo {
                        target_source_id,
                        alias: import.alias.clone(),
                    });
                }
                Item::Function(function) => {
                    let key = (function.span.source_id, function.name.clone());
                    if self.functions.contains_key(&key) {
                        self.error_at(function.span, format!("duplicate function '{}'", function.name));
                        continue;
                    }
                    self.functions.insert(
                        key,
                        FunctionSig {
                            params: function.params.iter().map(|p| Type::from_ref(&p.ty)).collect(),
                            return_type: function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                            public: function.public,
                            source_id: function.span.source_id,
                        },
                    );
                }
                Item::Struct(structure) => {
                    let key = (structure.span.source_id, structure.name.clone());
                    if self.structs.contains_key(&key) {
                        self.error_at(structure.span, format!("duplicate struct '{}'", structure.name));
                        continue;
                    }
                    let mut fields = HashMap::new();
                    for field in &structure.fields {
                        if fields.insert(
                            field.name.clone(),
                            FieldInfo {
                                ty: Type::from_ref(&field.ty),
                                public: field.public,
                                source_id: field.span.source_id,
                            },
                        ).is_some() {
                            self.error_at(field.span, format!("duplicate field '{}.{}'", structure.name, field.name));
                        }
                    }
                    self.structs.insert(
                        key,
                        StructInfo {
                            fields,
                            public: structure.public,
                            source_id: structure.span.source_id,
                        },
                    );
                }
                Item::Impl(_) => {}
            }
        }

        for item in &program.items {
            let Item::Impl(implementation) = item else { continue };
            let source_id = implementation.span.source_id;
            if !self.structs.contains_key(&(source_id, implementation.type_name.clone())) {
                self.error_at(
                    implementation.span,
                    format!("unknown local type '{}' in impl", implementation.type_name),
                );
            }
            for function in &implementation.methods {
                let params = function.params.iter().map(|parameter| {
                    if parameter.name == "self" {
                        Type::Reference {
                            mutable: parameter.ty.reference == ReferenceKind::Mutable,
                            inner: Box::new(Type::Named(implementation.type_name.clone())),
                        }
                    } else {
                        Type::from_ref(&parameter.ty)
                    }
                }).collect();
                let key = (
                    source_id,
                    implementation.type_name.clone(),
                    function.name.clone(),
                );
                if self.methods.insert(
                    key.clone(),
                    FunctionSig {
                        params,
                        return_type: function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                        public: function.public,
                        source_id: function.span.source_id,
                    },
                ).is_some() {
                    self.error_at(
                        function.span,
                        format!("duplicate method '{}.{}'", key.1, key.2),
                    );
                }
            }
        }
    }

    fn check_items(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(function) => self.check_function(function, None),
                Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        self.check_function(function, Some(&implementation.type_name));
                    }
                }
                Item::Struct(_) | Item::Import(_) => {}
            }
        }
    }

    fn check_function(&mut self, function: &Function, impl_ty: Option<&str>) {
        let source_id = function.span.source_id;
        let signature = if let Some(ty) = impl_ty {
            self.methods.get(&(source_id, ty.to_string(), function.name.clone())).cloned()
        } else {
            self.functions.get(&(source_id, function.name.clone())).cloned()
        };
        let Some(signature) = signature else { return };

        let mut env = Env::default();
        env.push();
        for (index, parameter) in function.params.iter().enumerate() {
            let ty = signature.params[index].clone();
            let mutable = matches!(&ty, Type::Reference { mutable: true, .. });
            if !env.define(parameter.name.clone(), ty, mutable) {
                self.error_at(parameter.span, format!("duplicate parameter '{}'", parameter.name));
            }
        }
        self.check_block(&function.body, &mut env, &signature.return_type);
        env.pop();
    }

    fn check_block(&mut self, block: &Block, env: &mut Env, return_type: &Type) {
        env.push();
        for statement in &block.statements {
            self.check_stmt(statement, env, return_type);
        }
        env.pop();
    }

    fn check_stmt(&mut self, statement: &Stmt, env: &mut Env, return_type: &Type) {
        match &statement.kind {
            StmtKind::Let { name, mutable, ty, initializer } => {
                let actual = initializer.as_ref().map(|expr| self.expr(expr, env));
                let declared = ty.as_ref().map(Type::from_ref);

                if let (Some(type_ref), Some(expression)) = (ty.as_ref(), initializer.as_ref()) {
                    if let ExprKind::Array(values) = &expression.kind {
                        if let Some(expected_len) = type_ref.array_len {
                            if values.len() != expected_len {
                                self.error_at(
                                    expression.span,
                                    format!("array length mismatch: expected {}, found {}", expected_len, values.len()),
                                );
                            }
                        }
                    }
                }

                let final_ty = match (declared, actual) {
                    (Some(declared), Some(actual)) => {
                        if !self.compatible_expr(&declared, &actual, initializer.as_ref()) {
                            self.error_at(
                                initializer.as_ref().map(|x| x.span).unwrap_or(statement.span),
                                format!("type mismatch: expected {}, found {}", declared.display_name(), actual.display_name()),
                            );
                        }
                        declared
                    }
                    (Some(declared), None) => declared,
                    (None, Some(actual)) => actual,
                    (None, None) => Type::Unknown,
                };

                if !env.define(name.clone(), final_ty, *mutable) {
                    self.error_at(statement.span, format!("duplicate variable '{}'", name));
                }
            }
            StmtKind::Expr(expression) => {
                self.expr(expression, env);
            }
            StmtKind::Return(value) => {
                let actual = value.as_ref().map(|expr| self.expr(expr, env)).unwrap_or(Type::Unit);
                if !self.compatible(return_type, &actual) {
                    self.error_at(
                        value.as_ref().map(|x| x.span).unwrap_or(statement.span),
                        format!("return type mismatch: expected {}, found {}", return_type.display_name(), actual.display_name()),
                    );
                }
            }
            StmtKind::If { condition, then_branch, else_branch } => {
                let ty = self.expr(condition, env);
                if ty != Type::Bool {
                    self.error_at(condition.span, format!("if condition must be bool, found {}", ty.display_name()));
                }
                self.check_block(then_branch, env, return_type);
                if let Some(branch) = else_branch {
                    self.check_stmt(branch, env, return_type);
                }
            }
            StmtKind::While { condition, body } => {
                let ty = self.expr(condition, env);
                if ty != Type::Bool {
                    self.error_at(condition.span, format!("while condition must be bool, found {}", ty.display_name()));
                }
                self.check_block(body, env, return_type);
            }
            StmtKind::DoWhile { body, condition } => {
                self.check_block(body, env, return_type);
                let ty = self.expr(condition, env);
                if ty != Type::Bool {
                    self.error_at(condition.span, format!("do while condition must be bool, found {}", ty.display_name()));
                }
            }
            StmtKind::For { initializer, condition, update, body } => {
                env.push();
                if let Some(initializer) = initializer {
                    self.check_stmt(initializer, env, return_type);
                }
                if let Some(condition) = condition {
                    let ty = self.expr(condition, env);
                    if ty != Type::Bool {
                        self.error_at(condition.span, format!("for condition must be bool, found {}", ty.display_name()));
                    }
                }
                if let Some(update) = update {
                    self.expr(update, env);
                }
                self.check_block(body, env, return_type);
                env.pop();
            }
            StmtKind::Block(block) => self.check_block(block, env, return_type),
        }
    }

    fn expr(&mut self, expression: &Expr, env: &Env) -> Type {
        match &expression.kind {
            ExprKind::Literal(Literal::Bool(_)) => Type::Bool,
            ExprKind::Literal(Literal::String(_)) => Type::Str,
            ExprKind::Literal(Literal::Number(number)) => self.number_type(number),
            ExprKind::Identifier(name) => env.get(name)
                .or_else(|| self.functions.contains_key(name).then(|| Type::Named(format!("fn {}", name))))
                .unwrap_or_else(|| {
                    self.error_at(expression.span, format!("unknown identifier '{}'", name));
                    Type::Unknown
                }),
            ExprKind::Grouping(inner) => self.expr(inner, env),
            ExprKind::Unary { op, expr } => {
                let ty = self.expr(expr, env);
                match op {
                    UnaryOp::Not => {
                        if ty == Type::Bool { Type::Bool } else {
                            self.error_at(expression.span, "operator ! requires bool");
                            Type::Unknown
                        }
                    }
                    UnaryOp::Plus | UnaryOp::Minus => {
                        if ty.is_numeric() { ty } else {
                            self.error_at(expression.span, "unary operator requires number");
                            Type::Unknown
                        }
                    }
                    UnaryOp::BorrowShared => Type::Reference { mutable: false, inner: Box::new(ty) },
                    UnaryOp::BorrowMutable => {
                        if !self.assignable(expr, env) {
                            self.error_at(expression.span, "cannot mutably borrow immutable expression");
                        }
                        Type::Reference { mutable: true, inner: Box::new(ty) }
                    }
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_ty = self.expr(left, env);
                let right_ty = self.expr(right, env);
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        if left_ty != Type::Bool || right_ty != Type::Bool {
                            self.error_at(expression.span, "logical operators require bool operands");
                        }
                        Type::Bool
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessEqual |
                    BinaryOp::Greater | BinaryOp::GreaterEqual => {
                        if !self.compatible_exprs(&left_ty, left, &right_ty, right) {
                            self.error_at(expression.span, "incompatible comparison types");
                        }
                        Type::Bool
                    }
                    _ => {
                        if left_ty.is_numeric() && right_ty.is_numeric() &&
                            self.compatible_exprs(&left_ty, left, &right_ty, right)
                        {
                            left_ty
                        } else {
                            self.error_at(expression.span, "incompatible numeric operands");
                            Type::Unknown
                        }
                    }
                }
            }
            ExprKind::Assignment { target, op, value } => {
                let target_ty = self.expr(target, env);
                if !self.assignable(target, env) {
                    self.error_at(target.span, "cannot assign to immutable expression");
                }
                let value_ty = self.expr(value, env);
                if *op == AssignOp::Assign {
                    if !self.compatible_expr(&target_ty, &value_ty, Some(value)) {
                        self.error_at(value.span, "assignment type mismatch");
                    }
                } else if !target_ty.is_numeric() || !value_ty.is_numeric() ||
                    !self.compatible_expr(&target_ty, &value_ty, Some(value))
                {
                    self.error_at(expression.span, "compound assignment requires compatible numeric operands");
                }
                target_ty
            }
            ExprKind::Call { callee, args } => self.call(callee, args, env, expression.span),
            ExprKind::Member { object, name } => self.member(object, name, env, expression.span),
            ExprKind::Postfix { expr, .. } => {
                let ty = self.expr(expr, env);
                if !self.assignable(expr, env) {
                    self.error_at(expr.span, "cannot modify immutable expression");
                }
                if !ty.is_numeric() {
                    self.error_at(expression.span, "increment/decrement requires a numeric value");
                }
                ty
            }
            ExprKind::StructLiteral { name, fields } => self.struct_lit(name, fields, env, expression.span),
            ExprKind::Array(values) => {
                if values.is_empty() {
                    Type::Array { element: Box::new(Type::Unknown), len: 0 }
                } else {
                    let element = self.expr(&values[0], env);
                    for value in &values[1..] {
                        let found = self.expr(value, env);
                        if !self.compatible(&element, &found) {
                            self.error_at(value.span, "array elements must have compatible types");
                        }
                    }
                    Type::Array { element: Box::new(element), len: values.len() }
                }
            }
            ExprKind::Index { object, index } => {
                let object_ty = self.expr(object, env);
                let index_ty = self.expr(index, env);
                if !index_ty.is_integer() {
                    self.error_at(index.span, "array index must be an integer");
                }
                match object_ty {
                    Type::Array { element, .. } => *element,
                    Type::Str => Type::Char,
                    _ => {
                        self.error_at(object.span, "indexing requires an array or str");
                        Type::Unknown
                    }
                }
            }
        }
    }

    fn assignable(&self, expression: &Expr, env: &Env) -> bool {
        match &expression.kind {
            ExprKind::Identifier(name) => env.is_mutable(name),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                matches!(&object.kind, ExprKind::Identifier(name) if env.is_mutable(name))
            }
            _ => false,
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], env: &Env, span: Span) -> Type {
        if let ExprKind::Identifier(name) = &callee.kind {
            if name == "println" || name == "print" {
                for argument in args { self.expr(argument, env); }
                return Type::Unit;
            }
            if name == "typeof" {
                if args.len() != 1 {
                    self.error_at(span, format!("typeof expects 1 argument, found {}", args.len()));
                }
                for argument in args { self.expr(argument, env); }
                return Type::Str;
            }
            if name == "len" {
                if args.len() != 1 {
                    self.error_at(span, format!("len expects 1 argument, found {}", args.len()));
                }
                if let Some(argument) = args.first() {
                    if !matches!(self.expr(argument, env), Type::Array { .. }) {
                        self.error_at(argument.span, "len expects an array");
                    }
                }
                return Type::I32;
            }
            if let Some(signature) = self.functions.get(name).cloned() {
                if signature.source_id != callee.span.source_id && !signature.public {
                    self.error_at(callee.span, format!("function '{}' is private", name));
                }
                return self.signature(&signature, args, env, span);
            }
            self.error_at(callee.span, format!("unknown function '{}'", name));
            return Type::Unknown;
        }

        if let ExprKind::Member { object, name } = &callee.kind {
            let object_ty = self.expr(object, env);
            let type_name = match object_ty {
                Type::Named(name) => name,
                Type::Reference { inner, .. } => match *inner {
                    Type::Named(name) => name,
                    _ => String::new(),
                },
                _ => String::new(),
            };
            if let Some(signature) = self.methods.get(&(type_name.clone(), name.clone())).cloned() {
                if signature.source_id != callee.span.source_id && !signature.public {
                    self.error_at(callee.span, format!("method '{}.{}' is private", type_name, name));
                }
                let mut all = vec![object.as_ref().clone()];
                all.extend_from_slice(args);
                return self.signature(&signature, &all, env, span);
            }
            self.error_at(callee.span, format!("unknown method '{}.{}'", type_name, name));
            return Type::Unknown;
        }

        self.error_at(callee.span, "expression is not callable");
        Type::Unknown
    }

    fn signature(&mut self, signature: &FunctionSig, args: &[Expr], env: &Env, span: Span) -> Type {
        if args.len() != signature.params.len() {
            self.error_at(
                span,
                format!("wrong argument count: expected {}, found {}", signature.params.len(), args.len()),
            );
        }
        for (index, argument) in args.iter().enumerate() {
            let actual = self.expr(argument, env);
            if let Some(expected) = signature.params.get(index) {
                if !self.compatible(expected, &actual) {
                    self.error_at(
                        argument.span,
                        format!(
                            "argument {} type mismatch: expected {}, found {}",
                            index + 1,
                            expected.display_name(),
                            actual.display_name()
                        ),
                    );
                }
            }
        }
        signature.return_type.clone()
    }

    fn member(&mut self, object: &Expr, name: &str, env: &Env, span: Span) -> Type {
        let ty = self.expr(object, env);
        let type_name = match ty {
            Type::Named(name) => name,
            Type::Reference { inner, .. } => match *inner {
                Type::Named(name) => name,
                _ => String::new(),
            },
            _ => String::new(),
        };
        if let Some(structure) = self.structs.get(&type_name).cloned() {
            if structure.source_id != span.source_id && !structure.public {
                self.error_at(span, format!("struct '{}' is private", type_name));
            }
            if let Some(field) = structure.fields.get(name) {
                if field.source_id != span.source_id && !field.public {
                    self.error_at(span, format!("field '{}.{}' is private", type_name, name));
                }
                return field.ty.clone();
            }
        }
        self.error_at(span, format!("unknown member '{}.{}'", type_name, name));
        Type::Unknown
    }

    fn struct_lit(&mut self, name: &str, fields: &[(String, Expr)], env: &Env, span: Span) -> Type {
        let Some(info) = self.structs.get(name).cloned() else {
            self.error_at(span, format!("unknown struct '{}'", name));
            return Type::Unknown;
        };
        if info.source_id != span.source_id && !info.public {
            self.error_at(span, format!("struct '{}' is private", name));
        }
        let mut seen = HashMap::new();
        for (field, expression) in fields {
            let actual = self.expr(expression, env);
            if seen.insert(field, true).is_some() {
                self.error_at(expression.span, format!("duplicate field '{}.{}'", name, field));
            }
            match info.fields.get(field) {
                Some(expected) => {
                    if expected.source_id != expression.span.source_id && !expected.public {
                        self.error_at(expression.span, format!("field '{}.{}' is private", name, field));
                    }
                    if !self.compatible(&expected.ty, &actual) {
                        self.error_at(expression.span, format!("field '{}.{}' type mismatch", name, field));
                    }
                }
                None => self.error_at(expression.span, format!("unknown field '{}.{}'", name, field)),
            }
        }
        for field in info.fields.keys() {
            if !seen.contains_key(field) {
                self.error_at(span, format!("missing field '{}.{}'", name, field));
            }
        }
        Type::Named(name.into())
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

    fn compatible(&self, expected: &Type, actual: &Type) -> bool {
        expected == actual ||
            matches!(expected, Type::Unknown) ||
            matches!(actual, Type::Unknown) ||
            matches!(expected, Type::Reference { inner, .. } if inner.as_ref() == actual)
    }

    fn compatible_expr(&self, expected: &Type, actual: &Type, expr: Option<&Expr>) -> bool {
        let literal = match expr.map(|expr| &expr.kind) {
            Some(ExprKind::Literal(Literal::Number(number))) => Some((number, false)),
            Some(ExprKind::Unary { op: UnaryOp::Minus, expr }) => match &expr.kind {
                ExprKind::Literal(Literal::Number(number)) => Some((number, true)),
                _ => None,
            },
            _ => None,
        };

        if let Some((number, negative)) = literal {
            if expected.is_integer() && !self.literal_fits_integer(number, negative, expected) {
                return false;
            }
            if self.compatible(expected, actual) {
                return true;
            }
            return matches!(actual, Type::I32) && !number.chars().any(|c| c.is_ascii_alphabetic());
        }
        self.compatible(expected, actual)
    }

    fn literal_fits_integer(&self, number: &str, negative: bool, ty: &Type) -> bool {
        let suffix_at = number.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(number.len());
        let digits = &number[..suffix_at];
        if digits.is_empty() || digits.contains('.') { return false; }

        fn decimal_leq(value: &str, max: &str) -> bool {
            let value = value.trim_start_matches('0');
            let value = if value.is_empty() { "0" } else { value };
            value.len() < max.len() || (value.len() == max.len() && value <= max)
        }

        match ty {
            Type::I8 => decimal_leq(digits, if negative { "128" } else { "127" }),
            Type::I16 => decimal_leq(digits, if negative { "32768" } else { "32767" }),
            Type::I32 => decimal_leq(digits, if negative { "2147483648" } else { "2147483647" }),
            Type::I64 => decimal_leq(digits, if negative { "9223372036854775808" } else { "9223372036854775807" }),
            Type::I128 => decimal_leq(digits, if negative {
                "170141183460469231731687303715884105728"
            } else {
                "170141183460469231731687303715884105727"
            }),
            Type::I256 => decimal_leq(digits, if negative {
                "57896044618658097711785492504343953926634992332820282019728792003956564819968"
            } else {
                "57896044618658097711785492504343953926634992332820282019728792003956564819967"
            }),
            Type::U8 => !negative && decimal_leq(digits, "255"),
            Type::U16 => !negative && decimal_leq(digits, "65535"),
            Type::U32 => !negative && decimal_leq(digits, "4294967295"),
            Type::U64 => !negative && decimal_leq(digits, "18446744073709551615"),
            Type::U128 => !negative && decimal_leq(digits, "340282366920938463463374607431768211455"),
            Type::U256 => !negative && decimal_leq(
                digits,
                "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            ),
            _ => true,
        }
    }

    fn compatible_exprs(&self, left: &Type, left_expr: &Expr, right: &Type, right_expr: &Expr) -> bool {
        self.compatible_expr(left, right, Some(right_expr)) ||
            self.compatible_expr(right, left, Some(left_expr))
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
    fn types() {
        assert!(check("fn main(){let x:i32=10 let y:u64=20u64}").is_ok());
        assert!(check("fn main(){let x:i32=true}").is_err());
    }

    #[test]
    fn integer_literal_must_fit_declared_type() {
        assert!(check("fn main(){let x:i8=127}").is_ok());
        assert!(check("fn main(){let x:i8=128}").is_err());
    }

    #[test]
    fn names() {
        let error = check("fn main(){println(missing)}").unwrap_err().remove(0);
        assert_eq!(error.span.line, 1);
        assert!(error.span.column > 1);
    }

    #[test]
    fn mutability() {
        assert!(check("fn main(){let x=1 x=2}").is_err());
        assert!(check("fn main(){let mut x=1 x+=2 x++}").is_ok());
    }

    #[test]
    fn else_if() {
        assert!(check("fn main(){if true{}else if false{}else{}}").is_ok());
    }

    #[test]
    fn visibility_uses_source_ids() {
        let source = "pub fn visible(){}\nfn hidden(){}\nfn local(){hidden()}";
        let tokens = Lexer::with_source_id(source, 1).tokenize().unwrap();
        let imported = Parser::new(tokens).parse().unwrap();

        let caller_source = "fn main(){visible() hidden()}";
        let tokens = Lexer::with_source_id(caller_source, 0).tokenize().unwrap();
        let caller = Parser::new(tokens).parse().unwrap();

        let mut items = imported.items;
        items.extend(caller.items);
        let errors = SemanticAnalyzer::check(&Program { items }).unwrap_err();
        assert!(errors.iter().any(|error| error.message.contains("hidden") && error.message.contains("private")));
        assert!(!errors.iter().any(|error| error.message.contains("visible") && error.message.contains("private")));
    }

    #[test]
    fn i256_range_is_checked_without_u128() {
        assert!(check("fn main(){let x:i256=57896044618658097711785492504343953926634992332820282019728792003956564819967i256}").is_ok());
        assert!(check("fn main(){let x:i256=57896044618658097711785492504343953926634992332820282019728792003956564819968i256}").is_err());
    }
}
