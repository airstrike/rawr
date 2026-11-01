//! One cumulative graph account across resident checks and native fallback.

use std::sync::OnceLock;

use crate::node;

/// Accounting scope created when the original parent reload request begins.
///
/// This is accounting ownership, not revision, freshness or admission authority.
/// Retain this same scope across resident compilation and any full preparation
/// fallback; creating a new scope would start a different attempt's budget.
#[must_use]
pub struct Scope {
    budget: OnceLock<node::GraphBudget>,
}

/// Exact Rawr snapshot source evidence held with its original graph account.
///
/// Every revalidation charges the same cumulative account, even after the initiating
/// Scope retires. The proof cannot select or substitute another graph account.
#[must_use]
pub struct Freshness {
    snapshot: super::freshness::Freshness,
    budget: node::GraphBudget,
}

impl Scope {
    /// Begin the original reload attempt's cumulative graph account.
    pub fn begin() -> Self {
        Self {
            budget: OnceLock::new(),
        }
    }

    /// Bind exact snapshot frames and check every retained source/manifest now.
    /// Later post-compile and final checks use the returned proof's account.
    pub fn observe(&self, bytes: &super::Bytes) -> Result<Freshness, node::Error> {
        let budget = self.budget();
        let snapshot = bytes.freshness(budget)?;
        snapshot.revalidate(budget)?;
        Ok(Freshness {
            snapshot,
            budget: budget.clone(),
        })
    }

    /// The attempt's one cumulative graph account.
    pub(crate) fn budget(&self) -> &node::GraphBudget {
        self.budget.get_or_init(node::GraphBudget::new)
    }
}

impl Freshness {
    /// Compare with the exact bound transport; this does not replace revalidation.
    #[must_use]
    pub fn matches(&self, snapshot: &super::Bytes) -> bool {
        self.snapshot.matches(snapshot)
    }

    /// Recheck exact Rawr bytes with this original attempt's cumulative account.
    pub fn revalidate(&self) -> Result<(), node::Error> {
        self.snapshot.revalidate(&self.budget)
    }
}

#[cfg(test)]
mod tests;
