//! Complete owned domains, authenticated independently of consumer demand.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::api::{identity, model};
use crate::cargo;
use crate::Result;

/// One independent capture: its crates' public exports with their targets,
/// and their complete intrinsic fact payload. No API graph of the capture
/// is built. A demand-selected capture cannot be promoted into this type.
pub struct Acquired {
    exports: Box<[(identity::Export, identity::Definition)]>,
    facts: Facts,
    fingerprint: [u8; 32],
    /// What the nightly's build scripts gave, which each entry keeps.
    scripts: super::store::Scripts,
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

    /// Seal facts extraction assembled. Facts that break their own
    /// invariants are a bug in that assembly.
    pub(crate) fn authenticate(
        exports: Box<[(identity::Export, identity::Definition)]>,
        facts: Facts,
    ) -> Self {
        if let Err(violation) = facts.validate() {
            panic!("captured crate facts break their invariant: {violation}");
        }
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.intrinsic-inventory/v2");
        fingerprint.update(serde_json::to_vec(&exports).expect("exports encode as JSON"));
        fingerprint.update(serde_json::to_vec(&facts).expect("facts encode as JSON"));
        Self {
            exports,
            facts,
            fingerprint: fingerprint.finalize().into(),
            scripts: super::store::Scripts::default(),
        }
    }

    /// Synthetic corruption is available only to invariant tests. Production
    /// capture still has no constructor from caller-supplied canonical facts.
    #[cfg(test)]
    pub(super) fn mutate_for_test(self, change: impl FnOnce(&mut Facts)) -> Self {
        let Self {
            exports, mut facts, ..
        } = self;
        change(&mut facts);
        Self::authenticate(exports, facts)
    }

    #[cfg(test)]
    pub(super) fn rejects_for_test(&self, change: impl FnOnce(&mut Facts)) -> bool {
        let mut facts = self.facts.clone();
        change(&mut facts);
        facts.validate().is_err()
    }

