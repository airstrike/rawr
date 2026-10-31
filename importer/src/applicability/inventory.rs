//! Complete owned domains, authenticated independently of consumer demand.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
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
    /// What the nightly's build scripts gave, which each entry keeps.
    scripts: super::store::Scripts,
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
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Facts {
    pub(crate) definitions: Box<[model::Definition]>,
    pub(crate) implementations: Box<[model::Implementation]>,
    pub(crate) partitions: Box<[Partition]>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Prelude {
    pub(crate) module: identity::Definition,
    pub(crate) edition: Box<str>,
    pub(crate) traits: Box<[TraitNames]>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct TraitNames {
    pub(crate) id: identity::Definition,
    pub(crate) methods: Box<[Box<str>]>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Association {
    pub(crate) implementation: identity::Definition,
    pub(crate) item: identity::Definition,
    pub(crate) declaration: Option<identity::Definition>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Inherent {
    pub(crate) owner: identity::Definition,
    pub(crate) implementations: Box<[identity::Definition]>,
    pub(crate) associated: Box<[Association]>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Lookup {
    pub(crate) trait_id: identity::Definition,
    pub(crate) domains: Box<[LookupDomain]>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct LookupDomain {
    pub(crate) root: identity::Definition,
    pub(crate) implementations: Box<[LookupHeader]>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct LookupHeader {
    pub(crate) implementation: identity::Definition,
    pub(crate) receiver: Receiver,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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
        let mut resolution = request.resolve(native, limits)?;
        let scripts = resolution.take_nightly_scripts();
        let mut acquired = resolution.publication(native, limits, |resolution| {
            resolution
                .into_extraction_uncached(limits)?
                .inventory(limits)
        })?;
        acquired.scripts = scripts;
        Ok(acquired)
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
            scripts: super::store::Scripts::default(),
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
            ..
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

/// One public export relative to its crate's root: its path, namespace and
/// exact target.
pub(super) type RelativeExport = (Box<[Box<str>]>, identity::Namespace, identity::Definition);

/// The format of a shared crate entry.
/// 2: an entry keeps the nightly's build-script outputs (#403).
const ENTRY_SCHEMA: u32 = 2;

/// One crate's complete intrinsic facts, shared per machine under a key the
/// crate's own native inputs set before any nightly build (D609.3). Its
/// exports are relative to the crate root: the alias a program spells it by
/// is local and applied when entries join (`Catalog::stored`).
#[derive(Deserialize, Serialize)]
pub struct Entry {
    schema: u32,
    key: [u8; 32],
    crate_name: Box<str>,
    exports: Box<[RelativeExport]>,
    facts: Facts,
    /// The outputs the nightly's build scripts gave when the facts were
    /// captured, by package: the entry holds where a build gives the same.
    scripts: super::store::Scripts,
    #[serde(skip)]
    fingerprint: [u8; 32],
}

impl Entry {
    fn seal(mut self) -> Result<Self> {
        self.facts.validate()?;
        if self.schema != ENTRY_SCHEMA
            || self.facts.partitions.len() != 1
            || self.facts.partitions[0].unit.crate_name() != self.crate_name.as_ref()
        {
            return Err(invalid("shared crate entry differs from its one crate"));
        }
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.intrinsic-entry/v1");
        fingerprint.update(
            serde_json::to_vec(&self).map_err(|_| invalid("shared entry encoding failed"))?,
        );
        self.fingerprint = fingerprint.finalize().into();
        Ok(self)
    }

    /// Decode a stored entry; it must be the one its key names.
    pub fn decode(bytes: &[u8], key: &[u8; 32]) -> Result<Self> {
        let entry: Self =
            serde_json::from_slice(bytes).map_err(|_| invalid("shared entry does not decode"))?;
        if &entry.key != key {
            return Err(invalid("shared entry is not the one its key names"));
        }
        entry.seal()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| invalid("shared entry encoding failed"))
    }

    pub const fn key(&self) -> &[u8; 32] {
        &self.key
    }

    pub fn crate_name(&self) -> &str {
        &self.crate_name
    }

    /// The compiler unit whose native inputs this entry was captured from,
    /// and their witness: a consumer's build must agree before reuse.
    pub fn native(&self) -> (&str, &[u8; 32]) {
        let partition = &self.facts.partitions[0];
        (&partition.native_unit, &partition.native_inputs)
    }

    pub(super) fn exports(&self) -> impl Iterator<Item = &RelativeExport> {
        self.exports.iter()
    }

    pub(super) const fn facts(&self) -> &Facts {
        &self.facts
    }

    pub(super) const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// The outputs the nightly's build scripts gave at capture.
    #[must_use]
    pub const fn scripts(&self) -> &super::store::Scripts {
        &self.scripts
    }

    /// Whether `project`'s build gave each of `packages` the build-script
    /// outputs the nightly gave when this entry was captured.
    #[must_use]
    pub fn scripts_agree(&self, packages: &[Box<str>], project: &super::store::Scripts) -> bool {
        self.scripts.agree(packages, project)
    }

    /// A copy of this entry under another key, as a project's entry is
    /// published to rawr's home under its machine-wide key.
    pub fn clone_rekeyed(&self, key: [u8; 32]) -> Result<Self> {
        Self {
            schema: self.schema,
            key,
            crate_name: self.crate_name.clone(),
            exports: self.exports.clone(),
            facts: self.facts.clone(),
            scripts: self.scripts.clone(),
            fingerprint: [0; 32],
        }
        .seal()
    }
}

impl Acquired {
    /// Split this capture into one entry per crate. `key` gives each
    /// crate's shared key from its compiler unit. Declarations of its
    /// dependencies and of crates no partition owns, such as the standard
    /// library's, go with each entry that may read them; identical copies
    /// join as one.
    pub fn entries(&self, key: impl Fn(&str) -> Option<[u8; 32]>) -> Result<Vec<Entry>> {
        let owned = self
            .facts
            .partitions
            .iter()
            .map(|partition| &partition.unit)
            .collect::<BTreeSet<_>>();
        let mut entries = Vec::new();
        for partition in self.facts.partitions.iter() {
            let unit = &partition.unit;
            let key = key(&partition.native_unit)
                .ok_or_else(|| invalid("a captured crate has no shared key"))?;
            // A crate's facts read its dependencies' declarations, which
            // its key covers too; its own implementations are its alone.
            let readable = |owner: &identity::Unit| {
                owner == unit
                    || !owned.contains(owner)
                    || partition.dependencies.binary_search(owner).is_ok()
            };
            let ours = |owner: &identity::Unit| owner == unit || !owned.contains(owner);
            let crate_name = unit.crate_name();
            let mut exports = self
                .graph
                .exports()
                .filter_map(|path| match path {
                    api::ledger::Path::Imported(export)
                        if export.key().root_alias() == crate_name =>
                    {
                        Some((
                            export.key().path().map(Into::into).collect(),
                            export.key().namespace(),
                            export.target().clone(),
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            exports.sort_unstable();
            exports.dedup();
            entries.push(
                Entry {
                    schema: ENTRY_SCHEMA,
                    key,
                    crate_name: crate_name.into(),
                    exports: exports.into_boxed_slice(),
                    facts: Facts {
                        definitions: self
                            .facts
                            .definitions
                            .iter()
                            .filter(|definition| readable(definition.id().unit()))
                            .cloned()
                            .collect(),
                        implementations: self
                            .facts
                            .implementations
                            .iter()
                            .filter(|implementation| ours(implementation.id().unit()))
                            .cloned()
                            .collect(),
                        partitions: Box::new([partition.clone()]),
                    },
                    scripts: self.scripts.clone(),
                    fingerprint: [0; 32],
                }
                .seal()?,
            );
        }
        Ok(entries)
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
