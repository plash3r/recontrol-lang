use crate::mir::{BasicBlockId, MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct MirError {
    pub function: String,
    pub message: String,
}

pub struct MirValidator;

impl MirValidator {
    pub fn validate(program: &MirProgram) -> Result<(), Vec<MirError>> {
        let mut errors = Vec::new();
        for function in &program.functions {
            errors.extend(Self::validate_function(function));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn validate_function(function: &MirFunction) -> Vec<MirError> {
        let mut errors = Vec::new();
        let count = function.blocks.len();
        let local_ids: HashSet<_> = function.locals.iter().map(|l| l.id).collect();

        for block in &function.blocks {
            if block.id >= count {
                errors.push(Self::error(function, format!("invalid basic block {}", block.id)));
            }
            let check_target = |target: BasicBlockId, errors: &mut Vec<MirError>| {
                if target >= count {
                    errors.push(Self::error(function, format!("invalid CFG target {}", target)));
                }
            };

            match &block.terminator {
                Terminator::Goto(target) => check_target(*target, &mut errors),
                Terminator::SwitchBool { then_block, else_block, condition } => {
                    check_target(*then_block, &mut errors);
                    check_target(*else_block, &mut errors);
                    Self::validate_operand(function, condition, &local_ids, &mut errors);
                }
                Terminator::Return(value) => { if let Some(value) = value { Self::validate_rvalue(function, value, &local_ids, &mut errors); } }
                Terminator::Unreachable => {}
            }

            for statement in &block.statements {
                match statement {
                    MirStatement::StorageLive(id) | MirStatement::StorageDead(id) => {
                        Self::check_local(function, *id, &local_ids, &mut errors);
                    }
                    MirStatement::Assign { place, rvalue } => {
                        Self::validate_place(function, place, &local_ids, &mut errors);
                        Self::validate_rvalue(function, rvalue, &local_ids, &mut errors);
                    }
                    MirStatement::Evaluate(rvalue) => {
                        Self::validate_rvalue(function, rvalue, &local_ids, &mut errors);
                    }
                }
            }
        }

        let reachable = Self::reachable(function);
        for block in &function.blocks {
            if !reachable.contains(&block.id) {
                errors.push(Self::error(function, format!("unreachable basic block {}", block.id)));
            }
        }

        errors
    }

    fn validate_rvalue(function: &MirFunction, value: &Rvalue, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match value {
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => Self::validate_operand(function, op, locals, errors),
            Rvalue::Binary { left, right, .. } => {
                Self::validate_operand(function, left, locals, errors);
                Self::validate_operand(function, right, locals, errors);
            }
            Rvalue::Ref { place, .. } => Self::validate_place(function, place, locals, errors),
            Rvalue::Call { callee, args } => {
                Self::validate_operand(function, callee, locals, errors);
                for arg in args { Self::validate_operand(function, arg, locals, errors); }
            }
            Rvalue::Aggregate { fields, .. } => {
                for (_, op) in fields { Self::validate_operand(function, op, locals, errors); }
            }
            Rvalue::Array(values) => {
                for op in values { Self::validate_operand(function, op, locals, errors); }
            }
        }
    }

    fn validate_operand(function: &MirFunction, operand: &Operand, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => Self::validate_place(function, place, locals, errors),
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    fn validate_place(function: &MirFunction, place: &Place, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match place {
            Place::Local(id) => Self::check_local(function, *id, locals, errors),
            Place::Field { base, .. } => Self::validate_place(function, base, locals, errors),
            Place::Index { base, index } => {
                Self::validate_place(function, base, locals, errors);
                Self::validate_operand(function, index, locals, errors);
            }
        }
    }

    fn check_local(function: &MirFunction, id: usize, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        if !locals.contains(&id) {
            errors.push(Self::error(function, format!("invalid local {}", id)));
        }
    }

    fn reachable(function: &MirFunction) -> HashSet<BasicBlockId> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([0]);
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) { continue; }
            if let Some(block) = function.blocks.get(id) {
                match block.terminator {
                    Terminator::Goto(t) => queue.push_back(t),
                    Terminator::SwitchBool { then_block, else_block, .. } => {
                        queue.push_back(then_block);
                        queue.push_back(else_block);
                    }
                    Terminator::Return(_) | Terminator::Unreachable => {}
                }
            }
        }
        seen
    }

    fn error(function: &MirFunction, message: String) -> MirError {
        MirError { function: function.name.clone(), message }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalFlow {
    pub predecessors: HashMap<BasicBlockId, Vec<BasicBlockId>>,
    pub successors: HashMap<BasicBlockId, Vec<BasicBlockId>>,
}

pub fn build_cfg(function: &MirFunction) -> LocalFlow {
    let mut predecessors = HashMap::<BasicBlockId, Vec<BasicBlockId>>::new();
    let mut successors = HashMap::<BasicBlockId, Vec<BasicBlockId>>::new();

    for block in &function.blocks {
        let next = match block.terminator {
            Terminator::Goto(t) => vec![t],
            Terminator::SwitchBool { then_block, else_block, .. } => vec![then_block, else_block],
            Terminator::Return(_) | Terminator::Unreachable => Vec::new(),
        };
        successors.insert(block.id, next.clone());
        for target in next {
            predecessors.entry(target).or_default().push(block.id);
        }
    }

    LocalFlow { predecessors, successors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::HirLowerer, lexer::Lexer, parser::Parser, sema::SemanticAnalyzer};
    use crate::mir::MirLowerer;

    fn lower(source: &str) -> MirProgram {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        MirLowerer::lower(&HirLowerer::lower(&program))
    }

    #[test]
    fn valid_mir_passes_validation() {
        let mir = lower("fn main(){let x:i32=10 println(x)}");
        assert!(MirValidator::validate(&mir).is_ok());
    }

    #[test]
    fn cfg_contains_edges() {
        let mir = lower("fn main(){if true { println(1) } else { println(2) }}");
        let cfg = build_cfg(&mir.functions[0]);
        assert!(cfg.successors.get(&0).unwrap().len() == 2);
        assert!(cfg.predecessors.get(&1).is_some());
    }
}
