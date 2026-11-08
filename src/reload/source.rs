//! Canonical source/package evidence and exact live-reload inputs.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{diagnostic, package, ImmutableString};

/// Bounded filesystem observation transport; never a current membership proof.
#[cfg(all(feature = "rust-import", unix))]
pub mod observation;

/// One canonical package in the resolved source graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    name: ImmutableString,
    root: PathBuf,
    source: PathBuf,
    /// Native modules of a marked crate: Rust inputs, never Rawr sources.
    rust_modules: Box<[(PathBuf, String)]>,
    /// The root `Cargo.toml` of a package app: a Rust input.
    cargo: Option<(PathBuf, String)>,
    /// The features Cargo enables for the package's target.
    features: crate::project::cargo::features::Enabled,
}

/// One exact Rawr source snapshot in the resolved graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    path: PathBuf,
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
    evidence: diagnostic::Source,
}

/// The immutable canonical package/source graph used to compile a generation.
///
/// This graph is created from the same parsed package graph consumed to make
/// the generation's executable. It retains canonical filesystem identities and
/// exact source snapshots without retaining a second parsed program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Graph {
    packages: Box<[Package]>,
    sources: Box<[Source]>,
    entry_package: usize,
    entry_source: usize,
}

/// Canonical filesystem identities whose changes can affect one generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inputs {
    paths: Box<[PathBuf]>,
    directories: Box<[PathBuf]>,
    trees: Box<[PathBuf]>,
    sources: Box<[PathBuf]>,
    /// Directories the OS watches nothing under, and no watched tree holds:
    /// what builds write, and Cargo's registry and git checkouts.
    unwatched: Box<[PathBuf]>,
}

/// Exact source and manifest contents consumed by one compiled candidate.
///
/// A snapshot becomes final-admission evidence only after a publication stage
/// revalidates every file against the filesystem.
#[derive(Debug)]
pub struct Snapshot {
    inputs: Arc<Inputs>,
    files: Box<[File]>,
    #[cfg(feature = "rust-import")]
    entry: PathBuf,
    /// The entry package's features, which a runner's compile of the
    /// snapshot holds its `cfg(feature = "..")`s under.
    #[cfg(feature = "rust-import")]
    features: crate::project::cargo::features::Enabled,
}

