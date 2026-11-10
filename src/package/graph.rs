//! Canonical static source/package graph construction.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::item;
use crate::package::{error, manifest, source, Error};
use crate::{diagnostic, parse, project, Engine, ImmutableString, Position, Span};

type Parts = (
    Box<[Package]>,
    Box<[source::Source]>,
    Box<[Unit]>,
    Id,
    source::Id,
);

/// An opaque package identity local to one source graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

/// One canonical package inside a static graph: the crate being run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    id: Id,
    name: ImmutableString,
    root: PathBuf,
    source: source::Id,
    rust_modules: Vec<RustModule>,
    /// The crate root, when it is native Rust that declares marked modules.
    native_root: Option<NativeRoot>,
    /// The package's native binary, when the program runs from its library.
    bin: Option<Bin>,
    /// A marked root's `#[X::main]` above its `async fn main`, as written:
    /// the shadow crate's native entry carries it (D513.4).
    entry: Option<Box<str>>,
    cargo: Option<manifest::cargo::Cargo>,
    pub(crate) rust: Option<manifest::rust::Config>,
    /// The profile's panic strategy, as Cargo builds the target.
    panic: project::cargo::Panic,
    /// The features Cargo enables for the target: its `cfg(feature)`s.
    features: project::cargo::features::Enabled,
    /// The crate is the package's library, not a binary.
    library: bool,
    /// The package's target the crate is, as Cargo lists it.
    target: Option<project::cargo::Target>,
    /// The crate root has a frontmatter block: a cargo script.
    script: bool,
}

/// One native module of a marked crate: an unmarked `.rs` file in the
/// crate's module tree, compiled by Rust, never by Rawr.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustModule {
    modules: Box<[ImmutableString]>,
    path: PathBuf,
    text: String,
}

/// A native crate root (D501) and the marked modules of its tree. Every
/// other name at the root is native: a native module or one of the root's
/// own items, which marked code reaches through the app crate (#1185).
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeRoot {
    module: RustModule,
    /// Each marked module's path from the crate root, at any depth.
    marked: Box<[Box<[ImmutableString]>]>,
    /// The native modules on the way from the root to a marked module
    /// (#1467), as `["workspace", "screen"]` for a marked
    /// `workspace::screen::home`: native, and also Rawr sources of their
    /// `mod` declarations alone, as the root is ([`island::module_text`]).
    ///
    /// [`island::module_text`]: super::island::module_text
    bridges: Vec<Box<[ImmutableString]>>,
    /// The root's top-level `use` declarations, parsed: what a marked
    /// module's path through `crate` stands for, as `pub use
    /// theme::Theme;` makes `crate::Theme` the marked `theme::Theme`.
    uses: Box<[crate::item::Parsed]>,
}

/// A package's binary crate when the program runs from its library: an
/// unmarked `src/main.rs` whose tree has no marked module, beside a native
/// `src/lib.rs` that declares one. It is native, its own crate, and it
/// reaches the library as Cargo builds it, by the library's crate name. Its
/// `main` is the program's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bin {
    root: RustModule,
    modules: Vec<RustModule>,
}

/// The module tree a native module is recorded in.
#[derive(Clone, Copy)]
enum Tree {
    /// The tree of the crate Rawr runs.
    Crate,
    /// The tree of a native binary beside it ([`Bin`]).
    Bin,
}

/// Which `mod` declarations of a native module Rust's rules are followed
/// for.
#[derive(Clone, Copy)]
enum Declared {
    /// Every one.
    All,
    /// Those inside its inline modules: a bridge's own are a Rawr source's
    /// (#1467).
    Inline,
}

/// The file a `mod name;` declaration resolves to under Rust's rules.
enum ModuleFile {
    /// No file: the path reported missing.
    Missing(PathBuf),
    /// A `.rs` file marked `// rawr`, with the text read to see the marker.
    Marked(PathBuf, String),
    /// An unmarked `.rs` file.
    Rust(PathBuf),
}

/// A parsed source paired with its opaque graph identity.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Unit {
    pub(crate) source: source::Id,
    pub(crate) parsed: parse::Unit,
}

/// A canonical, fully parsed static package and file-module graph.
#[derive(Debug)]
pub struct Graph {
    packages: Box<[Package]>,
    sources: Box<[source::Source]>,
    #[allow(dead_code)]
    units: Box<[Unit]>,
    entry_package: Id,
    entry_source: source::Id,
    /// Each crate derive whose expansion was loaded beside its item, by the
    /// derive's source and span: the macro the expansion came from (#593).
    expanded: BTreeMap<(source::Id, Span), ImmutableString>,
    /// Each item whose kept expansion is of its text before an edit
    /// (#1040), with the derives that are due again.
    #[cfg(feature = "rust-import")]
    stale: Vec<rawr_import::expand::Item>,
    /// A host's one in-memory file ([`Graph::load_parsed`]), whose paths
    /// may start at any crate the host registered, as no manifest names
    /// its dependencies.
    hosted: bool,
}

impl Id {
    fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

impl Package {
    pub(crate) const fn id(&self) -> Id {
        self.id
    }
    /// Return the name Rawr gives the crate's items (`main`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the canonical package root: where Cargo builds the package
    /// and Rawr keeps what it builds for it. A script's is its directory in
    /// rawr's area for scripts, which holds its manifest and lockfile.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the package is a cargo script: a crate root with a
    /// frontmatter block, its own package wherever it sits.
    #[must_use]
    pub const fn is_script(&self) -> bool {
        self.script
    }

    /// Return the crate root source (`src/main.rs`, `src/lib.rs` or the
    /// explicit file).
    #[must_use]
    pub const fn source(&self) -> source::Id {
        self.source
    }

    /// Return the native `.rs` modules of this package's module tree, in discovery order.
    #[must_use]
    pub fn rust_modules(&self) -> &[RustModule] {
        &self.rust_modules
    }

    /// The crate root when it is native Rust: an unmarked `src/main.rs` (or
    /// `src/lib.rs`) that declares marked modules. Rawr reads only its `mod`
    /// declarations; the native build compiles it, and it calls the marked
    /// modules through the trampolines Rawr generates for them.
    #[must_use]
    pub fn native_root(&self) -> Option<&RustModule> {
        self.native_root.as_ref().map(|root| &root.module)
    }

    /// The package's native binary, when the program runs from its native
    /// library root: its `main` is the program's, and it reaches the
    /// library's marked modules through their trampolines.
    #[must_use]
    pub const fn bin(&self) -> Option<&Bin> {
        self.bin.as_ref()
    }

    /// The package's target the crate is, as Cargo lists it: `None` for a
    /// file no package lists, or when Cargo is not installed.
    #[must_use]
    pub const fn cargo_target(&self) -> Option<&project::cargo::Target> {
        self.target.as_ref()
    }

    /// Whether the crate is the package's library, not one of its binaries.
    #[must_use]
    pub const fn is_library(&self) -> bool {
        self.library
    }

    /// The profile's panic strategy, as Cargo builds the package's target:
    /// with `panic = "abort"` a panic aborts the process.
    #[must_use]
    pub const fn panic(&self) -> project::cargo::Panic {
        self.panic
    }

    /// The features Cargo enables for the package's target, under which
    /// its `cfg(feature = "..")`s hold: none for a file outside a package,
    /// and for a runner's snapshot, which does not ask Cargo, the ones its
    /// parent's Cargo enabled for the entry package.
    #[must_use]
    pub const fn features(&self) -> &project::cargo::features::Enabled {
        &self.features
    }

    /// The crate name the native modules are imported under, when the
    /// crate has native code and its root `Cargo.toml` is a package.
    #[must_use]
    pub fn rust_crate(&self) -> Option<&str> {
        self.cargo
            .as_ref()
            .filter(|_| {
                !self.rust_modules.is_empty() || self.native_root.is_some() || self.entry.is_some()
            })
            .map(|cargo| cargo.crate_name.as_str())
    }

    /// The attribute of a marked root's `async fn main`, as written, such
    /// as `#[tokio::main]`: its runtime's entry, which the shadow crate's
    /// native entry carries (D513.4).
    #[must_use]
    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }

