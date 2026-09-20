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
    Array(Box<Type>),
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

        match type_ref.reference {
            ReferenceKind::Value => base,
            ReferenceKind::Shared => Type::Reference { mutable: false, inner: Box::new(base) },
            ReferenceKind::Mutable => Type::Reference { mutable: true, inner: Box::new(base) },
        }
    }

    pub fn is_copy(&self) -> bool {
        matches!(
            self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::I256 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::U256 |
            Type::F32 | Type::F64 | Type::F128 | Type::Bool | Type::Char | Type::Unit |
            Type::Reference { .. }
        )
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
            Type::I8 => "i8", Type::I16 => "i16", Type::I32 => "i32", Type::I64 => "i64",
            Type::I128 => "i128", Type::I256 => "i256", Type::U8 => "u8", Type::U16 => "u16",
            Type::U32 => "u32", Type::U64 => "u64", Type::U128 => "u128", Type::U256 => "u256",
            Type::F32 => "f32", Type::F64 => "f64", Type::F128 => "f128", Type::Bool => "bool",
            Type::Char => "char", Type::Str => "str", Type::Unit => "void",
            Type::Named(name) => name.clone(), Type::Reference { mutable, inner } =>
                format!("&{}{}", if *mutable { "mut " } else { "" }, inner.display_name()),
            Type::Array(inner) => format!("[{}]", inner.display_name()),
            Type::Unknown => "<unknown>",
        }.into()
    }
}
