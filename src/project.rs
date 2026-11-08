//! Canonical Rawr source and project discovery.

pub mod cargo;
mod error;
pub mod frontmatter;
pub mod marker;
pub mod root;
pub mod source;

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

pub use error::{Error, Resource};
pub use marker::is_marked;

const SOURCE_DIRECTORY: &str = "src";
const CARGO_NAME: &str = "Cargo.toml";
/// The manifest of the old `.rawr` projects, which Rawr no longer reads.
const LEGACY_CONFIG: &str = "rawr.toml";
/// The entry of the old `.rawr` projects, which Rawr no longer reads.
const LEGACY_SOURCE: &str = "main.rawr";
const VCS_BOUNDARY: &str = ".git";

/// A Cargo package's binary or library whose crate root starts with
/// `// rawr`, or whose root is native Rust and declares, directly or
/// through native modules, a module that starts with it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    root: root::Directory,
    source: source::File,
    /// `None` when Cargo is not installed.
    package: Option<cargo::Package>,
}

/// A validated explicit source file or canonical Rawr project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// One explicitly selected `.rs` file that no package lists as a
    /// target; ancestor projects are ignored. An explicit `.rs` file runs
    /// whether or not it is marked.
    File(source::File),
    /// A package's target, discovered from a directory, an explicit
    /// `Cargo.toml`, or its crate root's `.rs` file.
    Project(Project),
}

impl Project {
    /// Return the canonical project root: the package's directory.
    #[must_use]
    pub const fn root(&self) -> &root::Directory {
        &self.root
    }

    /// Return the validated entry source: the crate root of the target
    /// that runs, the binary `cargo run` runs when its tree is marked,
    /// else the library.
    #[must_use]
    pub const fn source(&self) -> &source::File {
        &self.source
    }

    /// The package, as Cargo describes it: `None` when Cargo is not
    /// installed.
    #[must_use]
    pub const fn package(&self) -> Option<&cargo::Package> {
        self.package.as_ref()
    }

    fn load(
        root: PathBuf,
        source: PathBuf,
        package: Option<cargo::Package>,
    ) -> Result<Self, Error> {
        let root = root::Directory::from_canonical(root);
        let source = source::File::load_project(source, &root)?;
        Ok(Self {
            root,
            source,
            package,
        })
    }
}

/// The program of the Cargo package at `root`: the package's crate, or the
/// script its crate root is.
fn load_directory(root: PathBuf) -> Result<Target, Error> {
    match entry(&root)? {
        Entry::Found(source, package) => Project::load(root, source, package).map(Target::Project),
        Entry::Script(source) => source::File::load_explicit(source).map(Target::File),
        Entry::Unmarked(entry) => Err(Error::Unmarked { root, entry }),
        Entry::Ambiguous(binaries) => Err(Error::AmbiguousBinary { root, binaries }),
        Entry::None => Err(Error::MissingSource {
            expected: root.join(SOURCE_DIRECTORY).join("main.rs"),
            root,
        }),
    }
}

impl Target {
    /// Discover and validate an explicit source file, manifest, or directory.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let requested = path.as_ref().to_path_buf();
        let canonical = canonicalize_input(&requested)?;
        let metadata = fs::metadata(&canonical).map_err(|source| Error::Unreadable {
            resource: Resource::Input,
            path: canonical.clone(),
            source,
        })?;

        if metadata.is_file() {
            return discover_file(canonical);
        }
        if !metadata.is_dir() {
            return Err(Error::UnsupportedInput { path: canonical });
        }

        discover_directory(canonical)
    }

    /// The crate root of the binary target `name`, as `cargo run --bin
    /// <name>` finds it from `start`: of the package whose manifest is
    /// nearest above, or of any member of a workspace with no root package.
    pub fn binary(start: &Path, name: &str) -> Result<PathBuf, Error> {
        let start = canonicalize_input(start)?;
        let manifest = start
            .ancestors()
            .map(|directory| directory.join(CARGO_NAME))
            .find(|manifest| manifest.is_file())
            .ok_or_else(|| Error::NotFound {
                start: start.clone(),
                boundary: start.ancestors().last().unwrap_or(&start).to_path_buf(),
                legacy: None,
            })?;
        let workspace = cargo::Workspace::of(&manifest).map_err(|source| Error::Cargo {
            manifest: manifest.clone(),
            source,
        })?;
        let packages = match workspace.package(&manifest) {
            Some(package) => vec![package],
            None => workspace.packages().collect(),
        };
        if let Some(binary) = packages.iter().find_map(|package| package.binary(name)) {
            return Ok(binary.path().to_path_buf());
        }
        let mut binaries = packages
            .iter()
            .flat_map(|package| package.binaries())
            .map(|binary| Box::from(binary.name()))
            .collect::<Vec<_>>();
        binaries.sort_unstable();
        Err(Error::NoBinary {
            name: name.into(),
            binaries: binaries.into_boxed_slice(),
        })
    }

    /// Discover a project from the process current directory.
    pub fn current() -> Result<Self, Error> {
        let current = std::env::current_dir().map_err(Error::CurrentDirectory)?;
        Self::discover(current)
    }

    /// Return the validated source selected by this target.
    #[must_use]
    pub const fn source(&self) -> &source::File {
        match self {
            Self::File(source) => source,
            Self::Project(project) => project.source(),
        }
    }

    /// Return project metadata when discovery selected a project.
    #[must_use]
    pub const fn project(&self) -> Option<&Project> {
        match self {
            Self::File(_) => None,
            Self::Project(project) => Some(project),
        }
    }
}

