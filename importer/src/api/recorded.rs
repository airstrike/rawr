//! Pieces of work kept under what each read, for later work over another
//! graph to take where a piece reads the same (#1618, #2222).
//!
//! A [`Table`] keeps what each unit of work gave, beside the digest of the
//! unit itself and the keys it read, each with its digest where the work
//! ran. A later run of the same unit takes the kept value when the unit's
//! own digest is the same and every key it read has the same digest there:
//! the work would read the same, so it would give the same. What a key is,
//! and how a later run digests it, is the work's own: reads of a graph
//! ([`super::reads::Read`]) and whatever else the work reads.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::reads::Digest;

/// What each unit of some work gave, and what it read: for a unit run
/// under other reads, a record of each, newest first.
#[derive(Deserialize, Serialize)]
#[serde(bound(
    serialize = "U: Serialize, K: Serialize, V: Serialize",
    deserialize = "U: Deserialize<'de> + Ord, K: Deserialize<'de>, V: Deserialize<'de>"
))]
pub(crate) struct Table<U, K, V> {
    /// Every key a unit read, with its digest where the unit ran.
    keys: Box<[(K, Digested)]>,
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    units: BTreeMap<U, Vec<Record<V>>>,
}

impl<U, K, V> Default for Table<U, K, V> {
    fn default() -> Self {
        Self {
            keys: Box::default(),
            units: BTreeMap::new(),
        }
    }
}

/// A key's digest, as a record writes it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Digested(#[serde(with = "crate::hex")] Digest);

/// What one unit gave, under the keys it read.
#[derive(Clone, Deserialize, Serialize)]
struct Record<V> {
    /// The digest of the unit itself.
    #[serde(with = "crate::hex")]
    own: Digest,
    premises: Box<[u32]>,
    value: V,
    /// Whether the work that gave the record ran the unit.
    #[serde(skip)]
    fresh: bool,
}

/// The most records a table keeps of one unit, each of other reads.
const CANDIDATES: usize = 4;

impl<U: Ord + Clone, K: Ord + Clone, V: Clone> Table<U, K, V> {
    /// Whether these hold no record.
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Whether the work that gave these ran any unit.
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    pub(crate) fn any_fresh(&self) -> bool {
        self.units.values().flatten().any(|record| record.fresh)
    }

    /// The records of the units `keep` keeps, with the keys they read.
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    pub(crate) fn select(&self, mut keep: impl FnMut(&U) -> bool) -> Self {
        let mut gathered = Gathered::default();
        for (unit, records) in &self.units {
            if keep(unit) {
                for record in records {
                    gathered.carry_record(unit, self, record, record.fresh);
                }
            }
        }
        gathered.finish()
    }

    /// These records with those of `later` first: of each unit, `later`'s,
    /// then up to [`CANDIDATES`] in all, as another run may read a unit
    /// as an earlier one did.
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    pub(crate) fn with(&self, later: &Self) -> Self {
        let mut gathered = Gathered::default();
        for table in [later, self] {
            for (unit, records) in &table.units {
                for record in records {
                    gathered.carry_record(unit, table, record, record.fresh);
                }
            }
        }
        gathered.finish()
    }
}

/// Whether each key of earlier tables still reads the same, asked once.
pub(crate) struct Check<'t, U, K, V> {
    sources: Vec<Source<'t, U, K, V>>,
}

/// One earlier table, and whether each of its keys reads the same, once
/// asked.
struct Source<'t, U, K, V> {
    table: &'t Table<U, K, V>,
    holds: Box<[OnceLock<bool>]>,
}

/// A kept record a check found, and which of the checked tables it is of.
pub(crate) struct Carried<'t, V> {
    source: usize,
    record: &'t Record<V>,
}

impl<V> Carried<'_, V> {
    /// What the unit gave.
    pub(crate) const fn value(&self) -> &V {
        &self.record.value
    }
}

