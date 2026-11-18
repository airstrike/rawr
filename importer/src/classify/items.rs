//! Each item's classification, with what it read, for a later
//! classification to take where the item reads the same (#1618).
//!
//! A classification records each definition's plan and each export's
//! outcome beside the keys it read (`premise`), with each key's digest in
//! the graph it classified. A later classification of another graph takes
//! an item's record when the item is the same and every key it read has
//! the same digest there: the item's classification would read the same,
//! so it would be the same. The rest it classifies. What every item shares
//! — the support ledger, the fields no path names, nameability, the
//! canonical encoding — is computed again whole.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::premise::{Digest, Key};
use super::{
    api, representability, Context, DefinitionPlan, Error, OpaqueBound, OutcomeEvidence,
    OutcomeRecord,
};

/// What classifications found for each item, and what each read: for an
/// item classified under other reads, a record of each, newest first.
#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Items {
    /// Every key an item read, with its digest in the classified graph.
    pub(super) keys: Box<[(Key, Digested)]>,
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    pub(super) definitions: BTreeMap<api::identity::Definition, Vec<Definition>>,
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    pub(super) exports: BTreeMap<api::identity::Export, Vec<Export>>,
    /// How many definitions and exports the classification that gave
    /// these classified anew, rather than taking an earlier record.
    #[serde(skip)]
    fresh: Fresh,
}

/// A key's digest, as a record writes it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) struct Digested(#[serde(with = "crate::hex")] pub(super) Digest);

/// How many items a classification classified anew.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Fresh {
    pub(crate) definitions: usize,
    pub(crate) exports: usize,
}

/// One definition's classification.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Definition {
    /// The digest of the definition itself.
    #[serde(with = "crate::hex")]
    own: Digest,
    pub(super) representability: representability::Representability,
    /// For a function that gives an opaque type, what its classification
    /// recorded of the types it gives (`Shared::left_out`, `Shared::bound`).
    pub(super) opaque: Option<Opaque>,
    pub(super) charged: Charged,
    premises: Box<[u32]>,
    /// Whether the classification that gave the record classified the item.
    #[serde(skip)]
    pub(super) fresh: bool,
}

/// What a function's classification recorded of the opaque types it gives.
#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct Opaque {
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    pub(super) excluded: BTreeMap<api::identity::Definition, Box<[u32]>>,
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    pub(super) bound: BTreeMap<api::identity::Definition, OpaqueBound>,
}

/// One export's classification.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Export {
    /// The digest of the export's path itself.
    #[serde(with = "crate::hex")]
    own: Digest,
    pub(super) outcome: OutcomeRecord,
    pub(super) charged: Charged,
    premises: Box<[u32]>,
    /// Whether the classification that gave the record classified the item.
    #[serde(skip)]
    pub(super) fresh: bool,
}

/// What one item's classification retained, and the scratch it peaked at.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Charged {
    pub(super) items: usize,
    pub(super) bytes: usize,
    pub(super) scratch: usize,
}

impl Items {
    /// How many items the classification that gave these classified anew.
    pub(crate) const fn fresh(&self) -> Fresh {
        self.fresh
    }

    /// The records of the definitions `keep_definition` keeps and of the
    /// exports `keep_export` keeps, with the keys they read.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn select(
        &self,
        mut keep_definition: impl FnMut(&api::identity::Definition) -> bool,
        mut keep_export: impl FnMut(&Export) -> bool,
    ) -> Self {
        let mut gathered = Gathered::default();
        for (id, records) in &self.definitions {
            if keep_definition(id) {
                for record in records {
                    gathered.carry_definition(id, self, record);
                }
            }
        }
        for (export, records) in &self.exports {
            for record in records.iter().filter(|record| keep_export(record)) {
                gathered.carry_export(export, self, record);
            }
        }
        gathered.carried()
    }

    /// These items with those of `later` first: what a record of a crate's
    /// items holds once another classification of them is done. An item's
    /// record of other reads stays beside the later one, as another graph
    /// may read the same as it did, up to [`CANDIDATES`] of them.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn with(&self, later: &Self) -> Self {
        let mut gathered = Gathered::default();
        for items in [later, self] {
            for (id, records) in &items.definitions {
                for record in records {
                    gathered.carry_definition(id, items, record);
                }
            }
            for (export, records) in &items.exports {
                for record in records {
                    gathered.carry_export(export, items, record);
                }
            }
        }
        gathered.carried()
    }

    /// Whether the classification that gave these classified any of them.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn any_fresh(&self) -> bool {
        self.definitions
            .values()
            .flatten()
            .any(|record| record.fresh)
            || self.exports.values().flatten().any(|record| record.fresh)
    }

    /// Whether these hold no record.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn is_empty(&self) -> bool {
        self.definitions.is_empty() && self.exports.is_empty()
    }
}