    /// Whether `path`, from the crate root, lands in native code: its
    /// innermost module is native. That is a native module it is below, or
    /// a native root for any name there but the `main` Rawr gives it and the
    /// island module its shadow crate adds (`__rawr_island`), whose items
    /// are native, unless a marked module holds the path more closely: a
    /// marked module's items are Rawr's at any depth, below a native module
    /// too (#1467), and those of the native modules it declares are native.
    pub(crate) fn is_native_path<S: AsRef<str>>(&self, path: &[S]) -> bool {
        let starts = |modules: &[ImmutableString]| {
            path.len() >= modules.len()
                && modules
                    .iter()
                    .zip(path)
                    .all(|(module, segment)| module.as_str() == segment.as_ref())
        };
        let root = self.native_root.as_ref().and_then(|_| {
            let first = path.first()?.as_ref();
            let rawr = first == "main" || (first == super::island::MODULE && path.len() == 1);
            (!rawr).then_some(0)
        });
        let native = self
            .rust_modules
            .iter()
            .filter(|module| path.len() > module.modules.len() && starts(&module.modules))
            .map(|module| module.modules.len())
            .chain(root)
            .max();
        let marked = self.native_root.as_ref().and_then(|root| {
            root.marked
                .iter()
                .filter(|modules| starts(modules))
                .map(|modules| modules.len())
                .max()
        });
        native.is_some_and(|native| marked.is_none_or(|marked| native > marked))
            || (self.entry.is_some() && path.len() > 1 && path[0].as_ref() == super::entry::MODULE)
    }

    /// Whether `path`, from the crate root, names native code: a native
    /// module itself (`["units"]`, which `use crate::units;` binds) or a
    /// path below one. A native module on the way to a marked one is Rawr's
    /// module too, whose `mod` declarations lead Rawr to the marked one.
    pub(crate) fn names_native<S: AsRef<str>>(&self, path: &[S]) -> bool {
        self.is_native_path(path)
            || self.rust_modules.iter().any(|module| {
                module.modules.len() == path.len()
                    && module
                        .modules
                        .iter()
                        .zip(path)
                        .all(|(module, segment)| module.as_str() == segment.as_ref())
                    && !self.is_bridge(&module.modules)
            })
    }

    /// Whether the native module `modules` is on the way from a native
    /// root to a marked module (#1467).
    pub(crate) fn is_bridge(&self, modules: &[ImmutableString]) -> bool {
        self.native_root
            .as_ref()
            .is_some_and(|root| root.bridges.iter().any(|bridge| **bridge == *modules))
    }

    /// The root `Cargo.toml` path and exact text, when it declares a `[package]`.
    #[must_use]
    pub fn cargo_manifest(&self) -> Option<(&Path, &str)> {
        self.cargo
            .as_ref()
            .map(|cargo| (cargo.path.as_path(), cargo.text.as_str()))
    }

    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    pub(crate) const fn cargo(&self) -> Option<&manifest::cargo::Cargo> {
        self.cargo.as_ref()
    }
}

impl RustModule {
    /// Return the module path from the crate root (`["view", "widgets"]`).
    #[must_use]
    pub fn modules(&self) -> &[ImmutableString] {
        &self.modules
    }

    /// Return the canonical `.rs` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the exact Rust source read while loading this graph.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Bin {
    /// The binary's crate root, `src/main.rs`.
    #[must_use]
    pub const fn root(&self) -> &RustModule {
        &self.root
    }

    /// The native modules of the binary's own tree, in discovery order.
    #[must_use]
    pub fn modules(&self) -> &[RustModule] {
        &self.modules
    }
}

impl Graph {
    /// Load and parse one complete target graph through the supplied Engine.
    ///
    /// Every source path is canonicalized before it enters this proof
    /// bundle. Only declared `mod` edges are traversed.
    pub fn load(engine: &Engine, target: &project::Target) -> Result<Self, Error> {
        Builder::new(engine).load(target)
    }

    /// The root of the package `target` selects: the project's root, or for
    /// one file, its directory (the crate's root for a `src/` file beside a
    /// `Cargo.toml`). A script's is its directory in rawr's area for
    /// scripts, as [`Package::root`] has it.
    #[must_use]
    pub fn root_of(target: &project::Target) -> PathBuf {
        match target {
            project::Target::File(source)
                if crate::project::frontmatter::is_script(source.text()) =>
            {
                manifest::script::directory(source.path())
            }
            project::Target::File(source) => {
                file_root(source.path(), source.text(), |manifest| manifest.is_file())
            }
            project::Target::Project(project) => project.root().as_path().to_path_buf(),
        }
    }

    /// [`Graph::load`], reading each source's parse from its cell in
    /// `cells` when one holds exactly its text, and parsing the rest.
    pub fn load_with_cells(
        engine: &Engine,
        target: &project::Target,
        cells: &super::Cells,
    ) -> Result<Self, Error> {
        let mut builder = Builder::new(engine);
        builder.cells = Some(cells);
        builder.load(target)
    }

    /// Parse one immutable in-memory snapshot through the package graph
    /// loader: a runner's, or one file whose inline modules make a crate.
    #[doc(hidden)]
    pub fn load_snapshot(
        engine: &Engine,
        snapshot: &super::snapshot::Snapshot,
    ) -> Result<Self, Error> {
        Builder::from_snapshot(engine, snapshot).load_snapshot()
    }

    /// [`Graph::load_snapshot`] of one file the engine already parsed as
    /// `entry`, which the graph keeps rather than parsing its text again.
    pub(crate) fn load_parsed(
        engine: &Engine,
        snapshot: &super::snapshot::Snapshot,
        entry: crate::parse::Unit,
    ) -> Result<Self, Error> {
        let mut builder = Builder::from_snapshot(engine, snapshot);
        builder.entry = Some(entry);
        builder.load_snapshot()
    }

    /// Return all packages in deterministic dependency-discovery order.
    #[must_use]
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// Return all declared Rawr source snapshots.
    #[must_use]
    pub fn sources(&self) -> &[source::Source] {
        &self.sources
    }

    /// Return the application package.
    #[must_use]
    pub fn entry_package(&self) -> &Package {
        &self.packages[self.entry_package.0]
    }

    /// Return the application entry source.
    #[must_use]
    pub fn entry_source(&self) -> &source::Source {
        &self.sources[self.entry_source.index()]
    }

    /// Return whether the application package declares native Rust imports.
    #[must_use]
    pub fn declares_rust_import(&self) -> bool {
        self.entry_package().rust.is_some()
    }

    /// The features the application package's Cargo.toml names for its
    /// dependency `alias`; empty when it has no such dependency.
    #[must_use]
    pub fn dependency_features(&self, alias: &str) -> Vec<String> {
        self.entry_package()
            .rust
            .iter()
            .flat_map(|config| config.dependencies.iter())
            .filter(|dependency| dependency.alias.as_str() == alias)
            .flat_map(|dependency| dependency.features.iter().cloned())
            .collect()
    }

    /// What native code sees of the marked modules, when the crate root is
    /// native Rust (D501): a save that changes it needs a new native build.
    #[must_use]
    pub fn island_shape(&self) -> Option<super::island::Shape> {
        self.island_shadow().map(|shadow| shadow.shape)
    }

    /// The generated files that stand for the marked modules in a native
    /// root's shadow crate.
    pub(crate) fn island_shadow(&self) -> Option<super::island::Shadow> {
        let entry = self.entry_package();
        let root = entry.native_root()?;
        let module_root = root.path().parent()?;
        let sources = self.parsed_units().filter(|(source, _)| {
            source.package() == entry.name()
                && !(source.modules().is_empty() && source.path() == root.path())
                && !entry.is_bridge(source.modules())
        });
        Some(super::island::shadow(
            module_root,
            sources.map(|(source, unit)| {
                (source.modules(), source.path(), source.text(), unit.items())
            }),
            self.native_root_items(),
        ))
    }

    /// The `use` declarations of the native crate root, which name marked
    /// types by other paths, as `pub use theme::Theme;` does.
    fn native_root_items(&self) -> &[crate::item::Parsed] {
        self.entry_package()
            .native_root
            .as_ref()
            .map_or(&[], |root| &root.uses)
    }

