//! Local imported-call proofs over independently authenticated intrinsic facts.

use std::collections::BTreeMap;

use sha2::{Digest as _, Sha256};

use crate::api::{self, identity, ledger, model};
use crate::surface;

mod application;
pub mod call;
pub mod inventory;
pub mod native;
mod resolve;
pub mod scope;

#[cfg(test)]
mod tests;

const RULES: u32 = 1;

/// Immutable union of independently authenticated intrinsic partitions.
/// References retain the exact captures; no merged single-capture Graph is made.
pub struct Catalog<'facts> {
    graphs: Vec<&'facts api::Graph<api::Extracted>>,
    contracts: Vec<&'facts crate::Contract<crate::Classified>>,
    definitions: BTreeMap<&'facts identity::Definition, &'facts model::Definition>,
    implementations: BTreeMap<&'facts identity::Definition, &'facts model::Implementation>,
    partitions: BTreeMap<&'facts identity::Unit, &'facts inventory::Partition>,
    identities: BTreeMap<surface::identity::Definition, &'facts identity::Definition>,
    fingerprint: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the intrinsic inventory union is empty")]
    Empty,
    #[error("independent intrinsic facts disagree for definition {0}")]
    Definition(Box<str>),
    #[error("independent intrinsic facts disagree for implementation {0}")]
    Implementation(Box<str>),
    #[error("independent intrinsic compiler-unit evidence disagrees for {0}")]
    Unit(Box<str>),
    #[error("the imported identity digest names distinct definitions")]
    Identity,
    #[error("the intrinsic inventory union could not retain its index")]
    Allocation,
}

struct Constituent<'facts> {
    graph: &'facts api::Graph<api::Extracted>,
    facts: &'facts inventory::Facts,
    fingerprint: &'facts [u8; 32],
    contract: Option<&'facts crate::Contract<crate::Classified>>,
}

impl<'facts> Catalog<'facts> {
    /// Bind independent captures only when every repeated fact and compiled
    /// unit agrees. All constituent fingerprints remain proof dependencies.
    pub fn join(inventories: &'facts [inventory::Acquired]) -> Result<Self, Error> {
        let constituents = inventories
            .iter()
            .map(|inventory| Constituent {
                graph: inventory.graph(),
                facts: inventory.facts(),
                fingerprint: inventory.fingerprint(),
                contract: None,
            })
            .collect::<Vec<_>>();
        Self::from_constituents(&constituents)
    }

    /// Join ordinary contracts only after each independent intrinsic capture
    /// was classified through its own graph and input proof.
    pub fn prepared(inventories: &'facts [inventory::Prepared]) -> Result<Self, Error> {
        let constituents = inventories
            .iter()
            .map(|inventory| Constituent {
                graph: inventory.contract().raw_graph(),
                facts: inventory.facts(),
                fingerprint: inventory.fingerprint(),
                contract: Some(inventory.contract()),
            })
            .collect::<Vec<_>>();
        Self::from_constituents(&constituents)
    }

    fn from_constituents(inventories: &[Constituent<'facts>]) -> Result<Self, Error> {
        if inventories.is_empty() {
            return Err(Error::Empty);
        }
        let mut compiler = None;
        for partition in inventories
            .iter()
            .flat_map(|inventory| inventory.facts.partitions.iter())
        {
            let current = (&partition.compiler, &partition.target);
            if compiler.is_some_and(|previous| previous != current) {
                return Err(Error::Unit(partition.unit.crate_name().into()));
            }
            compiler = Some(current);
        }
        let mut definitions = BTreeMap::new();
        let mut implementations = BTreeMap::new();
        let mut partitions = BTreeMap::new();
        let mut identities = BTreeMap::new();
        let mut fingerprints = Vec::new();
        fingerprints
            .try_reserve_exact(inventories.len())
            .map_err(|_| Error::Allocation)?;
        for inventory in inventories {
            fingerprints.push(*inventory.fingerprint);
            for definition in &inventory.facts.definitions {
                if let Some(previous) = definitions.insert(definition.id(), definition)
                    && previous != definition
                {
                    return Err(Error::Definition(definition.id().def_path_hash().into()));
                }
                let identity = surface::identity::Definition::of(definition.id());
                if let Some(previous) = identities.insert(identity, definition.id())
                    && previous != definition.id()
                {
                    return Err(Error::Identity);
                }
            }
            for implementation in &inventory.facts.implementations {
                if let Some(previous) = implementations.insert(implementation.id(), implementation)
                    && previous != implementation
                {
                    return Err(Error::Implementation(
                        implementation.id().def_path_hash().into(),
                    ));
                }
            }
            for partition in &inventory.facts.partitions {
                if let Some(previous) = partitions.insert(&partition.unit, partition)
                    && previous != partition
                {
                    return Err(Error::Unit(partition.unit.crate_name().into()));
                }
            }
        }
        fingerprints.sort_unstable();
        fingerprints.dedup();
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.applicability.catalog/v1");
        fingerprint.update(RULES.to_le_bytes());
        for parent in fingerprints {
            fingerprint.update(parent);
        }
        Ok(Self {
            graphs: inventories.iter().map(|part| part.graph).collect(),
            contracts: inventories
                .iter()
                .filter_map(|part| part.contract)
                .collect(),
            definitions,
            implementations,
            partitions,
            identities,
            fingerprint: fingerprint.finalize().into(),
        })
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Resolve a public export to the stable definition identity core already
    /// uses for imported types and traits. Spelling never becomes semantic ID.
    pub fn export(&self, root: &str, path: &[&str]) -> Option<surface::identity::Definition> {
        let mut selected = None;
        for graph in &self.graphs {
            for export in graph.exports() {
                let ledger::Path::Imported(export) = export else {
                    continue;
                };
                if export.target().namespace() == identity::Namespace::Type
                    && export.key().root_alias() == root
                    && export.key().path().eq(path.iter().copied())
                {
                    let identity = surface::identity::Definition::of(export.target());
                    if selected.is_some_and(|previous| previous != identity) {
                        return None;
                    }
                    selected = Some(identity);
                }
            }
        }
        selected
    }

    /// Ordinary constituent contracts remain distinct and independently bound.
    pub fn contracts(&self) -> impl ExactSizeIterator<Item = &crate::Contract<crate::Classified>> {
        self.contracts.iter().copied()
    }

    /// Counts describe facts, not contextual applications or compiler queries.
    pub fn work(&self) -> (usize, usize, usize) {
        (
            self.definitions.len(),
            self.implementations.len(),
            self.partitions.len(),
        )
    }
}
