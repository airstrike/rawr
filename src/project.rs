//! Canonical Rawr source and project discovery.

pub mod config;
mod error;
pub mod root;
pub mod source;

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

pub use error::{Error, Resource};

const CONFIG_NAME: &str = "rawr.toml";
const SOURCE_DIRECTORY: &str = "src";
const SOURCE_NAME: &str = "main.rawr";
const VCS_BOUNDARY: &str = ".git";

/// A canonical Rawr project with readable source and optional configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    root: root::Directory,
    source: source::File,
    config: Option<config::File>,
}

/// A validated explicit source file or canonical Rawr project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// One explicitly selected `.rawr` file; ancestor projects are ignored.
    File(source::File),
    /// A project discovered from a directory or explicit `rawr.toml`.
    Project(Project),
}

impl Project {
    /// Return the canonical project root.
    #[must_use]
    pub const fn root(&self) -> &root::Directory {
        &self.root
    }

    /// Return the validated canonical `src/main.rawr` source.
    #[must_use]
    pub const fn source(&self) -> &source::File {
        &self.source
    }

    /// Return the validated optional `rawr.toml` configuration.
    #[must_use]
    pub const fn config(&self) -> Option<&config::File> {
        self.config.as_ref()
    }

    fn load(path: PathBuf, require_config: bool) -> Result<Self, Error> {
        let root = root::Directory::from_canonical(path);
        let source_path = root.as_path().join(SOURCE_DIRECTORY).join(SOURCE_NAME);

        if !path_exists(&source_path, Resource::Source)? {
            return Err(Error::MissingSource {
                root: root.as_path().to_path_buf(),
                expected: source_path,
                config: require_config.then(|| root.as_path().join(CONFIG_NAME)),
            });
        }

        let source = source::File::load_project(source_path, &root)?;
        let config_path = root.as_path().join(CONFIG_NAME);
        let config = if path_exists(&config_path, Resource::Config)? {
            Some(config::File::load_project(config_path, &root)?)
        } else if require_config {
            return Err(Error::MissingConfig {
                root: root.as_path().to_path_buf(),
                expected: config_path,
            });
        } else {
            None
        };

        Ok(Self {
            root,
            source,
            config,
        })
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
    if path.file_name() == Some(OsStr::new(CONFIG_NAME)) {
        let Some(parent) = path.parent() else {
            return Err(Error::UnsupportedInput { path });
        };
        return Project::load(parent.to_path_buf(), true).map(Target::Project);
    }

    if path.extension() != Some(OsStr::new("rawr")) {
        return Err(Error::UnsupportedInput { path });
    }

    source::File::load_explicit(path).map(Target::File)
}

fn discover_directory(start: PathBuf) -> Result<Target, Error> {
    if let Some(project) = load_exact_candidate(&start)? {
        return Ok(Target::Project(project));
    }

    let mut candidates = Vec::new();
    let mut current = start.clone();

    loop {
        let config_path = current.join(CONFIG_NAME);
        if path_exists(&config_path, Resource::Config)? {
            if candidates.is_empty() {
                return Project::load(current, true).map(Target::Project);
            }
            candidates.push(current.clone());
            return Err(Error::Ambiguous {
                start,
                candidates: candidates.into_boxed_slice(),
            });
        }

        if path_exists(
            &current.join(SOURCE_DIRECTORY).join(SOURCE_NAME),
            Resource::Source,
        )? {
            candidates.push(current.clone());
        }

        let at_boundary = path_exists(&current.join(VCS_BOUNDARY), Resource::Boundary)?;
        let Some(parent) = current.parent().map(Path::to_path_buf) else {
            return finish_search(start, current, candidates);
        };
        if at_boundary || parent == current {
            return finish_search(start, current, candidates);
        }
        current = parent;
    }
}

fn load_exact_candidate(path: &Path) -> Result<Option<Project>, Error> {
    if path_exists(&path.join(CONFIG_NAME), Resource::Config)? {
        return Project::load(path.to_path_buf(), true).map(Some);
    }
    if path_exists(
        &path.join(SOURCE_DIRECTORY).join(SOURCE_NAME),
        Resource::Source,
    )? {
        return Project::load(path.to_path_buf(), false).map(Some);
    }
    Ok(None)
}

fn finish_search(
    start: PathBuf,
    boundary: PathBuf,
    candidates: Vec<PathBuf>,
) -> Result<Target, Error> {
    if candidates.is_empty() {
        return Err(Error::NotFound { start, boundary });
    }
    if candidates.len() == 1 {
        return Project::load(candidates[0].clone(), false).map(Target::Project);
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
