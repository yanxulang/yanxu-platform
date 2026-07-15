//! ABI v2 与平台模型共享的递归值。

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Nil,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Self>),
    Map(BTreeMap<String, Self>),
    Resource(u64),
    Callback(u64),
}

impl Data {
    #[must_use]
    pub fn map(entries: impl IntoIterator<Item = (impl Into<String>, Self)>) -> Self {
        Self::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_map(&self) -> Option<&BTreeMap<String, Self>> {
        if let Self::Map(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

impl From<bool> for Data {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Data {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for Data {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<String> for Data {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Data {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}
