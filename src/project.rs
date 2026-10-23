//! Canonical Rawr source and project discovery.

mod error;
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
/// The crate roots a Cargo package's program starts from, in order.
const RUST_ROOTS: [&str; 2] = ["main.rs", "lib.rs"];
const VCS_BOUNDARY: &str = ".git";

/// A Cargo crate whose root source starts with `// rawr`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    root: root::Directory,
    source: source::File,
}

/// A validated explicit source file or canonical Rawr project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// One explicitly selected `.rs` file; ancestor projects are ignored.
    /// An explicit `.rs` file runs whether or not it is marked.
    File(source::File),
    /// A marked crate discovered from a directory or an explicit `Cargo.toml`.
    Project(Project),
}

impl Project {
    /// Return the canonical project root.
    #[must_use]
    pub const fn root(&self) -> &root::Directory {
        &self.root
    }

    /// Return the validated entry source: the marked `src/main.rs` (else
    /// `src/lib.rs`) of the crate.
    #[must_use]
    pub const fn source(&self) -> &source::File {
        &self.source
    }

    fn load(path: PathBuf) -> Result<Self, Error> {
        let root = root::Directory::from_canonical(path);
        let source_path = match entry(root.as_path())? {
            Entry::Found(path) => path,
            Entry::Unmarked(entry) => {
                return Err(Error::Unmarked {
                    root: root.as_path().to_path_buf(),
                    entry,
                });
            }
            Entry::None => {
                return Err(Error::MissingSource {
                    root: root.as_path().to_path_buf(),
                    expected: root
                        .as_path()
                        .join(SOURCE_DIRECTORY)
                        .join(RUST_ROOTS[0]),
                });
            }
        };
        let source = source::File::load_project(source_path, &root)?;
        Ok(Self { root, source })
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
        return Project::load(parent.to_path_buf()).map(Target::Project);
    }
    if name == Some(OsStr::new(LEGACY_CONFIG)) || path.extension() == Some(OsStr::new("rawr")) {
        return Err(Error::Legacy { path });
    }
    if path.extension() != Some(OsStr::new("rs")) {
        return Err(Error::UnsupportedInput { path });
    }
    source::File::load_explicit(path).map(Target::File)
}

fn discover_directory(start: PathBuf) -> Result<Target, Error> {
    match entry(&start)? {
        Entry::Found(_) => return Project::load(start).map(Target::Project),
        Entry::Unmarked(entry) => return Err(Error::Unmarked { root: start, entry }),
        Entry::None => {}
    }

    let mut candidates = Vec::new();
    let mut legacy = None;
    let mut current = start.clone();

    loop {
        if matches!(entry(&current)?, Entry::Found(_)) {
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
    Found(PathBuf),
    /// A Cargo package's crate root without the `// rawr` marker.
    Unmarked(PathBuf),
    None,
}

/// The entry of the crate at `root`: the first of `src/main.rs` and
/// `src/lib.rs` that exists, when its first line is the `// rawr` marker.
/// Only that first line is read.
fn entry(root: &Path) -> Result<Entry, Error> {
    if !path_exists(&root.join(CARGO_NAME), Resource::Manifest)? {
        return Ok(Entry::None);
    }
    let source = root.join(SOURCE_DIRECTORY);
    for name in RUST_ROOTS {
        let path = source.join(name);
        if !path_exists(&path, Resource::Source)? {
            continue;
        }
        return match marker::file_is_marked(&path) {
            Ok(true) => Ok(Entry::Found(path)),
            Ok(false) => Ok(Entry::Unmarked(path)),
            Err(source) => Err(Error::Unreadable {
                resource: Resource::Source,
                path,
                source,
            }),
        };
    }
    Ok(Entry::None)
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
        return Project::load(candidates[0].clone()).map(Target::Project);
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