/// Failure to revalidate one compiled source snapshot.
#[derive(Debug)]
pub enum Error {
    /// An exact input could not be read.
    Read {
        /// Canonical input path.
        path: PathBuf,
        /// Filesystem failure.
        source: io::Error,
    },
    /// An input no longer contains the bytes compiled into the candidate.
    Changed {
        /// Canonical input path.
        path: PathBuf,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct File {
    path: PathBuf,
    contents: Box<[u8]>,
}

impl Graph {
    /// Retain the canonical exact source graph from a parsed package graph.
    #[must_use]
    pub fn from_package(graph: &package::Graph) -> Self {
        let packages = graph
            .packages()
            .iter()
            .map(|package| Package {
                name: package.name().into(),
                root: package.root().to_path_buf(),
                source: graph.sources()[package.source().index()]
                    .path()
                    .to_path_buf(),
                // A native root is a Rust input too, with its whole text;
                // its Rawr source holds only its `mod` declarations. So is
                // a native binary that runs the program, with its tree.
                rust_modules: package
                    .native_root()
                    .into_iter()
                    .chain(package.rust_modules())
                    .chain(
                        package
                            .bin()
                            .into_iter()
                            .flat_map(|bin| std::iter::once(bin.root()).chain(bin.modules())),
                    )
                    .map(|module| (module.path().to_path_buf(), module.text().to_owned()))
                    .collect(),
                cargo: package
                    .cargo_manifest()
                    .map(|(path, text)| (path.to_path_buf(), text.to_owned())),
                features: package.features().clone(),
            })
            .collect();
        let sources = graph
            .sources()
            .iter()
            .map(|source| Source {
                path: source.path().to_path_buf(),
                package: source.package().into(),
                modules: source.modules().into(),
                evidence: source.diagnostic().clone(),
            })
            .collect();
        let entry_package = graph.entry_package().id().index();
        let entry_source = graph.entry_source().id().index();
        Self {
            packages,
            sources,
            entry_package,
            entry_source,
        }
    }

    /// Return packages in deterministic dependency-discovery order.
    #[must_use]
    pub const fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// Return sources in deterministic module-discovery order.
    #[must_use]
    pub const fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Return the application package.
    #[must_use]
    pub fn entry_package(&self) -> &Package {
        &self.packages[self.entry_package]
    }

    /// Return the application entry source.
    #[must_use]
    pub fn entry_source(&self) -> &Source {
        &self.sources[self.entry_source]
    }
}

impl Inputs {
    /// Derive the exact canonical source and manifest identities in a graph.
    #[must_use]
    pub fn from_graph(graph: &Graph) -> Self {
        let sources = graph
            .sources
            .iter()
            .map(|source| source.path.clone())
            .collect::<BTreeSet<_>>();
        let mut paths = sources.clone();
        let mut native = BTreeSet::new();
        // A `.rs` module edit takes the Rust-change path, never hot reload.
        for (path, _) in graph
            .packages
            .iter()
            .flat_map(|package| package.rust_modules.iter().chain(&package.cargo))
        {
            paths.insert(path.clone());
            native.insert(path.clone());
        }
        let mut inputs = Self::from_paths(paths, BTreeSet::new());
        // A native root is both: its edits are native.
        inputs.sources = sources.difference(&native).cloned().collect();
        inputs
    }

    /// Extend Rawr inputs with exact files and recursively observed native
    /// source roots retained by an imported generation proof.
    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    #[doc(hidden)]
    #[must_use]
    pub fn with_imported(
        self,
        imported: impl IntoIterator<Item = rawr_import::input::proof::Watch>,
    ) -> Self {
        self.with_native(&Self::imported(imported))
    }

    /// The exact files and recursive roots of an imported generation proof
    /// alone, so one native generation derives its directories once.
    ///
    /// A proof keeps each path as Cargo spelled it, which may hold `..`
    /// (a relative target directory, a `package.workspace` redirect). Each
    /// is watched where that spelling reaches, the one identity a change to
    /// it has ([`rawr_import::location::resolved`]).
    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    #[doc(hidden)]
    #[must_use]
    pub fn imported(imported: impl IntoIterator<Item = rawr_import::input::proof::Watch>) -> Self {
        use rawr_import::location::resolved;
        let mut paths = BTreeSet::new();
        let mut trees = BTreeSet::new();
        for input in imported {
            match input {
                rawr_import::input::proof::Watch::Exact(path) => paths.insert(resolved(&path)),
                rawr_import::input::proof::Watch::Tree(path) => trees.insert(resolved(&path)),
            };
        }
        Self::from_paths(paths, trees)
    }

    /// These inputs less what lies under `outputs`, the directories builds
    /// write ([`rawr_import::location::outputs_here`]): a change there is a
    /// build's own write, rawr's copies of the marked files among them,
    /// never a save. A tree that holds an output is watched as its entries
    /// beside the path down to it, so the watcher never covers the output.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn outside(self, outputs: &[PathBuf]) -> Self {
        let output = |path: &Path| outputs.iter().any(|output| path.starts_with(output));
        let mut paths = self
            .paths
            .into_vec()
            .into_iter()
            .filter(|path| !output(path))
            .collect::<BTreeSet<_>>();
        let mut trees = BTreeSet::new();
        let mut pending = self.trees.into_vec();
        while let Some(tree) = pending.pop() {
            if output(&tree) {
                continue;
            }
            if !outputs.iter().any(|output| output.starts_with(&tree)) {
                trees.insert(tree);
                continue;
            }
            // A tree that cannot be read is watched whole.
            let Ok(entries) = fs::read_dir(&tree) else {
                trees.insert(tree);
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    paths.insert(path);
                }
            }
        }
        let sources = self
            .sources
            .into_vec()
            .into_iter()
            .filter(|source| !output(source))
            .collect();
        Self {
            sources,
            unwatched: union(self.unwatched, &sorted(outputs)),
            ..Self::from_paths(paths, trees)
        }
    }

