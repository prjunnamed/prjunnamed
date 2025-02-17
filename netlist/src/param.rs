use crate::{Const, Trit};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamValue {
    Const(Const),
    Int(i64),
    Float(u64),
    String(String),
}

impl From<bool> for ParamValue {
    fn from(value: bool) -> Self {
        Self::Const(Trit::from(value).into())
    }
}

impl From<Trit> for ParamValue {
    fn from(value: Trit) -> Self {
        Self::Const(value.into())
    }
}

impl From<Const> for ParamValue {
    fn from(value: Const) -> Self {
        Self::Const(value)
    }
}

impl From<&Const> for ParamValue {
    fn from(value: &Const) -> Self {
        Self::Const(value.clone())
    }
}

impl From<i64> for ParamValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<String> for ParamValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for ParamValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
