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
    mutable: Vec<HashMap<String, bool>>,
}

impl Env {
    fn push(&mut self) {
        self.variables.push(HashMap::new());
        self.mutable.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.variables.pop();
        self.mutable.pop();
    }

    fn define(&mut self, name: String, ty: Type, is_mutable: bool) -> bool {
        let scope = self.variables.last_mut().expect("scope exists");
        let mutable_scope = self.mutable.last_mut().expect("mutable scope exists");
        let fresh = scope.insert(name.clone(), ty).is_none();
        if fresh {
            mutable_scope.insert(name, is_mutable);
        }
        fresh
    }

    fn get(&self, name: &str) -> Option<Type> {
        self.variables.iter().rev().find_map(|scope| scope.get(name).cloned())
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutable.iter().rev().find_map(|scope| scope.get(name).copied()).unwrap_or(false)
    }
}

