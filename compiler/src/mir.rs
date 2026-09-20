use crate::hir::{HirExpr, HirExprKind, HirFunction, HirProgram, HirStmt, LocalId};
use crate::ast::{AssignOp, BinaryOp, Literal, PostfixOp, UnaryOp};
use crate::types::Type;

pub type BasicBlockId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub name: String,
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
    Index { base: Box<Place>, index: Operand },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rvalue {
    Use(Operand),
    Unary { op: UnaryOp, operand: Operand },
    Binary { left: Operand, op: BinaryOp, right: Operand },
    Ref { mutable: bool, place: Place },
    Call { callee: Operand, args: Vec<Operand> },
    Aggregate { name: String, fields: Vec<(String, Operand)> },
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
    Return,
    Unreachable,
}

struct Builder {
    blocks: Vec<BasicBlock>,
    current: BasicBlockId,
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
        }
    }

    fn lower_function(function: &HirFunction) -> MirFunction {
        let locals = function.body.locals.iter().map(|local| MirLocal {
            id: local.id,
            ty: local.ty.clone(),
            mutable: local.mutable,
        }).collect();

        let mut builder = Builder::new();
        Self::lower_block(&mut builder, &function.body);
        if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
            builder.finish_block(Terminator::Return);
        }

        MirFunction {
            name: function.name.clone(),
            locals,
            blocks: builder.blocks,
        }
    }

    fn lower_block(builder: &mut Builder, block: &crate::hir::HirBlock) {
        for stmt in &block.statements {
            Self::lower_stmt(builder, stmt);
        }
    }

    fn lower_stmt(builder: &mut Builder, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let { local, initializer } => {
                builder.statement(MirStatement::StorageLive(*local));
                if let Some(value) = initializer {
                    builder.statement(MirStatement::Assign {
                        place: Place::Local(*local),
                        rvalue: Self::lower_rvalue(value),
                    });
                }
            }
            HirStmt::Expr(expr) => {
                if let HirExprKind::Assignment { target, op, value } = &expr.kind {
                    let rvalue = Self::lower_assignment_rvalue(target, *op, value);
                    builder.statement(MirStatement::Assign {
                        place: Self::lower_place(target),
                        rvalue,
                    });
                } else if let HirExprKind::Postfix { expr: target, op } = &expr.kind {
                    let rvalue = Self::lower_postfix_rvalue(target, *op);
                    builder.statement(MirStatement::Assign {
                        place: Self::lower_place(target),
                        rvalue,
                    });
                } else {
                    builder.statement(MirStatement::Evaluate(Self::lower_rvalue(expr)));
                }
            }
            HirStmt::Return(expr) => {
                if let Some(expr) = expr {
                    builder.statement(MirStatement::Evaluate(Self::lower_rvalue(expr)));
                }
                builder.finish_block(Terminator::Return);
                let next = builder.new_block();
                builder.switch_to(next);
            }
            HirStmt::Block(block) => Self::lower_block(builder, block),
            HirStmt::If { condition, then_branch, else_branch } => {
                let then_block = builder.new_block();
                let else_block = builder.new_block();
                let join_block = builder.new_block();
                let condition = Self::lower_operand(condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block,
                    else_block,
                });

                builder.switch_to(then_block);
                Self::lower_block(builder, then_branch);
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(join_block));
                }

                builder.switch_to(else_block);
                if let Some(stmt) = else_branch {
                    Self::lower_stmt(builder, stmt);
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
                let condition = Self::lower_operand(condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block: body_block,
                    else_block: exit,
                });

                builder.switch_to(body_block);
                Self::lower_block(builder, body);
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
                Self::lower_block(builder, body);
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(condition_block));
                }

                builder.switch_to(condition_block);
                let condition = Self::lower_operand(condition);
                builder.finish_block(Terminator::SwitchBool {
                    condition,
                    then_block: body_block,
                    else_block: exit,
                });
                builder.switch_to(exit);
            }
            HirStmt::For { initializer, condition, update, body } => {
                if let Some(init) = initializer {
                    Self::lower_stmt(builder, init);
                }
                let head = builder.new_block();
                let body_block = builder.new_block();
                let update_block = builder.new_block();
                let exit = builder.new_block();

                builder.finish_block(Terminator::Goto(head));
                builder.switch_to(head);
                if let Some(condition) = condition {
                    let condition = Self::lower_operand(condition);
                    builder.finish_block(Terminator::SwitchBool {
                        condition,
                        then_block: body_block,
                        else_block: exit,
                    });
                } else {
                    builder.finish_block(Terminator::Goto(body_block));
                }

                builder.switch_to(body_block);
                Self::lower_block(builder, body);
                if matches!(builder.blocks[builder.current].terminator, Terminator::Unreachable) {
                    builder.finish_block(Terminator::Goto(update_block));
                }

                builder.switch_to(update_block);
                if let Some(update) = update {
                    builder.statement(MirStatement::Evaluate(Self::lower_rvalue(update)));
                }
                builder.finish_block(Terminator::Goto(head));
                builder.switch_to(exit);
            }
        }
    }

    fn lower_assignment_rvalue(target: &HirExpr, op: AssignOp, value: &HirExpr) -> Rvalue {
        match op {
            AssignOp::Assign => Self::lower_rvalue(value),
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
                    left: Self::lower_operand(target),
                    op: binary_op,
                    right: Self::lower_operand(value),
                }
            }
        }
    }

    fn lower_postfix_rvalue(target: &HirExpr, op: PostfixOp) -> Rvalue {
        let binary_op = match op {
            PostfixOp::Increment => BinaryOp::Add,
            PostfixOp::Decrement => BinaryOp::Subtract,
        };
        Rvalue::Binary {
            left: Self::lower_operand(target),
            op: binary_op,
            right: Operand::Constant(Literal::Integer("1".to_string())),
        }
    }

    fn lower_rvalue(expr: &HirExpr) -> Rvalue {
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
                    operand: Self::lower_operand(expr),
                }
            }
            HirExprKind::Binary { left, op, right } => Rvalue::Binary {
                left: Self::lower_operand(left),
                op: *op,
                right: Self::lower_operand(right),
            },
            HirExprKind::Assignment { target, op, value } => Self::lower_assignment_rvalue(target, *op, value),
            HirExprKind::Call { callee, args } => Rvalue::Call {
                callee: Self::lower_operand(callee),
                args: args.iter().map(Self::lower_operand).collect(),
            },
            HirExprKind::Member { object, name } => {
                let place = Place::Field {
                    base: Box::new(Self::lower_place(object)),
                    name: name.clone(),
                };
                Rvalue::Use(Operand::Copy(place))
            }
            HirExprKind::Postfix { expr, .. } => Rvalue::Use(Self::lower_operand(expr)),
            HirExprKind::StructLiteral { name, fields } => Rvalue::Aggregate {
                name: name.clone(),
                fields: fields.iter().map(|(n, e)| (n.clone(), Self::lower_operand(e))).collect(),
            },
            HirExprKind::Array(values) => Rvalue::Array(values.iter().map(Self::lower_operand).collect()),
            HirExprKind::Index { object, index } => Rvalue::Use(Operand::Copy(Place::Index {
                base: Box::new(Self::lower_place(object)),
                index: Self::lower_operand(index),
            })),
            HirExprKind::Function(id) => Rvalue::Use(Operand::Function(*id)),
        }
    }

    fn lower_operand(expr: &HirExpr) -> Operand {
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
            _ => Operand::Move(Place::Local(usize::MAX)),
        }
    }

    fn lower_place(expr: &HirExpr) -> Place {
        match &expr.kind {
            HirExprKind::Local(local) => Place::Local(*local),
            HirExprKind::Member { object, name } => Place::Field {
                base: Box::new(Self::lower_place(object)),
                name: name.clone(),
            },
            HirExprKind::Index { object, index } => Place::Index {
                base: Box::new(Self::lower_place(object)),
                index: Self::lower_operand(index),
            },
            _ => Place::Local(usize::MAX),
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
        assert!(matches!(mir.functions[0].blocks[0].terminator, Terminator::Return));
    }

    #[test]
    fn lowers_assignment_to_place_write() {
        let mir = lower("fn main(){let mut x:i32=10 x += 5}");
        assert!(mir.functions[0].blocks[0].statements.iter().any(|statement| {
            matches!(statement, MirStatement::Assign { place: Place::Local(_), rvalue: Rvalue::Binary { .. } })
        }));
    }

    #[test]
    fn lowers_if_to_cfg() {
        let mir = lower("fn main(){if true { let x=1 } else { let x=2 }}");
        assert!(mir.functions[0].blocks.len() >= 4);
        assert!(matches!(mir.functions[0].blocks[0].terminator, Terminator::SwitchBool { .. }));
    }
}