/// Whether each key of earlier items still reads the same, asked once.
pub(super) struct Check<'items> {
    sources: Vec<(&'items Items, Box<[OnceLock<bool>]>)>,
}

impl<'items> Check<'items> {
    /// A check of each of `sources`, taken in order: the first record of an
    /// item that holds is the one carried.
    pub(super) fn new(sources: &[&'items Items]) -> Self {
        Self {
            sources: sources
                .iter()
                .map(|items| (*items, items.keys.iter().map(|_| OnceLock::new()).collect()))
                .collect(),
        }
    }

    /// An earlier record of the definition `id`, whose own digest is `own`,
    /// whose every key reads the same in `context`.
    pub(super) fn definition<P: api::Phase>(
        &self,
        id: &api::identity::Definition,
        own: Digest,
        context: &Context<'_, P>,
    ) -> Option<Carried<'items, Definition>> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, (items, holds))| {
                let record = items.definitions.get(id)?.iter().find(|record| {
                    record.own == own && hold(items, holds, &record.premises, context)
                })?;
                Some(Carried { source, record })
            })
    }

    /// An earlier record of the export `export`, whose own digest is `own`,
    /// whose every key reads the same in `context`.
    pub(super) fn export<P: api::Phase>(
        &self,
        export: &api::identity::Export,
        own: Digest,
        context: &Context<'_, P>,
    ) -> Option<Carried<'items, Export>> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, (items, holds))| {
                let record = items.exports.get(export)?.iter().find(|record| {
                    record.own == own && hold(items, holds, &record.premises, context)
                })?;
                Some(Carried { source, record })
            })
    }

    /// The items a carried record came from.
    fn source(&self, source: usize) -> &'items Items {
        self.sources[source].0
    }
}

/// Whether every one of `premises`, keys of `items`, reads the same in
/// `context`, each asked once (`holds`).
fn hold<P: api::Phase>(
    items: &Items,
    holds: &[OnceLock<bool>],
    premises: &[u32],
    context: &Context<'_, P>,
) -> bool {
    premises.iter().all(|index| {
        let index = *index as usize;
        *holds[index].get_or_init(|| {
            let (key, Digested(digest)) = &items.keys[index];
            context.digest(key) == *digest
        })
    })
}

/// An earlier record of an item, and which of the checked items it is of.
pub(super) struct Carried<'items, T> {
    source: usize,
    pub(super) record: &'items T,
}

/// What one item's classification gave and read, or the earlier record it
/// took.
pub(super) enum Found<'items, T> {
    Fresh { record: T, keys: Vec<Key> },
    Carried(Carried<'items, T>),
}

/// The items of one classification, gathered as they are found.
#[derive(Default)]
pub(super) struct Gathered {
    indices: BTreeMap<(Key, Option<Digest>), u32>,
    /// Each key with its digest, by its index; none yet for a fresh item's
    /// key, whose digest is read once every item is found.
    keys: Vec<(Key, Option<Digest>)>,
    definitions: BTreeMap<api::identity::Definition, Vec<Definition>>,
    exports: BTreeMap<api::identity::Export, Vec<Export>>,
    fresh: Fresh,
}

/// The most records an entry keeps of one item, each of other reads.
const CANDIDATES: usize = 4;

/// Add `record` to an item's `records` unless one of the same item and
/// reads is there, or they hold [`CANDIDATES`] already.
fn candidate<T>(records: &mut Vec<T>, record: T, same: impl Fn(&T, &T) -> bool) {
    if records.len() < CANDIDATES && !records.iter().any(|kept| same(kept, &record)) {
        records.push(record);
    }
}

