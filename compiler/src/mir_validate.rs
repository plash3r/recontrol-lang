use crate::mir::{BasicBlockId, MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct MirError {
    pub function: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageState {
    Dead,
    Live,
    Maybe,
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
                    Self::validate_operand_shape(function, condition, &local_ids, &mut errors);
                }
                Terminator::Return(value) => {
                    if let Some(value) = value {
                        Self::validate_rvalue_shape(function, value, &local_ids, &mut errors);
                    }
                }
                Terminator::Unreachable => {}
            }

            for statement in &block.statements {
                match statement {
                    MirStatement::StorageLive(id) | MirStatement::StorageDead(id) => {
                        Self::check_local(function, *id, &local_ids, &mut errors);
                    }
                    MirStatement::Assign { place, rvalue } => {
                        Self::validate_place_shape(function, place, &local_ids, &mut errors);
                        Self::validate_rvalue_shape(function, rvalue, &local_ids, &mut errors);
                    }
                    MirStatement::Evaluate(rvalue) => {
                        Self::validate_rvalue_shape(function, rvalue, &local_ids, &mut errors);
                    }
                }
            }
        }

        errors.extend(Self::validate_storage_lifetimes(function, &local_ids));

        let reachable = Self::reachable(function);
        for block in &function.blocks {
            if !reachable.contains(&block.id) {
                errors.push(Self::error(function, format!("unreachable basic block {}", block.id)));
            }
        }

        errors
    }

    fn validate_storage_lifetimes(
        function: &MirFunction,
        local_ids: &HashSet<usize>,
    ) -> Vec<MirError> {
        let cfg = build_cfg(function);
        // This is a "definitely live" (must) analysis.  For a loop header,
        // one predecessor can be the back-edge whose state depends on the header
        // itself.  Treating an as-yet-uncomputed predecessor as Dead makes the
        // header permanently Maybe and incorrectly rejects perfectly valid loops.
        //
        // Start non-entry blocks at the optimistic/top state (Live) and let the
        // fixed-point iteration monotonically refine them.  The real function
        // entry starts Dead, so use-before-live in the entry block is still caught.
        let mut out_states = HashMap::<BasicBlockId, HashMap<usize, StorageState>>::new();
        for block in &function.blocks {
            let initial = if block.id == 0 {
                StorageState::Dead
            } else {
                StorageState::Live
            };
            out_states.insert(
                block.id,
                local_ids.iter().map(|id| (*id, initial)).collect(),
            );
        }

        // Fixed-point pass: compute the storage state at the end of every block.
        // At joins we keep a local Live only when every incoming path keeps it live.
        let mut changed = true;
        while changed {
            changed = false;
            for block in &function.blocks {
                let incoming = Self::merge_storage_inputs(block.id, &cfg, &out_states, local_ids);
                let mut ignored_errors = Vec::new();
                let out = Self::transfer_storage(
                    function,
                    block,
                    incoming,
                    false,
                    &mut ignored_errors,
                    local_ids,
                );
                if out_states.get(&block.id) != Some(&out) {
                    out_states.insert(block.id, out);
                    changed = true;
                }
            }
        }

        // Validation pass using the converged entry state of each block.
        let mut errors = Vec::new();
        for block in &function.blocks {
            let incoming = Self::merge_storage_inputs(block.id, &cfg, &out_states, local_ids);
            Self::transfer_storage(
                function,
                block,
                incoming,
                true,
                &mut errors,
                local_ids,
            );
        }
        errors
    }

    fn merge_storage_inputs(
        block_id: BasicBlockId,
        cfg: &LocalFlow,
        out_states: &HashMap<BasicBlockId, HashMap<usize, StorageState>>,
        local_ids: &HashSet<usize>,
    ) -> HashMap<usize, StorageState> {
        let mut incoming = HashMap::new();

        for local in local_ids {
            let state = if block_id == 0 {
                StorageState::Dead
            } else if let Some(preds) = cfg.predecessors.get(&block_id) {
                let values = preds
                    .iter()
                    .map(|pred| {
                        out_states
                            .get(pred)
                            .and_then(|states| states.get(local).copied())
                            .unwrap_or(StorageState::Dead)
                    })
                    .collect::<Vec<_>>();
                Self::merge_storage_states(&values)
            } else {
                StorageState::Dead
            };
            incoming.insert(*local, state);
        }

        incoming
    }

    fn merge_storage_states(states: &[StorageState]) -> StorageState {
        if states.is_empty() || states.iter().all(|state| *state == StorageState::Dead) {
            StorageState::Dead
        } else if states.iter().all(|state| *state == StorageState::Live) {
            StorageState::Live
        } else {
            StorageState::Maybe
        }
    }

    fn transfer_storage(
        function: &MirFunction,
        block: &crate::mir::BasicBlock,
        mut state: HashMap<usize, StorageState>,
        validate: bool,
        errors: &mut Vec<MirError>,
        local_ids: &HashSet<usize>,
    ) -> HashMap<usize, StorageState> {
        for statement in &block.statements {
            match statement {
                MirStatement::StorageLive(id) => {
                    if validate && state.get(id).copied().unwrap_or(StorageState::Dead) == StorageState::Live {
                        errors.push(Self::error(
                            function,
                            format!("duplicate StorageLive for local {}", id),
                        ));
                    }
                    state.insert(*id, StorageState::Live);
                }
                MirStatement::StorageDead(id) => {
                    if validate && state.get(id).copied().unwrap_or(StorageState::Dead) != StorageState::Live {
                        errors.push(Self::error(
                            function,
                            format!("StorageDead for local {} while storage is not definitely live", id),
                        ));
                    }
                    state.insert(*id, StorageState::Dead);
                }
                MirStatement::Assign { place, rvalue } => {
                    Self::check_storage_place(function, place, &state, local_ids, validate, errors, true);
                    Self::check_storage_rvalue(function, rvalue, &state, local_ids, validate, errors);
                }
                MirStatement::Evaluate(rvalue) => {
                    Self::check_storage_rvalue(function, rvalue, &state, local_ids, validate, errors);
                }
            }
        }

        match &block.terminator {
            Terminator::SwitchBool { condition, .. } => {
                Self::check_storage_operand(function, condition, &state, local_ids, validate, errors);
            }
            Terminator::Return(Some(value)) => {
                Self::check_storage_rvalue(function, value, &state, local_ids, validate, errors);
            }
            Terminator::Goto(_) | Terminator::Return(None) | Terminator::Unreachable => {}
        }

        state
    }

    fn check_storage_rvalue(
        function: &MirFunction,
        value: &Rvalue,
        state: &HashMap<usize, StorageState>,
        local_ids: &HashSet<usize>,
        validate: bool,
        errors: &mut Vec<MirError>,
    ) {
        match value {
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => {
                Self::check_storage_operand(function, op, state, local_ids, validate, errors)
            }
            Rvalue::Binary { left, right, .. } => {
                Self::check_storage_operand(function, left, state, local_ids, validate, errors);
                Self::check_storage_operand(function, right, state, local_ids, validate, errors);
            }
            Rvalue::Ref { place, .. } => {
                Self::check_storage_place(function, place, state, local_ids, validate, errors, false)
            }
            Rvalue::Call { callee, args } => {
                Self::check_storage_operand(function, callee, state, local_ids, validate, errors);
                for arg in args {
                    Self::check_storage_operand(function, arg, state, local_ids, validate, errors);
                }
            }
            Rvalue::Aggregate { fields, .. } => {
                for (_, op) in fields {
                    Self::check_storage_operand(function, op, state, local_ids, validate, errors);
                }
            }
            Rvalue::Array(values) => {
                for op in values {
                    Self::check_storage_operand(function, op, state, local_ids, validate, errors);
                }
            }
        }
    }

    fn check_storage_operand(
        function: &MirFunction,
        operand: &Operand,
        state: &HashMap<usize, StorageState>,
        local_ids: &HashSet<usize>,
        validate: bool,
        errors: &mut Vec<MirError>,
    ) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::check_storage_place(function, place, state, local_ids, validate, errors, false)
            }
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    fn check_storage_place(
        function: &MirFunction,
        place: &Place,
        state: &HashMap<usize, StorageState>,
        local_ids: &HashSet<usize>,
        validate: bool,
        errors: &mut Vec<MirError>,
        assignment_target: bool,
    ) {
        match place {
            Place::Local(id) => {
                if !local_ids.contains(id) || !validate {
                    return;
                }

                let current = state.get(id).copied().unwrap_or(StorageState::Dead);
                let valid = current == StorageState::Live;

                if !valid {
                    let message = if assignment_target {
                        format!("assignment to local {} while storage is not definitely live", id)
                    } else {
                        format!("use of local {} while storage is not definitely live", id)
                    };
                    errors.push(Self::error(function, message));
                }
            }
            Place::Field { base, .. } => {
                Self::check_storage_place(function, base, state, local_ids, validate, errors, false)
            }
            Place::Index { base, index } => {
                Self::check_storage_place(function, base, state, local_ids, validate, errors, false);
                Self::check_storage_operand(function, index, state, local_ids, validate, errors);
            }
        }
    }

    fn validate_rvalue_shape(function: &MirFunction, value: &Rvalue, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match value {
            Rvalue::Use(op) | Rvalue::Unary { operand: op, .. } => Self::validate_operand_shape(function, op, locals, errors),
            Rvalue::Binary { left, right, .. } => {
                Self::validate_operand_shape(function, left, locals, errors);
                Self::validate_operand_shape(function, right, locals, errors);
            }
            Rvalue::Ref { place, .. } => Self::validate_place_shape(function, place, locals, errors),
            Rvalue::Call { callee, args } => {
                Self::validate_operand_shape(function, callee, locals, errors);
                for arg in args { Self::validate_operand_shape(function, arg, locals, errors); }
            }
            Rvalue::Aggregate { fields, .. } => {
                for (_, op) in fields { Self::validate_operand_shape(function, op, locals, errors); }
            }
            Rvalue::Array(values) => {
                for op in values { Self::validate_operand_shape(function, op, locals, errors); }
            }
        }
    }

    fn validate_operand_shape(function: &MirFunction, operand: &Operand, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => Self::validate_place_shape(function, place, locals, errors),
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    fn validate_place_shape(function: &MirFunction, place: &Place, locals: &HashSet<usize>, errors: &mut Vec<MirError>) {
        match place {
            Place::Local(id) => Self::check_local(function, *id, locals, errors),
            Place::Field { base, .. } => Self::validate_place_shape(function, base, locals, errors),
            Place::Index { base, index } => {
                Self::validate_place_shape(function, base, locals, errors);
                Self::validate_operand_shape(function, index, locals, errors);
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

    fn manually_validate(function: MirFunction) -> Result<(), Vec<MirError>> {
        MirValidator::validate(&MirProgram { functions: vec![function] })
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

    #[test]
    fn use_before_storage_live_is_rejected() {
        let function = MirFunction {
            param_count: 0,
            name: "bad".into(),
            locals: vec![crate::mir::MirLocal { id: 0, ty: crate::types::Type::I32, mutable: false }],
            blocks: vec![crate::mir::BasicBlock {
                id: 0,
                statements: vec![MirStatement::Evaluate(Rvalue::Use(Operand::Copy(Place::Local(0))))],
                terminator: Terminator::Return(None),
            }],
        };
        assert!(manually_validate(function).is_err());
    }

    #[test]
    fn use_after_storage_dead_is_rejected() {
        let function = MirFunction {
            param_count: 0,
            name: "bad".into(),
            locals: vec![crate::mir::MirLocal { id: 0, ty: crate::types::Type::I32, mutable: false }],
            blocks: vec![crate::mir::BasicBlock {
                id: 0,
                statements: vec![
                    MirStatement::StorageLive(0),
                    MirStatement::StorageDead(0),
                    MirStatement::Evaluate(Rvalue::Use(Operand::Copy(Place::Local(0)))),
                ],
                terminator: Terminator::Return(None),
            }],
        };
        assert!(manually_validate(function).is_err());
    }

    #[test]
    fn double_storage_dead_is_rejected() {
        let function = MirFunction {
            param_count: 0,
            name: "bad".into(),
            locals: vec![crate::mir::MirLocal { id: 0, ty: crate::types::Type::I32, mutable: false }],
            blocks: vec![crate::mir::BasicBlock {
                id: 0,
                statements: vec![MirStatement::StorageLive(0), MirStatement::StorageDead(0), MirStatement::StorageDead(0)],
                terminator: Terminator::Return(None),
            }],
        };
        assert!(manually_validate(function).is_err());
    }

    #[test]
    fn parameter_storage_is_live_at_entry() {
        let mir = lower("fn main(x:i32){println(x)}");
        assert!(MirValidator::validate(&mir).is_ok());
        assert!(matches!(mir.functions[0].blocks[0].statements.first(), Some(MirStatement::StorageLive(0))));
    }

    #[test]
    fn for_loop_storage_liveness_passes() {
        let mir = lower("fn main(){for(let mut i:i32=0; i<5; i++){println(\"Hello!\")}}");
        assert!(MirValidator::validate(&mir).is_ok());
    }

    #[test]
    fn branch_join_with_live_storage_passes() {
        let function = MirFunction {
            param_count: 0,
            name: "branch".into(),
            locals: vec![crate::mir::MirLocal { id: 0, ty: crate::types::Type::I32, mutable: false }],
            blocks: vec![
                crate::mir::BasicBlock {
                    id: 0,
                    statements: vec![MirStatement::StorageLive(0)],
                    terminator: Terminator::SwitchBool { condition: Operand::Constant(crate::ast::Literal::Bool(true)), then_block: 1, else_block: 2 },
                },
                crate::mir::BasicBlock {
                    id: 1,
                    statements: vec![],
                    terminator: Terminator::Goto(3),
                },
                crate::mir::BasicBlock {
                    id: 2,
                    statements: vec![],
                    terminator: Terminator::Goto(3),
                },
                crate::mir::BasicBlock {
                    id: 3,
                    statements: vec![MirStatement::Evaluate(Rvalue::Use(Operand::Copy(Place::Local(0))))],
                    terminator: Terminator::Return(None),
                },
            ],
        };
        assert!(manually_validate(function).is_ok());
    }
}
