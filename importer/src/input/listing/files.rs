//! The listing itself: Git's view of the package when a repository tracks
//! its manifest and it declares no `include`, else a walk of its directory.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process;

use super::rules::{self, Filter, Mode};

/// Why a package's files could not be listed.
#[derive(Debug, thiserror::Error)]
pub(in crate::input) enum Error {
    #[error("{operation} `{}`: {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    /// A manifest whose `include` or `exclude` can't be read as Cargo reads it.
    #[error("`{}`: {reason}", path.display())]
    Manifest { path: PathBuf, reason: Box<str> },
}

impl Error {
    pub(super) fn manifest(path: &Path, reason: impl std::fmt::Display) -> Self {
        Self::Manifest {
            path: path.to_path_buf(),
            reason: reason.to_string().into_boxed_str(),
        }
    }
}

/// The files Cargo lists for the package at the canonical `root`, relative
/// to it and spelled through any link the listing followed, in order.
pub(in crate::input) fn files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let lists = rules::Lists::read(root)?;
    let mut listed = Vec::new();
    let repository = if lists.includes() {
        None
    } else {
        repository(root)?
    };
    match repository {
        Some((top, output)) => {
            let filter = lists.filter(root, Mode::Git)?;
            git(root, root, top, &output, &filter, &mut listed)?;
        }
        None => {
            let filter = lists.filter(root, Mode::Walk)?;
            walk(root, Start::Package, &filter, &mut Vec::new(), &mut listed)?;
        }
    }
    let relative: BTreeSet<PathBuf> = listed
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok())
        .map(Path::to_path_buf)
        .collect();
    Ok(relative.into_iter().collect())
}

/// Whether a Git listing's directory is its repository's top, where an
/// untracked `Cargo.lock` is left out.
#[derive(Clone, Copy)]
enum Top {
    Repository,
    Inside,
}

/// Whether a walk starts at the package root, whose own `target` is left
/// out and which is entered whatever the filter says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Start {
    Package,
    Directory,
}

/// Whether a Git repository tracks `root/Cargo.toml`, and if so where
/// `root` is in it and what Git lists under it. Git's variables naming a
/// repository are not passed on: the repository is the one found from the
/// directory, as Cargo finds it.
fn repository(root: &Path) -> Result<Option<(Top, Vec<u8>)>, Error> {
    let Some(prefix) = command(root, &["rev-parse", "--show-prefix"])? else {
        return Ok(None);
    };
    let Some(output) = listing(root)? else {
        return Ok(None);
    };
    if !records(&output).any(|(tracked, name)| tracked && name == b"Cargo.toml") {
        return Ok(None);
    }
    let top = if prefix.trim_ascii().is_empty() {
        Top::Repository
    } else {
        Top::Inside
    };
    Ok(Some((top, output)))
}

/// What Git tracks under `directory` and what it would track there,
/// untracked and not ignored, or `None` outside a repository.
fn listing(directory: &Path) -> Result<Option<Vec<u8>>, Error> {
    let arguments = [
        "ls-files",
        "-z",
        "-t",
        "--cached",
        "--others",
        "--exclude-standard",
    ];
    command(directory, &arguments)
}

/// Each path of a listing once, relative to its directory, and whether Git
/// tracks it. A nested repository is listed as a directory.
fn records(output: &[u8]) -> impl Iterator<Item = (bool, &[u8])> {
    let mut seen = BTreeSet::new();
    output.split(|&byte| byte == 0).filter_map(move |record| {
        let (tag, name) = record.split_first_chunk::<2>()?;
        let name = name.strip_suffix(b"/").unwrap_or(name);
        seen.insert(name).then_some((tag[0] != b'?', name))
    })
}

/// The files of a Git listing `output` of `directory`, listed into
/// `listed` as the package at `root` lists them.
fn git(
    root: &Path,
    directory: &Path,
    top: Top,
    output: &[u8],
    filter: &Filter,
    listed: &mut Vec<PathBuf>,
) -> Result<(), Error> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut nested: Vec<PathBuf> = Vec::new();
    for (tracked, name) in records(output) {
        let relative = Path::new(OsStr::from_bytes(name));
        // The package's own `target` keeps only what Git tracks there, and
        // an untracked lock at the repository's top is one Cargo writes.
        let in_target = relative.components().count() > 1 && relative.starts_with("target");
        let lock = matches!(top, Top::Repository) && relative == Path::new("Cargo.lock");
        if !tracked && (in_target || lock) {
            continue;
        }
        let path = directory.join(relative);
        let manifest = path.file_name() == Some(OsStr::new("Cargo.toml"));
        if let Some(parent) = path.parent().filter(|parent| manifest && *parent != root) {
            files.retain(|file| !file.starts_with(parent));
            nested.push(parent.to_path_buf());
            continue;
        }
        if nested.iter().any(|package| path.starts_with(package)) {
            continue;
        }
        // A tracked file missing from the disk is not listed, and a link
        // that names nothing has no bytes to read.
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if crate::input::walk::dangling(&error) => continue,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect listed package file",
                    path,
                    source,
                });
            }
        };
        if metadata.is_dir() {
            // A submodule, a nested repository or a link to a directory: a
            // repository lists its own files, anything else is walked.
            let repository = if path.join(".git").exists() {
                listing(&path)?
            } else {
                None
            };
            match repository {
                Some(output) => git(root, &path, Top::Repository, &output, filter, &mut files)?,
                None => walk(&path, Start::Directory, filter, &mut Vec::new(), &mut files)?,
            }
        } else if metadata.is_file() && filter.keeps(&path, false) {
            files.push(path);
        }
    }
    listed.append(&mut files);
    Ok(())
}

/// The files under `directory`, walked following links, listed into
/// `listed`. `inside` holds the canonical directories the walk is in, so a
/// link back to one of them is not entered again.
fn walk(
    directory: &Path,
    start: Start,
    filter: &Filter,
    inside: &mut Vec<PathBuf>,
    listed: &mut Vec<PathBuf>,
) -> Result<(), Error> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io {
            operation,
            path,
            source,
        }
    };
    if start == Start::Directory
        && (!filter.keeps(directory, true) || directory.join("Cargo.toml").exists())
    {
        return Ok(());
    }
    let canonical =
        std::fs::canonicalize(directory).map_err(io("resolve package directory", directory))?;
    if inside.contains(&canonical) {
        return Ok(());
    }
    inside.push(canonical);
    let mut names = std::fs::read_dir(directory)
        .map_err(io("read package directory", directory))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io("read package directory entry", directory))?;
    names.sort_unstable();
    for name in names {
        let path = directory.join(&name);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if crate::input::walk::dangling(&error) => continue,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect package file",
                    path,
                    source,
                });
            }
        };
        if metadata.is_dir() {
            let target = start == Start::Package && name == "target";
            if !target {
                walk(&path, Start::Directory, filter, inside, listed)?;
            }
        } else if metadata.is_file() && filter.keeps(&path, false) {
            listed.push(path);
        }
    }
    inside.pop();
    Ok(())
}

/// Git's output for `arguments` run in `directory`, or `None` when it
/// fails there, as it does outside a repository, or isn't installed: a
/// machine without Git has no repository to read.
fn command(directory: &Path, arguments: &[&str]) -> Result<Option<Vec<u8>>, Error> {
    let mut command = process::Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null());
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(variable);
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                operation: "run git in",
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    Ok(output.status.success().then_some(output.stdout))
}