impl Gathered {
    fn index(&mut self, key: &Key, digest: Option<Digest>) -> u32 {
        let entry = (key.clone(), digest);
        if let Some(index) = self.indices.get(&entry) {
            return *index;
        }
        let index = self.keys.len() as u32;
        self.keys.push(entry.clone());
        self.indices.insert(entry, index);
        index
    }

    /// The premises of a fresh item's `keys`.
    fn premises(&mut self, keys: &[Key]) -> Box<[u32]> {
        keys.iter().map(|key| self.index(key, None)).collect()
    }

    /// The premises of a carried item's `premises` in `earlier`.
    fn carried_premises(&mut self, earlier: &Items, premises: &[u32]) -> Box<[u32]> {
        premises
            .iter()
            .map(|index| {
                let (key, Digested(digest)) = &earlier.keys[*index as usize];
                self.index(key, Some(*digest))
            })
            .collect()
    }

    fn carry_definition(
        &mut self,
        id: &api::identity::Definition,
        earlier: &Items,
        record: &Definition,
    ) {
        let mut record = record.clone();
        record.premises = self.carried_premises(earlier, &record.premises);
        candidate(
            self.definitions.entry(id.clone()).or_default(),
            record,
            |kept, record| kept.own == record.own && kept.premises == record.premises,
        );
    }

    fn carry_export(&mut self, export: &api::identity::Export, earlier: &Items, record: &Export) {
        let mut record = record.clone();
        record.premises = self.carried_premises(earlier, &record.premises);
        candidate(
            self.exports.entry(export.clone()).or_default(),
            record,
            |kept, record| kept.own == record.own && kept.premises == record.premises,
        );
    }

    /// Gather the definition `id`, whose own digest is `own`.
    pub(super) fn definition(
        &mut self,
        id: &api::identity::Definition,
        own: Digest,
        found: Found<'_, Definition>,
        check: Option<&Check<'_>>,
    ) {
        match found {
            Found::Fresh { mut record, keys } => {
                self.fresh.definitions += 1;
                record.own = own;
                record.fresh = true;
                record.premises = self.premises(&keys);
                self.definitions.insert(id.clone(), vec![record]);
            }
            Found::Carried(Carried { source, record }) => {
                let earlier = check
                    .expect("a carried record is one of the checked items")
                    .source(source);
                self.carry_definition(id, earlier, record);
            }
        }
    }

    /// Gather the export `export`, whose own digest is `own`.
    pub(super) fn export(
        &mut self,
        export: &api::identity::Export,
        own: Digest,
        found: Found<'_, Export>,
        check: Option<&Check<'_>>,
    ) {
        match found {
            Found::Fresh { mut record, keys } => {
                self.fresh.exports += 1;
                record.own = own;
                record.fresh = true;
                record.premises = self.premises(&keys);
                self.exports.insert(export.clone(), vec![record]);
            }
            Found::Carried(Carried { source, record }) => {
                let earlier = check
                    .expect("a carried record is one of the checked items")
                    .source(source);
                self.carry_export(export, earlier, record);
            }
        }
    }

    /// The items, each fresh key's digest read in `context`, the graph and
    /// shared answers every item was classified against.
    pub(super) fn finish<P: api::Phase>(self, context: &Context<'_, P>) -> Items {
        Items {
            keys: self
                .keys
                .into_iter()
                .map(|(key, digest)| {
                    let digest = digest.unwrap_or_else(|| context.digest(&key));
                    (key, Digested(digest))
                })
                .collect(),
            definitions: self.definitions,
            exports: self.exports,
            fresh: self.fresh,
        }
    }

    /// The items, every record carried with its digests.
    #[cfg(feature = "rustc-driver")]
    fn carried(self) -> Items {
        Items {
            keys: self
                .keys
                .into_iter()
                .map(|(key, digest)| {
                    let digest = digest.expect("a carried record's keys have their digests");
                    (key, Digested(digest))
                })
                .collect(),
            definitions: self.definitions,
            exports: self.exports,
            fresh: self.fresh,
        }
    }
}

