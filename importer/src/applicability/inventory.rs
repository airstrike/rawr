//! Complete owned domains, authenticated independently of consumer demand.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::api::{self, identity, model};
use crate::cargo;
use crate::{Error, Result};

/// One independently captured graph and its complete intrinsic fact payload.
/// A demand-selected graph cannot be promoted into this type.
pub struct Acquired {
    graph: api::Graph<api::Extracted>,
    facts: Facts,
    fingerprint: [u8; 32],
}

/// Intrinsic facts with their own independently classified ordinary contract.
/// Classification never merges source graphs or changes their input authority.
pub struct Prepared {
    contract: crate::Contract<crate::Classified>,
    facts: Facts,
    fingerprint: [u8; 32],
}

impl Prepared {
    pub const fn contract(&self) -> &crate::Contract<crate::Classified> {
        &self.contract
    }
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    pub(super) const fn facts(&self) -> &Facts {
        &self.facts
    }
}

/// Unvalidated canonical facts from one authenticated compiler document.
#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Facts {
    pub(crate) definitions: Box<[model::Definition]>,
    pub(crate) implementations: Box<[model::Implementation]>,
    pub(crate) partitions: Box<[Partition]>,
}

#[derive(Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Partition {
    pub(crate) unit: identity::Unit,
    pub(crate) native_unit: Box<str>,
    pub(crate) native_inputs: [u8; 32],
    pub(crate) compiler: Box<str>,
    pub(crate) target: Box<str>,
    pub(crate) crate_hash: Box<str>,
    pub(crate) dependencies: Box<[identity::Unit]>,
    pub(crate) missing_dependencies: Box<[Box<str>]>,
    pub(crate) implementations: Box<[identity::Definition]>,
    pub(crate) associated: Box<[Association]>,
    pub(crate) unsupported: Box<[identity::Definition]>,
    pub(crate) inherent: Box<[Inherent]>,
    pub(crate) preludes: Box<[Prelude]>,
    pub(crate) deref: Option<Lookup>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Prelude {
    pub(crate) module: identity::Definition,
    pub(crate) edition: Box<str>,
    pub(crate) traits: Box<[TraitNames]>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct TraitNames {
    pub(crate) id: identity::Definition,
    pub(crate) methods: Box<[Box<str>]>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Association {
    pub(crate) implementation: identity::Definition,
    pub(crate) item: identity::Definition,
    pub(crate) declaration: Option<identity::Definition>,
}

#[derive(Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Inherent {
    pub(crate) owner: identity::Definition,
    pub(crate) implementations: Box<[identity::Definition]>,
    pub(crate) associated: Box<[Association]>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Lookup {
    pub(crate) trait_id: identity::Definition,
    pub(crate) domains: Box<[LookupDomain]>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct LookupDomain {
    pub(crate) root: identity::Definition,
    pub(crate) implementations: Box<[LookupHeader]>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct LookupHeader {
    pub(crate) implementation: identity::Definition,
    pub(crate) receiver: Receiver,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Receiver {
    Nominal(identity::Definition),
    Closed,
    Unresolved,
}

impl Acquired {
    pub(super) const fn facts(&self) -> &Facts {
        &self.facts
    }
    /// Acquire intrinsic facts through normal artifact and input authentication.
    /// There is no constructor from caller-provided facts.
    pub fn capture(
        request: cargo::Request,
        native: &crate::process::Native,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let resolution = request.resolve(native, limits)?;
        resolution.publication(native, limits, |resolution| {
            resolution
                .into_extraction_uncached(limits)?
                .inventory(limits)
        })
    }

    pub(crate) fn authenticate(graph: api::Graph<api::Extracted>, facts: Facts) -> Result<Self> {
        facts.validate()?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.intrinsic-inventory/v1");
        fingerprint.update(graph.fingerprint().as_bytes());
        let canonical =
            serde_json::to_vec(&facts).map_err(|_| invalid("intrinsic fact encoding failed"))?;
        fingerprint.update(canonical);
        Ok(Self {
            graph,
            facts,
            fingerprint: fingerprint.finalize().into(),
        })
    }

    /// Synthetic corruption is available only to invariant tests. Production
    /// capture still has no constructor from caller-supplied canonical facts.
    #[cfg(test)]
    pub(super) fn mutate_for_test(self, change: impl FnOnce(&mut Facts)) -> Result<Self> {
        let Self {
            graph, mut facts, ..
        } = self;
        change(&mut facts);
        Self::authenticate(graph, facts)
    }

    #[cfg(test)]
    pub(super) fn rejects_for_test(&self, change: impl FnOnce(&mut Facts)) -> bool {
        let mut facts = self.facts.clone();
        change(&mut facts);
        facts.validate().is_err()
    }

    /// Classify this constituent alone; the union cannot fabricate a contract
    /// from a graph captured in a different compiler session.
    pub fn classify(
        self,
        limits: &cargo::Limits,
    ) -> std::result::Result<Prepared, crate::classify::Error> {
        let Self {
            graph,
            facts,
            fingerprint,
        } = self;
        let contract = graph.classify(&crate::classify::Policy::standard(), limits)?;
        Ok(Prepared {
            contract,
            facts,
            fingerprint,
        })
    }

    /// Identity includes the full fact payload and exact completeness domains.
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// The ordinary selected graph retains its normal closure semantics.
    pub const fn graph(&self) -> &api::Graph<api::Extracted> {
        &self.graph
    }

    /// Full intrinsic definitions, including declarations outside that closure.
    pub fn definitions(&self) -> impl ExactSizeIterator<Item = &model::Definition> {
        self.facts.definitions.iter()
    }

    pub fn implementations(&self) -> impl ExactSizeIterator<Item = &model::Implementation> {
        self.facts.implementations.iter()
    }

    /// Number of independently completed selected-crate domains.
    pub fn partitions(&self) -> usize {
        self.facts.partitions.len()
    }
}

impl Facts {
    fn validate(&self) -> Result<()> {
        if self.partitions.is_empty()
            || self
                .partitions
                .windows(2)
                .any(|pair| pair[0].unit >= pair[1].unit)
        {
            return Err(invalid(
                "intrinsic unit domains are absent, unsorted or duplicated",
            ));
        }
        if self
            .definitions
            .windows(2)
            .any(|pair| pair[0].id() >= pair[1].id())
            || self
                .implementations
                .windows(2)
                .any(|pair| pair[0].id() >= pair[1].id())
        {
            return Err(invalid("intrinsic facts are unsorted or duplicated"));
        }
        let definitions = self
            .definitions
            .iter()
            .map(|item| (item.id(), item))
            .collect::<BTreeMap<_, _>>();
        let implementations = self
            .implementations
            .iter()
            .map(|item| (item.id(), item))
            .collect::<BTreeMap<_, _>>();
        let mut declared = BTreeSet::new();
        for partition in &self.partitions {
            if let Some(lookup) = &partition.deref {
                if !matches!(
                    lookup.trait_id.unit().authority(),
                    identity::Authority::Sysroot(_)
                ) || lookup.trait_id.namespace() != identity::Namespace::Type
                    || lookup
                        .domains
                        .windows(2)
                        .any(|pair| pair[0].root >= pair[1].root)
                    || !lookup
                        .domains
                        .iter()
                        .any(|domain| domain.root.unit() == lookup.trait_id.unit())
                {
                    return Err(invalid(
                        "intrinsic Deref domain has invalid trait or crate authority",
                    ));
                }
                for domain in &lookup.domains {
                    if !matches!(domain.root.unit().authority(),identity::Authority::Sysroot(_))
                        || domain.root.namespace() != identity::Namespace::Type || domain.implementations.windows(2).any(|pair| pair[0].implementation >= pair[1].implementation)
                        || domain.implementations.iter().any(|header| header.implementation.unit() != domain.root.unit()
                            || header.implementation.namespace() != identity::Namespace::Type
                            || matches!(&header.receiver,Receiver::Nominal(id) if id.namespace() != identity::Namespace::Type)) {
                        return Err(invalid("intrinsic Deref headers differ from their exact upstream domain"));
                    }
                }
            }
            if partition
                .preludes
                .windows(2)
                .any(|pair| pair[0].module >= pair[1].module)
            {
                return Err(invalid(
                    "intrinsic prelude domains are unsorted or duplicated",
                ));
            }
            for prelude in &partition.preludes {
                if !matches!(
                    prelude.module.unit().authority(),
                    identity::Authority::Sysroot(_)
                ) || !matches!(prelude.edition.as_ref(), "2015" | "2018" | "2021" | "2024")
                    || prelude
                        .traits
                        .windows(2)
                        .any(|pair| pair[0].id >= pair[1].id)
                    || prelude.traits.iter().any(|trait_| {
                        trait_.id.namespace() != identity::Namespace::Type
                            || !matches!(
                                trait_.id.unit().authority(),
                                identity::Authority::Sysroot(_)
                            )
                            || !ordered(&trait_.methods)
                    })
                {
                    return Err(invalid(
                        "intrinsic prelude domain has invalid compiler authority or method names",
                    ));
                }
            }
            if !ordered(&partition.dependencies) || !ordered(&partition.missing_dependencies) {
                return Err(invalid(
                    "intrinsic dependency evidence is unsorted or duplicated",
                ));
            }
            if !ordered(&partition.implementations)
                || !ordered(&partition.unsupported)
                || partition
                    .unsupported
                    .iter()
                    .any(|id| partition.implementations.binary_search(id).is_err())
            {
                return Err(invalid(
                    "intrinsic trait domain or unsupported subset differs",
                ));
            }
            if partition
                .inherent
                .windows(2)
                .any(|pair| pair[0].owner >= pair[1].owner)
            {
                return Err(invalid(
                    "intrinsic inherent domains are unsorted or duplicated",
                ));
            }
            for id in &partition.implementations {
                if id.unit() != &partition.unit
                    || id.namespace() != identity::Namespace::Type
                    || !declared.insert(id)
                {
                    return Err(invalid(
                        "intrinsic implementation has a different exact owning unit or is repeated",
                    ));
                }
                if partition.unsupported.binary_search(id).is_ok() {
                    if implementations.contains_key(id) {
                        return Err(invalid("unsupported implementation also has a usable body"));
                    }
                    continue;
                }
                let implementation = implementations
                    .get(id)
                    .ok_or_else(|| invalid("owned trait implementation is absent"))?;
                if !matches!(implementation.trait_ref(), model::Trait::Trait { .. }) {
                    return Err(invalid("trait domain contains an inherent implementation"));
                }
                if implementation.key() != id.def_path_hash() {
                    return Err(invalid("contextual application entered intrinsic facts"));
                }
            }
            associations(
                &partition.implementations,
                &partition.associated,
                &definitions,
                &implementations,
            )?;
            let nominal_owners = self
                .definitions
                .iter()
                .filter(|definition| {
                    definition.id().unit() == &partition.unit
                        && matches!(
                            definition.kind(),
                            model::Kind::Struct { .. }
                                | model::Kind::Enum { .. }
                                | model::Kind::Union { .. }
                        )
                })
                .map(model::Definition::id)
                .collect::<BTreeSet<_>>();
            if nominal_owners
                != partition
                    .inherent
                    .iter()
                    .map(|domain| &domain.owner)
                    .collect()
            {
                let recorded = partition
                    .inherent
                    .iter()
                    .map(|domain| &domain.owner)
                    .collect::<BTreeSet<_>>();
                let missing = nominal_owners
                    .difference(&recorded)
                    .map(|id| format!("{}:{}", id.def_path_hash(), id.namespace().as_str()))
                    .collect::<Vec<_>>();
                let extra = recorded
                    .difference(&nominal_owners)
                    .map(|id| format!("{}:{}", id.def_path_hash(), id.namespace().as_str()))
                    .collect::<Vec<_>>();
                return Err(invalid(format!("intrinsic inherent domains differ for {}: missing {missing:?}; extra {extra:?}",partition.unit.crate_name())));
            }
            for domain in &partition.inherent {
                if domain.owner.unit() != &partition.unit
                    || !ordered(&domain.implementations)
                    || !definitions.get(&domain.owner).is_some_and(|definition| {
                        matches!(
                            definition.kind(),
                            model::Kind::Struct { .. }
                                | model::Kind::Enum { .. }
                                | model::Kind::Union { .. }
                        )
                    })
                {
                    return Err(invalid(
                        "intrinsic inherent owner is absent or has another exact unit",
                    ));
                }
                for id in &domain.implementations {
                    if id.unit() != &partition.unit
                        || id.namespace() != identity::Namespace::Type
                        || !declared.insert(id)
                    {
                        return Err(invalid("intrinsic inherent implementation has another exact unit or is repeated"));
                    }
                    let implementation = implementations
                        .get(id)
                        .ok_or_else(|| invalid("inherent implementation is absent"))?;
                    if !matches!(implementation.trait_ref(), model::Trait::Inherent)
                        || !matches!(implementation.self_type(), model::Type::Definition { id, .. } if id == &domain.owner)
                        || implementation.key() != id.def_path_hash()
                    {
                        return Err(invalid(
                            "inherent implementation does not belong to its nominal domain",
                        ));
                    }
                }
                associations(
                    &domain.implementations,
                    &domain.associated,
                    &definitions,
                    &implementations,
                )?;
            }
        }
        if implementations.keys().any(|id| !declared.contains(id)) {
            return Err(invalid(
                "intrinsic facts contain an undeclared implementation",
            ));
        }
        Ok(())
    }
}

fn associations(
    domain: &[identity::Definition],
    links: &[Association],
    definitions: &BTreeMap<&identity::Definition, &model::Definition>,
    implementations: &BTreeMap<&identity::Definition, &model::Implementation>,
) -> Result<()> {
    if !ordered(links) {
        return Err(invalid(
            "intrinsic associated links are unsorted or duplicated",
        ));
    }
    let mut actual = BTreeSet::new();
    for link in links {
        if domain.binary_search(&link.implementation).is_err()
            || !actual.insert((&link.implementation, &link.item))
        {
            return Err(invalid(
                "intrinsic associated link has another implementation or is repeated",
            ));
        }
        let definition = definitions
            .get(&link.item)
            .ok_or_else(|| invalid("intrinsic associated item is absent"))?;
        if !matches!(definition.owner(), model::Owner::Implementation(id) if id == &link.implementation)
        {
            return Err(invalid("intrinsic associated item's exact owner differs"));
        }
        let implementation = implementations
            .get(&link.implementation)
            .ok_or_else(|| invalid("intrinsic associated implementation is absent"))?;
        match (implementation.trait_ref(), &link.declaration) {
            (model::Trait::Trait { id, .. }, Some(declaration)) => {
                let declaration = definitions
                    .get(declaration)
                    .ok_or_else(|| invalid("intrinsic associated declaration is absent"))?;
                if !matches!(declaration.owner(), model::Owner::Definition(owner) if owner == id)
                    || declaration.id().namespace() != link.item.namespace()
                {
                    return Err(invalid(
                        "intrinsic associated declaration has another trait or namespace",
                    ));
                }
            }
            (model::Trait::Inherent, None) => {}
            _ => {
                return Err(invalid(
                    "intrinsic associated declaration correspondence differs",
                ))
            }
        }
    }
    let expected = domain
        .iter()
        .filter_map(|id| implementations.get(id))
        .flat_map(|implementation| {
            implementation
                .associated()
                .map(|item| (implementation.id(), item))
        })
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid(
            "intrinsic associated domain does not match the implementation facts",
        ));
    }
    Ok(())
}

fn ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn invalid(reason: impl Into<Box<str>>) -> Error {
    Error::Extraction {
        code: "intrinsic-inventory",
        reason: reason.into(),
    }
}
