use crate::ast::{ReferenceKind, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I8, I16, I32, I64, I128, I256,
    U8, U16, U32, U64, U128, U256,
    F32, F64, F128,
    Bool,
    Char,
    Str,
    Unit,
    Named(String),
    Reference { mutable: bool, inner: Box<Type> },
    Array { element: Box<Type>, len: usize },
    Unknown,
}

impl Type {
    pub fn from_ref(type_ref: &TypeRef) -> Self {
        let base = match type_ref.name.as_str() {
            "i8" => Type::I8, "i16" => Type::I16, "i32" => Type::I32,
            "i64" => Type::I64, "i128" => Type::I128, "i256" => Type::I256,
            "u8" => Type::U8, "u16" => Type::U16, "u32" => Type::U32,
            "u64" => Type::U64, "u128" => Type::U128, "u256" => Type::U256,
            "f32" => Type::F32, "f64" => Type::F64, "f128" => Type::F128,
            "bool" => Type::Bool, "char" => Type::Char, "str" => Type::Str,
            "void" => Type::Unit, "Self" => Type::Named("Self".into()),
            other => Type::Named(other.into()),
        };

        let base = match type_ref.array_len {
            Some(len) => Type::Array { element: Box::new(base), len },
            None => base,
        };

        match type_ref.reference {
            ReferenceKind::Value => base,
            ReferenceKind::Shared => Type::Reference { mutable: false, inner: Box::new(base) },
            ReferenceKind::Mutable => Type::Reference { mutable: true, inner: Box::new(base) },
        }
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::I256 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::U256 |
            Type::F32 | Type::F64 | Type::F128 | Type::Bool | Type::Char | Type::Unit => true,
            Type::Reference { mutable: false, .. } => true,
            Type::Array { element, .. } => element.is_copy(),
            Type::Reference { mutable: true, .. } | Type::Str | Type::Named(_) | Type::Unknown => false,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::I256 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::U256)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64 | Type::F128)
    }

    pub fn is_numeric(&self) -> bool { self.is_integer() || self.is_float() }

    pub fn display_name(&self) -> String {
        match self {
            Type::I8 => "i8".into(), Type::I16 => "i16".into(), Type::I32 => "i32".into(), Type::I64 => "i64".into(),
            Type::I128 => "i128".into(), Type::I256 => "i256".into(), Type::U8 => "u8".into(), Type::U16 => "u16".into(),
            Type::U32 => "u32".into(), Type::U64 => "u64".into(), Type::U128 => "u128".into(), Type::U256 => "u256".into(),
            Type::F32 => "f32".into(), Type::F64 => "f64".into(), Type::F128 => "f128".into(), Type::Bool => "bool".into(),
            Type::Char => "char".into(), Type::Str => "str".into(), Type::Unit => "void".into(),
            Type::Named(name) => name.clone(),
            Type::Reference { mutable, inner } => format!("&{}{}", if *mutable { "mut " } else { "" }, inner.display_name()),
            Type::Array { element, len } => format!("{}[{}]", element.display_name(), len),
            Type::Unknown => "<unknown>".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutable_references_are_not_copy() {
        let shared = Type::Reference { mutable: false, inner: Box::new(Type::I32) };
        let mutable = Type::Reference { mutable: true, inner: Box::new(Type::I32) };
        assert!(shared.is_copy());
        assert!(!mutable.is_copy());
    }

    #[test]
    fn fixed_array_length_is_part_of_the_type() {
        assert_ne!(
            Type::Array { element: Box::new(Type::I32), len: 3 },
            Type::Array { element: Box::new(Type::I32), len: 100 },
        );
    }
}