    /// The marked statics of a native root that marked code reads as native
    /// code's items, by their paths from the crate root
    /// (`island::native_statics`); none when the root is marked.
    #[cfg(feature = "rust-import")]
    pub(crate) fn native_statics(&self) -> Vec<Vec<ImmutableString>> {
        let entry = self.entry_package();
        let Some(root) = entry.native_root() else {
            return Vec::new();
        };
        super::island::native_statics(
            self.parsed_units()
                .filter(|(source, _)| {
                    source.package() == entry.name()
                        && !(source.modules().is_empty() && source.path() == root.path())
                        && !entry.is_bridge(source.modules())
                })
                .map(|(source, unit)| (source.modules(), source.text(), unit.items())),
            self.native_root_items(),
        )
    }

    /// The marked types of a native root that the runner holds as their
    /// mirrors in rawr's copy of the crate where a crate's generic takes
    /// one (`island::mirrors`, #1249); none when the root is marked.
    #[cfg(feature = "rust-import")]
    pub(crate) fn island_mirrors(&self) -> Vec<Vec<ImmutableString>> {
        let entry = self.entry_package();
        let Some(root) = entry.native_root() else {
            return Vec::new();
        };
        super::island::mirrors(
            self.parsed_units()
                .filter(|(source, _)| {
                    source.package() == entry.name()
                        && !(source.modules().is_empty() && source.path() == root.path())
                        && !entry.is_bridge(source.modules())
                })
                .map(|(source, unit)| (source.modules(), source.text(), unit.items())),
            self.native_root_items(),
        )
    }

    /// Every source with its parse.
    pub(crate) fn parsed_units(&self) -> impl Iterator<Item = (&source::Source, &parse::Unit)> {
        self.units
            .iter()
            .map(|unit| (&self.sources[unit.source.index()], &unit.parsed))
    }

    /// Every source with its items, as loading parsed them.
    pub(crate) fn parsed_sources(
        &self,
    ) -> impl Iterator<Item = (&source::Source, &[crate::item::Parsed])> {
        self.units
            .iter()
            .map(|unit| (&self.sources[unit.source.index()], unit.parsed.items()))
    }

    /// Each item whose kept expansion is of its text before an edit: rustc
    /// expands it again, and the expansion's source changes in place, so an
    /// edit that changes only the expansion's bodies is a body edit (#1040).
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn stale_expansions(&self) -> &[rawr_import::expand::Item] {
        &self.stale
    }

    /// Each crate derive whose expansion was loaded beside its item, by the
    /// derive's source and span as written: the macro it came from.
    pub(crate) fn expanded(&self) -> crate::capability::Expanded {
        self.expanded
            .iter()
            .map(|(&(source, span), resolved)| {
                (
                    (crate::program::source::Id::from_index(source.index()), span),
                    resolved.clone(),
                )
            })
            .collect()
    }

    /// Whether this is a host's one in-memory file, which no manifest's
    /// extern prelude bounds.
    pub(crate) const fn hosted(&self) -> bool {
        self.hosted
    }

    #[allow(dead_code)]
    pub(crate) fn into_parts(self) -> Parts {
        (
            self.packages,
            self.sources,
            self.units,
            self.entry_package,
            self.entry_source,
        )
    }

    pub(crate) fn diagnostic_sources(&self) -> Box<[diagnostic::Source]> {
        self.sources
            .iter()
            .map(|source| source.diagnostic().clone())
            .collect()
    }
}

/// The package root of a source file target: its directory, or for
/// `src/<file>.rs` of a Cargo package, the directory holding
/// `Cargo.toml`, as project discovery chooses. A file with a frontmatter
/// block is a script wherever it sits, as cargo reads it (D608 point 6),
/// so its root is its directory.
fn file_root(source: &Path, text: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let parent = source
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if crate::project::frontmatter::is_script(text) {
        return parent;
    }
    parent
        .parent()
        .filter(|root| {
            parent.file_name() == Some(std::ffi::OsStr::new("src"))
                && exists(&root.join("Cargo.toml"))
        })
        .map_or(parent.clone(), Path::to_path_buf)
}

/// The package root of a runner snapshot's `entry`: the nearest directory
/// above it whose `Cargo.toml` the snapshot holds, as its parent's graph
/// read that package, else [`file_root`]'s.
fn snapshot_root(entry: &Path, snapshot: &super::snapshot::Snapshot) -> PathBuf {
    let text = snapshot.read(entry).unwrap_or_default();
    if !crate::project::frontmatter::is_script(text)
        && let Some(root) = entry
            .ancestors()
            .skip(1)
            .find(|directory| snapshot.read(&directory.join("Cargo.toml")).is_some())
    {
        return root.to_path_buf();
    }
    file_root(entry, text, |manifest| snapshot.read(manifest).is_some())
}

struct Builder<'a> {
    engine: &'a Engine,
    cells: Option<&'a super::Cells>,
    snapshot: Option<&'a super::snapshot::Snapshot>,
    /// The entry source's parse, when the caller already has it.
    entry: Option<crate::parse::Unit>,
    evidence: Vec<diagnostic::Source>,
    sources: Vec<source::Source>,
    units: Vec<Unit>,
    packages: Vec<Package>,
    module_targets: BTreeMap<PathBuf, error::Label>,
    active_modules: Vec<PathBuf>,
    expanded: BTreeMap<(source::Id, Span), ImmutableString>,
    #[cfg(feature = "rust-import")]
    stale: Vec<rawr_import::expand::Item>,
}

struct Request {
    root: PathBuf,
    source: PathBuf,
    /// The package, as Cargo describes it, when discovery asked.
    package: Option<project::cargo::Package>,
}

impl<'a> Builder<'a> {
    fn new(engine: &'a Engine) -> Self {
        Self {
            engine,
            cells: None,
            snapshot: None,
            entry: None,
            evidence: Vec::new(),
            sources: Vec::new(),
            units: Vec::new(),
            packages: Vec::new(),
            module_targets: BTreeMap::new(),
            active_modules: Vec::new(),
            expanded: BTreeMap::new(),
            #[cfg(feature = "rust-import")]
            stale: Vec::new(),
        }
    }

    fn from_snapshot(engine: &'a Engine, snapshot: &'a super::snapshot::Snapshot) -> Self {
        let mut builder = Self::new(engine);
        builder.snapshot = Some(snapshot);
        builder
    }

    fn load(mut self, target: &project::Target) -> Result<Graph, Error> {
        let request = match target {
            project::Target::File(source) => Request {
                root: file_root(source.path(), source.text(), |manifest| manifest.is_file()),
                source: source.path().to_path_buf(),
                package: None,
            },
            project::Target::Project(project) => Request {
                root: project.root().as_path().to_path_buf(),
                source: project.source().path().to_path_buf(),
                package: project.package().cloned(),
            },
        };
        let entry_package = self.load_package(request)?;
        let entry_source = self.packages[entry_package.0].source;
        Ok(Graph {
            packages: self.packages.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            units: self.units.into_boxed_slice(),
            entry_package,
            entry_source,
            expanded: self.expanded,
            #[cfg(feature = "rust-import")]
            stale: self.stale,
            hosted: false,
        })
    }

    fn load_snapshot(mut self) -> Result<Graph, Error> {
        let hosted = self.entry.is_some();
        let snapshot = self
            .snapshot
            .expect("snapshot builder retains its immutable input");
        let entry = snapshot.entry().to_path_buf();
        let request = Request {
            root: snapshot_root(&entry, snapshot),
            source: entry,
            package: None,
        };
        let entry_package = self.load_package(request)?;
        let entry_source = self.packages[entry_package.0].source;
        Ok(Graph {
            packages: self.packages.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            units: self.units.into_boxed_slice(),
            entry_package,
            entry_source,
            expanded: self.expanded,
            #[cfg(feature = "rust-import")]
            stale: self.stale,
            hosted,
        })
    }

