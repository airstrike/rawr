//! Each crate's classification records and facade contributions, kept
//! in rawr's home for a shared crate, and in the project for the rest
//! (#271, #1618).
//!
//! Every crate an API reaches keeps the records of its own items, and the
//! facade contributions of the paths to them, in an entry keyed by its
//! compiler unit: the package, target, features and stable crate id that
//! make it that crate. A crate the API selects or reaches the same way in
//! another project has the same key there, so its entry is the same
//! whichever crates select or reach it (#2223). A path that names no
//! definition is its root's: the crate its root alias selects keeps it
//! (#2233). An entry is in a directory of the classification's version, or
//! of the facade plan's. A crate shared per machine (D608.4), and one a
//! registry, a git commit or the toolchain gives that a project sharing
//! any reaches, keeps its entry in rawr's home beside the shared tables;
//! the project's own crates, and path crates, keep theirs in the project's
//! Rawr directory, for its next round or run. A later classification on
//! the machine of an API that reads the same of any item takes its record
//! instead of classifying it again (`items`), and a plan of an outcome kept
//! takes its contribution (`surface::facade::contribution`), whatever
//! project it is for.
//!
//! An entry is a stack of layers, each the records one classification
//! found anew, newest last. Publishing adds a layer and links the others
//! unchanged, so it writes only what changed (#2224); a layer no larger
//! than the one added is joined into it first, so each layer is larger
//! than the one after it, and an entry holds a logarithmic number of them.
//! An entry is a cache: one that can't be read is a miss, and publishing
//! is best effort. A part that could not be published, as when a reader
//! held its entry, is published with the next one of that entry.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::items::Items;
use super::{api, Policy};
use crate::extract::engine::home;
use crate::surface::facade::contribution::{Contributions, Owner};

/// The format of a records entry's file: bump it when what a record holds
/// changes. 2: an entry is a stack of layers (#2224). 3: a record keeps
/// its item's canonical bytes (#2221).
const RECORDS_SCHEMA: u32 = 3;
/// The format of a facade entry's file: bump it when what a contribution
/// or a header unit holds changes. 2: the entry keeps its plan's header
/// units (#2222). 3: an entry is a stack of layers (#2224), and a
/// contribution of a path naming no definition keeps the path (#2233).
/// 4: a contribution is kept by the digest of its outcome's canonical bytes
/// and its plans' (#2221).
const FACADE_SCHEMA: u32 = 4;
/// The project's own entry, under its classified directory.
const PROJECT: &str = "project";

/// Where a classification's records are kept: one entry for each crate
/// it reaches, and the project's own.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    crates: Vec<Crate>,
    /// The registry and git packages whose closure reaches a path package,
    /// by Cargo id: a crate of one is kept with the project (D608.4).
    local_closures: BTreeSet<Box<str>>,
    /// The project's classified directory.
    project: Option<PathBuf>,
    /// The classified directory in rawr's home, beside its bindings.
    home: Option<PathBuf>,
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

impl Crate {
    /// Whether this names the same table as `other`, whichever of the two
    /// settlements said its stable crate id.
    fn names(&self, other: &Self) -> bool {
        self.key == other.key && self.bindings == other.bindings && self.shared == other.shared
    }
}

/// What an entry keeps.
#[derive(Clone, Copy)]
enum Kind {
    /// Each item's classification record (`items`).
    Records,
    /// Each path's facade contribution.
    Facade,
}

/// An entry's layer: the compiler units its contents name once, and the
/// contents, naming each unit by its place.
#[derive(Deserialize, Serialize)]
struct Stored {
    schema: u32,
    units: Box<[Arc<api::identity::Unit>]>,
    contents: Box<serde_json::value::RawValue>,
}

/// What an entry holds, as layers of it join.
trait Layer: Serialize + DeserializeOwned {
    /// These with `later`'s beside them, `later`'s first.
    fn with(&self, later: &Self) -> Self;
}

impl Layer for Items {
    fn with(&self, later: &Self) -> Self {
        Self::with(self, later)
    }
}