impl Definition {
    /// A fresh record, its own digest and premises given when gathered.
    pub(super) fn new(
        representability: representability::Representability,
        opaque: Option<Opaque>,
        charged: Charged,
    ) -> Self {
        Self {
            own: [0; 32],
            representability,
            opaque,
            charged,
            premises: Box::default(),
            fresh: false,
        }
    }
}

impl Export {
    /// A fresh record, its own digest and premises given when gathered.
    pub(super) fn new(outcome: OutcomeRecord, charged: Charged) -> Self {
        Self {
            own: [0; 32],
            outcome,
            charged,
            premises: Box::default(),
            fresh: false,
        }
    }

    /// The outcome again, each plan it names found in `plans`, the plans of
    /// the graph it is carried into.
    pub(super) fn outcome_in(&self, plans: &[DefinitionPlan]) -> Result<OutcomeRecord, Error> {
        let mut outcome = self.outcome.clone();
        reseat(&mut outcome, plans)?;
        Ok(outcome)
    }

    /// The definition the export's path names, for an imported path.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn target(&self) -> Option<&crate::surface::identity::Definition> {
        match &self.outcome.evidence {
            OutcomeEvidence::Plan { definition, .. } => Some(definition),
            OutcomeEvidence::Inline(_) => None,
        }
    }
}

/// Point each plan `outcome` names at its index in `plans`.
fn reseat(outcome: &mut OutcomeRecord, plans: &[DefinitionPlan]) -> Result<(), Error> {
    if let OutcomeEvidence::Plan { definition, index } = &mut outcome.evidence {
        let found = plans
            .binary_search_by_key(definition, DefinitionPlan::definition)
            .map_err(|_| Error::Cardinality)?;
        *index = u32::try_from(found).map_err(|_| Error::Cardinality)?;
    }
    for implementation in &mut outcome.implementations {
        for associated in &mut implementation.associated {
            reseat(associated, plans)?;
        }
    }
    Ok(())
}

/// A map written as a sequence of its entries, as JSON keys are strings.
fn entries<K: Serialize, V: Serialize, S: serde::Serializer>(
    map: &BTreeMap<K, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(map.iter())
}

