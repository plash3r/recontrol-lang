use crate::hir::{HirExpr, HirExprKind, HirFunction, HirProgram, HirStmt, LocalId, BUILTIN_LEN_ID, BUILTIN_TYPEOF_ID};
use crate::ast::{AssignOp, BinaryOp, Literal, PostfixOp, UnaryOp};
use crate::types::Type;

pub type BasicBlockId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub structs: Vec<MirStruct>,
    pub enums: Vec<MirEnum>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirEnum {
    pub name: String,
    pub variants: Vec<MirEnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirEnumVariant {
    pub name: String,
    pub payload: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirStruct {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub name: String,
    pub param_count: usize,
    pub locals: Vec<MirLocal>,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirLocal {
    pub id: LocalId,
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub statements: Vec<MirStatement>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirStatement {
    StorageLive(LocalId),
    StorageDead(LocalId),
    Assign { place: Place, rvalue: Rvalue },
    Evaluate(Rvalue),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Local(LocalId),
    Field { base: Box<Place>, name: String },
    Index { base: Box<Place>, index: Box<Operand> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rvalue {
    Use(Operand),
    Unary { op: UnaryOp, operand: Operand },
    Binary { left: Operand, op: BinaryOp, right: Operand },
    Ref { mutable: bool, place: Place },
    Call { callee: Operand, args: Vec<Operand> },
    Aggregate { name: String, fields: Vec<(String, Operand)> },
    EnumVariant { name: String, discriminant: usize, values: Vec<Operand> },
    EnumTag { operand: Operand },
    EnumPayload { operand: Operand, discriminant: usize, field_index: usize },
    Array(Vec<Operand>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    Constant(Literal),
    Function(crate::hir::FunctionId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    Goto(BasicBlockId),
    SwitchBool { condition: Operand, then_block: BasicBlockId, else_block: BasicBlockId },
    Return(Option<Rvalue>),
    Unreachable,
}

struct Builder {
    blocks: Vec<BasicBlock>,
    current: BasicBlockId,
    loop_targets: Vec<(BasicBlockId, BasicBlockId)>,
}

impl Builder {
    fn new() -> Self {
        Self {
            blocks: vec![BasicBlock {
                id: 0,
                statements: Vec::new(),
                terminator: Terminator::Unreachable,
            }],
            current: 0,
            loop_targets: Vec::new(),
        }
    }

    fn statement(&mut self, statement: MirStatement) {
        self.blocks[self.current].statements.push(statement);
    }

    fn finish_block(&mut self, terminator: Terminator) {
        self.blocks[self.current].terminator = terminator;
    }

    fn new_block(&mut self) -> BasicBlockId {
        let id = self.blocks.len();
        self.blocks.push(BasicBlock {
            id,
            statements: Vec::new(),
            terminator: Terminator::Unreachable,
        });
        id
    }

    fn switch_to(&mut self, block: BasicBlockId) {
        self.current = block;
    }
}

pub struct MirLowerer;

impl MirLowerer {
    pub fn lower(program: &HirProgram) -> MirProgram {
        MirProgram {
            functions: program.functions.iter().map(Self::lower_function).collect(),
            structs: program.structs.iter().map(|structure| MirStruct {
                name: structure.name.clone(),
                fields: structure.fields.iter().map(|field| (field.name.clone(), field.ty.clone())).collect(),
            }).collect(),
            enums: program.enums.iter().map(|definition| MirEnum {
                name: definition.name.clone(),
                variants: definition.variants.iter().map(|variant| MirEnumVariant {
                    name: variant.name.clone(),
                    payload: variant.payload.clone(),
                }).collect(),
            }).collect(),
        }
    }

    fn finalize_temporaries(function: &mut MirFunction, temp_start: usize) {
        let mut dead_at_entry = std::collections::HashMap::<BasicBlockId, Vec<LocalId>>::new();

        let mut referenced_in_blocks = std::collections::HashMap::<LocalId, std::collections::HashSet<BasicBlockId>>::new();
        for block in &function.blocks {
            let mut referenced = std::collections::HashSet::<LocalId>::new();
            for statement in &block.statements {
                match statement {
                    MirStatement::StorageLive(_) | MirStatement::StorageDead(_) => {}
                    MirStatement::Assign { place, rvalue } => {
                        referenced.extend(Self::place_locals(place));
                        referenced.extend(Self::rvalue_locals(rvalue));
                    }
                    MirStatement::Evaluate(rvalue) => referenced.extend(Self::rvalue_locals(rvalue)),
                }
            }
            match &block.terminator {
                Terminator::SwitchBool { condition, .. } => referenced.extend(Self::operand_locals(condition)),
                Terminator::Return(Some(value)) => referenced.extend(Self::rvalue_locals(value)),
                Terminator::Goto(_) | Terminator::Return(None) | Terminator::Unreachable => {}
            }
            for local in referenced {
                referenced_in_blocks.entry(local).or_default().insert(block.id);
            }
        }

        let crosses_block = |local: LocalId, defining_block: BasicBlockId| {
            referenced_in_blocks
                .get(&local)
                .map(|blocks| blocks.iter().any(|block| *block != defining_block))
                .unwrap_or(false)
        };

        for block in &function.blocks {
            let mut temps = std::collections::HashSet::<LocalId>::new();
            for statement in &block.statements {
                match statement {
                    MirStatement::StorageLive(id) if *id >= temp_start => { temps.insert(*id); }
                    _ => {}
                }
            }

            // Temporaries used by ordinary statements die immediately after their
            // last statement use. We currently keep this conservative: a temp that
            // is live in a block but not used by its terminator can die at block end.
            let terminator_temps = match &block.terminator {
                Terminator::SwitchBool { condition, .. } => Self::operand_locals(condition),
                Terminator::Return(Some(value)) => Self::rvalue_locals(value),
                _ => std::collections::HashSet::new(),
            };

            for temp in temps {
                if terminator_temps.contains(&temp) && !crosses_block(temp, block.id) {
                    // The value is consumed by the terminator. It cannot be dead
                    // before the terminator, so release it at every successor.
                    for successor in match &block.terminator {
                        Terminator::Goto(t) => vec![*t],
                        Terminator::SwitchBool { then_block, else_block, .. } => vec![*then_block, *else_block],
                        _ => Vec::new(),
                    } {
                        dead_at_entry.entry(successor).or_default().push(temp);
                    }
                }
            }
        }

        let mut dead_before_terminator = std::collections::HashMap::<BasicBlockId, Vec<LocalId>>::new();
        for block in &function.blocks {
            let terminator_temps = match &block.terminator {
                Terminator::SwitchBool { condition, .. } => Self::operand_locals(condition),
                Terminator::Return(Some(value)) => Self::rvalue_locals(value),
                _ => std::collections::HashSet::new(),
            };
            for statement in &block.statements {
                if let MirStatement::StorageLive(id) = statement {
                    if *id >= temp_start && !terminator_temps.contains(id) && !crosses_block(*id, block.id) {
                        dead_before_terminator.entry(block.id).or_default().push(*id);
                    }
                }
            }
        }
        for (block_id, mut locals) in dead_before_terminator {
            locals.sort_unstable();
            locals.dedup();
            if let Some(block) = function.blocks.iter_mut().find(|b| b.id == block_id) {
                for id in locals {
                    block.statements.push(MirStatement::StorageDead(id));
                }
            }
        }

        for (block_id, mut locals) in dead_at_entry {
            locals.sort_unstable();
            locals.dedup();
            if let Some(block) = function.blocks.get_mut(block_id) {
                let mut prefix = locals.into_iter().map(MirStatement::StorageDead).collect::<Vec<_>>();
                prefix.append(&mut block.statements);
                block.statements = prefix;
            }
        }
    }

    fn operand_locals(operand: &Operand) -> std::collections::HashSet<LocalId> {
        match operand {
            Operand::Copy(Place::Local(id)) | Operand::Move(Place::Local(id)) => [*id].into_iter().collect(),
            Operand::Copy(place) | Operand::Move(place) => Self::place_locals(place),
            Operand::Constant(_) | Operand::Function(_) => std::collections::HashSet::new(),
        }
    }

    fn place_locals(place: &Place) -> std::collections::HashSet<LocalId> {
        match place {
            Place::Local(id) => [*id].into_iter().collect(),
            Place::Field { base, .. } => Self::place_locals(base),
            Place::Index { base, index } => {
                let mut result = Self::place_locals(base);
                result.extend(Self::operand_locals(index));
                result
            }
        }
    }

    fn rvalue_locals(value: &Rvalue) -> std::collections::HashSet<LocalId> {
        match value {
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => Self::operand_locals(op),
            Rvalue::Binary { left, right, .. } => {
                let mut result = Self::operand_locals(left);
                result.extend(Self::operand_locals(right));
                result
            }
            Rvalue::Ref { place, .. } => Self::place_locals(place),
            Rvalue::Call { callee, args } => {
                let mut result = Self::operand_locals(callee);
                for arg in args { result.extend(Self::operand_locals(arg)); }
                result
            }
            Rvalue::Aggregate { fields, .. } => {
                let mut result = std::collections::HashSet::new();
                for (_, op) in fields { result.extend(Self::operand_locals(op)); }
                result
            }
            Rvalue::EnumVariant { values, .. } => {
                let mut result = std::collections::HashSet::new();
                for operand in values { result.extend(Self::operand_locals(operand)); }
                result
            }
            Rvalue::EnumTag { operand } | Rvalue::EnumPayload { operand, .. } => Self::operand_locals(operand),
            Rvalue::Array(values) => {
                let mut result = std::collections::HashSet::new();
                for op in values { result.extend(Self::operand_locals(op)); }
                result
            }
        }
    }

    fn lower_function(function: &HirFunction) -> MirFunction {
        let mut locals = Vec::new();
        for param in &function.params {
            locals.push(MirLocal {
                id: param.local,
                ty: param.ty.clone(),
                mutable: matches!(param.ty, Type::Reference { mutable: true, .. }),
            });
        }
        let mut all_locals = Vec::new();
        Self::collect_block_locals(&function.body, &mut all_locals);
        for local in &all_locals {
            if !locals.iter().any(|existing: &MirLocal| existing.id == local.id) {
                locals.push(MirLocal {
                    id: local.id,
                    ty: local.ty.clone(),
                    mutable: local.mutable,
                });
            }
        }

        let temp_start = locals.iter().map(|local| local.id).max().map(|id| id + 1).unwrap_or(0);
        let mut builder = Builder::new();

        // Parameters are live for the whole function body.
        for param in &function.params {
            builder.statement(MirStatement::StorageLive(param.local));
        }

        Self::lower_block(&mut builder, &function.body, &mut locals);
        if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
            builder.finish_block(Terminator::Return(None));
        }

        let mut result = MirFunction {
            name: function.name.clone(),
            param_count: function.params.len(),
            locals,
            blocks: builder.blocks,
        };
        Self::finalize_temporaries(&mut result, temp_start);
        result
    }

    fn collect_block_locals(block: &crate::hir::HirBlock, locals: &mut Vec<crate::hir::HirLocal>) {
        locals.extend(block.locals.iter().cloned());
        for statement in &block.statements {
            Self::collect_stmt_locals(statement, locals);
        }
    }

    fn collect_stmt_locals(statement: &HirStmt, locals: &mut Vec<crate::hir::HirLocal>) {
        match statement {
            HirStmt::If { then_branch, else_branch, .. } => {
                Self::collect_block_locals(then_branch, locals);
                if let Some(else_branch) = else_branch {
                    Self::collect_stmt_locals(else_branch, locals);
                }
            }
            HirStmt::While { body, .. } | HirStmt::DoWhile { body, .. } => Self::collect_block_locals(body, locals),
            HirStmt::For { initializer, body, .. } => {
                if let Some(initializer) = initializer {
                    Self::collect_stmt_locals(initializer, locals);
                }
                Self::collect_block_locals(body, locals);
            }
            HirStmt::Match { arms, .. } => {
                for arm in arms {
                    Self::collect_block_locals(&arm.body, locals);
                }
            }
            HirStmt::Block(block) => Self::collect_block_locals(block, locals),
            HirStmt::Let { .. } | HirStmt::Expr(_) | HirStmt::Return(_) |
            HirStmt::Break | HirStmt::Continue => {}
        }
    }

    fn lower_block(builder: &mut Builder, block: &crate::hir::HirBlock, locals: &mut Vec<MirLocal>) {
        for stmt in &block.statements {
            Self::lower_stmt(builder, stmt, locals);
        }
    }

    fn new_temp(locals: &mut Vec<MirLocal>, ty: Type) -> LocalId {
        let id = locals.iter().map(|local| local.id).max().map(|id| id + 1).unwrap_or(0);
        locals.push(MirLocal { id, ty, mutable: true });
        id
    }

    fn lower_operand(builder: &mut Builder, locals: &mut Vec<MirLocal>, expr: &HirExpr) -> Operand {
        match &expr.kind {
            HirExprKind::Literal(literal) => Operand::Constant(literal.clone()),
            HirExprKind::Local(local) => {
                if expr.ty.is_copy() {
                    Operand::Copy(Place::Local(*local))
                } else {
                    Operand::Move(Place::Local(*local))
                }
            }
            HirExprKind::Function(id) => Operand::Function(*id),
            HirExprKind::Binary { left, op: op @ (BinaryOp::And | BinaryOp::Or), right } => {
                Self::lower_short_circuit(builder, locals, left, *op, right)
            }
            _ => {
                let temp = Self::new_temp(locals, expr.ty.clone());
                builder.statement(MirStatement::StorageLive(temp));
                let rvalue = Self::lower_rvalue(builder, locals, expr);
                builder.statement(MirStatement::Assign {
                    place: Place::Local(temp),
                    rvalue,
                });
                    if expr.ty.is_copy() {
                        Operand::Copy(Place::Local(temp))
                    } else {
                        Operand::Move(Place::Local(temp))
                    }
            }
        }
    }

    fn lower_short_circuit(
        builder: &mut Builder,
        locals: &mut Vec<MirLocal>,
        left: &HirExpr,
        op: BinaryOp,
        right: &HirExpr,
    ) -> Operand {
        debug_assert!(matches!(op, BinaryOp::And | BinaryOp::Or));

        let result = Self::new_temp(locals, Type::Bool);
        builder.statement(MirStatement::StorageLive(result));

        let left = Self::lower_operand(builder, locals, left);
        let rhs_block = builder.new_block();
        let short_block = builder.new_block();
        let join_block = builder.new_block();

        let (then_block, else_block, short_value) = match op {
            BinaryOp::And => (rhs_block, short_block, false),
            BinaryOp::Or => (short_block, rhs_block, true),
            _ => unreachable!(),
        };

        builder.finish_block(Terminator::SwitchBool {
            condition: left,
            then_block,
            else_block,
        });

        builder.switch_to(short_block);
        builder.statement(MirStatement::Assign {
            place: Place::Local(result),
            rvalue: Rvalue::Use(Operand::Constant(Literal::Bool(short_value))),
        });
        builder.finish_block(Terminator::Goto(join_block));

        builder.switch_to(rhs_block);
        let right = Self::lower_operand(builder, locals, right);
        builder.statement(MirStatement::Assign {
            place: Place::Local(result),
            rvalue: Rvalue::Use(right),
        });
        builder.finish_block(Terminator::Goto(join_block));

        builder.switch_to(join_block);
        Operand::Copy(Place::Local(result))
    }

    fn lower_rvalue(builder: &mut Builder, locals: &mut Vec<MirLocal>, expr: &HirExpr) -> Rvalue {
        match &expr.kind {
            HirExprKind::Literal(literal) => Rvalue::Use(Operand::Constant(literal.clone())),
            HirExprKind::Local(local) => {
                if expr.ty.is_copy() {
                    Rvalue::Use(Operand::Copy(Place::Local(*local)))
                } else {
                    Rvalue::Use(Operand::Move(Place::Local(*local)))
                }
            }
            HirExprKind::Unary { op, expr } => {
                if matches!(op, UnaryOp::BorrowShared | UnaryOp::BorrowMutable) {
                    if let HirExprKind::Local(local) = expr.kind {
                        return Rvalue::Ref {
                            mutable: *op == UnaryOp::BorrowMutable,
                            place: Place::Local(local),
                        };
                    }
                }
                Rvalue::Unary {
                    op: *op,
                    operand: Self::lower_operand(builder, locals, expr),
                }
            }
            HirExprKind::Binary { left, op: op @ (BinaryOp::And | BinaryOp::Or), right } => {
                Rvalue::Use(Self::lower_short_circuit(builder, locals, left, *op, right))
            }
            HirExprKind::Binary { left, op, right } => Rvalue::Binary {
                left: Self::lower_operand(builder, locals, left),
                op: *op,
                right: Self::lower_operand(builder, locals, right),
            },
            HirExprKind::Assignment { target, op, value } => Self::lower_assignment_rvalue(builder, locals, target, *op, value),
            HirExprKind::Call { callee, args } => {
                let inspection_call = matches!(&callee.kind, HirExprKind::Function(id) if *id == BUILTIN_TYPEOF_ID || *id == BUILTIN_LEN_ID);
                Rvalue::Call {
                    callee: Self::lower_operand(builder, locals, callee),
                    args: args.iter().map(|arg| {
                        if inspection_call {
                            if let HirExprKind::Local(local) = arg.kind {
                                return Operand::Copy(Place::Local(local));
                            }
                        }
                        Self::lower_operand(builder, locals, arg)
                    }).collect(),
                }
            }
            HirExprKind::Member { object, name } => {
                let place = Self::lower_place(builder, locals, object);
                Rvalue::Use(Operand::Copy(Place::Field {
                    base: Box::new(place),
                    name: name.clone(),
                }))
            }
            HirExprKind::Postfix { expr, .. } => Rvalue::Use(Self::lower_operand(builder, locals, expr)),
            HirExprKind::StructLiteral { name, fields } => Rvalue::Aggregate {
                name: name.clone(),
                fields: fields.iter().map(|(n, e)| (n.clone(), Self::lower_operand(builder, locals, e))).collect(),
            },
            HirExprKind::Array(values) => Rvalue::Array(values.iter().map(|e| Self::lower_operand(builder, locals, e)).collect()),
            HirExprKind::Index { object, index } => Rvalue::Use(Operand::Copy(Place::Index {
                base: Box::new(Self::lower_place(builder, locals, object)),
                index: Box::new(Self::lower_operand(builder, locals, index)),
            })),
            HirExprKind::EnumVariant { name, discriminant, values } => Rvalue::EnumVariant {
                name: name.clone(),
                discriminant: *discriminant,
                values: values.iter().map(|value| Self::lower_operand(builder, locals, value)).collect(),
            },
            HirExprKind::Function(id) => Rvalue::Use(Operand::Function(*id)),
        }
    }

    fn lower_stmt(builder: &mut Builder, stmt: &HirStmt, locals: &mut Vec<MirLocal>) {
        match stmt {
            HirStmt::Let { local, initializer } => {
                builder.statement(MirStatement::StorageLive(*local));
                if let Some(value) = initializer {
                    let rvalue = Self::lower_rvalue(builder, locals, value);
                    builder.statement(MirStatement::Assign {
                        place: Place::Local(*local),
                        rvalue,
                    });
                }
            }
            HirStmt::Expr(expr) => {
                if let HirExprKind::Assignment { target, op, value } = &expr.kind {
                    let rvalue = Self::lower_assignment_rvalue(builder, locals, target, *op, value);
                    let place = Self::lower_place(builder, locals, target);
                    builder.statement(MirStatement::Assign { place, rvalue });
                } else if let HirExprKind::Postfix { expr: target, op } = &expr.kind {
                    let rvalue = Self::lower_postfix_rvalue(builder, locals, target, *op);
                    let place = Self::lower_place(builder, locals, target);
                    builder.statement(MirStatement::Assign { place, rvalue });
                } else {
                    let rvalue = Self::lower_rvalue(builder, locals, expr);
                    builder.statement(MirStatement::Evaluate(rvalue));
                }
            }
            HirStmt::Return(expr) => {
                let value = expr.as_ref().map(|expr| Self::lower_rvalue(builder, locals, expr));
                builder.finish_block(Terminator::Return(value));
                let next = builder.new_block();
                builder.switch_to(next);
            }
            HirStmt::Break => {
                let target = builder.loop_targets.last().map(|(_, break_target)| *break_target)
                    .expect("semantic analysis guarantees break is inside a loop");
                builder.finish_block(Terminator::Goto(target));
                let next = builder.new_block();
                builder.switch_to(next);
            }
            HirStmt::Continue => {
                let target = builder.loop_targets.last().map(|(continue_target, _)| *continue_target)
                    .expect("semantic analysis guarantees continue is inside a loop");
                builder.finish_block(Terminator::Goto(target));
                let next = builder.new_block();
                builder.switch_to(next);
            }
            HirStmt::Block(block) => Self::lower_block(builder, block, locals),
            HirStmt::If { condition, then_branch, else_branch } => {
                let then_block = builder.new_block();
                let else_block = builder.new_block();
                let join_block = builder.new_block();
                let condition = Self::lower_operand(builder, locals, condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block,
                    else_block,
                });

                builder.switch_to(then_block);
                Self::lower_block(builder, then_branch, locals);
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(join_block));
                }

                builder.switch_to(else_block);
                if let Some(stmt) = else_branch {
                    Self::lower_stmt(builder, stmt, locals);
                }
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(join_block));
                }

                builder.switch_to(join_block);
            }
            HirStmt::While { condition, body } => {
                let head = builder.new_block();
                let body_block = builder.new_block();
                let exit = builder.new_block();
                builder.finish_block(Terminator::Goto(head));

                builder.switch_to(head);
                let condition = Self::lower_operand(builder, locals, condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block: body_block,
                    else_block: exit,
                });

                builder.switch_to(body_block);
                builder.loop_targets.push((head, exit));
                Self::lower_block(builder, body, locals);
                builder.loop_targets.pop();
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(head));
                }
                builder.switch_to(exit);
            }
            HirStmt::DoWhile { body, condition } => {
                let body_block = builder.new_block();
                let condition_block = builder.new_block();
                let exit = builder.new_block();
                builder.finish_block(Terminator::Goto(body_block));

                builder.switch_to(body_block);
                builder.loop_targets.push((condition_block, exit));
                Self::lower_block(builder, body, locals);
                builder.loop_targets.pop();
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(condition_block));
                }

                builder.switch_to(condition_block);
                let condition = Self::lower_operand(builder, locals, condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block: body_block,
                    else_block: exit,
                });
                builder.switch_to(exit);
            }
            HirStmt::Match { value, arms } => {
                let scrutinee = Self::new_temp(locals, value.ty.clone());
                builder.statement(MirStatement::StorageLive(scrutinee));
                let scrutinee_value = Self::lower_rvalue(builder, locals, value);
                builder.statement(MirStatement::Assign {
                    place: Place::Local(scrutinee),
                    rvalue: scrutinee_value,
                });

                let tag = Self::new_temp(locals, Type::I32);
                builder.statement(MirStatement::StorageLive(tag));
                builder.statement(MirStatement::Assign {
                    place: Place::Local(tag),
                    rvalue: Rvalue::EnumTag { operand: Operand::Copy(Place::Local(scrutinee)) },
                });

                let join = builder.new_block();
                for arm in arms {
                    let arm_block = builder.new_block();
                    let next_test = builder.new_block();
                    let condition = Self::new_temp(locals, Type::Bool);
                    builder.statement(MirStatement::StorageLive(condition));
                    builder.statement(MirStatement::Assign {
                        place: Place::Local(condition),
                        rvalue: Rvalue::Binary {
                            left: Operand::Copy(Place::Local(tag)),
                            op: BinaryOp::Equal,
                            right: Operand::Constant(Literal::Number(arm.discriminant.to_string())),
                        },
                    });
                    builder.finish_block(Terminator::SwitchBool {
                        condition: Operand::Copy(Place::Local(condition)),
                        then_block: arm_block,
                        else_block: next_test,
                    });

                    builder.switch_to(arm_block);
                    for binding in &arm.bindings {
                        builder.statement(MirStatement::StorageLive(binding.local));
                        builder.statement(MirStatement::Assign {
                            place: Place::Local(binding.local),
                            rvalue: Rvalue::EnumPayload {
                                operand: Operand::Copy(Place::Local(scrutinee)),
                                discriminant: arm.discriminant,
                                field_index: binding.field_index,
                            },
                        });
                    }
                    Self::lower_block(builder, &arm.body, locals);
                    if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                        builder.finish_block(Terminator::Goto(join));
                    }

                    builder.switch_to(next_test);
                }
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Unreachable);
                }
                builder.switch_to(join);
            }
            HirStmt::For { initializer, condition, update, body } => {
                if let Some(init) = initializer {
                    Self::lower_stmt(builder, init, locals);
                }
                let head = builder.new_block();
                let body_block = builder.new_block();
                let update_block = builder.new_block();
                let exit = builder.new_block();

                builder.finish_block(Terminator::Goto(head));
                builder.switch_to(head);
                if let Some(condition) = condition {
                    let condition = Self::lower_operand(builder, locals, condition);
                    builder.finish_block(Terminator::SwitchBool {
                        condition,
                        then_block: body_block,
                        else_block: exit,
                    });
                } else {
                    builder.finish_block(Terminator::Goto(body_block));
                }

                builder.switch_to(body_block);
                builder.loop_targets.push((update_block, exit));
                Self::lower_block(builder, body, locals);
                builder.loop_targets.pop();
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(update_block));
                }

                builder.switch_to(update_block);
                if let Some(update) = update {
                    let (place, rvalue) = if let HirExprKind::Postfix { expr: target, op } = &update.kind {
                        (Some(Self::lower_place(builder, locals, target)), Self::lower_postfix_rvalue(builder, locals, target, *op))
                    } else if let HirExprKind::Assignment { target, op, value } = &update.kind {
                        (Some(Self::lower_place(builder, locals, target)), Self::lower_assignment_rvalue(builder, locals, target, *op, value))
                    } else {
                        (None, Self::lower_rvalue(builder, locals, update))
                    };
                    if let Some(place) = place {
                        builder.statement(MirStatement::Assign { place, rvalue });
                    } else {
                        builder.statement(MirStatement::Evaluate(rvalue));
                    }
                }
                builder.finish_block(Terminator::Goto(head));
                builder.switch_to(exit);
            }
        }
    }

    fn lower_assignment_rvalue(
        builder: &mut Builder,
        locals: &mut Vec<MirLocal>,
        target: &HirExpr,
        op: AssignOp,
        value: &HirExpr,
    ) -> Rvalue {
        match op {
            AssignOp::Assign => Self::lower_rvalue(builder, locals, value),
            AssignOp::Add | AssignOp::Subtract | AssignOp::Multiply | AssignOp::Divide | AssignOp::Modulo => {
                let binary_op = match op {
                    AssignOp::Add => BinaryOp::Add,
                    AssignOp::Subtract => BinaryOp::Subtract,
                    AssignOp::Multiply => BinaryOp::Multiply,
                    AssignOp::Divide => BinaryOp::Divide,
                    AssignOp::Modulo => BinaryOp::Modulo,
                    AssignOp::Assign => unreachable!(),
                };
                Rvalue::Binary {
                    left: Self::lower_operand(builder, locals, target),
                    op: binary_op,
                    right: Self::lower_operand(builder, locals, value),
                }
            }
        }
    }

    fn lower_postfix_rvalue(
        builder: &mut Builder,
        locals: &mut Vec<MirLocal>,
        target: &HirExpr,
        op: PostfixOp,
    ) -> Rvalue {
        let binary_op = match op {
            PostfixOp::Increment => BinaryOp::Add,
            PostfixOp::Decrement => BinaryOp::Subtract,
        };
        Rvalue::Binary {
            left: Self::lower_operand(builder, locals, target),
            op: binary_op,
            right: Operand::Constant(Literal::Number("1".to_string())),
        }
    }

    fn lower_place(builder: &mut Builder, locals: &mut Vec<MirLocal>, expr: &HirExpr) -> Place {
        match &expr.kind {
            HirExprKind::Local(local) => Place::Local(*local),
            HirExprKind::Member { object, name } => Place::Field {
                base: Box::new(Self::lower_place(builder, locals, object)),
                name: name.clone(),
            },
            HirExprKind::Index { object, index } => Place::Index {
                base: Box::new(Self::lower_place(builder, locals, object)),
                index: Box::new(Self::lower_operand(builder, locals, index)),
            },
            _ => {
                let temp = Self::new_temp(locals, expr.ty.clone());
                builder.statement(MirStatement::StorageLive(temp));
                let rvalue = Self::lower_rvalue(builder, locals, expr);
                builder.statement(MirStatement::Assign {
                    place: Place::Local(temp),
                    rvalue,
                });
                Place::Local(temp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::HirLowerer, lexer::Lexer, parser::Parser, sema::SemanticAnalyzer};

    fn lower(source: &str) -> MirProgram {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        MirLowerer::lower(&HirLowerer::lower(&program))
    }

    #[test]
    fn lowers_locals_and_return() {
        let mir = lower("fn main(){let x:i32=10 return}");
        assert_eq!(mir.functions.len(), 1);
        assert!(matches!(mir.functions[0].blocks[0].statements[0], MirStatement::StorageLive(_)));
        assert!(matches!(mir.functions[0].blocks[0].terminator, Terminator::Return(None)));
    }

    #[test]
    fn lowers_assignment_to_place_write() {
        let mir = lower("fn main(){let mut x:i32=10 x += 5}");
        assert!(mir.functions[0].blocks[0].statements.iter().any(|statement| {
            matches!(statement, MirStatement::Assign { place: Place::Local(_), rvalue: Rvalue::Binary { .. } })
        }));
    }

    #[test]
    fn lowers_expression_to_temporary() {
        let mir = lower("fn main(){let x:i32=1 let y:i32=2 println(x + y)}");
        let locals = &mir.functions[0].locals;
        assert!(locals.len() > 2);
        assert!(mir.functions[0].blocks[0].statements.iter().any(|statement| {
            matches!(statement, MirStatement::Assign { place: Place::Local(id), .. } if *id != usize::MAX)
        }));
        assert!(mir.functions[0].blocks[0].statements.iter().any(|statement| {
            matches!(statement, MirStatement::StorageDead(id) if *id != usize::MAX)
        }));
    }

    #[test]
    fn lowers_if_to_cfg() {
        let mir = lower("fn main(){if true { let x=1 } else { let x=2 }}");
        assert!(mir.functions[0].blocks.len() >= 4);
        assert!(matches!(mir.functions[0].blocks[0].terminator, Terminator::SwitchBool { .. }));
    }
}
