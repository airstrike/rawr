//! Preview-host theme selection.

use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Choice {
    Inherited,
    Explicit { index: usize, name: String },
}

impl fmt::Display for Choice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inherited => formatter.write_str("Inherited theme"),
            Self::Explicit { name, .. } => formatter.write_str(name),
        }
    }
}
