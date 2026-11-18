//! Each crate's classification records and facade contributions, kept
//! beside its shared table in rawr's home, and the project's own (#271,
//! #1618).
//!
//! A selected crate shared per machine (D608.4) keeps the records of its
//! items under its table's key, in a directory of the classification's
//! version: the definitions of the crate, of the crates it reaches that a
//! registry, a git commit or the toolchain give, and the paths to them. Its
//! paths' facade contributions are kept the same way, in a directory of the
//! facade plan's version. A later classification on the machine of an API
//! that reads the same of any item takes its record instead of classifying
//! it again (`items`), and a plan of an outcome kept takes its contribution
//! (`surface::facade::contribution`), whatever project it is for. The items
//! of the project's own crates, and of path crates it reaches, are kept in
//! the project's Rawr directory instead, for its next round or run. An entry
//! is a cache: one that can't be read is a miss, and publishing is best
//! effort.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::items::Items;
use super::{api, Policy};
use crate::extract::engine::home;
use crate::surface::facade::contribution::Contributions;

/// The format of a records entry's file: bump it when what a record holds
/// changes.
const RECORDS_SCHEMA: u32 = 1;
/// The format of a facade entry's file: bump it when what a contribution
/// or a header unit holds changes. 2: the entry keeps its plan's header
/// units (#2222).
const FACADE_SCHEMA: u32 = 2;
/// The project's own entry, under its classified directory.
const PROJECT: &str = "project";

/// Where a classification's records are kept: one entry for each selected
/// crate, by its table's key, and the project's own.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    crates: Vec<Crate>,
    /// The project's classified directory.
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

/// What an entry keeps.
#[derive(Clone, Copy)]
enum Kind {
    /// Each item's classification record (`items`).
    Records,
    /// Each path's facade contribution.
    Facade,
}

/// An entry's file: the compiler units its contents name once, and the
/// contents, naming each unit by its place.
#[derive(Deserialize, Serialize)]
struct Stored {
    schema: u32,
    units: Box<[Arc<api::identity::Unit>]>,
    contents: Box<serde_json::value::RawValue>,
}

/// What a classification and its plan read of the entries, by the entry
/// each came from.
#[derive(Default)]
pub(crate) struct Loaded {
    records: Vec<(Entry, Arc<Items>)>,
    facade: Vec<(Entry, Arc<Contributions>)>,
}

/// Where one entry is kept.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Entry {
    Crate(usize),
    Project,
}

impl Kind {
    /// The directory of this kind's version under a classified directory:
    /// an entry another classifier or plan made is in another directory.
    fn version(self) -> String {
        match self {
            Self::Records => {
                let policy = Policy::standard();
                format!(
                    "{}-{}-{}-{RECORDS_SCHEMA}",
                    policy.schema, policy.classifier, policy.compiler_abi
                )
            }
            Self::Facade => format!(
                "facade-{}-{FACADE_SCHEMA}",
                crate::surface::facade::schema()
            ),
        }
    }

    /// The format of this kind's file.
    const fn schema(self) -> u32 {
        match self {
            Self::Records => RECORDS_SCHEMA,
            Self::Facade => FACADE_SCHEMA,
        }
    }

    const fn file(self) -> &'static str {
        match self {
            Self::Records => "records.json",
            Self::Facade => "facade.json",
        }
    }
}