impl Layer for Contributions {
    fn with(&self, later: &Self) -> Self {
        Self::with(self, later)
    }
}

/// What a classification and its plan read of the entries, and what they
/// have yet to publish there.
#[derive(Default)]
pub(crate) struct Loaded {
    records: Entries<Items>,
    facade: Entries<Contributions>,
}

/// The entries of one kind: what was read of each, which were tried, and
/// each one's part not yet published.
struct Entries<T> {
    read: Vec<Arc<T>>,
    tried: BTreeSet<Entry>,
    pending: BTreeMap<Entry, T>,
}

impl<T> Default for Entries<T> {
    fn default() -> Self {
        Self {
            read: Vec::new(),
            tried: BTreeSet::new(),
            pending: BTreeMap::new(),
        }
    }
}

/// Where one entry is kept.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Entry {
    /// A crate's, by its compiler unit's key ([`unit_key`]): in rawr's
    /// home when it is shared, else in the project's Rawr directory.
    Crate { key: Box<str>, shared: bool },
    /// What no crate keeps, in the project's Rawr directory.
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

    /// The stem of this kind's layers' names.
    const fn stem(self) -> &'static str {
        match self {
            Self::Records => "records",
            Self::Facade => "facade",
        }
    }

    /// The name of this kind's layer `index`.
    fn layer(self, index: u64) -> String {
        format!("{}.{index:016}.json", self.stem())
    }

    /// The index of this kind's layer named `name`.
    fn index(self, name: &str) -> Option<u64> {
        name.strip_prefix(self.stem())?
            .strip_prefix('.')?
            .strip_suffix(".json")?
            .parse()
            .ok()
    }
}

impl Sources {
    /// The entries of `crates`, and the project's, under its Rawr directory
    /// `rawr`, with the shared ones beside the home's `bindings`.
    /// `local_closures` are the registry and git packages whose closure
    /// reaches a path package.
    pub(crate) fn new(
        crates: Vec<Crate>,
        local_closures: Box<[Box<str>]>,
        rawr: Option<&Path>,
        bindings: Option<&Path>,
    ) -> Self {
        let project = rawr.map(|rawr| {
            crate::bindings::format::project(rawr, crate::bindings::format::Kept::Classified)
        });
        Self {
            crates,
            local_closures: local_closures.into_vec().into_iter().collect(),
            project,
            home: bindings.map(crate::bindings::format::classified_beside),
        }
    }

    /// Whether these name every entry `other` names.
    pub(crate) fn covers(&self, other: &Self) -> bool {
        self.project == other.project
            && (other.home.is_none() || self.home == other.home)
            && other.local_closures.is_subset(&self.local_closures)
            && other
                .crates
                .iter()
                .all(|krate| self.crates.iter().any(|known| known.names(krate)))
    }

    /// Add the entries of `other` these don't name.
    pub(crate) fn join(&mut self, other: Self) {
        if other.project.is_some() {
            self.project = other.project;
        }
        if other.home.is_some() {
            self.home = other.home;
        }
        self.local_closures.extend(other.local_closures);
        for krate in other.crates {
            if !self.crates.iter().any(|known| known.names(&krate)) {
                self.crates.push(krate);
            }
        }
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

    /// The classified directory in rawr's home.
    fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    fn place<'e>(&'e self, entry: &'e Entry, kind: Kind) -> Option<(PathBuf, &'e str)> {
        match entry {
            Entry::Crate { key, shared: true } => Some((self.home()?.join(kind.version()), key)),
            Entry::Crate { key, shared: false } => {
                Some((self.project.as_ref()?.join(kind.version()), key))
            }
            Entry::Project => Some((self.project.as_ref()?.join(kind.version()), PROJECT)),
        }
    }

