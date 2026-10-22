//! Validated project roots.

use std::path::{Path, PathBuf};

/// A canonical directory proven to be a Rawr project root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Directory(PathBuf);

impl Directory {
    pub(super) const fn from_canonical(path: PathBuf) -> Self {
        Self(path)
    }

    /// Return the canonical filesystem path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
