//! Validated Rawr project configuration files.

use std::fs;
use std::path::{Path, PathBuf};

use super::{root, Error, Resource};

/// A canonical readable UTF-8 `rawr.toml` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    path: PathBuf,
    text: String,
}

impl File {
    pub(super) fn load_project(path: PathBuf, root: &root::Directory) -> Result<Self, Error> {
        let canonical = fs::canonicalize(&path).map_err(|source| Error::Unreadable {
            resource: Resource::Config,
            path: path.clone(),
            source,
        })?;
        if !canonical.starts_with(root.as_path()) {
            return Err(Error::EscapesRoot {
                resource: Resource::Config,
                path,
                resolved: canonical,
                root: root.as_path().to_path_buf(),
            });
        }
        let text = fs::read_to_string(&canonical).map_err(|source| Error::Unreadable {
            resource: Resource::Config,
            path: canonical.clone(),
            source,
        })?;
        Ok(Self {
            path: canonical,
            text,
        })
    }

    /// Return the canonical configuration path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the validated UTF-8 configuration text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}
