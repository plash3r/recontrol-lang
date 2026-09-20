use crate::mir::{BasicBlockId, MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use crate::mir_validate::build_cfg;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowState {
    NoBorrow,
    Shared,
    Mutable,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BorrowFlowState {
    pub locals: HashMap<usize, BorrowState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BorrowError {
    pub function: String,
    pub block: BasicBlockId,
    pub local: usize,
    pub message: String,
}

pub struct MirBorrowAnalyzer;

impl MirBorrowAnalyzer {
    pub fn analyze(program: &MirProgram) -> Result<(), Vec<BorrowError>> {
        let mut errors = Vec::new();
        for function in &program.functions {
            errors.extend(Self::analyze_function(function));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn analyze_function(function: &MirFunction) -> Vec<BorrowError> {
        let cfg = build_cfg(function);
        let initial = BorrowFlowState {
            locals: function.locals.iter().map(|l| (l.id, BorrowState::NoBorrow)).collect(),
        };
        let mut in_states = HashMap::new();
        let mut out_states = HashMap::new();
        in_states.insert(0, initial.clone());
        let mut queue = VecDeque::from([0]);
        let mut queued = HashSet::from([0]);
        let mut errors = Vec::new();

        while let Some(block_id) = queue.pop_front() {
            queued.remove(&block_id);
            let Some(block) = function.blocks.get(block_id) else { continue };
            let input = if block_id == 0 {
                in_states.get(&0).cloned().unwrap_or_else(|| initial.clone())
            } else {
                Self::merge_predecessors(&out_states, cfg.predecessors.get(&block_id), &initial)
            };
            in_states.insert(block_id, input.clone());
            let mut state = input;
            for statement in &block.statements {
                Self::transfer_statement(function, block_id, statement, &mut state, &mut errors);
            }
            Self::transfer_terminator(function, block_id, &block.terminator, &mut state, &mut errors);
            let changed = out_states.get(&block_id) != Some(&state);
            out_states.insert(block_id, state);
            if changed {
                for successor in cfg.successors.get(&block_id).cloned().unwrap_or_default() {
                    if queued.insert(successor) { queue.push_back(successor); }
                }
            }
        }
        errors
    }

    fn merge_predecessors(
        out_states: &HashMap<BasicBlockId, BorrowFlowState>,
        predecessors: Option<&Vec<BasicBlockId>>,
        initial: &BorrowFlowState,
    ) -> BorrowFlowState {
        let predecessors = predecessors.cloned().unwrap_or_default();
        if predecessors.is_empty() { return initial.clone(); }
        let mut result = initial.clone();
        for id in result.locals.keys().copied().collect::<Vec<_>>() {
            let states: Vec<_> = predecessors.iter()
                .filter_map(|p| out_states.get(p).and_then(|s| s.locals.get(&id)).copied())
                .collect();
            if states.is_empty() { continue; }
            let merged = if states.iter().all(|s| *s == BorrowState::NoBorrow) {
                BorrowState::NoBorrow
            } else if states.iter().all(|s| *s == BorrowState::Mutable) {
                BorrowState::Mutable
            } else if states.iter().all(|s| matches!(s, BorrowState::Shared | BorrowState::NoBorrow)) {
                BorrowState::Shared
            } else {
                BorrowState::Conflicted
            };
            result.locals.insert(id, merged);
        }
        result
    }

    fn transfer_statement(
        function: &MirFunction,
        block: BasicBlockId,
        statement: &MirStatement,
        state: &mut BorrowFlowState,
        errors: &mut Vec<BorrowError>,
    ) {
        match statement {
            MirStatement::StorageLive(local) => { state.locals.insert(*local, BorrowState::NoBorrow); }
            MirStatement::StorageDead(local) => { state.locals.insert(*local, BorrowState::NoBorrow); }
            MirStatement::Assign { place, rvalue } => {
                Self::check_rvalue(function, block, rvalue, state, errors);
                if let Place::Local(local) = place {
                    state.locals.insert(*local, BorrowState::NoBorrow);
                }
            }
            MirStatement::Evaluate(rvalue) => Self::check_rvalue(function, block, rvalue, state, errors),
        }
    }

    fn transfer_terminator(
        function: &MirFunction,
        block: BasicBlockId,
        terminator: &Terminator,
        state: &mut BorrowFlowState,
        errors: &mut Vec<BorrowError>,
    ) {
        match terminator {
            Terminator::SwitchBool { condition, .. } => Self::check_operand(function, block, condition, state, errors),
            Terminator::Return(Some(value)) => Self::check_rvalue(function, block, value, state, errors),
            Terminator::Return(None) | Terminator::Goto(_) | Terminator::Unreachable => {}
        }
    }

    fn check_rvalue(
        function: &MirFunction,
        block: BasicBlockId,
        value: &Rvalue,
        state: &mut BorrowFlowState,
        errors: &mut Vec<BorrowError>,
    ) {
        match value {
            Rvalue::Ref { mutable, place } => {
                if let Place::Local(local) = place {
                    let current = state.locals.get(local).copied().unwrap_or(BorrowState::NoBorrow);
                    match (current, *mutable) {
                        (BorrowState::NoBorrow, true) => { state.locals.insert(*local, BorrowState::Mutable); }
                        (BorrowState::NoBorrow, false) => { state.locals.insert(*local, BorrowState::Shared); }
                        (BorrowState::Shared, false) => {}
                        (BorrowState::Mutable, _) | (BorrowState::Shared, true) | (BorrowState::Conflicted, _) => {
                            errors.push(Self::error(function, block, *local, "conflicting borrow"));
                            state.locals.insert(*local, BorrowState::Conflicted);
                        }
                    }
                } else {
                    Self::check_place(function, block, place, state, errors);
                }
            }
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => Self::check_operand(function, block, op, state, errors),
            Rvalue::Binary { left, right, .. } => {
                Self::check_operand(function, block, left, state, errors);
                Self::check_operand(function, block, right, state, errors);
            }
            Rvalue::Call { callee, args } => {
                Self::check_operand(function, block, callee, state, errors);
                for arg in args { Self::check_operand(function, block, arg, state, errors); }
            }
            Rvalue::Aggregate { fields, .. } => {
                for (_, op) in fields { Self::check_operand(function, block, op, state, errors); }
            }
            Rvalue::Array(values) => {
                for op in values { Self::check_operand(function, block, op, state, errors); }
            }
        }
    }

    fn check_operand(
        function: &MirFunction,
        block: BasicBlockId,
        operand: &Operand,
        state: &mut BorrowFlowState,
        errors: &mut Vec<BorrowError>,
    ) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => Self::check_place(function, block, place, state, errors),
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    fn check_place(
        function: &MirFunction,
        block: BasicBlockId,
        place: &Place,
        state: &BorrowFlowState,
        errors: &mut Vec<BorrowError>,
    ) {
        match place {
            Place::Local(local) => {
                if state.locals.get(local) == Some(&BorrowState::Conflicted) {
                    errors.push(Self::error(function, block, *local, "use of value with conflicting borrow"));
                }
            }
            Place::Field { base, .. } => Self::check_place(function, block, base, state, errors),
            Place::Index { base, index } => {
                Self::check_place(function, block, base, state, errors);
                Self::check_operand(function, block, index, &mut state.clone(), errors);
            }
        }
    }

    fn error(function: &MirFunction, block: BasicBlockId, local: usize, message: &str) -> BorrowError {
        BorrowError { function: function.name.clone(), block, local, message: message.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::HirLowerer, lexer::Lexer, mir::MirLowerer, parser::Parser, sema::SemanticAnalyzer};

    fn lower(source: &str) -> MirProgram {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        MirLowerer::lower(&HirLowerer::lower(&program))
    }

    #[test]
    fn shared_borrow_is_tracked() {
        let mir = lower("fn main(){let mut x:i32=10 let r=&x println(x)}");
        assert!(MirBorrowAnalyzer::analyze(&mir).is_ok());
    }
}