    /// These inputs with Cargo's registry and git `checkouts`
    /// ([`rawr_import::location::checkouts_here`]) left unwatched. Their
    /// sources never change while a program runs: a changed dependency is
    /// a new checkout, which a changed `Cargo.lock` names. The files under
    /// them stay inputs.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn with_checkouts(self, checkouts: &[PathBuf]) -> Self {
        Self {
            unwatched: union(self.unwatched, &sorted(checkouts)),
            ..self
        }
    }

    /// Extend Rawr inputs with [`Self::imported`] native inputs. Each parent
    /// derives its directory alone, so the union's directories are the union
    /// of both; a native identity takes the Rawr source role away.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn with_native(self, native: &Self) -> Self {
        let sources = self
            .sources
            .into_vec()
            .into_iter()
            .filter(|source| {
                native.paths.binary_search(source).is_err()
                    && !native.trees.iter().any(|tree| source.starts_with(tree))
            })
            .collect();
        Self {
            paths: union(self.paths, &native.paths),
            directories: union(self.directories, &native.directories),
            trees: union(self.trees, &native.trees),
            sources,
            unwatched: union(self.unwatched, &native.unwatched),
        }
    }

    /// Return all exact canonical input paths in stable order.
    #[must_use]
    pub const fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Return exact graph source identities with no manifest or native role.
    /// Transported identities are metadata, never current admission authority.
    #[must_use]
    pub const fn source_paths(&self) -> &[PathBuf] {
        &self.sources
    }

    /// Return whether one exact identity has only a Rawr source role.
    #[must_use]
    pub fn is_source_only(&self, path: &Path) -> bool {
        self.sources
            .binary_search_by(|source| source.as_path().cmp(path))
            .is_ok()
    }

    /// Return the nearest existing canonical ancestor directories required by
    /// a watcher.
    #[must_use]
    pub const fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    /// Return recursively observed native source roots in stable order.
    #[must_use]
    pub const fn trees(&self) -> &[PathBuf] {
        &self.trees
    }

    /// Return the directories the OS watches nothing under, in stable order.
    #[must_use]
    pub const fn unwatched(&self) -> &[PathBuf] {
        &self.unwatched
    }

    /// Return whether this exact input set contains one canonical path.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths
            .binary_search_by(|candidate| candidate.as_path().cmp(path))
            .is_ok()
            || self.trees.iter().any(|tree| path.starts_with(tree))
    }

    fn from_paths(paths: BTreeSet<PathBuf>, trees: BTreeSet<PathBuf>) -> Self {
        // Sibling files share one parent; canonicalize each parent once.
        let directories = paths
            .iter()
            .filter_map(|path| path.parent())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|parent| {
                parent.ancestors().find_map(|candidate| {
                    fs::canonicalize(candidate)
                        .ok()
                        .filter(|canonical| canonical == candidate && canonical.is_dir())
                })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            paths: paths.into_iter().collect(),
            directories,
            trees: trees.into_iter().collect(),
            sources: Box::default(),
            unwatched: Box::default(),
        }
    }
}

