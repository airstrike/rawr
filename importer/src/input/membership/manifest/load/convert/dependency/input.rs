//! Borrowed declarations share conversion without discarding normalized paths.
use crate::input::membership::manifest::{
    load::normalize::{self, dependencies, inherit},
    schema::dependency::{direct, Field},
};

#[derive(Clone, Copy)]
pub(in super::super::super) enum Input<'a, 'd> {
    Normalized(&'a dependencies::Dependency<'a, 'd>),
    Patch(&'a normalize::Dependency<'a, 'd>),
    Direct {
        name: &'a str,
        value: &'a direct::Direct<'d>,
    },
}

impl<'a, 'd> From<&'a dependencies::Dependency<'a, 'd>> for Input<'a, 'd> {
    fn from(value: &'a dependencies::Dependency<'a, 'd>) -> Self {
        Self::Normalized(value)
    }
}
impl<'a, 'd> From<&'a normalize::Dependency<'a, 'd>> for Input<'a, 'd> {
    fn from(value: &'a normalize::Dependency<'a, 'd>) -> Self {
        Self::Patch(value)
    }
}

impl<'a, 'd> Input<'a, 'd> {
    pub(super) fn name(self) -> &'a str {
        match self {
            Self::Normalized(value) => value.name(),
            Self::Patch(value) => value.name.as_str(),
            Self::Direct { name, .. } => name,
        }
    }
    pub(super) fn simple(self) -> Option<&'a str> {
        match self {
            Self::Normalized(value) => value.simple(),
            Self::Patch(value) => value.simple(),
            Self::Direct { value, .. } => match value.kind() {
                direct::Kind::Simple(value) => Some(value.as_str()),
                direct::Kind::Detailed(_) => None,
            },
        }
    }
    pub(super) fn get(self, field: Field) -> Option<inherit::Value<'a, 'd>> {
        match self {
            Self::Normalized(value) => value.get(field),
            Self::Patch(value) => value.get(field).map(|value| match value {
                normalize::Field::Original(value) => inherit::Value::Original(value),
                normalize::Field::Path(value) => inherit::Value::Text(value),
            }),
            Self::Direct { value, .. } => match value.kind() {
                direct::Kind::Simple(_) => None,
                direct::Kind::Detailed(value) => value.get(field).map(inherit::Value::Original),
            },
        }
    }
}