fn discover_file(path: PathBuf) -> Result<Target, Error> {
    let name = path.file_name();
    if name == Some(OsStr::new(CARGO_NAME)) {
        let Some(parent) = path.parent() else {
            return Err(Error::UnsupportedInput { path });
        };
        return load_directory(parent.to_path_buf());
    }
    if name == Some(OsStr::new(LEGACY_CONFIG)) || path.extension() == Some(OsStr::new("rawr")) {
        return Err(Error::Legacy { path });
    }
    if path.extension() != Some(OsStr::new("rs")) {
        return Err(Error::UnsupportedInput { path });
    }
    let file = source::File::load_explicit(path)?;
    // A script is its own package wherever it sits (D608 point 6).
    if frontmatter::is_script(file.text()) {
        return Ok(Target::File(file));
    }
    Ok(match owner(file.path())? {
        Some(package) => Target::Project(Project {
            root: root::Directory::from_canonical(package.directory().to_path_buf()),
            source: file,
            package: Some(package),
        }),
        None => Target::File(file),
    })
}

/// The package that lists the file at `path` as a library or binary
/// target, as Cargo describes the workspace of the nearest manifest above
/// it, within the VCS boundary.
fn owner(path: &Path) -> Result<Option<cargo::Package>, Error> {
    let mut directory = path.parent();
    while let Some(current) = directory {
        let manifest = current.join(CARGO_NAME);
        if path_exists(&manifest, Resource::Manifest)? {
            return Ok(workspace(&manifest)?.and_then(|workspace| {
                workspace
                    .target_at(path)
                    .map(|(package, _)| package.clone())
            }));
        }
        if path_exists(&current.join(VCS_BOUNDARY), Resource::Boundary)? {
            break;
        }
        directory = current.parent();
    }
    Ok(None)
}

fn discover_directory(start: PathBuf) -> Result<Target, Error> {
    if !matches!(entry(&start)?, Entry::None) {
        return load_directory(start);
    }

    let mut candidates = Vec::new();
    let mut legacy = None;
    let mut current = start.clone();

    loop {
        if matches!(entry(&current)?, Entry::Found(..) | Entry::Script(_)) {
            candidates.push(current.clone());
        }
        if legacy.is_none() {
            legacy = [
                current.join(LEGACY_CONFIG),
                current.join(SOURCE_DIRECTORY).join(LEGACY_SOURCE),
            ]
            .into_iter()
            .find(|path| path.exists());
        }

        let at_boundary = path_exists(&current.join(VCS_BOUNDARY), Resource::Boundary)?;
        let Some(parent) = current.parent().map(Path::to_path_buf) else {
            return finish_search(start, current, candidates, legacy);
        };
        if at_boundary || parent == current {
            return finish_search(start, current, candidates, legacy);
        }
        current = parent;
    }
}

/// What a directory holds as a program's entry.
enum Entry {
    /// A Cargo package's marked crate root.
    Found(PathBuf, Option<cargo::Package>),
    /// A marked crate root with a frontmatter block: a cargo script, which
    /// is one wherever it sits (D608 point 6), so it is not the package's.
    Script(PathBuf),
    /// A Cargo package's crate root without the `// rawr` marker.
    Unmarked(PathBuf),
    /// A package with marked code and several binaries, none of them its
    /// `default-run`: these.
    Ambiguous(Box<[Box<str>]>),
    None,
}

/// The entry of the package at `root`, as Cargo lists its targets: the
/// first of the binary `cargo run` runs and the library whose first line
/// of Rust is the `// rawr` marker or whose `mod` tree reaches a file that
/// starts with it. So a package whose marked modules are all in its
/// library's tree runs from its library, and its binary is native. A root
/// that starts with the marker is read only to its first line, and one
/// that may start with a shebang or frontmatter is read whole. An unmarked
/// one is read whole, with the native modules it declares, until a marked
/// module turns up. With neither marked, the first that exists is the
/// unmarked entry. With several binaries and no `default-run`, any marked
/// target makes the package's entry ambiguous, as `cargo run` finds it.
fn entry(root: &Path) -> Result<Entry, Error> {
    let manifest = root.join(CARGO_NAME);
    if !path_exists(&manifest, Resource::Manifest)? {
        return Ok(Entry::None);
    }
    let Some(workspace) = workspace(&manifest)? else {
        // Without Cargo, Rawr knows only Cargo's default crate roots.
        let source = root.join(SOURCE_DIRECTORY);
        return marked_root([source.join("main.rs"), source.join("lib.rs")], None, None);
    };
    let Some(package) = workspace.package(&manifest) else {
        return Ok(Entry::None);
    };
    let paths = |targets: Vec<&cargo::Target>| {
        targets
            .into_iter()
            .map(|target| target.path().to_path_buf())
            .collect::<Vec<_>>()
    };
    if package.run_binary().is_none() && package.binaries().nth(1).is_some() {
        let binaries = package
            .binaries()
            .map(|binary| binary.name().into())
            .collect();
        let roots = paths(package.binaries().chain(package.library()).collect());
        return marked_root(roots, Some(binaries), None);
    }
    let roots = paths(
        package
            .run_binary()
            .into_iter()
            .chain(package.library())
            .collect(),
    );
    marked_root(roots, None, Some(package))
}