    fn load_package(&mut self, request: Request) -> Result<Id, Error> {
        let root = self.canonical_directory(request.root)?;
        let canonical = self.canonical_source(request.source.clone(), &root, None)?;
        let text = self.read_to_string(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("read Rawr source", canonical.clone(), &source),
                source,
                None,
            )
        })?;
        // A script's manifest is its frontmatter, wherever it sits, and its
        // package lives in rawr's area for scripts (D608 point 6).
        let script = crate::project::frontmatter::is_script(&text);
        let cargo = if script {
            self.script_cargo(&canonical, &text)?
        } else {
            self.load_cargo(&root)?
        };
        let package_root = if script {
            manifest::script::directory(&canonical)
        } else {
            root.clone()
        };
        // The package and the target it runs, as Cargo describes them.
        let described = match (request.package, &cargo) {
            (Some(package), _) => Some(package),
            (None, Some((cargo, _))) => {
                // A script's package is its expanded manifest in rawr's
                // area, which Cargo reads when there are tables to decide.
                #[cfg(feature = "rust-import")]
                if script && self.snapshot.is_none() && !cargo.targets.is_empty() {
                    let _ = manifest::script::materialize(cargo);
                }
                self.described(&cargo.path)
            }
            (None, None) => None,
        };
        let target = described
            .as_ref()
            .and_then(|package| package.target_at(&canonical).cloned());
        let library = target.as_ref().map_or_else(
            || {
                request
                    .source
                    .file_name()
                    .is_some_and(|name| name == "lib.rs")
            },
            |target| target.kind() == project::cargo::Kind::Library,
        );
        let (cargo, unit) = match cargo {
            Some((cargo, evidence)) => {
                let (cargo, unit) =
                    self.on_host(cargo, &evidence, described.as_ref(), target.as_ref())?;
                (Some((cargo, evidence)), unit)
            }
            None => (None, None),
        };
        let panic = unit
            .as_ref()
            .map_or_else(project::cargo::Panic::default, project::cargo::Unit::panic);
        // A runner's snapshot asks no Cargo: its entry package has the
        // features the parent's Cargo enabled, which the snapshot carries.
        let features = match (unit, self.snapshot) {
            (Some(unit), _) => unit.features().clone(),
            (None, Some(snapshot)) if self.packages.is_empty() => snapshot.features().clone(),
            (None, _) => project::cargo::features::Enabled::default(),
        };
        // The crate name rustc gives this target, so item paths read as
        // Rust's do. A file outside a Cargo package is `main`.
        let name: ImmutableString = match (&target, &cargo) {
            (Some(target), _) => target.crate_name().into(),
            (None, Some((cargo, _))) if library => cargo.crate_name.as_str().into(),
            (None, Some((cargo, _))) => cargo.package.replace('-', "_").into(),
            (None, None) => "main".into(),
        };
        let package = Id::from_index(self.packages.len());
        self.packages.push(Package {
            id: package,
            name: name.clone(),
            root: package_root,
            source: source::Id::from_index(0),
            rust_modules: Vec::new(),
            native_root: None,
            bin: None,
            entry: None,
            cargo: None,
            rust: None,
            panic,
            features,
            library,
            target: target.clone(),
            script,
        });

        let module_root = request
            .source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        // A library beside a binary whose tree has no marked module runs
        // from that binary, as `cargo run` does: its `main` is the program's.
        let bin = if library && !script && cargo.is_some() {
            let path = match &described {
                Some(package) => package
                    .run_binary()
                    .map(|binary| binary.path().to_path_buf()),
                None => Some(module_root.join("main.rs")),
            };
            match path {
                Some(path) => self.bin_root(package, &root, &module_root, path)?,
                None => None,
            }
        } else {
            None
        };
        let has_bin = bin.is_some();
        self.packages[package.0].bin = bin.map(|root| Bin {
            root,
            modules: Vec::new(),
        });
        // An unmarked root that declares a marked module is native Rust
        // (D501): Rawr reads only its `mod` declarations, as the root of the
        // marked modules, and the native build compiles the rest.
        let marked = crate::project::is_marked(&text);
        let read = if !marked && self.reaches_marked(package, &module_root, &text).is_some() {
            let declarations =
                super::island::root_text(&text, has_bin, &self.packages[package.0].features);
            // A `use` Rawr can't parse names nothing marked: the native
            // build reads it.
            let uses = self
                .engine
                .parse_unit_with(
                    &super::polyglot::uses_text(&text),
                    &self.packages[package.0].features,
                )
                .map(|unit| unit.items().into())
                .unwrap_or_default();
            self.packages[package.0].native_root = Some(NativeRoot {
                module: RustModule {
                    modules: Box::default(),
                    path: canonical,
                    text,
                },
                marked: Box::default(),
                bridges: Vec::new(),
                uses,
            });
            declarations
        } else {
            self.packages[package.0].entry =
                super::entry::attribute(&text).map(String::into_boxed_str);
            text
        };
        let root_source = self.load_module(
            package,
            &name,
            &root,
            &module_root,
            Box::default(),
            request.source,
            None,
            Some(read),
        )?;
        self.packages[package.0].source = root_source;
        if let Some(root) = &self.packages[package.0].native_root {
            let marked = self.sources[root_source.index()..]
                .iter()
                .filter(|source| {
                    source.package() == name.as_str()
                        && !source.modules().is_empty()
                        && !root
                            .bridges
                            .iter()
                            .any(|bridge| **bridge == *source.modules())
                })
                .map(|source| Box::from(source.modules()))
                .collect();
            if let Some(root) = &mut self.packages[package.0].native_root {
                root.marked = marked;
            }
        }
        if !library && !script && cargo.is_some() {
            let library = match &described {
                Some(package) => package
                    .library()
                    .map(|library| library.path().to_path_buf()),
                None => Some(module_root.join("lib.rs")),
            };
            if let Some(library) = library {
                self.check_one_marked_tree(package, &root, &module_root, library)?;
            }
        }
        if has_bin && marked {
            return Err(self.marked_library_root(package));
        }
        if let Some(bin) = &self.packages[package.0].bin {
            let (path, text) = (bin.root.path.clone(), bin.root.text.clone());
            // A binary's modules are beside its root, as `src/bin/` holds
            // a binary's there.
            let bin_root = path.parent().unwrap_or(&module_root).to_path_buf();
            self.load_rust_children(
                package,
                Tree::Bin,
                &root,
                &bin_root,
                &[],
                &path,
                &text,
                Declared::All,
            )?;
        }
        if let Some(cargo) = cargo {
            // A binary reaches its package's library by the library's crate
            // name, as a dependency Cargo builds before it (#1164).
            let own_library = !library
                && !script
                && match &described {
                    Some(package) => package.library().is_some(),
                    None => cargo
                        .0
                        .library
                        .as_deref()
                        .is_some_and(|path| self.is_file(path)),
                };
            self.attach_cargo(package, cargo, own_library);
        }
        if self.packages[package.0].native_root.is_none() {
            self.check_native_direction(package)?;
        }
        Ok(package)
    }

    /// A binary whose tree is marked runs, so a marked module in the tree
    /// of the library beside it would not: an error that names both.
    fn check_one_marked_tree(
        &mut self,
        package: Id,
        root: &Path,
        module_root: &Path,
        library: PathBuf,
    ) -> Result<(), Error> {
        if !self.is_file(&library) {
            return Ok(());
        }
        let Ok(library) = self.canonical_source(library, root, None) else {
            return Ok(());
        };
        let Ok(text) = self.read_to_string(&library) else {
            return Ok(());
        };
        let marked = if crate::project::is_marked(&text) {
            Some(library.clone())
        } else {
            self.reaches_marked(package, module_root, &text)
        };
        let Some(marked) = marked else {
            return Ok(());
        };
        let binary = self.sources[self.packages[package.0].source.index()]
            .path()
            .to_path_buf();
        Err(Error::module(
            self.evidence.clone(),
            format!(
                "the binary `{}` has marked modules, and so does the library `{}`: `{}` starts with `// rawr`. \
                 Rawr runs the marked modules of one crate of a package, \
                 the binary's when its tree has one, else the library's. \
                 Remove `// rawr` from the marked files of one of the two trees.",
                binary.display(),
                library.display(),
                marked.display(),
            )
            .into(),
            self.start_label(),
            None,
        ))
    }

    /// A marked library root beside a native binary: the binary runs the
    /// program through a native library root only, whose Rawr text holds
    /// the `main` that starts it.
    fn marked_library_root(&self, package: Id) -> Error {
        let package = &self.packages[package.0];
        let library = self.sources[package.source.index()].path();
        let binary = package.bin.as_ref().map_or(library, |bin| bin.root.path());
        Error::module(
            self.evidence.clone(),
            format!(
                "`{}` starts with `// rawr`, and the package's binary `{}` is native. \
                 A native binary runs the marked modules of a native library root, \
                 not yet a marked library root. \
                 Remove `// rawr` from `{}` and mark the modules it declares.",
                library.display(),
                binary.display(),
                library.display(),
            )
            .into(),
            self.start_label(),
            None,
        )
    }

    /// The package's binary root at `path`, the binary `cargo run` runs,
    /// when it is native and its tree reaches no marked module: then the
    /// binary runs the program from the library whose root is in
    /// `module_root`.
    fn bin_root(
        &self,
        package: Id,
        root: &Path,
        module_root: &Path,
        path: PathBuf,
    ) -> Result<Option<RustModule>, Error> {
        if !self.is_file(&path) {
            return Ok(None);
        }
        let canonical = self.canonical_source(path, root, None)?;
        let text = self.read_to_string(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("read Rust module", canonical.clone(), &source),
                source,
                None,
            )
        })?;
        if crate::project::is_marked(&text)
            || self.reaches_marked(package, module_root, &text).is_some()
        {
            return Ok(None);
        }
        Ok(Some(RustModule {
            modules: Box::default(),
            path: canonical,
            text,
        }))
    }

    /// The first marked module the native crate root `text` reaches
    /// through its `mod` declarations and those of the native modules it
    /// declares, as project discovery decides, its `cfg`s read under the
    /// features and target options of `package`.
    fn reaches_marked(&self, package: Id, module_root: &Path, text: &str) -> Option<PathBuf> {
        self.reaches_marked_from(package, module_root, &[], text)
    }

    /// The first marked module the native module `modules`, whose text is
    /// `text`, reaches as [`Builder::reaches_marked`] has the root reach one.
    fn reaches_marked_from(
        &self,
        package: Id,
        module_root: &Path,
        modules: &[ImmutableString],
        text: &str,
    ) -> Option<PathBuf> {
        let mut pending = vec![(modules.to_vec(), text.to_owned())];
        let mut seen = std::collections::BTreeSet::new();
        while let Some((modules, text)) = pending.pop() {
            for declaration in
                super::polyglot::declarations(&text, &self.packages[package.0].features)
            {
                if declaration.explicit_path || declaration.disabled {
                    continue;
                }
                let mut child = modules.clone();
                child.extend(declaration.path.iter().map(|name| name.as_str().into()));
                let mut base = module_root.to_path_buf();
                for segment in &child {
                    base.push(segment.as_str());
                }
                let found = [base.with_extension("rs"), base.join("mod.rs")]
                    .into_iter()
                    .filter(|path| self.is_file(path))
                    .collect::<Vec<_>>();
                let [path] = found.as_slice() else {
                    continue;
                };
                if !seen.insert(path.clone()) {
                    continue;
                }
                let Ok(text) = self.read_to_string(path) else {
                    continue;
                };
                if crate::project::is_marked(&text) {
                    return Some(path.clone());
                }
                pending.push((child, text));
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn load_module(
        &mut self,
        package: Id,
        package_name: &str,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        path: PathBuf,
        declaration: Option<error::Label>,
        read: Option<String>,
    ) -> Result<source::Id, Error> {
        let canonical = self.canonical_source(path, package_root, declaration)?;
        if let Some(position) = self
            .active_modules
            .iter()
            .position(|path| path == &canonical)
        {
            let primary = declaration.unwrap_or_else(|| self.start_label());
            let secondary = self.module_targets.get(&canonical).copied();
            let chain = self.active_modules[position..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(Error::module(
                self.evidence.clone(),
                format!("file-module cycle: {chain} -> {}", canonical.display()).into(),
                primary,
                secondary,
            ));
        }
        if let Some(previous) = self.module_targets.get(&canonical).copied() {
            let primary = declaration.unwrap_or_else(|| self.start_label());
            return Err(Error::module(
                self.evidence.clone(),
                format!(
                    "more than one module declaration resolves to `{}`",
                    canonical.display()
                )
                .into(),
                primary,
                Some(previous),
            ));
        }

        let text = match read {
            Some(text) => text,
            None => self.read_to_string(&canonical).map_err(|source| {
                Error::resource(
                    self.evidence.clone(),
                    error::resource_message("read Rawr source", canonical.clone(), &source),
                    source,
                    declaration,
                )
            })?,
        };
        self.load_module_text(
            package,
            package_name,
            package_root,
            module_root,
            modules,
            canonical.clone(),
            canonical,
            text,
            declaration,
        )
    }

    /// One module's source `text`, read from `canonical` (`key` tells the
    /// module apart: an inline module shares its file's path), and the
    /// modules it declares.
    #[allow(clippy::too_many_arguments)]
    fn load_module_text(
        &mut self,
        package: Id,
        package_name: &str,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        canonical: PathBuf,
        key: PathBuf,
        text: String,
        declaration: Option<error::Label>,
    ) -> Result<source::Id, Error> {
        let id = source::Id::from_index(self.sources.len());
        let source = source::Source::new(
            id,
            canonical.clone(),
            package_name.into(),
            modules.clone(),
            text,
        )
        .map_err(Error::diagnostic)?;
        let evidence = self.push_evidence(source.diagnostic().clone());
        let features = &self.packages[package.0].features;
        let entry = self
            .entry
            .take()
            .filter(|_| *features == crate::project::cargo::features::Enabled::default());
        let parsed = match (entry, self.cells) {
            (Some(parsed), _) => parsed,
            (None, Some(cells)) => match cells.get(&canonical, source.text(), features) {
                Some(parsed) => parsed,
                None => {
                    let parsed = self
                        .engine
                        .parse_unit_with(source.text(), features)
                        .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))?;
                    cells.parsed(&canonical, source.text(), features, &parsed);
                    parsed
                }
            },
            (None, None) => self
                .engine
                .parse_unit_with(source.text(), features)
                .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))?,
        };
        let declarations = parsed
            .items()
            .iter()
            .filter_map(|item| match item {
                item::Parsed::Module(module) => Some(module.clone()),
                item::Parsed::Use(_)
                | item::Parsed::Struct(_)
                | item::Parsed::Enum(_)
                | item::Parsed::Impl(_)
                | item::Parsed::Trait(_)
                | item::Parsed::Function(_)
                | item::Parsed::TypeAlias(_) => None,
            })
            .collect::<Vec<_>>();
        let file_text = source.text().to_string();
        #[cfg(feature = "rust-import")]
        let derived = derived_items(&file_text, parsed.items());
        #[cfg(feature = "rust-import")]
        let item_source = source.diagnostic().clone();
        self.sources.push(source);
        self.units.push(Unit { source: id, parsed });
        #[cfg(feature = "rust-import")]
        for derived in derived {
            self.load_expansion(package_name, &modules, id, &item_source, derived)?;
        }
        self.active_modules.push(key.clone());
        self.module_targets.insert(
            key.clone(),
            declaration.unwrap_or(error::Label {
                source: evidence,
                span: Span::new(Position::START, Position::START),
            }),
        );

        for module in declarations {
            let mut child_modules = modules.to_vec();
            child_modules.push(module.name.text.clone());
            let label = error::Label {
                source: evidence,
                span: module.span,
            };
            // `mod name { .. }`: the text inside its braces, where it is in
            // the file, so its positions stay the file's.
            if let Some(inline) = module.inline {
                let child_key = key.join(format!("#{}", module.name.text));
                self.load_module_text(
                    package,
                    package_name,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    canonical.clone(),
                    child_key,
                    keep_span(&file_text, inline),
                    Some(label),
                )?;
                continue;
            }
            match self.module_file(module_root, &child_modules, label)? {
                ModuleFile::Missing(child_path) => {
                    self.load_module(
                        package,
                        package_name,
                        package_root,
                        module_root,
                        child_modules.into_boxed_slice(),
                        child_path,
                        Some(label),
                        None,
                    )?;
                }
                ModuleFile::Marked(child_path, text) => {
                    self.load_module(
                        package,
                        package_name,
                        package_root,
                        module_root,
                        child_modules.into_boxed_slice(),
                        child_path,
                        Some(label),
                        Some(text),
                    )?;
                }
                ModuleFile::Rust(child_path) => self.load_declared_rust_module(
                    package,
                    package_name,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    child_path,
                    label,
                )?,
            }
        }

        self.active_modules.pop();
        Ok(id)
    }

    /// The expansion kept for one crate derive of an item in the module
    /// `modules`, when the program has one: a source of its own, whose
    /// positions are reported at the derive (#593). Its items are in a
    /// module of their own below the item's, whose `use super::*;` sees
    /// every name the item's module binds, as the block of rustc's
    /// `const _: () = { .. }` does: two derives' `__Field` types are two
    /// items.
    #[cfg(feature = "rust-import")]
    fn load_expansion(
        &mut self,
        package_name: &str,
        modules: &[ImmutableString],
        item_source: source::Id,
        item_evidence: &diagnostic::Source,
        derived: Derived,
    ) -> Result<(), Error> {
        // Expansions are kept with the project: the entry package.
        let Some(project) = self.packages.first().map(|package| package.root.clone()) else {
            return Ok(());
        };
        let item_path = expansion_path(modules, &derived.item).to_string();
        let path = rawr_import::expand::source_path(&project, &item_path, &derived.written);
        if !self.is_file(&path) {
            return Ok(());
        }
        let Ok(text) = self.read_to_string(&path) else {
            return Ok(());
        };
        let resolved = match rawr_import::expand::kept(&text, &derived.text) {
            Some(rawr_import::expand::Kept::Current(resolved)) => ImmutableString::from(resolved),
            // The item changed since it was expanded: the derive is a
            // crate's, and its expansion is due again.
            Some(rawr_import::expand::Kept::Stale(resolved)) => {
                let derive = (derived.written, resolved.to_owned());
                match self.stale.iter_mut().find(|stale| stale.path == item_path) {
                    Some(stale) => stale.derives.push(derive),
                    None => self.stale.push(rawr_import::expand::Item {
                        path: item_path,
                        text: derived.text,
                        derives: vec![derive],
                    }),
                }
                return Ok(());
            }
            None => return Ok(()),
        };
        let name: ImmutableString = derived
            .written
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .into();
        let block = expansion_module(&derived.item, &name);
        let id = source::Id::from_index(self.sources.len());
        let source = source::Source::expansion(
            id,
            path,
            package_name.into(),
            modules.iter().cloned().chain([block]).collect(),
            text,
            diagnostic::source::Origin {
                source: item_evidence.clone(),
                span: derived.attribute,
                name,
            },
        )
        .map_err(Error::diagnostic)?;
        let evidence = self.push_evidence(source.diagnostic().clone());
        let parse = |text: &str| {
            self.engine
                .parse_unit(text)
                .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))
        };
        let parent = parse("use super::*;")?;
        let parsed = parse(source.text())?.with_items(parent.items());
        self.sources.push(source);
        self.units.push(Unit { source: id, parsed });
        self.expanded
            .insert((item_source, derived.attribute), resolved);
        Ok(())
    }

    /// In a crate whose root is marked, native code must not depend on
    /// marked code: a native module's `crate::…` and `super::…` paths must
    /// not land in a marked module (the crate root included). That native
    /// part is built without the marked modules, so such a path would not
    /// compile there. A native root's crate has trampolines for its marked
    /// modules instead (D501), and is not checked.
    fn check_native_direction(&mut self, package: Id) -> Result<(), Error> {
        let name = self.packages[package.0].name.clone();
        let marked = self
            .sources
            .iter()
            .filter(|source| source.package() == name.as_str())
            .map(|source| (source.modules().to_vec(), source.path().to_path_buf()))
            .collect::<Vec<_>>();
        let native = self.packages[package.0]
            .rust_modules
            .iter()
            .map(|module| module.modules.to_vec())
            .collect::<Vec<_>>();
        let mut violation = None;
        'modules: for module in &self.packages[package.0].rust_modules {
            for reference in
                super::polyglot::references(&module.text, &self.packages[package.0].features)
            {
                if reference.disabled {
                    continue;
                }
                let mut base = module
                    .modules
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                base.extend(reference.inline.iter().cloned());
                let supers = reference
                    .segments
                    .iter()
                    .take_while(|segment| *segment == "super")
                    .count();
                let absolute = if reference.segments[0] == "crate" {
                    reference.segments[1..].to_vec()
                } else {
                    let Some(kept) = base.len().checked_sub(supers) else {
                        continue;
                    };
                    let mut absolute = base[..kept].to_vec();
                    absolute.extend(reference.segments[supers..].iter().cloned());
                    absolute
                };
                // The innermost module the path lands in.
                let is = |modules: &[ImmutableString], length: usize| {
                    modules.len() == length
                        && modules
                            .iter()
                            .zip(&absolute)
                            .all(|(left, right)| left.as_str() == right)
                };
                for length in (0..=absolute.len()).rev() {
                    if native.iter().any(|modules| is(modules, length)) {
                        break;
                    }
                    if let Some((modules, path)) =
                        marked.iter().find(|(modules, _)| is(modules, length))
                    {
                        violation = Some((
                            module.modules.clone(),
                            module.path.clone(),
                            module.text.clone(),
                            reference,
                            modules.clone(),
                            path.clone(),
                        ));
                        break 'modules;
                    }
                }
            }
        }
        let Some((modules, path, text, reference, owner, owner_path)) = violation else {
            return Ok(());
        };
        let spelled = |modules: &[ImmutableString]| {
            std::iter::once("crate")
                .chain(modules.iter().map(ImmutableString::as_str))
                .collect::<Vec<_>>()
                .join("::")
        };
        let source = diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
            .map_err(Error::diagnostic)?;
        let index = self.push_evidence(source);
        let label = error::Label {
            source: index,
            span: manifest::byte_span(&text, reference.range.clone()),
        };
        Err(Error::module(
            self.evidence.clone(),
            format!(
                "native module `{}` cannot use `{used}`, which is in the marked module `{}` (`{}`): \
                 native modules cannot use marked code when the crate root is marked. \
                 Move the item into a native module, mark `{}` with `// rawr`, \
                 or leave the crate root unmarked so native code calls marked modules through rawr.",
                spelled(&modules),
                spelled(&owner),
                owner_path.display(),
                path.display(),
                used = &text[reference.range.clone()],
            )
            .into(),
            label,
            None,
        ))
    }

    /// Rust's file rules for `mod name;`: `name.rs` or `name/mod.rs`,
    /// exactly one of them. With none, `name.rs` is reported missing. A
    /// `.rs` file is Rawr when its first line is `// rawr`.
    fn module_file(
        &self,
        module_root: &Path,
        modules: &[ImmutableString],
        declaration: error::Label,
    ) -> Result<ModuleFile, Error> {
        let mut base = module_root.to_path_buf();
        for segment in modules {
            base.push(segment.as_str());
        }
        let found = [base.with_extension("rs"), base.join("mod.rs")]
            .into_iter()
            .filter(|path| self.is_file(path))
            .collect::<Vec<_>>();
        match found.as_slice() {
            [] => Ok(ModuleFile::Missing(base.with_extension("rs"))),
            [path] => Ok(match self.read_to_string(path) {
                Ok(text) if crate::project::is_marked(&text) => {
                    ModuleFile::Marked(path.clone(), text)
                }
                _ => ModuleFile::Rust(path.clone()),
            }),
            [first, second, ..] => Err(Error::module(
                self.evidence.clone(),
                format!(
                    "file for module `{}` found at both `{}` and `{}`; delete or rename one of them",
                    modules.join("::"),
                    first.display(),
                    second.display()
                )
                .into(),
                declaration,
                None,
            )),
        }
    }

    /// A native module that the crate root or a marked module declares. In
    /// a native root's crate, one whose tree reaches a marked module is a
    /// bridge (#1467): native, as any other, and a Rawr source of its own
    /// `mod` declarations alone, as the root is, so Rawr reaches the marked
    /// modules below it through the modules rustc does, and the paths of
    /// marked code into and out of them resolve as rustc resolves them.
    /// Its declarations inside inline modules are Rust's to follow.
    #[allow(clippy::too_many_arguments)]
    fn load_declared_rust_module(
        &mut self,
        package: Id,
        package_name: &str,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        path: PathBuf,
        declaration: error::Label,
    ) -> Result<(), Error> {
        let canonical = self.canonical_source(path, package_root, Some(declaration))?;
        let text = self.read_rust_module(&canonical, declaration)?;
        if self.packages[package.0].native_root.is_none()
            || self
                .reaches_marked_from(package, module_root, &modules, &text)
                .is_none()
        {
            return self.record_rust_module(
                package,
                Tree::Crate,
                package_root,
                module_root,
                modules,
                canonical,
                text,
                declaration,
                Declared::All,
            );
        }
        let rawr = super::island::module_text(&text, &self.packages[package.0].features);
        if let Some(root) = &mut self.packages[package.0].native_root {
            root.bridges.push(modules.clone());
        }
        self.load_module(
            package,
            package_name,
            package_root,
            module_root,
            modules.clone(),
            canonical.clone(),
            Some(declaration),
            Some(rawr),
        )?;
        self.load_rust_children(
            package,
            Tree::Crate,
            package_root,
            module_root,
            &modules,
            &canonical,
            &text,
            Declared::Inline,
        )?;
        self.packages[package.0].rust_modules.push(RustModule {
            modules,
            path: canonical,
            text,
        });
        Ok(())
    }

    /// The text of the native module at `canonical`, which `declaration`
    /// declares.
    fn read_rust_module(
        &self,
        canonical: &Path,
        declaration: error::Label,
    ) -> Result<String, Error> {
        self.read_to_string(canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("read Rust module", canonical.to_path_buf(), &source),
                source,
                Some(declaration),
            )
        })
    }

    /// Record one native module of `tree` and, under Rust's rules, the
    /// modules it declares. A binary's tree is a crate of its own, which
    /// may declare a file the library's tree declares too.
    #[allow(clippy::too_many_arguments)]
    fn load_rust_module(
        &mut self,
        package: Id,
        tree: Tree,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        path: PathBuf,
        declaration: error::Label,
    ) -> Result<(), Error> {
        let canonical = self.canonical_source(path, package_root, Some(declaration))?;
        let text = self.read_rust_module(&canonical, declaration)?;
        self.record_rust_module(
            package,
            tree,
            package_root,
            module_root,
            modules,
            canonical,
            text,
            declaration,
            Declared::All,
        )
    }

    /// Record the native module `modules` of `tree`, read from `canonical`
    /// as `text`, and the modules it declares that `declared` names.
    #[allow(clippy::too_many_arguments)]
    fn record_rust_module(
        &mut self,
        package: Id,
        tree: Tree,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        canonical: PathBuf,
        text: String,
        declaration: error::Label,
        declared: Declared,
    ) -> Result<(), Error> {
        if let Tree::Crate = tree
            && let Some(previous) = self.module_targets.get(&canonical).copied()
        {
            return Err(Error::module(
                self.evidence.clone(),
                format!(
                    "more than one module declaration resolves to `{}`",
                    canonical.display()
                )
                .into(),
                declaration,
                Some(previous),
            ));
        }
        if let Tree::Crate = tree {
            self.module_targets.insert(canonical.clone(), declaration);
        }
        self.load_rust_children(
            package,
            tree,
            package_root,
            module_root,
            &modules,
            &canonical,
            &text,
            declared,
        )?;
        let module = RustModule {
            modules,
            path: canonical,
            text,
        };
        let package = &mut self.packages[package.0];
        match (tree, package.bin.as_mut()) {
            (Tree::Bin, Some(bin)) => bin.modules.push(module),
            _ => package.rust_modules.push(module),
        }
        Ok(())
    }

    /// Record the native modules that the native module `modules` of
    /// `tree`, read from `canonical` as `text`, declares, those `declared`
    /// names. A marked one is an error at load: Rawr reaches a marked
    /// module through the crate root's, a marked module's or a bridge's own
    /// `mod` declarations ([`Builder::load_declared_rust_module`]).
    #[allow(clippy::too_many_arguments)]
    fn load_rust_children(
        &mut self,
        package: Id,
        tree: Tree,
        package_root: &Path,
        module_root: &Path,
        modules: &[ImmutableString],
        canonical: &Path,
        text: &str,
        declared: Declared,
    ) -> Result<(), Error> {
        let declarations = super::polyglot::declarations(text, &self.packages[package.0].features);
        let mut evidence = None;
        for child in declarations {
            // `#[path]` is Rust's choice; a `#[cfg]`-disabled one is
            // never loaded by Rust.
            if child.explicit_path
                || child.disabled
                || matches!(declared, Declared::Inline) && child.path.len() == 1
            {
                continue;
            }
            let mut child_modules = modules.to_vec();
            child_modules.extend(child.path.iter().map(|segment| segment.as_str().into()));
            let index = match evidence {
                Some(index) => index,
                None => {
                    let source = diagnostic::Source::new(
                        canonical.to_string_lossy().into_owned(),
                        text.to_owned(),
                    )
                    .map_err(Error::diagnostic)?;
                    let index = self.push_evidence(source);
                    evidence = Some(index);
                    index
                }
            };
            let label = error::Label {
                source: index,
                span: manifest::byte_span(text, child.range.clone()),
            };
            match self.module_file(module_root, &child_modules, label)? {
                ModuleFile::Rust(child_path) => self.load_rust_module(
                    package,
                    tree,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    child_path,
                    label,
                )?,
                ModuleFile::Marked(child_path, _) => {
                    return Err(Error::module(
                        self.evidence.clone(),
                        format!(
                            "native module `crate::{}` cannot declare the marked module `mod {};` (`{}`) yet. \
                             Declare it from the crate root or a marked module, or remove `// rawr` from its first line.",
                            modules.join("::"),
                            child.path.last().map_or("", String::as_str),
                            child_path.display()
                        )
                        .into(),
                        label,
                        None,
                    ));
                }
                // A missing file is Rust's own error to report when it builds.
                ModuleFile::Missing(_) => {}
            }
        }
        Ok(())
    }

    /// The package whose manifest is `manifest`, as Cargo describes it. A
    /// manifest Cargo can't read has none; its own errors are reported as
    /// Rawr reads it.
    fn described(&self, manifest: &Path) -> Option<project::cargo::Package> {
        project::cargo::Workspace::of(manifest)
            .ok()?
            .package(manifest)
            .cloned()
    }

    /// `cargo` as Cargo builds `target` of `package` on the host: its
    /// `[target.'<platform>']` dependencies Cargo links there joined, and
    /// Cargo's unit for it (its profile's panic strategy and its features),
    /// from Cargo's unit graph. A runner's snapshot does not ask Cargo
    /// ([`manifest::cargo::Host::Unasked`]), and a file that is no target
    /// of its package builds as the binary `cargo run` runs, else the
    /// library.
    fn on_host(
        &mut self,
        cargo: manifest::cargo::Cargo,
        evidence: &diagnostic::Source,
        package: Option<&project::cargo::Package>,
        target: Option<&project::cargo::Target>,
    ) -> Result<(manifest::cargo::Cargo, Option<project::cargo::Unit>), Error> {
        let built = package
            .filter(|_| self.snapshot.is_none())
            .and_then(|package| {
                target
                    .or_else(|| package.run_binary())
                    .or_else(|| package.library())
                    .map(|target| project::cargo::Unit::of(package, target))
            });
        match built {
            Some(Ok(unit)) => Ok((
                cargo.on_host(manifest::cargo::Host::Built(&unit)),
                Some(unit),
            )),
            Some(Err(error)) => {
                let evidence = self.push_evidence(evidence.clone());
                Err(Error::manifest(
                    self.evidence.clone(),
                    evidence,
                    error.to_string().into(),
                    Span::new(Position::START, Position::START),
                ))
            }
            None => Ok((cargo.on_host(manifest::cargo::Host::Unasked), None)),
        }
    }

    /// The crate's root `Cargo.toml`, when it is a package. Its
    /// `[dependencies]` are the Rust dependencies, and its native modules
    /// import under its crate name.
    fn load_cargo(
        &mut self,
        root: &Path,
    ) -> Result<Option<(manifest::cargo::Cargo, diagnostic::Source)>, Error> {
        let path = root.join("Cargo.toml");
        let text = match self.read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::resource(
                    self.evidence.clone(),
                    error::resource_message("read Cargo manifest", path, &source),
                    source,
                    None,
                ));
            }
        };
        let source = diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
            .map_err(Error::diagnostic)?;
        let ask_cargo = self.snapshot.is_none();
        match manifest::cargo::Cargo::parse(path, text, root, ask_cargo) {
            Ok(cargo) => Ok(cargo.map(|cargo| (cargo, source))),
            Err(error) => {
                let evidence = self.push_evidence(source);
                Err(Error::manifest(
                    self.evidence.clone(),
                    evidence,
                    error.message,
                    error.span,
                ))
            }
        }
    }

    /// A script's manifest, expanded from its frontmatter, with the
    /// expansion as its evidence. Its errors point into the script.
    fn script_cargo(
        &mut self,
        script: &Path,
        text: &str,
    ) -> Result<Option<(manifest::cargo::Cargo, diagnostic::Source)>, Error> {
        match manifest::script::cargo(script, text) {
            Ok(None) => Ok(None),
            Ok(Some(cargo)) => {
                let source = diagnostic::Source::new(
                    cargo.path.to_string_lossy().into_owned(),
                    cargo.text.clone(),
                )
                .map_err(Error::diagnostic)?;
                Ok(Some((cargo, source)))
            }
            Err(error) => {
                let source =
                    diagnostic::Source::new(script.to_string_lossy().into_owned(), text.to_owned())
                        .map_err(Error::diagnostic)?;
                let evidence = self.push_evidence(source);
                Err(Error::manifest(
                    self.evidence.clone(),
                    evidence,
                    error.message,
                    error.span,
                ))
            }
        }
    }

    /// Attach the crate's parsed `Cargo.toml` once its module tree is read,
    /// since its native modules decide whether the crate imports Rust. Its
    /// evidence follows the module sources'.
    fn attach_cargo(
        &mut self,
        package: Id,
        (cargo, source): (manifest::cargo::Cargo, diagnostic::Source),
        own_library: bool,
    ) {
        self.push_evidence(source);
        {
            let package = &mut self.packages[package.0];
            // A marked crate with any Cargo dependency imports Rust: its
            // runner builds each dependency as its own `cargo build` builds
            // it, never one built into an installed binary. Its
            // dependencies are those its default features build, and a
            // binary's include its package's library (`own_library`).
            let mut dependencies = cargo.enabled_dependencies().into_vec();
            if own_library && let Some(root) = cargo.path.parent() {
                dependencies.push(manifest::rust::Dependency::library(&cargo, root));
                dependencies.sort_unstable_by(|left, right| left.alias.cmp(&right.alias));
            }
            let dependencies = dependencies.into_boxed_slice();
            let imports = !package.rust_modules.is_empty()
                || package.native_root.is_some()
                || !dependencies.is_empty();
            if imports {
                package.rust = Some(manifest::rust::Config::from_cargo(
                    &cargo.path,
                    dependencies,
                ));
            }
        }
        self.packages[package.0].cargo = Some(cargo);
    }

    fn is_file(&self, path: &Path) -> bool {
        if let Some(snapshot) = self.snapshot {
            return snapshot.read(path).is_some();
        }
        path.is_file()
    }

    fn canonical_directory(&self, path: PathBuf) -> Result<PathBuf, Error> {
        let primary = None;
        if let Some(snapshot) = self.snapshot {
            let canonical = super::snapshot::normalize(path.clone()).map_err(|_| {
                self.input_error(
                    "open crate directory",
                    path.clone(),
                    io::ErrorKind::InvalidInput,
                )
            })?;
            if !snapshot.contains_directory(&canonical) {
                return Err(self.input_error(
                    "open crate directory",
                    canonical,
                    io::ErrorKind::NotFound,
                ));
            }
            return Ok(canonical);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("open crate directory", path.clone(), &source),
                source,
                primary,
            )
        })?;
        let metadata = fs::metadata(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("inspect crate directory", canonical.clone(), &source),
                source,
                primary,
            )
        })?;
        if !metadata.is_dir() {
            let source = std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "crate path is not a directory",
            );
            return Err(Error::resource(
                self.evidence.clone(),
                error::resource_message("open crate directory", canonical, &source),
                source,
                primary,
            ));
        }
        Ok(canonical)
    }

    fn canonical_source(
        &self,
        path: PathBuf,
        root: &Path,
        primary: Option<error::Label>,
    ) -> Result<PathBuf, Error> {
        if let Some(snapshot) = self.snapshot {
            let canonical = super::snapshot::normalize(path.clone()).map_err(|_| {
                self.input_error(
                    "open declared Rawr module",
                    path.clone(),
                    io::ErrorKind::InvalidInput,
                )
            })?;
            if !canonical.starts_with(root) {
                let source = io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("source resolves outside package root `{}`", root.display()),
                );
                return Err(Error::resource(
                    self.evidence.clone(),
                    error::resource_message("open declared Rawr module", canonical, &source),
                    source,
                    primary,
                ));
            }
            if snapshot.read(&canonical).is_none() {
                return Err(self.input_error(
                    "open declared Rawr module",
                    canonical,
                    io::ErrorKind::NotFound,
                ));
            }
            return Ok(canonical);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("open declared Rawr module", path.clone(), &source),
                source,
                primary,
            )
        })?;
        if !canonical.starts_with(root) {
            let source = std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("source resolves outside package root `{}`", root.display()),
            );
            return Err(Error::resource(
                self.evidence.clone(),
                error::resource_message("open declared Rawr module", canonical, &source),
                source,
                primary,
            ));
        }
        Ok(canonical)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if let Some(snapshot) = self.snapshot {
            return snapshot
                .read(path)
                .map(str::to_owned)
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound));
        }
        fs::read_to_string(path)
    }

    fn input_error(&self, action: &str, path: PathBuf, kind: io::ErrorKind) -> Error {
        let source = io::Error::new(kind, "path is absent from the immutable Rawr snapshot");
        Error::resource(
            self.evidence.clone(),
            error::resource_message(action, path, &source),
            source,
            None,
        )
    }

    fn push_evidence(&mut self, source: diagnostic::Source) -> usize {
        let index = self.evidence.len();
        self.evidence.push(source);
        index
    }

    fn start_label(&self) -> error::Label {
        error::Label {
            source: 0,
            span: Span::new(Position::START, Position::START),
        }
    }
}