impl Sources {
    /// The entries of `crates`, and the project's, under its Rawr directory
    /// `rawr`.
    pub(crate) fn new(crates: Vec<Crate>, rawr: Option<&Path>) -> Self {
        let project = rawr.map(|rawr| {
            crate::bindings::format::project(rawr, crate::bindings::format::Kept::Classified)
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

    fn place(&self, entry: &Entry, kind: Kind) -> Option<(PathBuf, &str)> {
        match entry {
            Entry::Crate(index) => {
                let krate = &self.crates[*index];
                let directory = if krate.shared {
                    crate::bindings::format::classified_beside(&krate.bindings)
                } else {
                    self.project.clone()?
                };
                Some((directory.join(kind.version()), &krate.key))
            }
            Entry::Project => Some((self.project.as_ref()?.join(kind.version()), PROJECT)),
        }
    }

    fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        (0..self.crates.len())
            .map(Entry::Crate)
            .chain(std::iter::once(Entry::Project))
    }

    /// Every entry that can be read, of both kinds.
    pub(crate) fn load(&self) -> Loaded {
        Loaded {
            records: self.load_kind(Kind::Records),
            facade: self.load_kind(Kind::Facade),
        }
    }

    fn load_kind<T: DeserializeOwned>(&self, kind: Kind) -> Vec<(Entry, Arc<T>)> {
        self.entries()
            .filter_map(|entry| {
                let (directory, key) = self.place(&entry, kind)?;
                let contents = read(&directory, key, kind)?;
                Some((entry, Arc::new(contents)))
            })
            .collect()
    }

    /// Keep each entry's part of `items`, a classification's records, in
    /// place of what `loaded` read of it, and in `loaded`: an entry none of
    /// whose records were classified anew is left as it is.
    pub(crate) fn publish(&self, items: &Items, loaded: &mut Loaded) {
        let owners = Owners::of(self, items);
        for entry in self.entries() {
            let part = items.select(
                |id| owners.definition(id) == Some(&entry),
                |export| owners.owns(&entry, export.target()),
            );
            if part.is_empty() || !part.any_fresh() {
                continue;
            }
            self.keep(
                &entry,
                Kind::Records,
                &mut loaded.records,
                part,
                |kept, part| kept.with(part),
            );
        }
    }

    /// Keep each entry's part of `contributions`, a plan's, as
    /// [`publish`](Self::publish) keeps records: each is its path's, whose
    /// owner `items` tells.
    pub(crate) fn publish_facade(
        &self,
        contributions: &Contributions,
        items: &Items,
        loaded: &mut Loaded,
    ) {
        let owners = Owners::of(self, items);
        for entry in self.entries() {
            let part = contributions.select(
                |target| owners.owns(&entry, target),
                |id| owners.entry(self, id) == entry,
            );
            if part.is_empty() || !part.any_fresh() {
                continue;
            }
            self.keep(
                &entry,
                Kind::Facade,
                &mut loaded.facade,
                part,
                |kept, part| kept.with(part),
            );
        }
    }

    /// Write `part` as `entry`'s, joined to what `loaded` holds of it.
    fn keep<T: Serialize>(
        &self,
        entry: &Entry,
        kind: Kind,
        loaded: &mut Vec<(Entry, Arc<T>)>,
        part: T,
        join: impl FnOnce(&T, &T) -> T,
    ) {
        let Some((directory, key)) = self.place(entry, kind) else {
            return;
        };
        let position = loaded.iter().position(|(loaded, _)| loaded == entry);
        let kept = match position {
            Some(position) => join(&loaded[position].1, &part),
            None => part,
        };
        write(&directory, key, kind, &kept);
        let kept = Arc::new(kept);
        match position {
            Some(position) => loaded[position].1 = kept,
            None => loaded.push((entry.clone(), kept)),
        }
    }
}

impl Loaded {
    /// The records each entry holds.
    pub(crate) fn items(&self) -> impl Iterator<Item = &Items> {
        self.records.iter().map(|(_, items)| &**items)
    }

    /// The contributions each entry holds.
    pub(crate) fn contributions(&self) -> impl Iterator<Item = &Contributions> {
        self.facade
            .iter()
            .map(|(_, contributions)| &**contributions)
    }
}

/// Which entry keeps each item's record.
struct Owners {
    definitions: BTreeMap<api::identity::Definition, Entry>,
    surface: BTreeMap<crate::surface::identity::Definition, Entry>,
    /// The first shared crate's entry.
    first_shared: Option<Entry>,
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
        let mut owners = Self {
            definitions: BTreeMap::new(),
            surface: BTreeMap::new(),
            first_shared,
        };
        for id in items.definitions.keys() {
            let entry = owners.entry(sources, id);
            owners
                .surface
                .insert(crate::surface::identity::Definition::of(id), entry.clone());
            owners.definitions.insert(id.clone(), entry);
        }
        owners
    }

    /// The entry that keeps what belongs to the item `id`: its selected
    /// crate's, else the first shared crate's when a registry, a git
    /// commit or the toolchain gives it, else the project's.
    fn entry(&self, sources: &Sources, id: &api::identity::Definition) -> Entry {
        let unit = id.unit();
        let own = sources.crates.iter().position(|krate| {
            krate.crate_name == unit.crate_name()
                && krate.stable_crate_id.as_deref() == Some(unit.stable_crate_id())
        });
        let shareable = match unit.package() {
            Some(package) => {
                package.source().starts_with("registry+") || package.source().starts_with("git+")
            }
            None => true,
        };
        match (own, &self.first_shared) {
            (Some(index), _) => Entry::Crate(index),
            (None, Some(shared)) if shareable => shared.clone(),
            _ => Entry::Project,
        }
    }

    fn definition(&self, id: &api::identity::Definition) -> Option<&Entry> {
        self.definitions.get(id)
    }

    /// Whether `entry` keeps what belongs to the path naming `target`: the
    /// owner of its definition, or the project for a path naming none.
    fn owns(&self, entry: &Entry, target: Option<&crate::surface::identity::Definition>) -> bool {
        match target.and_then(|target| self.surface.get(target)) {
            Some(owner) => owner == entry,
            None => *entry == Entry::Project,
        }
    }
}

/// The contents of `key`'s entry of `kind` under `directory`, read under
/// its lease.
fn read<T: DeserializeOwned>(directory: &Path, key: &str, kind: Kind) -> Option<T> {
    let _lease = home::hold(directory, key)?;
    let bytes = std::fs::read(directory.join(key).join(kind.file())).ok()?;
    let stored = crate::extract::sidecar::read::<Stored>(&bytes).ok()?;
    if stored.schema != kind.schema() {
        return None;
    }
    api::units::read(stored.units, || {
        crate::extract::sidecar::read::<T>(stored.contents.get().as_bytes()).ok()
    })
}

/// Publish `contents` as `key`'s entry of `kind` under `directory`. Best
/// effort.
fn write<T: Serialize>(directory: &Path, key: &str, kind: Kind, contents: &T) {
    let (encoded, units) = api::units::write(|| serde_json::to_string(contents));
    let Ok(contents) = encoded.and_then(serde_json::value::RawValue::from_string) else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(&Stored {
        schema: kind.schema(),
        units,
        contents,
    }) else {
        return;
    };
    if std::fs::create_dir_all(directory).is_ok() {
        let _ = home::publish(directory, key, &[(kind.file(), &bytes)]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Authority, Policy};
    use super::Kind;

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
            super::write(&directory, &key, Kind::Records, &items);
            let read: super::Items =
                super::read(&directory, &key, Kind::Records).expect("the entry reads back");
            let (again, _, carried) = graph
                .classify_items_on(&Policy::standard(), authority, 8, &[&read])
                .expect("the fixture classifies from the records read back");
            assert_eq!(first.canonical, again.canonical);
            assert_eq!(carried.fresh(), super::super::items::Fresh::default());
            let (plan, contributions) =
                crate::surface::facade::Plan::encode_parts_from(&graph, &first, &[])
                    .expect("the fixture's plan encodes");
            assert_eq!(contributions.fresh(), graph.exports().len());
            super::write(&directory, &key, Kind::Facade, &contributions);
            let read: super::Contributions =
                super::read(&directory, &key, Kind::Facade).expect("the facade entry reads back");
            let (again, kept) =
                crate::surface::facade::Plan::encode_parts_from(&graph, &first, &[&read])
                    .expect("the fixture's plan encodes from its contributions");
            assert_eq!(plan, again, "a plan of kept contributions is the plan");
            assert_eq!(kept.fresh(), 0);
            assert_eq!(
                kept.written(),
                crate::surface::facade::contribution::Written::default(),
                "a plan of kept header units and sites writes none of them"
            );
        }
        std::fs::remove_dir_all(&directory).expect("the scratch directory goes");
    }
}