/// The entry the first of `roots` that is marked or reaches a marked
/// module makes, as [`entry`] picks it: `package`'s, or when the package
/// has several `binaries` and no `default-run`, an ambiguous one.
fn marked_root(
    roots: impl IntoIterator<Item = PathBuf>,
    binaries: Option<Box<[Box<str>]>>,
    package: Option<&cargo::Package>,
) -> Result<Entry, Error> {
    let mut unmarked = None;
    for path in roots {
        if !path_exists(&path, Resource::Source)? {
            continue;
        }
        let marking = marker::file_marking(&path).map_err(|source| Error::Unreadable {
            resource: Resource::Source,
            path: path.clone(),
            source,
        })?;
        if !marking.marked && !reaches_marked(&path)? {
            unmarked.get_or_insert(path);
            continue;
        }
        return Ok(match binaries {
            Some(binaries) => Entry::Ambiguous(binaries),
            None if marking.script => Entry::Script(path),
            None => Entry::Found(path, package.cloned()),
        });
    }
    Ok(unmarked.map_or(Entry::None, Entry::Unmarked))
}

/// Cargo's description of the workspace of `manifest`, or none when Cargo
/// is not installed. Then Rawr runs what needs no build, as it skips
/// `cargo check` (D459.5), and knows only Cargo's default crate roots.
fn workspace(manifest: &Path) -> Result<Option<std::sync::Arc<cargo::Workspace>>, Error> {
    match cargo::Workspace::of(manifest) {
        Ok(workspace) => Ok(Some(workspace)),
        Err(source) if source.is_not_installed() => Ok(None),
        Err(source) => Err(Error::Cargo {
            manifest: manifest.to_path_buf(),
            source,
        }),
    }
}

/// Whether the unmarked crate root `root` reaches a marked module through
/// the `mod` declarations of it and of the native modules it declares,
/// under Rust's file rules (`name.rs` or `name/mod.rs`). `#[path]` and
/// `#[cfg]`-disabled declarations are Rust's own and are not followed.
fn reaches_marked(root: &Path) -> Result<bool, Error> {
    let Some(directory) = root.parent() else {
        return Ok(false);
    };
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|source| Error::Unreadable {
            resource: Resource::Source,
            path: path.to_path_buf(),
            source,
        })
    };
    let mut pending = vec![(Vec::<String>::new(), read(root)?)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((modules, text)) = pending.pop() {
        for declaration in crate::package::polyglot::declarations(&text) {
            if declaration.explicit_path || declaration.disabled {
                continue;
            }
            let mut child = modules.clone();
            child.extend(declaration.path);
            let base = child
                .iter()
                .fold(directory.to_path_buf(), |path, name| path.join(name));
            let mut found = Vec::new();
            for path in [base.with_extension("rs"), base.join("mod.rs")] {
                if path_exists(&path, Resource::Source)? {
                    found.push(path);
                }
            }
            // None, or both: Rust reports it when it builds.
            let [path] = found.as_slice() else {
                continue;
            };
            if !seen.insert(path.clone()) {
                continue;
            }
            let text = read(path)?;
            if marker::is_marked(&text) {
                return Ok(true);
            }
            pending.push((child, text));
        }
    }
    Ok(false)
}

fn finish_search(
    start: PathBuf,
    boundary: PathBuf,
    candidates: Vec<PathBuf>,
    legacy: Option<PathBuf>,
) -> Result<Target, Error> {
    if candidates.is_empty() {
        return Err(Error::NotFound {
            start,
            boundary,
            legacy,
        });
    }
    if candidates.len() == 1 {
        return load_directory(candidates[0].clone());
    }
    Err(Error::Ambiguous {
        start,
        candidates: candidates.into_boxed_slice(),
    })
}

fn canonicalize_input(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            Error::InputNotFound {
                path: path.to_path_buf(),
                source,
            }
        } else {
            Error::Unreadable {
                resource: Resource::Input,
                path: path.to_path_buf(),
                source,
            }
        }
    })
}

fn path_exists(path: &Path, resource: Resource) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Unreadable {
            resource,
            path: path.to_path_buf(),
            source,
        }),
    }
}
