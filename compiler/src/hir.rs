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
    pub enums: Vec<HirEnum>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirEnum {
    pub name: String,
    pub variants: Vec<HirEnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirEnumVariant {
    pub name: String,
    pub payload: Vec<Type>,
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
    Break,
    Continue,
    If { condition: HirExpr, then_branch: HirBlock, else_branch: Option<Box<HirStmt>> },
    While { condition: HirExpr, body: HirBlock },
    Loop { body: HirBlock },
    DoWhile { body: HirBlock, condition: HirExpr },
    For { initializer: Option<Box<HirStmt>>, condition: Option<HirExpr>, update: Option<HirExpr>, body: HirBlock },
    Match { value: HirExpr, arms: Vec<HirMatchArm> },
    Block(HirBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirMatchArm {
    pub discriminant: usize,
    pub bindings: Vec<HirMatchBinding>,
    pub body: HirBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirMatchBinding {
    pub local: LocalId,
    pub field_index: usize,
    pub ty: Type,
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
    EnumVariant { name: String, discriminant: usize, values: Vec<HirExpr> },
    Function(FunctionId),
}

#[derive(Debug)]
pub struct HirLowerer {
    next_function: FunctionId,
    next_local: LocalId,
    functions: Vec<HirFunction>,
    structs: Vec<HirStruct>,
    enums: Vec<HirEnum>,
    enum_variants: HashMap<(usize, String), Vec<(String, Vec<Type>)>>,
    scopes: Vec<HashMap<String, LocalId>>,
    local_types: HashMap<LocalId, Type>,
    function_ids: HashMap<(usize, String), FunctionId>,
    function_returns: HashMap<FunctionId, Type>,
    method_ids: HashMap<(usize, String, String), FunctionId>,
    imports: HashMap<usize, Vec<(Option<String>, usize)>>,
}

impl HirLowerer {
    pub fn lower(program: &ast::Program) -> HirProgram {
        let mut lowerer = Self {
            next_function: 0,
            next_local: 0,
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            enum_variants: HashMap::new(),
            scopes: Vec::new(),
            local_types: HashMap::new(),
            function_ids: HashMap::new(),
            function_returns: HashMap::new(),
            method_ids: HashMap::new(),
            imports: HashMap::new(),
        };

        for item in &program.items {
            if let ast::Item::Import(import) = item {
                if let Some(target_source_id) = import.target_source_id {
                    lowerer.imports
                        .entry(import.span.source_id)
                        .or_default()
                        .push((import.alias.clone(), target_source_id));
                }
            }
        }

        for item in &program.items {
            if let ast::Item::Enum(definition) = item {
                lowerer.enum_variants.insert(
                    (definition.span.source_id, definition.name.clone()),
                    definition.variants.iter().map(|variant| {
                        (
                            variant.name.clone(),
                            variant.payload.iter().map(Type::from_ref).collect(),
                        )
                    }).collect(),
                );
            }
        }

        for item in &program.items {
            match item {
                ast::Item::Function(function) => {
                    let id = lowerer.next_function;
                    lowerer.function_ids.insert(
                        (function.span.source_id, function.name.clone()),
                        id,
                    );
                    lowerer.function_returns.insert(
                        id,
                        function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                    );
                    lowerer.next_function += 1;
                }
                ast::Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        let id = lowerer.next_function;
                        lowerer.function_returns.insert(
                            id,
                            function.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit),
                        );
                        lowerer.method_ids.insert(
                            (
                                implementation.span.source_id,
                                implementation.type_name.clone(),
                                function.name.clone(),
                            ),
                            id,
                        );
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
                ast::Item::Enum(e) => lowerer.enums.push(HirEnum {
                    name: e.name.clone(),
                    variants: e.variants.iter().map(|variant| HirEnumVariant {
                        name: variant.name.clone(),
                        payload: variant.payload.iter().map(Type::from_ref).collect(),
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
            enums: lowerer.enums,
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
        let lowered_name = if function.span.source_id == 0 {
            function.name.clone()
        } else {
            format!("m{}_{}", function.span.source_id, function.name)
        };
        self.functions.push(HirFunction {
            id,
            name: lowered_name,
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
            ast::StmtKind::Break => HirStmt::Break,
            ast::StmtKind::Continue => HirStmt::Continue,
            ast::StmtKind::If { condition, then_branch, else_branch } => HirStmt::If {
                condition: self.lower_expr(condition),
                then_branch: self.lower_block(then_branch),
                else_branch: else_branch.as_ref().map(|s| Box::new(self.lower_stmt(s, &mut Vec::new()))),
            },
            ast::StmtKind::While { condition, body } => HirStmt::While {
                condition: self.lower_expr(condition),
                body: self.lower_block(body),
            },
            ast::StmtKind::Loop { body } => HirStmt::Loop {
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
            ast::StmtKind::Match { value, arms } => HirStmt::Match {
                value: self.lower_expr(value),
                arms: arms.iter().map(|arm| {
                    let (discriminant, payload) = self.resolve_enum_variant(
                        arm.span.source_id,
                        &arm.enum_name,
                        &arm.variant,
                    ).unwrap_or((0, Vec::new()));
                    self.scopes.push(HashMap::new());
                    let mut bindings = Vec::new();
                    let mut body = HirBlock { locals: Vec::new(), statements: Vec::new() };
                    for (field_index, binding) in arm.bindings.iter().enumerate() {
                        if binding == "_" { continue; }
                        let local = self.new_local();
                        let ty = payload.get(field_index).cloned().unwrap_or(Type::Unknown);
                        self.scopes.last_mut().unwrap().insert(binding.clone(), local);
                        self.local_types.insert(local, ty.clone());
                        body.locals.push(HirLocal {
                            id: local,
                            name: binding.clone(),
                            ty: ty.clone(),
                            mutable: false,
                        });
                        bindings.push(HirMatchBinding { local, field_index, ty });
                    }
                    for statement in &arm.body.statements {
                        body.statements.push(self.lower_stmt(statement, &mut body.locals));
                    }
                    self.scopes.pop();
                    HirMatchArm { discriminant, bindings, body }
                }).collect(),
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
                } else if let Some(id) = self.resolve_function_id(expr.span.source_id, name) {
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
                if let ast::ExprKind::Member { object, name } = &callee.kind {
                    if let ast::ExprKind::Identifier(enum_name) = &object.kind {
                        if self.namespace_target(callee.span.source_id, enum_name).is_none() {
                            if let Some((discriminant, _payload)) =
                                self.resolve_enum_variant(callee.span.source_id, enum_name, name)
                            {
                                return HirExpr {
                                    ty: Type::Named(enum_name.clone()),
                                    kind: HirExprKind::EnumVariant {
                                        name: enum_name.clone(),
                                        discriminant,
                                        values: args.iter().map(|argument| self.lower_expr(argument)).collect(),
                                    },
                                };
                            }
                        }
                    }
                }

                let (callee, args) = if let ast::ExprKind::Member { object, name } = &callee.kind {
                    if let ast::ExprKind::Identifier(namespace) = &object.kind {
                        if let Some(target_source_id) =
                            self.namespace_target(callee.span.source_id, namespace)
                        {
                            if let Some(id) = self.function_ids
                                .get(&(target_source_id, name.clone()))
                                .copied()
                            {
                                (
                                    HirExpr { ty: Type::Unknown, kind: HirExprKind::Function(id) },
                                    args.iter().map(|argument| self.lower_expr(argument)).collect(),
                                )
                            } else {
                                (
                                    self.lower_expr(callee),
                                    args.iter().map(|argument| self.lower_expr(argument)).collect(),
                                )
                            }
                        } else {
                            self.lower_method_call(callee.span.source_id, object, name, args, callee)
                        }
                    } else {
                        self.lower_method_call(callee.span.source_id, object, name, args, callee)
                    }
                } else {
                    (self.lower_expr(callee), args.iter().map(|argument| self.lower_expr(argument)).collect())
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
                if let ast::ExprKind::Identifier(enum_name) = &object.kind {
                    if let Some((discriminant, payload)) =
                        self.resolve_enum_variant(expr.span.source_id, enum_name, name)
                    {
                        if payload.is_empty() {
                            return HirExpr {
                                ty: Type::Named(enum_name.clone()),
                                kind: HirExprKind::EnumVariant {
                                    name: enum_name.clone(),
                                    discriminant,
                                    values: Vec::new(),
                                },
                            };
                        }
                    }
                }
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

    fn namespace_target(&self, source_id: usize, alias: &str) -> Option<usize> {
        self.imports.get(&source_id)
            .and_then(|imports| imports.iter()
                .find(|(candidate, _)| candidate.as_deref() == Some(alias))
                .map(|(_, target)| *target))
    }

    fn resolve_function_id(&self, source_id: usize, name: &str) -> Option<FunctionId> {
        if let Some(id) = self.function_ids.get(&(source_id, name.to_string())).copied() {
            return Some(id);
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .find_map(|(_, target)| self.function_ids.get(&(*target, name.to_string())).copied())
    }

    fn resolve_enum_variant(&self, source_id: usize, enum_name: &str, variant: &str) -> Option<(usize, Vec<Type>)> {
        if let Some(variants) = self.enum_variants.get(&(source_id, enum_name.to_string())) {
            if let Some((index, (_, payload))) = variants.iter().enumerate()
                .find(|(_, (candidate, _))| candidate == variant)
            {
                return Some((index, payload.clone()));
            }
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .find_map(|(_, target)| self.enum_variants
                .get(&(*target, enum_name.to_string()))
                .and_then(|variants| variants.iter().enumerate()
                    .find(|(_, (candidate, _))| candidate == variant)
                    .map(|(index, (_, payload))| (index, payload.clone()))))
    }

    fn resolve_method_id(&self, source_id: usize, type_name: &str, name: &str) -> Option<FunctionId> {
        if let Some(id) = self.method_ids
            .get(&(source_id, type_name.to_string(), name.to_string()))
            .copied()
        {
            return Some(id);
        }
        self.imports.get(&source_id)
            .into_iter()
            .flatten()
            .filter(|(alias, _)| alias.is_none())
            .find_map(|(_, target)| self.method_ids
                .get(&(*target, type_name.to_string(), name.to_string()))
                .copied())
    }

    fn lower_method_call(
        &mut self,
        source_id: usize,
        object: &ast::Expr,
        name: &str,
        args: &[ast::Expr],
        original_callee: &ast::Expr,
    ) -> (HirExpr, Vec<HirExpr>) {
        let object = self.lower_expr(object);
        if let Type::Named(type_name) = &object.ty {
            if let Some(id) = self.resolve_method_id(source_id, type_name, name) {
                let self_ref = HirExpr {
                    ty: Type::Reference { mutable: false, inner: Box::new(object.ty.clone()) },
                    kind: HirExprKind::Unary {
                        op: UnaryOp::BorrowShared,
                        expr: Box::new(object),
                    },
                };
                let mut method_args = vec![self_ref];
                method_args.extend(args.iter().map(|argument| self.lower_expr(argument)));
                return (
                    HirExpr { ty: Type::Unknown, kind: HirExprKind::Function(id) },
                    method_args,
                );
            }
        }

        (
            self.lower_expr(original_callee),
            args.iter().map(|argument| self.lower_expr(argument)).collect(),
        )
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