/// A map read from the sequence of entries [`entries`] writes.
fn from_entries<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Vec::<(K, V)>::deserialize(deserializer)?
        .into_iter()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{Authority, Policy};
    use crate::api;

    fn fixtures() -> [api::Graph<api::Extracted>; 4] {
        [
            crate::test_fixture::iced_profile(),
            crate::test_fixture::graph(),
            crate::test_fixture::native_shapes(),
            crate::test_fixture::opaque_left_out_functions(),
        ]
    }

    /// A classification that takes every item's record from one of the
    /// same graph classifies nothing again, and gives the same bytes.
    #[test]
    fn a_classification_of_the_same_graph_carries_every_item() {
        let limits = crate::limits::Limits::standard();
        let authority = Authority::production(&limits);
        for graph in fixtures() {
            let (first, first_scratch, items) = graph
                .classify_items_on(&Policy::standard(), authority, 8, &[])
                .expect("the fixture classifies");
            assert_eq!(items.fresh().definitions, graph.definitions().len());
            let (again, again_scratch, carried) = graph
                .classify_items_on(&Policy::standard(), authority, 8, &[&items])
                .expect("the fixture classifies from its records");
            assert_eq!(first.canonical, again.canonical);
            assert_eq!(
                (first.allocation.items, first.allocation.bytes),
                (again.allocation.items, again.allocation.bytes),
            );
            assert_eq!(first_scratch, again_scratch);
            assert_eq!(carried.fresh(), super::Fresh::default());
        }
    }

    /// A graph that names more paths than an earlier one, as a later
    /// demand round's does, classifies from the earlier one's records to
    /// the same bytes as a classification of it alone, and classifies
    /// again only the items whose reads changed.
    #[test]
    fn a_grown_graph_classifies_from_an_earlier_ones_records_to_the_same_bytes() {
        let limits = crate::limits::Limits::standard();
        let authority = Authority::production(&limits);
        for fixture in fixtures() {
            let exports = fixture.exports().len();
            let full = crate::test_fixture::with_exports(&fixture, |_| true);
            let (alone, alone_scratch, _) = full
                .classify_items_on(&Policy::standard(), authority, 8, &[])
                .expect("the grown graph classifies");
            for dropped in 0..exports {
                let earlier = crate::test_fixture::with_exports(&fixture, |index| index != dropped);
                let (_, _, items) = earlier
                    .classify_items_on(&Policy::standard(), authority, 8, &[])
                    .expect("the earlier graph classifies");
                let (grown, grown_scratch, carried) = full
                    .classify_items_on(&Policy::standard(), authority, 8, &[&items])
                    .expect("the grown graph classifies from the earlier records");
                assert_eq!(alone.canonical, grown.canonical, "export {dropped} dropped");
                assert_eq!(
                    (alone.allocation.items, alone.allocation.bytes),
                    (grown.allocation.items, grown.allocation.bytes),
                );
                assert_eq!(alone_scratch, grown_scratch);
                assert!(
                    carried.fresh().exports >= 1,
                    "the path the earlier graph lacked is classified"
                );
                assert!(
                    carried.fresh().exports < exports || exports == 1,
                    "the paths both graphs name are carried"
                );
            }
        }
    }

    /// `graph` sealed again without the definition at `dropped` and the
    /// paths to it, when the rest still seals and classifies: the API of a
    /// demand round before a later one reached that definition.
    fn without_definition(
        graph: &api::Graph<api::Extracted>,
        dropped: usize,
    ) -> Option<api::Graph<api::Extracted>> {
        let id = graph.definition_at(dropped)?.id().clone();
        let names = |path: &crate::api::ledger::Path| match path {
            crate::api::ledger::Path::Imported(export) => *export.target() == id,
            _ => false,
        };
        let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
        api::Graph::seal(
            &api::Context::fixture(),
            Vec::new(),
            graph
                .definitions()
                .filter(|definition| *definition.id() != id)
                .cloned()
                .collect(),
            graph
                .exports()
                .filter(|path| !names(path))
                .cloned()
                .collect(),
            graph.implementations().cloned().collect(),
            graph.scalar_predicate_proofs().cloned().collect(),
            graph.support().cloned().collect(),
            graph.anonymous_imports().cloned().collect(),
            &mut allocation,
        )
        .ok()?
        .publish(crate::input::proof::Proof::fixture())
        .ok()
    }

    /// A graph with a definition an earlier one lacked classifies that
    /// definition, and each item that read its absence, again, to the same
    /// bytes as a classification of it alone.
    #[test]
    fn a_definition_a_later_graph_adds_is_classified_with_what_read_its_absence() {
        let limits = crate::limits::Limits::standard();
        let authority = Authority::production(&limits);
        let mut grown = 0;
        for fixture in fixtures() {
            let full = crate::test_fixture::with_exports(&fixture, |_| true);
            let (alone, alone_scratch, _) = full
                .classify_items_on(&Policy::standard(), authority, 8, &[])
                .expect("the grown graph classifies");
            for dropped in 0..full.definitions().len() {
                let Some(earlier) = without_definition(&full, dropped) else {
                    continue;
                };
                let Ok((_, _, items)) =
                    earlier.classify_items_on(&Policy::standard(), authority, 8, &[])
                else {
                    continue;
                };
                let (again, again_scratch, carried) = full
                    .classify_items_on(&Policy::standard(), authority, 8, &[&items])
                    .expect("the grown graph classifies from the earlier records");
                assert_eq!(
                    alone.canonical, again.canonical,
                    "definition {dropped} added"
                );
                assert_eq!(
                    (alone.allocation.items, alone.allocation.bytes),
                    (again.allocation.items, again.allocation.bytes),
                );
                assert_eq!(alone_scratch, again_scratch);
                assert!(
                    carried.fresh().definitions >= 1,
                    "the added definition is classified"
                );
                grown += 1;
            }
        }
        assert!(grown > 4, "the fixtures grow by a definition {grown} times");
    }
}
