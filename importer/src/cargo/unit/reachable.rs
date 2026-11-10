//! The units a set of root units reaches through the build graph.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
use super::OWNED_REACHABILITY_NODES;
use super::{BuildGraph, JsonAllocation};
use crate::{Error, Result};

/// Every semantic key a set of roots reaches, the roots included. It is
/// walked once, so asking about any number of units costs one lookup each.
pub(crate) struct Reachable<'a>(BTreeSet<&'a str>);

/// The graph nodes reachability walks have touched on this thread.
#[cfg(test)]
pub(crate) fn walked_nodes() -> usize {
    OWNED_REACHABILITY_NODES.with(std::cell::Cell::get)
}

impl Reachable<'_> {
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.0.contains(key)
    }
}

impl BuildGraph {
    /// The units the selected extraction units reach.
    pub(crate) fn selected_reachable(&self) -> Result<Reachable<'_>> {
        let mut allocation = self._allocation;
        let mut roots = BTreeSet::new();
        for extraction in &self.extraction {
            let key = extraction.semantic_key();
            if !roots.contains(key) {
                allocation.member_nodes("Cargo reachability selected-root nodes", 1)?;
                roots.insert(key);
            }
        }
        self.reachable_budgeted(&roots, &mut allocation)
    }

    pub(crate) fn reachable<'a>(&'a self, roots: &BTreeSet<&'a str>) -> Result<Reachable<'a>> {
        let mut allocation = self._allocation;
        self.reachable_budgeted(roots, &mut allocation)
    }

    #[cfg(test)]
    pub(super) fn artifact_reachable(&self, roots: &BTreeSet<&str>, target: &str) -> Result<bool> {
        Ok(self.reachable(roots)?.contains(target))
    }

    fn reachable_budgeted<'a>(
        &'a self,
        roots: &BTreeSet<&'a str>,
        allocation: &mut JsonAllocation,
    ) -> Result<Reachable<'a>> {
        allocation.member_nodes(
            "Cargo reachability artifact-index nodes",
            self.artifacts.len(),
        )?;
        let mut artifacts = BTreeMap::new();
        for artifact in &self.artifacts {
            artifacts.insert(artifact.semantic_key(), artifact);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
        }
        allocation.nodes("Cargo reachability pending nodes", roots.len())?;
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(roots.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo artifact reachability",
                source,
            })?;
        for root in roots {
            pending.push(*root);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
        }
        let mut visited = BTreeSet::new();
        while let Some(key) = pending.pop() {
            if visited.contains(key) {
                continue;
            }
            allocation.member_nodes("Cargo reachability visited nodes", 1)?;
            visited.insert(key);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
            if let Some(artifact) = artifacts.get(key) {
                for dependency in artifact.dependency_keys() {
                    allocation.nodes("Cargo reachability pending nodes", 1)?;
                    pending.try_reserve(1).map_err(|source| Error::Allocation {
                        operation: "Cargo artifact reachability",
                        source,
                    })?;
                    pending.push(dependency);
                    #[cfg(test)]
                    OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
                }
            }
        }
        Ok(Reachable(visited))
    }

    #[cfg(test)]
    pub(super) fn artifact_reachable_allocation(
        &self,
        roots: &BTreeSet<&str>,
    ) -> Result<JsonAllocation> {
        let mut allocation = self._allocation;
        let _ = self.reachable_budgeted(roots, &mut allocation)?;
        Ok(allocation)
    }
}