    /// Identity includes the full fact payload and exact completeness domains.
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Each public export, with its target.
    pub fn exports(
        &self,
    ) -> impl ExactSizeIterator<Item = &(identity::Export, identity::Definition)> {
        self.exports.iter()
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
    /// Whether this entry is one crate's whole facts, as `entries` makes it.
    fn check(&self) -> std::result::Result<(), Box<str>> {
        self.facts.validate()?;
        if self.schema != ENTRY_SCHEMA
            || self.facts.partitions.len() != 1
            || self.facts.partitions[0].unit.crate_name() != self.crate_name.as_ref()
        {
            return Err("the entry is not its one crate's facts".into());
        }
        Ok(())
    }

    fn sealed(mut self) -> Self {
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.intrinsic-entry/v1");
        fingerprint.update(self.encode());
        self.fingerprint = fingerprint.finalize().into();
        self
    }

    /// Decode a stored entry. One that does not decode, is not the one
    /// `key` names, or does not hold together is absent.
    #[must_use]
    pub fn decode(bytes: &[u8], key: &[u8; 32]) -> Option<Self> {
        let entry: Self = serde_json::from_slice(bytes).ok()?;
        (&entry.key == key && entry.check().is_ok()).then(|| entry.sealed())
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an entry encodes as JSON")
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
    #[must_use]
    pub fn clone_rekeyed(&self, key: [u8; 32]) -> Self {
        Self {
            schema: self.schema,
            key,
            crate_name: self.crate_name.clone(),
            exports: self.exports.clone(),
            facts: self.facts.clone(),
            scripts: self.scripts.clone(),
            fingerprint: [0; 32],
        }
        .sealed()
    }
}

impl Acquired {
    /// Split this capture into one entry per crate. `key` gives each
    /// crate's shared key from its compiler unit. Declarations of its
    /// dependencies and of crates no partition owns, such as the standard
    /// library's, go with each entry that may read them; identical copies
    /// join as one.
    ///
    /// # Panics
    ///
    /// When `key` has no key for a crate this capture holds, as the
    /// caller keys every crate of the closure it captured.
    pub fn entries(&self, key: impl Fn(&str) -> Option<[u8; 32]>) -> Vec<Entry> {
        let owned = self
            .facts
            .partitions
            .iter()
            .map(|partition| &partition.unit)
            .collect::<BTreeSet<_>>();
        let mut entries = Vec::new();
        for partition in self.facts.partitions.iter() {
            let unit = &partition.unit;
            let key = key(&partition.native_unit).unwrap_or_else(|| {
                panic!(
                    "captured crate `{}` has no key in the closure it was captured for",
                    unit.crate_name()
                )
            });
            // A crate's facts read declarations of its whole dependency
            // closure, which its key covers too: an implementation of a
            // trait a dependency re-exports from its own dependency names
            // that crate's declarations. Its own implementations are its
            // alone.
            let mut closure = BTreeSet::from([unit]);
            let mut pending = vec![partition];
            while let Some(current) = pending.pop() {
                for dependency in &current.dependencies {
                    if closure.insert(dependency)
                        && let Ok(next) = self
                            .facts
                            .partitions
                            .binary_search_by(|partition| partition.unit.cmp(dependency))
                    {
                        pending.push(&self.facts.partitions[next]);
                    }
                }
            }
            let readable =
                |owner: &identity::Unit| !owned.contains(owner) || closure.contains(owner);
            let ours = |owner: &identity::Unit| owner == unit || !owned.contains(owner);
            let crate_name = unit.crate_name();
            let mut exports = self
                .exports
                .iter()
                .filter(|(key, _)| key.root_alias() == crate_name)
                .map(|(key, target)| {
                    (
                        key.path().map(Into::into).collect(),
                        key.namespace(),
                        target.clone(),
                    )
                })
                .collect::<Vec<_>>();
            exports.sort_unstable();
            exports.dedup();
            let entry = Entry {
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
            };
            if let Err(violation) = entry.check() {
                panic!(
                    "crate `{crate_name}`'s share of its capture breaks its invariant: {violation}"
                );
            }
            entries.push(entry.sealed());
        }
        entries
    }
}

impl Partition {
    /// List each of `unmodeled` this crate declares as unsupported: it
    /// exists, and the facts hold no body for it.
    pub(crate) fn mark_unsupported(&mut self, unmodeled: &BTreeSet<identity::Definition>) {
        let mut unsupported = self.unsupported.to_vec();
        unsupported.extend(
            self.implementations
                .iter()
                .chain(
                    self.inherent
                        .iter()
                        .flat_map(|domain| domain.implementations.iter()),
                )
                .filter(|id| unmodeled.contains(*id))
                .cloned(),
        );
        unsupported.sort_unstable();
        unsupported.dedup();
        self.unsupported = unsupported.into_boxed_slice();
    }
}

impl Facts {
    /// Whether these facts hold together. The violation is rawr's own
    /// wording, for a panic or a discarded stored entry.
    fn validate(&self) -> std::result::Result<(), Box<str>> {
        if self.partitions.is_empty()
            || self
                .partitions
                .windows(2)
                .any(|pair| pair[0].unit >= pair[1].unit)
        {
            return Err(violation(
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
            return Err(violation("intrinsic facts are unsorted or duplicated"));
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
                    return Err(violation(
                        "intrinsic Deref domain has invalid trait or crate authority",
                    ));
                }
                for domain in &lookup.domains {
                    if !matches!(domain.root.unit().authority(),identity::Authority::Sysroot(_))
                        || domain.root.namespace() != identity::Namespace::Type || domain.implementations.windows(2).any(|pair| pair[0].implementation >= pair[1].implementation)
                        || domain.implementations.iter().any(|header| header.implementation.unit() != domain.root.unit()
                            || header.implementation.namespace() != identity::Namespace::Type
                            || matches!(&header.receiver,Receiver::Nominal(id) if id.namespace() != identity::Namespace::Type)) {
                        return Err(violation("intrinsic Deref headers differ from their exact upstream domain"));
                    }
                }
            }
            if partition
                .preludes
                .windows(2)
                .any(|pair| pair[0].module >= pair[1].module)
            {
                return Err(violation(
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
                    return Err(violation(
                        "intrinsic prelude domain has invalid compiler authority or method names",
                    ));
                }
            }
            if !ordered(&partition.dependencies) || !ordered(&partition.missing_dependencies) {
                return Err(violation(
                    "intrinsic dependency evidence is unsorted or duplicated",
                ));
            }
            if !ordered(&partition.implementations)
                || !ordered(&partition.unsupported)
                || partition.unsupported.iter().any(|id| {
                    partition.implementations.binary_search(id).is_err()
                        && !partition
                            .inherent
                            .iter()
                            .any(|domain| domain.implementations.binary_search(id).is_ok())
                })
            {
                return Err(violation(
                    "intrinsic trait domain or unsupported subset differs",
                ));
            }
            if partition
                .inherent
                .windows(2)
                .any(|pair| pair[0].owner >= pair[1].owner)
            {
                return Err(violation(
                    "intrinsic inherent domains are unsorted or duplicated",
                ));
            }
            for id in &partition.implementations {
                if id.unit() != &partition.unit
                    || id.namespace() != identity::Namespace::Type
                    || !declared.insert(id)
                {
                    return Err(violation(
                        "intrinsic implementation has a different exact owning unit or is repeated",
                    ));
                }
                if partition.unsupported.binary_search(id).is_ok() {
                    if implementations.contains_key(id) {
                        return Err(violation(
                            "unsupported implementation also has a usable body",
                        ));
                    }
                    continue;
                }
                let implementation = implementations
                    .get(id)
                    .ok_or_else(|| violation("owned trait implementation is absent"))?;
                if !matches!(implementation.trait_ref(), model::Trait::Trait { .. }) {
                    return Err(violation(
                        "trait domain contains an inherent implementation",
                    ));
                }
                if implementation.key() != id.def_path_hash() {
                    return Err(violation("contextual application entered intrinsic facts"));
                }
            }
            associations(
                &partition.implementations,
                &partition.associated,
                &partition.unsupported,
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
                return Err(violation(format!("intrinsic inherent domains differ for {}: missing {missing:?}; extra {extra:?}",partition.unit.crate_name())));
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
                    return Err(violation(
                        "intrinsic inherent owner is absent or has another exact unit",
                    ));
                }
                for id in &domain.implementations {
                    if id.unit() != &partition.unit
                        || id.namespace() != identity::Namespace::Type
                        || !declared.insert(id)
                    {
                        return Err(violation("intrinsic inherent implementation has another exact unit or is repeated"));
                    }
                    if partition.unsupported.binary_search(id).is_ok() {
                        if implementations.contains_key(id) {
                            return Err(violation(
                                "unsupported implementation also has a usable body",
                            ));
                        }
                        continue;
                    }
                    let implementation = implementations
                        .get(id)
                        .ok_or_else(|| violation("inherent implementation is absent"))?;
                    if !matches!(implementation.trait_ref(), model::Trait::Inherent)
                        || !matches!(implementation.self_type(), model::Type::Definition { id, .. } if id == &domain.owner)
                        || implementation.key() != id.def_path_hash()
                    {
                        return Err(violation(
                            "inherent implementation does not belong to its nominal domain",
                        ));
                    }
                }
                associations(
                    &domain.implementations,
                    &domain.associated,
                    &partition.unsupported,
                    &definitions,
                    &implementations,
                )?;
            }
        }
        if implementations.keys().any(|id| !declared.contains(id)) {
            return Err(violation(
                "intrinsic facts contain an undeclared implementation",
            ));
        }
        Ok(())
    }
}

/// Each link of `domain`'s implementations to its members. An unsupported
/// implementation's links name its members, whose bodies are not read.
fn associations(
    domain: &[identity::Definition],
    links: &[Association],
    unsupported: &[identity::Definition],
    definitions: &BTreeMap<&identity::Definition, &model::Definition>,
    implementations: &BTreeMap<&identity::Definition, &model::Implementation>,
) -> std::result::Result<(), Box<str>> {
    if !ordered(links) {
        return Err(violation(
            "intrinsic associated links are unsorted or duplicated",
        ));
    }
    let mut actual = BTreeSet::new();
    let mut checked = BTreeSet::new();
    for link in links {
        if domain.binary_search(&link.implementation).is_err()
            || !actual.insert((&link.implementation, &link.item))
        {
            return Err(violation(
                "intrinsic associated link has another implementation or is repeated",
            ));
        }
        if unsupported.binary_search(&link.implementation).is_ok() {
            continue;
        }
        checked.insert((&link.implementation, &link.item));
        let definition = definitions
            .get(&link.item)
            .ok_or_else(|| violation("intrinsic associated item is absent"))?;
        if !matches!(definition.owner(), model::Owner::Implementation(id) if id == &link.implementation)
        {
            return Err(violation("intrinsic associated item's exact owner differs"));
        }
        let implementation = implementations
            .get(&link.implementation)
            .ok_or_else(|| violation("intrinsic associated implementation is absent"))?;
        match (implementation.trait_ref(), &link.declaration) {
            (model::Trait::Trait { id, .. }, Some(declaration)) => {
                let declaration = definitions
                    .get(declaration)
                    .ok_or_else(|| violation("intrinsic associated declaration is absent"))?;
                if !matches!(declaration.owner(), model::Owner::Definition(owner) if owner == id)
                    || declaration.id().namespace() != link.item.namespace()
                {
                    return Err(violation(
                        "intrinsic associated declaration has another trait or namespace",
                    ));
                }
            }
            (model::Trait::Inherent, None) => {}
            _ => {
                return Err(violation(
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
    if checked != expected {
        return Err(violation(
            "intrinsic associated domain does not match the implementation facts",
        ));
    }
    Ok(())
}

fn ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn violation(reason: impl Into<Box<str>>) -> Box<str> {
    reason.into()
}
