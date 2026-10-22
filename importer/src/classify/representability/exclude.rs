//! Exact Rawr policy exclusion evidence.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Reason {
    reason: Box<str>,
    policy: crate::api::model::Span,
    source: crate::api::model::Span,
}

impl Reason {
    pub(crate) const fn policy(
        reason: Box<str>,
        policy: crate::api::model::Span,
        source: crate::api::model::Span,
    ) -> Self {
        Self {
            reason,
            policy,
            source,
        }
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub const fn policy_span(&self) -> &crate::api::model::Span {
        &self.policy
    }

    #[must_use]
    pub const fn source_span(&self) -> &crate::api::model::Span {
        &self.source
    }
}