    /// The entry of `unit`'s crate: shared when it is a selected crate
    /// whose table is; else, as for a crate reached or one a later round's
    /// settlement no longer names, when a registry, a git commit or the
    /// toolchain gives it, no path package is in its closure, and rawr's
    /// home keeps entries.
    fn unit_entry(&self, unit: &api::identity::Unit) -> Entry {
        let selected = self.crates.iter().find(|krate| {
            krate.crate_name == unit.crate_name()
                && krate.stable_crate_id.as_deref() == Some(unit.stable_crate_id())
        });
        let shared = match selected {
            Some(krate) => krate.shared,
            None => {
                let given = unit.package().is_none_or(|package| {
                    (package.source().starts_with("registry+")
                        || package.source().starts_with("git+"))
                        && !self.local_closures.contains(package.cargo_id())
                });
                given && self.home.is_some()
            }
        };
        unit_key(unit).map_or(Entry::Project, |key| Entry::Crate { key, shared })
    }

    /// The entries a classification of `graph` may read: the crate's of
    /// each unit it reaches, and the project's.
    fn entries(&self, graph: &api::Graph<impl api::Phase>) -> BTreeSet<Entry> {
        graph
            .definition_units()
            .into_iter()
            .map(|unit| self.unit_entry(unit))
            .chain(std::iter::once(Entry::Project))
            .collect()
    }

    /// Read each entry of both kinds a classification of `graph` may read
    /// that `loaded` has not tried.
    pub(crate) fn load(&self, loaded: &mut Loaded, graph: &api::Graph<impl api::Phase>) {
        let entries = self.entries(graph);
        self.load_kind(&entries, Kind::Records, &mut loaded.records);
        self.load_kind(&entries, Kind::Facade, &mut loaded.facade);
    }

    fn load_kind<T: Layer>(&self, entries: &BTreeSet<Entry>, kind: Kind, kept: &mut Entries<T>) {
        for entry in entries {
            if !kept.tried.insert(entry.clone()) {
                continue;
            }
            let Some((directory, key)) = self.place(entry, kind) else {
                continue;
            };
            if let Some(contents) = read(&directory, key, kind) {
                kept.read.push(Arc::new(contents));
            }
        }
    }

    /// Publish each entry's part of `items`, a classification of `graph`:
    /// the records it classified anew.
    pub(crate) fn publish(
        &self,
        items: &Items,
        graph: &api::Graph<impl api::Phase>,
        loaded: &mut Loaded,
    ) {
        let owners = Owners::of(self, items, graph);
        let parts = items.fresh_parts(
            |id| owners.definition(id),
            |export, record| owners.export(export.root_alias(), record.target()),
        );
        self.keep(parts, Kind::Records, &mut loaded.records);
    }

    /// Publish each entry's part of `contributions`, a plan's, as
    /// [`publish`](Self::publish) does records: each is its path's, whose
    /// owner `items` and `graph` tell.
    pub(crate) fn publish_facade(
        &self,
        contributions: &Contributions,
        items: &Items,
        graph: &api::Graph<impl api::Phase>,
        loaded: &mut Loaded,
    ) {
        let owners = Owners::of(self, items, graph);
        let parts = contributions.fresh_parts(
            |owner| owners.contribution(owner),
            |id| owners.definition(id),
        );
        self.keep(parts, Kind::Facade, &mut loaded.facade);
    }

    /// Publish each of `parts` as a layer of its entry, with what `kept`
    /// had yet to publish there. A part an entry's reader kept out waits
    /// for the next publication.
    fn keep<T: Layer>(&self, parts: BTreeMap<Entry, T>, kind: Kind, kept: &mut Entries<T>) {
        let mut parts = parts;
        for (entry, pending) in std::mem::take(&mut kept.pending) {
            let joined = match parts.remove(&entry) {
                Some(part) => pending.with(&part),
                None => pending,
            };
            parts.insert(entry, joined);
        }
        for (entry, part) in parts {
            let Some((directory, key)) = self.place(&entry, kind) else {
                continue;
            };
            match publish(&directory, key, kind, &part) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    kept.pending.insert(entry, part);
                }
                _ => {}
            }
        }
    }
}

impl Loaded {
    /// The records each entry holds.
    pub(crate) fn items(&self) -> impl Iterator<Item = &Items> {
        self.records.read.iter().map(|items| &**items)
    }

