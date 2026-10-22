//! Immutable in-memory package inputs received by a generated runner.

use std::path::{Component, Path, PathBuf};

/// One exact Rawr package snapshot with canonical path identity.
pub struct Snapshot {
    entry: PathBuf,
    files: Box<[(PathBuf, Box<str>)]>,
}

impl Snapshot {
    /// Validate and retain one bounded, canonically ordered snapshot.
    pub fn new(entry: PathBuf, mut files: Vec<(PathBuf, Box<str>)>) -> Result<Self, Error> {
        let entry = normalize(entry)?;
        for (path, _) in &mut files {
            *path = normalize(std::mem::take(path))?;
        }
        files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        if files.is_empty() {
            return Err(Error::Empty);
        }
        if files.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(Error::Duplicate);
        }
        if files
            .binary_search_by(|(path, _)| path.cmp(&entry))
            .is_err()
        {
            return Err(Error::MissingEntry);
        }
        Ok(Self {
            entry,
            files: files.into_boxed_slice(),
        })
    }

    /// Return the exact target path selected by the parent compiler.
    #[must_use]
    pub fn entry(&self) -> &Path {
        &self.entry
    }

    pub(crate) fn read(&self, path: &Path) -> Option<&str> {
        let path = normalize(path.to_path_buf()).ok()?;
        self.files
            .binary_search_by(|(candidate, _)| candidate.cmp(&path))
            .ok()
            .map(|index| self.files[index].1.as_ref())
    }

    pub(crate) fn contains_directory(&self, path: &Path) -> bool {
        let Ok(path) = normalize(path.to_path_buf()) else {
            return false;
        };
        self.files.iter().any(|(file, _)| {
            file.parent()
                .is_some_and(|parent| parent == path || parent.starts_with(&path))
        })
    }
}

pub(crate) fn normalize(path: PathBuf) -> Result<PathBuf, Error> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(Error::EscapesRoot);
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

/// Invalid runner-owned snapshot structure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No source or manifest bytes were supplied.
    #[error("Rawr package snapshot is empty")]
    Empty,
    /// More than one frame supplied the same normalized path.
    #[error("Rawr package snapshot contains a duplicate path")]
    Duplicate,
    /// The selected target path is absent from the retained file set.
    #[error("Rawr package snapshot does not contain its entry")]
    MissingEntry,
    /// Lexical normalization encountered a parent outside the captured root.
    #[error("Rawr package snapshot path escapes its captured root")]
    EscapesRoot,
}
