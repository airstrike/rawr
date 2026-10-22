//! Stable identifiers for Rawr enum variants.

use crate::{item, ImmutableString};
use std::fmt;

/// A source-stable enum variant identifier.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Id(ImmutableString);

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Id {
    /// Transform a parser-validated source name into a stable variant ID.
    pub(crate) fn from_name(name: &item::Name) -> Self {
        Self(name.text.clone())
    }

    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}