    /// The contributions each entry holds.
    pub(crate) fn contributions(&self) -> impl Iterator<Item = &Contributions> {
        self.facade
            .read
            .iter()
            .map(|contributions| &**contributions)
    }
}

/// The key of the entry of the crate of `unit`: a digest of the unit's
/// whole identity, after its crate's name.
fn unit_key(unit: &api::identity::Unit) -> Option<Box<str>> {
    let identity = serde_json::to_vec(unit).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"rawr.classified.unit/v1\0");
    digest.update(&identity);
    let digest = crate::extract::engine::hex(&digest.finalize());
    Some(format!("{}-{digest}", unit.crate_name()).into_boxed_str())
}

/// Which entry keeps each item's record.
struct Owners<'g> {
    /// The entry of each unit seen, by the unit's place in memory.
    units: HashMap<*const api::identity::Unit, Entry>,
    surface: BTreeMap<crate::surface::identity::Definition, Entry>,
    /// The entry of the crate each root alias selects.
    roots: BTreeMap<&'g str, Entry>,
    sources: &'g Sources,
}

impl<'g> Owners<'g> {
    /// The entry of each unit of `graph`, of each definition `items` holds,
    /// and of each root alias of `graph`.
    fn of(sources: &'g Sources, items: &Items, graph: &'g api::Graph<impl api::Phase>) -> Self {
        let mut owners = Self {
            units: HashMap::new(),
            surface: BTreeMap::new(),
            roots: BTreeMap::new(),
            sources,
        };
        let units = graph.definition_units();
        for unit in &units {
            owners.unit(unit);
        }
        for (alias, crate_name, stable_crate_id) in graph.root_crates() {
            let entry = units
                .iter()
                .find(|unit| {
                    unit.crate_name() == crate_name && unit.stable_crate_id() == stable_crate_id
                })
                .map_or(Entry::Project, |unit| owners.unit(unit));
            owners.roots.insert(alias, entry);
        }
        for id in items.definitions.keys() {
            let entry = owners.unit(id.shared_unit());
            owners
                .surface
                .insert(crate::surface::identity::Definition::of(id), entry);
        }
        owners
    }

    /// The entry of `unit`, found once.
    fn unit(&mut self, unit: &Arc<api::identity::Unit>) -> Entry {
        self.units
            .entry(Arc::as_ptr(unit))
            .or_insert_with(|| self.sources.unit_entry(unit))
            .clone()
    }

    /// The entry that keeps what belongs to the item `id`.
    fn definition(&self, id: &api::identity::Definition) -> Entry {
        self.units
            .get(&Arc::as_ptr(id.shared_unit()))
            .cloned()
            .unwrap_or_else(|| self.sources.unit_entry(id.unit()))
    }

    /// The entry of the crate `alias` selects.
    fn root(&self, alias: &str) -> Entry {
        self.roots.get(alias).cloned().unwrap_or(Entry::Project)
    }

    /// The entry that keeps a path from the root `alias` naming `target`:
    /// its definition's, or its root's when it names none.
    fn export(&self, alias: &str, target: Option<&crate::surface::identity::Definition>) -> Entry {
        target
            .and_then(|target| self.surface.get(target))
            .cloned()
            .unwrap_or_else(|| self.root(alias))
    }

    /// The entry that keeps a contribution belonging to `owner`.
    fn contribution(&self, owner: Owner<'_>) -> Entry {
        match owner {
            Owner::Definition(target) => {
                self.surface.get(target).cloned().unwrap_or(Entry::Project)
            }
            Owner::Unnamed(_) => owner.root().map_or(Entry::Project, |root| self.root(root)),
        }
    }
}

/// Each layer of `kind` in the entry directory `entry`, oldest first: its
/// index, its name and its length.
fn layers(entry: &Path, kind: Kind) -> Vec<(u64, String, u64)> {
    let Ok(files) = std::fs::read_dir(entry) else {
        return Vec::new();
    };
    let mut layers = files
        .flatten()
        .filter_map(|file| {
            let name = file.file_name().into_string().ok()?;
            let index = kind.index(&name)?;
            let length = file.metadata().ok()?.len();
            Some((index, name, length))
        })
        .collect::<Vec<_>>();
    layers.sort_unstable();
    layers
}

