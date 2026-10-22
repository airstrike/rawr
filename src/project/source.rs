//! Validated Rawr source files.

use std::fs;
use std::path::{Path, PathBuf};

use super::{root, Error, Resource};

/// A canonical readable UTF-8 Rawr source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    path: PathBuf,
    text: String,
}

impl File {
    pub(super) fn load_explicit(path: PathBuf) -> Result<Self, Error> {
        load(path, None)
    }

    pub(super) fn load_project(path: PathBuf, root: &root::Directory) -> Result<Self, Error> {
        load(path, Some(root))
    }

    /// Return the canonical source path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the validated UTF-8 source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

fn load(path: PathBuf, root: Option<&root::Directory>) -> Result<File, Error> {
    let canonical = fs::canonicalize(&path).map_err(|source| Error::Unreadable {
        resource: Resource::Source,
        path: path.clone(),
        source,
    })?;
    if let Some(root) = root.filter(|root| !canonical.starts_with(root.as_path())) {
        return Err(Error::EscapesRoot {
            resource: Resource::Source,
            path,
            resolved: canonical,
            root: root.as_path().to_path_buf(),
        });
    }
    let text = fs::read_to_string(&canonical).map_err(|source| Error::Unreadable {
        resource: Resource::Source,
        path: canonical.clone(),
        source,
    })?;
    Ok(File {
        path: canonical,
        text,
    })
}