/// `paths` sorted and without duplicates.
#[cfg(feature = "rust-import")]
fn sorted(paths: &[PathBuf]) -> Box<[PathBuf]> {
    paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Merge two sorted, duplicate-free path lists.
#[cfg(feature = "rust-import")]
fn union(left: Box<[PathBuf]>, right: &[PathBuf]) -> Box<[PathBuf]> {
    let mut merged = Vec::with_capacity(left.len() + right.len());
    let mut right = right.iter().peekable();
    for path in left {
        while let Some(next) = right.next_if(|next| **next < path) {
            merged.push(next.clone());
        }
        right.next_if(|next| **next == path);
        merged.push(path);
    }
    merged.extend(right.cloned());
    merged.into_boxed_slice()
}

impl Snapshot {
    /// Capture the exact bytes already retained by one compiled source graph.
    #[must_use]
    pub fn from_graph(graph: &Graph) -> Self {
        // Native files first: a native root's whole text is kept over its
        // Rawr source, which holds only its `mod` declarations.
        let mut files = graph
            .packages
            .iter()
            .flat_map(|package| {
                package
                    .rust_modules
                    .iter()
                    .chain(&package.cargo)
                    .map(|(path, text)| File {
                        path: path.clone(),
                        contents: text.as_bytes().into(),
                    })
            })
            .collect::<Vec<_>>();
        files.extend(graph.sources.iter().map(|source| File {
            path: source.path.clone(),
            contents: source.text().as_bytes().into(),
        }));
        // The imported snapshot is keyed by path bytes, not path components:
        // `src/theme.rs` sorts after `src/theme/button.rs`.
        files.sort_by(|left, right| left.path.as_os_str().cmp(right.path.as_os_str()));
        // An inline `mod name { .. }` is a source of its own with its file's
        // path and only its braces' text. Its file comes first in source
        // order, and the stable sort keeps it first, so the file's whole text
        // is the one kept.
        files.dedup_by(|later, first| later.path == first.path);
        Self {
            inputs: Arc::new(Inputs::from_graph(graph)),
            files: files.into_boxed_slice(),
            #[cfg(feature = "rust-import")]
            entry: graph.entry_source().path().to_path_buf(),
            #[cfg(feature = "rust-import")]
            features: graph.entry_package().features.clone(),
        }
    }

    /// Borrow the exact canonical identities covered by this snapshot.
    #[must_use]
    pub fn inputs(&self) -> &Inputs {
        &self.inputs
    }

    pub(super) const fn shared_inputs(&self) -> &Arc<Inputs> {
        &self.inputs
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn entry(&self) -> &Path {
        &self.entry
    }

    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn imported(
        &self,
        limits: &rawr_import::limits::Limits,
    ) -> Result<rawr_import::surface::Snapshot, rawr_import::surface::Error> {
        let mut builder = rawr_import::surface::Snapshot::builder(limits);
        builder.entry(
            self.entry
                .to_str()
                .ok_or(rawr_import::surface::Error::NonUtf8Path)?,
        )?;
        let cfg = self.features.lines().collect::<Vec<_>>();
        builder.features(cfg.iter().map(String::as_str))?;
        for file in &self.files {
            builder.file(
                file.path
                    .to_str()
                    .ok_or(rawr_import::surface::Error::NonUtf8Path)?,
                &file.contents,
            )?;
        }
        builder.seal()
    }

    /// Encode the exact retained Rawr files for one generated runner handoff.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn imported_transport(
        &self,
        expected: rawr_import::surface::Snapshot,
        limits: &rawr_import::limits::Limits,
    ) -> Result<rawr_import::supervisor::snapshot::Bytes, rawr_import::supervisor::snapshot::Error>
    {
        let mut capture = rawr_import::supervisor::snapshot::Bytes::capture(limits);
        capture.entry(
            self.entry
                .to_str()
                .ok_or(rawr_import::supervisor::snapshot::Error::Malformed)?,
        )?;
        let cfg = self.features.lines().collect::<Vec<_>>();
        capture.features(cfg.iter().map(String::as_str))?;
        for file in &self.files {
            capture.file(
                file.path
                    .to_str()
                    .ok_or(rawr_import::supervisor::snapshot::Error::Malformed)?,
                &file.contents,
            )?;
        }
        capture.bind(expected)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn exactly_matches(&self, other: &Self) -> bool {
        self.entry == other.entry && self.files == other.files && self.features == other.features
    }

    /// Re-read every exact source and manifest and reject any content drift.
    ///
    /// This authority-free check is also used by initial native bootstrap
    /// after its watcher has registered the compiled graph.
    pub fn revalidate(&self) -> Result<(), Error> {
        for file in &self.files {
            let contents = fs::read(&file.path).map_err(|source| Error::Read {
                path: file.path.clone(),
                source,
            })?;
            if contents.as_slice() != file.contents.as_ref() {
                return Err(Error::Changed {
                    path: file.path.clone(),
                });
            }
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                formatter,
                "cannot revalidate reload input `{}`: {source}",
                path.display()
            ),
            Self::Changed { path } => write!(
                formatter,
                "reload input `{}` changed after compilation",
                path.display()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Changed { .. } => None,
        }
    }
}

impl Package {
    /// Return the stable manifest-declared package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the canonical package root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the canonical package root source path.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }
}

impl Source {
    /// Return the canonical Rawr source path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the stable package containing this source.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Return the lexical module path from the package root.
    #[must_use]
    pub const fn modules(&self) -> &[ImmutableString] {
        &self.modules
    }

    /// Return the exact source text compiled into this generation.
    #[must_use]
    pub fn text(&self) -> &str {
        self.evidence.text()
    }

    /// Return the exact diagnostic source evidence compiled into this generation.
    #[must_use]
    pub const fn diagnostic(&self) -> &diagnostic::Source {
        &self.evidence
    }
}

#[cfg(all(test, feature = "rust-import", unix))]
mod tests {
    use super::{File, Inputs, Snapshot};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn graph_source_roles_exclude_manifests_and_native_overlaps() {
        use rawr_import::input::proof::Watch;
        let root = std::env::temp_dir().join(format!(
            "rawr-source-roles-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"roles\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "// rawr\npub fn main() {}\n").unwrap();
        let target = crate::project::Target::discover(&root).unwrap();
        let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
        let graph = super::Graph::from_package(&graph);
        let inputs = Inputs::from_graph(&graph);
        let source = graph.entry_source().path();
        let manifest = graph.entry_package().root().join("Cargo.toml");
        assert!(inputs.contains(&manifest));
        assert_eq!(inputs.source_paths(), &[source.to_path_buf()]);
        assert!(!inputs.is_source_only(&manifest));
        let exact = inputs
            .clone()
            .with_imported([Watch::Exact(source.to_path_buf())]);
        assert!(exact.contains(source));
        assert!(!exact.is_source_only(source));
        assert!(!exact.with_imported([]).is_source_only(source));
        let tree = inputs.with_imported([Watch::Tree(source.parent().unwrap().to_path_buf())]);
        assert!(tree.contains(source));
        assert!(!tree.is_source_only(source));
    }

    /// #959: a crate in a git worktree inside another workspace, and an
    /// ordinary crate, each with a relative and an absolute target directory.
    /// Cargo spells the shared harness and the ancestor manifests through
    /// `..`; the watcher gets the paths they reach and its encoder takes them.
    #[test]
    fn spelled_native_inputs_encode_where_they_reach_from_every_crate_location() {
        use rawr_import::input::proof::Watch;
        use std::path::Path;
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "rawr-source-spelled-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let write = |path: PathBuf, text: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        let workspace = "[workspace]\nmembers = [\"member\"]\n";
        write(root.join("outer/Cargo.toml"), workspace);
        write(
            root.join("outer/.claude/worktrees/wt/Cargo.toml"),
            workspace,
        );
        let layouts = [
            root.join("outer/.claude/worktrees/wt/target/scratch/iced-examples/app"),
            root.join("examples/app"),
        ];
        for project in layouts {
            write(
                project.join("Cargo.toml"),
                "[package]\nname = \"app\"\n[workspace]\n",
            );
            write(project.join("src/main.rs"), "// rawr\npub fn main() {}\n");
            let target = crate::project::Target::discover(&project).unwrap();
            let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
            let graph = super::Graph::from_package(&graph);
            let source = graph.entry_source().path().to_path_buf();
            let shared = project.parent().unwrap().join("_target/rawr/shared");
            let observed = |target: &Path| {
                let mut watches = vec![
                    Watch::Exact(target.join("rawr/shared/Cargo.toml")),
                    Watch::Exact(target.join("rawr/shared/Cargo.lock")),
                    Watch::Tree(target.join("rawr/runner")),
                ];
                // The workspace scan's ancestor manifests, as spelled.
                watches.extend(
                    project
                        .ancestors()
                        .skip(1)
                        .map(|directory| Watch::Exact(directory.join("./Cargo.toml"))),
                );
                Inputs::from_graph(&graph).with_imported(watches)
            };
            let relative = observed(&project.join("../_target"));
            let absolute = observed(&project.parent().unwrap().join("_target"));
            assert_eq!(relative, absolute, "{}", project.display());
            let decoded = super::observation::decode(&relative.encode_observation().unwrap())
                .unwrap()
                .into_inputs();
            assert_eq!(decoded, relative);
            assert!(relative.contains(&shared.join("Cargo.toml")));
            assert!(relative.contains(&shared.parent().unwrap().join("runner/build.rs")));
            assert!(project
                .ancestors()
                .skip(1)
                .all(|directory| relative.contains(&directory.join("Cargo.toml"))));
            assert_eq!(relative.source_paths(), &[source]);
        }
    }

    #[test]
    fn native_inputs_derived_once_merge_as_one_derivation_of_the_union() {
        use rawr_import::input::proof::Watch;
        use std::collections::BTreeSet;
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("rawr-source-native-{}", std::process::id()));
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("crate/src")).unwrap();
        let rawr = [
            root.join("src/main.rs"),
            root.join("src/nested/a.rs"),
            root.join("Cargo.toml"),
        ];
        let native = [
            Watch::Exact(root.join("crate/src/lib.rs")),
            Watch::Exact(root.join("crate/Cargo.toml")),
            Watch::Exact(root.join("src/main.rs")),
            Watch::Exact(root.join("absent/build.rs")),
            Watch::Tree(root.join("src/nested")),
        ];
        let mut own = Inputs::from_paths(rawr.iter().cloned().collect(), BTreeSet::new());
        own.sources = rawr[..2].to_vec().into_boxed_slice();
        let merged = own.with_native(&Inputs::imported(native.clone()));
        let paths = rawr
            .iter()
            .cloned()
            .chain(native.iter().filter_map(|watch| match watch {
                Watch::Exact(path) => Some(path.clone()),
                Watch::Tree(_) => None,
            }))
            .collect();
        let whole = Inputs::from_paths(paths, [root.join("src/nested")].into());
        assert_eq!(merged.paths(), whole.paths());
        assert_eq!(merged.directories(), whole.directories());
        assert_eq!(merged.trees(), whole.trees());
        assert!(merged.source_paths().is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn imported_snapshot_accepts_a_module_file_beside_its_directory() {
        let root = std::env::temp_dir().join(format!(
            "rawr-nested-modules-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src/theme")).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"nested\"\n").unwrap();
        fs::write(
            root.join("src/main.rs"),
            "// rawr\nmod theme;\npub fn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("src/theme.rs"), "// rawr\npub mod button;\n").unwrap();
        fs::write(
            root.join("src/theme/button.rs"),
            "// rawr\npub fn edge() -> i64 { 1 }\n",
        )
        .unwrap();
        let target = crate::project::Target::discover(&root).unwrap();
        let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
        let snapshot = Snapshot::from_graph(&super::Graph::from_package(&graph));
        let limits = rawr_import::limits::Limits::standard();
        snapshot.imported(&limits).unwrap();
    }

    #[test]
    fn imported_snapshot_keeps_one_whole_file_for_an_inline_module() {
        let root = std::env::temp_dir().join(format!(
            "rawr-inline-module-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"inline\"\n").unwrap();
        let main = "// rawr\nmod toast {\n    pub fn edge() -> i64 { 1 }\n}\npub fn main() {}\n";
        fs::write(root.join("src/main.rs"), main).unwrap();
        let target = crate::project::Target::discover(&root).unwrap();
        let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
        let snapshot = Snapshot::from_graph(&super::Graph::from_package(&graph));
        let limits = rawr_import::limits::Limits::standard();
        snapshot.imported(&limits).unwrap();
        let mains = snapshot
            .files
            .iter()
            .filter(|file| file.path.ends_with("src/main.rs"))
            .collect::<Vec<_>>();
        assert_eq!(mains.len(), 1);
        assert_eq!(mains[0].contents.as_ref(), main.as_bytes());
        snapshot.revalidate().unwrap();
    }

    #[test]
    fn imported_snapshot_rejects_non_utf8_entry_and_file_paths() {
        let limits = rawr_import::limits::Limits::standard();
        let invalid = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
        let snapshot = |entry, files| Snapshot {
            inputs: Arc::new(Inputs {
                paths: Box::default(),
                directories: Box::default(),
                trees: Box::default(),
                sources: Box::default(),
                unwatched: Box::default(),
            }),
            files,
            entry,
            features: crate::project::cargo::features::Enabled::default(),
        };

        assert!(matches!(
            snapshot(invalid.clone(), Box::default()).imported(&limits),
            Err(rawr_import::surface::Error::NonUtf8Path)
        ));
        assert!(matches!(
            snapshot(
                "/entry.rs".into(),
                Box::from([File {
                    path: invalid,
                    contents: Box::default(),
                }]),
            )
            .imported(&limits),
            Err(rawr_import::surface::Error::NonUtf8Path)
        ));
    }

    #[test]
    fn missing_imported_path_uses_its_nearest_existing_transport_directory() {
        let root = std::env::temp_dir().join(format!(
            "rawr-imported-watch-root-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).expect("watch root can be created");
        let root = fs::canonicalize(root).expect("watch root canonicalizes");
        let missing = root.join("absent/config.toml");
        let inputs = Inputs {
            paths: Box::default(),
            directories: Box::default(),
            trees: Box::default(),
            sources: Box::default(),
            unwatched: Box::default(),
        }
        .with_imported([rawr_import::input::proof::Watch::Exact(missing.clone())]);

        assert_eq!(inputs.paths(), &[missing]);
        assert_eq!(inputs.directories(), std::slice::from_ref(&root));
        assert!(!root.join("absent").exists());
        fs::remove_dir(root).expect("watch root can be removed");
    }
}