/// The contents of `key`'s entry of `kind` under `directory`, every layer
/// joined, read under its lease. A layer that can't be read is left out.
fn read<T: Layer>(directory: &Path, key: &str, kind: Kind) -> Option<T> {
    let _lease = home::hold(directory, key)?;
    let entry = directory.join(key);
    let mut joined: Option<T> = None;
    for (_, name, _) in layers(&entry, kind) {
        let Some(layer) = read_layer::<T>(&entry.join(name), kind) else {
            continue;
        };
        joined = Some(match joined {
            Some(earlier) => earlier.with(&layer),
            None => layer,
        });
    }
    joined
}

/// The contents of the layer of `kind` at `path`.
fn read_layer<T: DeserializeOwned>(path: &Path, kind: Kind) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    let stored = crate::extract::sidecar::read::<Stored>(&bytes).ok()?;
    if stored.schema != kind.schema() {
        return None;
    }
    api::units::read(stored.units, || {
        crate::extract::sidecar::read::<T>(stored.contents.get().as_bytes()).ok()
    })
}

/// `contents` as a layer of `kind`.
fn encode<T: Serialize>(kind: Kind, contents: &T) -> io::Result<Vec<u8>> {
    let (encoded, units) = api::units::write(|| serde_json::to_string(contents));
    let contents = encoded.and_then(serde_json::value::RawValue::from_string)?;
    Ok(serde_json::to_vec(&Stored {
        schema: kind.schema(),
        units,
        contents,
    })?)
}

