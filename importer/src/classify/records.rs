//! Each crate's classification records, kept beside its shared table in
//! rawr's home, and the project's own (#271, #1618).
//!
//! A selected crate shared per machine (D608.4) keeps the records of its
//! items under its table's key, in a directory of the classification's
//! version: the definitions of the crate, of the crates it reaches that a
//! registry, a git commit or the toolchain give, and the paths to them. A
//! later classification on the machine of an API that reads the same of
//! any of them takes those records instead of classifying the items again
//! (`items`), whatever project it is for. The items of the project's own
//! crates, and of path crates it reaches, are kept in the project's Rawr
//! directory instead, for its next round or run. A record is a cache: one
//! that can't be read is a miss, and publishing is best effort.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::items::Items;
use super::{api, Policy};
use crate::extract::engine::home;

/// The file an entry keeps its records in.
const RECORDS: &str = "records.json";
/// The records' own format: bump it when what a record holds changes.
const RECORDS_SCHEMA: u32 = 1;
/// The project's own entry, under its classified directory.
const PROJECT: &str = "project";

/// Where a classification's records are kept: one entry for each selected
/// crate, by its table's key, and the project's own.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    crates: Vec<Crate>,
    project: Option<PathBuf>,
}

/// One selected crate's entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Crate {
    /// The bindings directory its table is in.
    pub(crate) bindings: PathBuf,
    pub(crate) key: String,
    pub(crate) crate_name: String,
    /// Its stable crate id: read from its table's metadata when the
    /// settlement that names it doesn't say ([`Sources::resolve`]).
    pub(crate) stable_crate_id: Option<String>,
    /// Whether its table is shared per machine, so its records are too.
    pub(crate) shared: bool,
}

/// An entry's records as its file holds them: the compiler units they
/// name once, and the records naming each unit by its place.
#[derive(Deserialize, Serialize)]
struct Stored {
    schema: u32,
    units: Box<[Arc<api::identity::Unit>]>,
    records: Box<serde_json::value::RawValue>,
}

/// The records one classification loaded, by the entry each came from.
#[derive(Default)]
pub(crate) struct Loaded {
    entries: Vec<(Entry, Arc<Items>)>,
}

/// Where one entry is kept.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Entry {
    Crate(usize),
    Project,
}

impl Sources {
    /// The entries of `crates`, and the project's, under its Rawr directory
    /// `rawr`.
    pub(crate) fn new(crates: Vec<Crate>, rawr: Option<&Path>) -> Self {
        let project = rawr.map(|rawr| {
            crate::bindings::format::project(rawr, crate::bindings::format::Kept::Classified)
                .join(version())
        });
        Self { crates, project }
    }

    /// Whether these name the same entries as `other`.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.crates == other.crates && self.project == other.project
    }

    /// Read each crate's stable crate id its settlement left out from its
    /// table's metadata. A crate whose metadata can't be read keeps no
    /// records.
    pub(crate) fn resolve(&mut self) {
        for krate in &mut self.crates {
            if krate.stable_crate_id.is_none() {
                krate.stable_crate_id = crate::bindings::meta(&krate.bindings, &krate.key)
                    .map(|meta| meta.stable_crate_id);
            }
        }
        self.crates.retain(|krate| krate.stable_crate_id.is_some());
    }

    fn place(&self, entry: &Entry) -> Option<(PathBuf, &str)> {
        match entry {
            Entry::Crate(index) => {
                let krate = &self.crates[*index];
                let directory = if krate.shared {
                    crate::bindings::format::classified_beside(&krate.bindings).join(version())
                } else {
                    self.project.clone()?
                };
                Some((directory, &krate.key))
            }
            Entry::Project => Some((self.project.clone()?, PROJECT)),
        }
    }

    fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        (0..self.crates.len())
            .map(Entry::Crate)
            .chain(std::iter::once(Entry::Project))
    }

    /// Every entry's records that can be read.
    pub(crate) fn load(&self) -> Loaded {
        let entries = self
            .entries()
            .filter_map(|entry| {
                let (directory, key) = self.place(&entry)?;
                let items = read(&directory, key)?;
                Some((entry, Arc::new(items)))
            })
            .collect();
        Loaded { entries }
    }

    /// Keep each entry's part of `items`, a classification's records, in
    /// place of what `loaded` read of it, and in `loaded`: an entry none of
    /// whose records were classified anew is left as it is.
    pub(crate) fn publish(&self, items: &Items, loaded: &mut Loaded) {
        let owners = Owners::of(self, items);
        for entry in self.entries() {
            let part = items.select(
                |id, _| owners.definition(id) == Some(&entry),
                |export| {
                    export
                        .target()
                        .and_then(|target| owners.surface(target))
                        .is_some_and(|owner| *owner == entry)
                },
            );
            if part.is_empty() || !part.any_fresh() {
                continue;
            }
            let Some((directory, key)) = self.place(&entry) else {
                continue;
            };
            let position = loaded
                .entries
                .iter()
                .position(|(loaded, _)| *loaded == entry);
            let kept = match position {
                Some(position) => loaded.entries[position].1.with(&part),
                None => part,
            };
            write(&directory, key, &kept);
            let kept = Arc::new(kept);
            match position {
                Some(position) => loaded.entries[position].1 = kept,
                None => loaded.entries.push((entry, kept)),
            }
        }
    }
}