impl<'t, U: Ord, K, V> Check<'t, U, K, V> {
    /// A check of each of `sources`, taken in order: the first record of a
    /// unit that holds is the one carried.
    pub(crate) fn new(sources: &[&'t Table<U, K, V>]) -> Self {
        Self {
            sources: sources
                .iter()
                .map(|table| Source {
                    table,
                    holds: table.keys.iter().map(|_| OnceLock::new()).collect(),
                })
                .collect(),
        }
    }

    /// A kept record of `unit`, whose own digest is `own`, each of whose
    /// keys `digest` gives the digest it was read at.
    pub(crate) fn find(
        &self,
        unit: &U,
        own: Digest,
        digest: impl Fn(&K) -> Digest,
    ) -> Option<Carried<'t, V>> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, Source { table, holds })| {
                let record = table.units.get(unit)?.iter().find(|record| {
                    record.own == own
                        && record.premises.iter().all(|index| {
                            let index = *index as usize;
                            *holds[index].get_or_init(|| {
                                let (key, Digested(read)) = &table.keys[index];
                                digest(key) == *read
                            })
                        })
                })?;
                Some(Carried { source, record })
            })
    }
}

/// The records of one run of the work, gathered as each unit is found.
pub(crate) struct Gathered<U, K, V> {
    indices: BTreeMap<(K, Digest), u32>,
    /// Each key with its digest, by its index.
    keys: Vec<(K, Digest)>,
    units: BTreeMap<U, Vec<Record<V>>>,
}

impl<U, K, V> Default for Gathered<U, K, V> {
    fn default() -> Self {
        Self {
            indices: BTreeMap::new(),
            keys: Vec::new(),
            units: BTreeMap::new(),
        }
    }
}

impl<U: Ord + Clone, K: Ord + Clone, V: Clone> Gathered<U, K, V> {
    fn index(&mut self, key: &K, digest: Digest) -> u32 {
        let entry = (key.clone(), digest);
        if let Some(index) = self.indices.get(&entry) {
            return *index;
        }
        let index = u32::try_from(self.keys.len()).expect("a table's keys fit a u32 index");
        self.keys.push(entry.clone());
        self.indices.insert(entry, index);
        index
    }

    /// Gather `record`, `earlier`'s, of `unit`; `fresh` when the work that
    /// gives these ran it.
    fn carry_record(
        &mut self,
        unit: &U,
        earlier: &Table<U, K, V>,
        record: &Record<V>,
        fresh: bool,
    ) {
        let premises = record
            .premises
            .iter()
            .map(|index| {
                let (key, Digested(digest)) = &earlier.keys[*index as usize];
                self.index(key, *digest)
            })
            .collect::<Box<[_]>>();
        self.push(
            unit,
            Record {
                own: record.own,
                premises,
                value: record.value.clone(),
                fresh,
            },
        );
    }

    /// Add `record` to `unit`'s records, after those gathered before it,
    /// unless one of the same reads is there or they hold [`CANDIDATES`].
    fn push(&mut self, unit: &U, record: Record<V>) {
        let records = self.units.entry(unit.clone()).or_default();
        if records.len() < CANDIDATES
            && !records
                .iter()
                .any(|kept| kept.own == record.own && kept.premises == record.premises)
        {
            records.push(record);
        }
    }

    /// Gather `unit`, whose own digest is `own`, run anew: it gave `value`
    /// and read `keys`, each with its digest where it ran.
    pub(crate) fn fresh(
        &mut self,
        unit: U,
        own: Digest,
        value: V,
        keys: impl IntoIterator<Item = (K, Digest)>,
    ) {
        let premises = keys
            .into_iter()
            .map(|(key, digest)| self.index(&key, digest))
            .collect::<Box<[_]>>();
        // The newest record leads, as a later run tries it first.
        let records = self.units.entry(unit).or_default();
        records.retain(|kept| kept.own != own || kept.premises != premises);
        records.insert(
            0,
            Record {
                own,
                premises,
                value,
                fresh: true,
            },
        );
        records.truncate(CANDIDATES);
    }

    /// Gather `unit` as the record `carried`, which `check` found.
    pub(crate) fn carried(
        &mut self,
        unit: &U,
        carried: &Carried<'_, V>,
        check: &Check<'_, U, K, V>,
    ) {
        let earlier = check.sources[carried.source].table;
        self.carry_record(unit, earlier, carried.record, false);
    }

    /// The table of every unit gathered.
    pub(crate) fn finish(self) -> Table<U, K, V> {
        Table {
            keys: self
                .keys
                .into_iter()
                .map(|(key, digest)| (key, Digested(digest)))
                .collect(),
            units: self.units,
        }
    }
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