/// One derive of a program item that a crate's macro may expand.
#[cfg(feature = "rust-import")]
struct Derived {
    /// The item as written, its attributes first.
    text: String,
    /// The item's name.
    item: ImmutableString,
    /// The derive as written, as `Serialize`.
    written: String,
    /// Where the derive is written.
    attribute: Span,
}

/// The module below an item's that the expansion of its derive `derive`
/// declares its items in, named so that no program item names it.
#[cfg(feature = "rust-import")]
fn expansion_module(item: &str, derive: &str) -> ImmutableString {
    format!("__rawr_derive_{item}_{derive}").into()
}

/// The path of the item `name` in the module `modules`, as its kept
/// expansion is found by (`app::Task`).
pub(crate) fn expansion_path(modules: &[ImmutableString], name: &str) -> ImmutableString {
    let mut path = String::new();
    for module in modules {
        path.push_str(module);
        path.push_str("::");
    }
    path.push_str(name);
    path.into()
}

/// Each derive of `items` (written in `text`) that isn't one of std's by
/// name: what a crate's macro may have expanded.
#[cfg(feature = "rust-import")]
fn derived_items(text: &str, items: &[item::Parsed]) -> Vec<Derived> {
    let mut derived = Vec::new();
    for parsed in items {
        let (header, span, name) = match parsed {
            item::Parsed::Struct(declared) => (&declared.header, declared.span, &declared.name),
            item::Parsed::Enum(declared) => (&declared.header, declared.span, &declared.name),
            _ => continue,
        };
        let mut arguments = header
            .derives()
            .filter(|path| {
                !matches!(path.segments.as_ref(), [only]
                    if !path.global && crate::capability::Kind::from_derive(&only.name.text).is_some())
            })
            .peekable();
        if arguments.peek().is_none() {
            continue;
        }
        let written = header.written_span(span).text(text);
        for argument in arguments {
            derived.push(Derived {
                text: written.to_owned(),
                item: name.text.clone(),
                written: item::written_path(argument),
                attribute: argument.span,
            });
        }
    }
    derived
}

/// `text` with everything outside `span` blanked, line breaks kept, so the
/// text inside keeps its lines and columns.
fn keep_span(text: &str, span: Span) -> String {
    let point = |position: Position| {
        (
            position.line().unwrap_or(0),
            position.position().unwrap_or(0),
        )
    };
    let (start, end) = (point(span.start()), point(span.end()));
    let (mut line, mut column) = (1usize, 1usize);
    let mut kept = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\n' {
            kept.push('\n');
            line += 1;
            column = 1;
            continue;
        }
        let inside = start <= (line, column) && (line, column) < end;
        if inside || character == '\r' {
            kept.push(character);
        } else {
            kept.push(' ');
        }
        column += 1;
    }
    kept
}
