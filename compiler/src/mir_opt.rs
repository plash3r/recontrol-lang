use crate::mir::{MirProgram, Terminator};
use crate::mir_validate::build_cfg;
use std::collections::{HashSet, VecDeque};

pub struct MirOptimizer;

impl MirOptimizer {
    pub fn optimize(program: &mut MirProgram) {
        for function in &mut program.functions {
            Self::remove_unreachable_blocks(function);
        }
    }

    fn remove_unreachable_blocks(function: &mut crate::mir::MirFunction) {
        if function.blocks.is_empty() { return; }
        let cfg = build_cfg(function);
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::from([0usize]);
        while let Some(id) = queue.pop_front() {
            if !reachable.insert(id) { continue; }
            for next in cfg.successors.get(&id).cloned().unwrap_or_default() {
                queue.push_back(next);
            }
        }
        if reachable.len() == function.blocks.len() { return; }

        let mut map = vec![usize::MAX; function.blocks.len()];
        let mut blocks = Vec::with_capacity(reachable.len());
        for (old, block) in function.blocks.iter().enumerate() {
            if reachable.contains(&old) {
                map[old] = blocks.len();
                blocks.push(block.clone());
            }
        }
        for block in &mut blocks {
            block.id = map[block.id];
            block.terminator = match &block.terminator {
                Terminator::Goto(target) => Terminator::Goto(map[*target]),
                Terminator::SwitchBool { condition, then_block, else_block } =>
                    Terminator::SwitchBool { condition: condition.clone(), then_block: map[*then_block], else_block: map[*else_block] },
                Terminator::Return(value) => Terminator::Return(value.clone()),
                Terminator::Unreachable => Terminator::Unreachable,
            };
        }
        function.blocks = blocks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::HirLowerer, lexer::Lexer, mir::MirLowerer, parser::Parser, sema::SemanticAnalyzer};

    #[test]
    fn removes_unreachable_return_continuation() {
        let source = "fn main(){return let x:i32=1}";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let mut mir = MirLowerer::lower(&HirLowerer::lower(&program));
        MirOptimizer::optimize(&mut mir);
        assert_eq!(mir.functions[0].blocks.len(), 1);
    }
}
