use crate::mir::{BasicBlockId, MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use crate::mir_validate::build_cfg;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalState {
    Uninitialized,
    Initialized,
    Moved,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowState {
    pub locals: HashMap<usize, LocalState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoveError {
    pub function: String,
    pub block: BasicBlockId,
    pub local: usize,
    pub message: String,
}

pub struct MirMoveAnalyzer;

impl MirMoveAnalyzer {
    pub fn analyze(program: &MirProgram) -> Result<(), Vec<MoveError>> {
        let mut errors = Vec::new();
        for function in &program.functions {
            errors.extend(Self::analyze_function(function));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn analyze_function(function: &MirFunction) -> Vec<MoveError> {
        let cfg = build_cfg(function);
        let mut in_states: HashMap<BasicBlockId, FlowState> = HashMap::new();
        let mut out_states: HashMap<BasicBlockId, FlowState> = HashMap::new();
        let mut initial = FlowState {
            locals: function.locals.iter().map(|l| (l.id, LocalState::Uninitialized)).collect(),
        };
        // Parameters arrive initialized from the caller. StorageLive models
        // lifetime only and therefore must not erase their value state.
        for local in function.locals.iter().take(function.param_count) {
            initial.locals.insert(local.id, LocalState::Initialized);
        }
        in_states.insert(0, initial);

        let mut queue = VecDeque::from([0]);
        let mut queued: HashSet<BasicBlockId> = HashSet::from([0]);
        let mut errors = Vec::new();

        while let Some(block_id) = queue.pop_front() {
            queued.remove(&block_id);
            let Some(block) = function.blocks.get(block_id) else { continue };

            let input = if block_id == 0 {
                in_states.get(&block_id).cloned().unwrap()
            } else {
                let preds = cfg.predecessors.get(&block_id).cloned().unwrap_or_default();
                Self::merge_predecessors(&out_states, &preds, &initial)
            };

            if in_states.get(&block_id) != Some(&input) {
                in_states.insert(block_id, input.clone());
            }

            let mut state = input;
            for statement in &block.statements {
                Self::transfer_statement(function, block_id, statement, &mut state, &mut errors);
            }
            Self::transfer_terminator(function, block_id, &block.terminator, &mut state, &mut errors);

            let changed = out_states.get(&block_id) != Some(&state);
            out_states.insert(block_id, state);

            if changed {
                for successor in cfg.successors.get(&block_id).cloned().unwrap_or_default() {
                    if queued.insert(successor) {
                        queue.push_back(successor);
                    }
                }
            }
        }

        errors
    }

    fn merge_predecessors(
        out_states: &HashMap<BasicBlockId, FlowState>,
        predecessors: &[BasicBlockId],
        initial: &FlowState,
    ) -> FlowState {
        if predecessors.is_empty() {
            return initial.clone();
        }

        let mut result = initial.clone();
        for id in result.locals.keys().copied().collect::<Vec<_>>() {
            let states: Vec<_> = predecessors.iter()
                .filter_map(|p| out_states.get(p).and_then(|s| s.locals.get(&id)).copied())
                .collect();

            if states.is_empty() {
                continue;
            }

            let merged = if states.iter().all(|s| *s == LocalState::Initialized) {
                LocalState::Initialized
            } else if states.iter().all(|s| *s == LocalState::Moved) {
                LocalState::Moved
            } else if states.iter().any(|s| *s == LocalState::Uninitialized) {
                LocalState::Uninitialized
            } else {
                LocalState::Moved
            };
            result.locals.insert(id, merged);
        }
        result
    }

    fn transfer_statement(
        function: &MirFunction,
        block: BasicBlockId,
        statement: &MirStatement,
        state: &mut FlowState,
        errors: &mut Vec<MoveError>,
    ) {
        match statement {
            MirStatement::StorageLive(local) => {
                if !function.locals.iter().take(function.param_count).any(|p| p.id == *local) {
                    state.locals.insert(*local, LocalState::Uninitialized);
                }
            }
            MirStatement::StorageDead(local) => {
                state.locals.insert(*local, LocalState::Moved);
            }
            MirStatement::Assign { place, rvalue } => {
                Self::check_rvalue(function, block, rvalue, state, errors);
                if let Place::Local(local) = place {
                    state.locals.insert(*local, LocalState::Initialized);
                }
            }
            MirStatement::Evaluate(rvalue) => {
                Self::check_rvalue(function, block, rvalue, state, errors);
            }
        }
    }

    fn transfer_terminator(
        function: &MirFunction,
        block: BasicBlockId,
        terminator: &Terminator,
        state: &mut FlowState,
        errors: &mut Vec<MoveError>,
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
        state: &mut FlowState,
        errors: &mut Vec<MoveError>,
    ) {
        match value {
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => Self::check_operand(function, block, op, state, errors),
            Rvalue::Binary { left, right, .. } => {
                Self::check_operand(function, block, left, state, errors);
                Self::check_operand(function, block, right, state, errors);
            }
            Rvalue::Ref { place, .. } => Self::check_place(function, block, place, state, errors),
            Rvalue::Call { callee, args } => {
                Self::check_operand(function, block, callee, state, errors);
                for arg in args {
                    Self::check_operand(function, block, arg, state, errors);
                }
            }
            Rvalue::Aggregate { fields, .. } => {
                for (_, op) in fields {
                    Self::check_operand(function, block, op, state, errors);
                }
            }
            Rvalue::Array(values) => {
                for op in values {
                    Self::check_operand(function, block, op, state, errors);
                }
            }
        }
    }

    fn check_operand(
        function: &MirFunction,
        block: BasicBlockId,
        operand: &Operand,
        state: &mut FlowState,
        errors: &mut Vec<MoveError>,
    ) {
        match operand {
            Operand::Copy(place) => Self::check_place(function, block, place, state, errors),
            Operand::Move(place) => {
                Self::check_place(function, block, place, state, errors);
                if let Place::Local(local) = place {
                    match state.locals.get(local).copied().unwrap_or(LocalState::Uninitialized) {
                        LocalState::Initialized => state.locals.insert(*local, LocalState::Moved),
                        LocalState::Moved => {
                            errors.push(Self::error(function, block, *local, "use of moved value"));
                            None
                        }
                        LocalState::Uninitialized => {
                            errors.push(Self::error(function, block, *local, "use of uninitialized value"));
                            None
                        }
                    };
                }
            }
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    fn check_place(
        function: &MirFunction,
        block: BasicBlockId,
        place: &Place,
        state: &FlowState,
        errors: &mut Vec<MoveError>,
    ) {
        if let Place::Local(local) = place {
            match state.locals.get(local).copied().unwrap_or(LocalState::Uninitialized) {
                LocalState::Initialized => {}
                LocalState::Moved => errors.push(Self::error(function, block, *local, "use of moved value")),
                LocalState::Uninitialized => errors.push(Self::error(function, block, *local, "use of uninitialized value")),
            }
        } else {
            match place {
                Place::Field { base, .. } => Self::check_place(function, block, base, state, errors),
                Place::Index { base, index } => {
                    Self::check_place(function, block, base, state, errors);
                    Self::check_operand(function, block, index, &mut state.clone(), errors);
                }
                Place::Local(_) => unreachable!(),
            }
        }
    }

    fn error(function: &MirFunction, block: BasicBlockId, local: usize, message: &str) -> MoveError {
        MoveError {
            function: function.name.clone(),
            block,
            local,
            message: message.to_string(),
        }
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
    fn initialized_local_is_usable() {
        let mir = lower("fn main(){let x:i32=10 println(x)}");
        assert!(MirMoveAnalyzer::analyze(&mir).is_ok());
    }
}
