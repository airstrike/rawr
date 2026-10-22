// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Borrowed configuration provenance, not an effective selected value.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use rawr_toml::document;

use super::super::super::{session::Session, Error, Step};
use super::super::path;

/// The eventual configuration loader supplies these historical definitions.
/// Borrowing their source owner does not grant current membership authority.
#[derive(Clone, Copy, Debug)]
pub(super) enum Definition<'a> {
    BuiltIn,
    Path(&'a Path),
    Environment(&'a str),
    Cli(Option<&'a Path>),
}

impl Definition<'_> {
    fn priority(&self) -> u8 {
        match self {
            Self::BuiltIn => 0,
            Self::Path(_) => 1,
            Self::Environment(_) => 2,
            Self::Cli(_) => 3,
        }
    }

    pub(super) fn is_higher_priority(&self, other: &Self) -> bool {
        self.priority() > other.priority()
    }

    pub(super) fn root<'a, A: document::Admission>(
        &'a self,
        cwd: &'a Path,
        session: &mut Session<'_, A>,
    ) -> Step<&'a Path> {
        session.check()?;
        match self {
            Self::Path(file) | Self::Cli(Some(file)) => {
                // Successful file loading must already supply a file path with
                // a parent. The source-cause classification does not cover
                // malformed internal geometry or authorize translation of the
                // first unwrap's failure.
                let Some(parent) = file.parent() else {
                    return session.fail(Error::State);
                };
                match parent.parent() {
                    Some(root) => Ok(root),
                    None => session.source_config_parent(),
                }
            }
            Self::Environment(_) | Self::Cli(None) | Self::BuiltIn => Ok(cwd),
        }
    }

    /// Exact ConfigRelativePath operation order, including absolute raw paths.
    pub(super) fn resolve<A: document::Admission>(
        &self,
        raw: &str,
        cwd: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<PathBuf> {
        let root = self.root(cwd, session)?;
        path::join(root, Path::new(raw), session)
    }
}

// Cargo context/value.rs:141–150; formatting is admitted by the calling formatter.
impl std::fmt::Display for Definition<'_> {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(path) | Self::Cli(Some(path)) => path.display().fmt(output),
            Self::Environment(key) => write!(output, "environment variable `{key}`"),
            Self::Cli(None) => output.write_str("--config cli option"),
            Self::BuiltIn => output.write_str("default"),
        }
    }
}

// Cargo compares the origin class, not the file path or environment key.
// Exact source identities remain in the independently retained observations.
impl PartialEq for Definition<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.priority() == other.priority()
    }
}
impl Eq for Definition<'_> {}
impl PartialOrd for Definition<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Definition<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority().cmp(&other.priority())
    }
}

#[cfg(test)]
mod tests;
