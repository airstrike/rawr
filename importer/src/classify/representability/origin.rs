//! Rust export and declaration positions retained together.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Set {
    source: crate::api::model::Span,
    declaration: crate::api::model::Span,
}

impl Set {
    pub(crate) const fn new(
        source: crate::api::model::Span,
        declaration: crate::api::model::Span,
    ) -> Self {
        Self {
            source,
            declaration,
        }
    }

    #[must_use]
    pub const fn source(&self) -> &crate::api::model::Span {
        &self.source
    }

    #[must_use]
    pub const fn declaration(&self) -> &crate::api::model::Span {
        &self.declaration
    }
}