/// Publish `part` as a new layer of `key`'s entry of `kind` under
/// `directory`, its other layers linked as they are. The newest layers no
/// larger than it are joined into it first.
fn publish<T: Layer>(directory: &Path, key: &str, kind: Kind, part: &T) -> io::Result<()> {
    let added = encode(kind, part)?;
    home::extend(directory, key, |entry| {
        let mut layers = layers(entry, kind);
        let index = layers.last().map_or(0, |(last, _, _)| last + 1);
        let mut joined: Option<T> = None;
        let mut bytes = added;
        while let Some((_, name, length)) = layers.last()
            && *length <= bytes.len() as u64
        {
            if let Some(earlier) = read_layer::<T>(&entry.join(name), kind) {
                let both = earlier.with(joined.as_ref().unwrap_or(part));
                bytes = encode(kind, &both)?;
                joined = Some(both);
            }
            layers.pop();
        }
        Ok(home::Extension {
            kept: layers.into_iter().map(|(_, name, _)| name).collect(),
            added: vec![(kind.layer(index), bytes)],
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::PathBuf;

    use super::super::{Authority, Policy};
    use super::{Kind, Layer};

    fn scratch(name: &str) -> PathBuf {
        let directory = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-classified-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the scratch directory is made");
        directory
    }

    /// Records written to an entry and read back are the records: a
    /// classification of the same graph from them classifies nothing, to
    /// the same bytes.
    #[test]
    fn records_read_back_from_an_entry_carry_every_item() {
        if !crate::extract::engine::home::alone(
            "classify::records::tests::records_read_back_from_an_entry_carry_every_item",
        ) {
            return;
        }
        let directory = scratch("records");
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
            super::publish(&directory, &key, Kind::Records, &items).expect("the entry publishes");
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
            super::publish(&directory, &key, Kind::Facade, &contributions)
                .expect("the facade entry publishes");
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

    /// Values by name, the later layer's first, as a test's layer.
    #[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
    struct Named(BTreeMap<String, String>);

    impl Layer for Named {
        fn with(&self, later: &Self) -> Self {
            let mut joined = later.0.clone();
            for (name, value) in &self.0 {
                joined.entry(name.clone()).or_insert_with(|| value.clone());
            }
            Self(joined)
        }
    }

    fn named(pairs: &[(&str, &str)]) -> Named {
        Named(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    /// Each layer of the entry `key`: its name and its file's inode.
    fn layers(directory: &std::path::Path, key: &str) -> Vec<(String, u64)> {
        super::layers(&directory.join(key), Kind::Records)
            .into_iter()
            .map(|(_, name, _)| {
                let inode = std::fs::metadata(directory.join(key).join(&name))
                    .unwrap()
                    .ino();
                (name, inode)
            })
            .collect()
    }

    /// A registry crate no settlement names is kept in rawr's home, unless
    /// its closure reaches a path package, as a `[patch]` makes it: then it
    /// stays with the project, as a path crate does (#2235).
    #[test]
    fn a_registry_crate_reaching_a_path_package_keeps_its_records_in_the_project() {
        use crate::api::identity::{Authority, Compilation, Package, Unit};
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        let unit = |name: &str, source: &str| {
            let id = format!("{source}#{name}@1.0.0");
            Unit::new(
                Authority::Cargo(Package::new(
                    id.into(),
                    name.into(),
                    "1.0.0".into(),
                    source.into(),
                )),
                format!("/{name}/src/lib.rs").into(),
                "lib".into(),
                name.into(),
                Compilation::Target,
                Box::default(),
                "0123456789abcdef".into(),
            )
        };
        let patched = unit("patched", registry);
        let plain = unit("plain", registry);
        let path = unit("path", "path+file:///project/path");
        let sources = super::Sources::new(
            Vec::new(),
            Box::from([patched.package().unwrap().cargo_id().into()]),
            Some(std::path::Path::new("/project/target/rawr")),
            Some(std::path::Path::new("/home/rawr/bindings")),
        );
        let shared = |unit: &Unit| {
            matches!(
                sources.unit_entry(unit),
                super::Entry::Crate { shared: true, .. }
            )
        };
        assert!(shared(&plain));
        assert!(!shared(&path));
        assert!(!shared(&patched));
        let mut joined = super::Sources::new(
            Vec::new(),
            Box::default(),
            Some(std::path::Path::new("/project/target/rawr")),
            Some(std::path::Path::new("/home/rawr/bindings")),
        );
        assert!(!joined.covers(&sources));
        joined.join(sources.clone());
        assert!(matches!(
            joined.unit_entry(&patched),
            super::Entry::Crate { shared: false, .. }
        ));
    }

    /// Publishing a part smaller than the entry's newest layer adds a layer
    /// and links the others as they are: it writes only the part. A part as
    /// large as the newest layer is joined with it into one, and the entry
    /// reads as every part joined, the later ones first (#2224).
    #[test]
    fn publishing_writes_a_layer_and_links_the_rest() {
        if !crate::extract::engine::home::alone(
            "classify::records::tests::publishing_writes_a_layer_and_links_the_rest",
        ) {
            return;
        }
        let directory = scratch("layers");
        let large = "x".repeat(4096);
        super::publish(
            &directory,
            "crate",
            Kind::Records,
            &named(&[("a", &large), ("b", "old")]),
        )
        .unwrap();
        let first = layers(&directory, "crate");
        assert_eq!(first.len(), 1);
        super::publish(&directory, "crate", Kind::Records, &named(&[("b", "new")])).unwrap();
        let second = layers(&directory, "crate");
        assert_eq!(second.len(), 2, "a small part is a layer of its own");
        assert_eq!(
            second[0], first[0],
            "the earlier layer is linked, not written"
        );
        super::publish(&directory, "crate", Kind::Records, &named(&[("c", "more")])).unwrap();
        let third = layers(&directory, "crate");
        assert_eq!(
            third.len(),
            2,
            "a part as large as the newest layer joins it"
        );
        assert_eq!(third[0], first[0]);
        let read: Named = super::read(&directory, "crate", Kind::Records).unwrap();
        assert_eq!(
            read,
            named(&[("a", &large), ("b", "new"), ("c", "more")]),
            "the entry reads as its parts joined, the later ones first"
        );
        std::fs::remove_dir_all(&directory).expect("the scratch directory goes");
    }
}