impl Loaded {
    /// The records each entry holds.
    pub(crate) fn items(&self) -> impl Iterator<Item = &Items> {
        self.entries.iter().map(|(_, items)| &**items)
    }
}

/// Which entry keeps each item's record.
struct Owners {
    definitions: BTreeMap<api::identity::Definition, Entry>,
    surface: BTreeMap<crate::surface::identity::Definition, Entry>,
}

impl Owners {
    /// The entry of each definition `items` holds: its selected crate's,
    /// else the first shared crate's when a registry, a git commit or the
    /// toolchain gives it, else the project's.
    fn of(sources: &Sources, items: &Items) -> Self {
        let first_shared = sources
            .crates
            .iter()
            .position(|krate| krate.shared)
            .map(Entry::Crate);
        let mut definitions = BTreeMap::new();
        let mut surface = BTreeMap::new();
        for id in items.definitions.keys() {
            let unit = id.unit();
            let own = sources.crates.iter().position(|krate| {
                krate.crate_name == unit.crate_name()
                    && krate.stable_crate_id.as_deref() == Some(unit.stable_crate_id())
            });
            let shareable = match unit.package() {
                Some(package) => {
                    package.source().starts_with("registry+")
                        || package.source().starts_with("git+")
                }
                None => true,
            };
            let entry = match (own, &first_shared) {
                (Some(index), _) => Entry::Crate(index),
                (None, Some(shared)) if shareable => shared.clone(),
                _ => Entry::Project,
            };
            surface.insert(crate::surface::identity::Definition::of(id), entry.clone());
            definitions.insert(id.clone(), entry);
        }
        Self {
            definitions,
            surface,
        }
    }

    fn definition(&self, id: &api::identity::Definition) -> Option<&Entry> {
        self.definitions.get(id)
    }

    fn surface(&self, definition: &crate::surface::identity::Definition) -> Option<&Entry> {
        self.surface.get(definition)
    }
}

/// The directory of this classification's version under a classified
/// directory: records of another classifier are of another directory.
fn version() -> String {
    let policy = Policy::standard();
    format!(
        "{}-{}-{}-{RECORDS_SCHEMA}",
        policy.schema, policy.classifier, policy.compiler_abi
    )
}

/// The records of `key`'s entry under `directory`, read under its lease.
fn read(directory: &Path, key: &str) -> Option<Items> {
    let _lease = home::hold(directory, key)?;
    let bytes = std::fs::read(directory.join(key).join(RECORDS)).ok()?;
    let stored = crate::extract::sidecar::read::<Stored>(&bytes).ok()?;
    if stored.schema != RECORDS_SCHEMA {
        return None;
    }
    api::units::read(stored.units, || {
        crate::extract::sidecar::read::<Items>(stored.records.get().as_bytes()).ok()
    })
}

/// Publish `items` as `key`'s entry under `directory`. Best effort.
fn write(directory: &Path, key: &str, items: &Items) {
    let (records, units) = api::units::write(|| serde_json::to_string(items));
    let Ok(records) = records.and_then(serde_json::value::RawValue::from_string) else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(&Stored {
        schema: RECORDS_SCHEMA,
        units,
        records,
    }) else {
        return;
    };
    if std::fs::create_dir_all(directory).is_ok() {
        let _ = home::publish(directory, key, &[(RECORDS, &bytes)]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Authority, Policy};

    /// Records written to an entry and read back are the records: a
    /// classification of the same graph from them classifies nothing, to
    /// the same bytes.
    #[test]
    fn records_read_back_from_an_entry_carry_every_item() {
        let directory =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-classified-records-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let limits = crate::limits::Limits::standard();
        let authority = Authority::production(&limits);
        for (index, graph) in [
            crate::test_fixture::iced_profile(),
            crate::test_fixture::native_shapes(),
            crate::test_fixture::opaque_left_out_functions(),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("fixture-{index}");
            let (first, _, items) = graph
                .classify_items_on(&Policy::standard(), authority, 8, &[])
                .expect("the fixture classifies");
            super::write(&directory, &key, &items);
            let read = super::read(&directory, &key).expect("the entry reads back");
            let (again, _, carried) = graph
                .classify_items_on(&Policy::standard(), authority, 8, &[&read])
                .expect("the fixture classifies from the records read back");
            assert_eq!(first.canonical, again.canonical);
            assert_eq!(carried.fresh(), super::super::items::Fresh::default());
        }
        std::fs::remove_dir_all(&directory).expect("the scratch directory goes");
    }
}
