//! One cumulative graph account across resident checks and native fallback.

use std::sync::OnceLock;

use crate::{cargo, node};

/// Accounting scope created when the original parent reload request begins.
///
/// This is accounting ownership, not revision, freshness or admission authority.
/// Retain this same scope across resident compilation and any full preparation
/// fallback; creating a new scope would start a different attempt's budget.
///
/// The scope begins before the project's `[rust.limits]` profile is read, so
/// its graph account binds its accounting targets on the first route
/// that brings the captured profile; every later route of the same attempt
/// measures against that same profile.
#[must_use]
pub struct Scope {
    budget: OnceLock<node::GraphBudget>,
}

/// Exact Rawr snapshot source evidence held with its original graph account.
///
/// Every revalidation uses the same cumulative limits, even after the initiating
/// Scope retires. The proof cannot select or substitute another graph account.
#[must_use]
pub struct Freshness {
    snapshot: super::freshness::Freshness,
    budget: node::GraphBudget,
}

impl Scope {
    /// Begin the original reload attempt's unchanged cumulative graph limits.
    pub fn begin() -> Self {
        Self {
            budget: OnceLock::new(),
        }
    }

    /// Bind exact snapshot frames and check every retained source/manifest now.
    /// Later post-compile and final checks use the returned proof's account.
    pub fn observe(
        &self,
        bytes: &super::Bytes,
        limits: &cargo::Limits,
    ) -> Result<Freshness, node::Error> {
        let budget = self.budget_for(limits)?;
        let snapshot = bytes.freshness(budget, limits)?;
        snapshot.revalidate(budget)?;
        Ok(Freshness {
            snapshot,
            budget: budget.clone(),
        })
    }

    /// The attempt's one cumulative graph account, measured against the
    /// captured profile's graph targets.
    ///
    /// The first route to bring the profile binds the account's targets. One
    /// attempt has one captured profile across its resident, owner and
    /// fallback routes, so a later route bringing different graph targets is a
    /// configuration drift within the attempt and is refused as invalid state;
    /// the account is never rebound or replaced.
    pub(crate) fn budget_for(
        &self,
        limits: &cargo::Limits,
    ) -> Result<&node::GraphBudget, node::Error> {
        let targets = limits.graph_targets();
        let budget = self
            .budget
            .get_or_init(|| node::GraphBudget::with_targets(targets));
        if budget.targets() == targets {
            Ok(budget)
        } else {
            Err(node::Error::State(
                "reload attempt graph targets differ from the profile that began its account",
            ))
        }
    }

    /// The attempt's graph account; a scope no route has bound yet measures
    /// against the standard targets.
    #[cfg(test)]
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
