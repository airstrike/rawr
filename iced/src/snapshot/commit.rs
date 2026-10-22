//! Proof that a complete artifact tree crossed its filesystem commit point.

use std::fmt;
use std::path::{Path, PathBuf};

use super::Error;

/// Recoverable data left after a snapshot artifact commit succeeded.
#[derive(Debug)]
pub struct Notice {
    target: PathBuf,
    recoverable: PathBuf,
    source: std::io::Error,
}

/// Proof that one or more artifact trees reached their commit points.
#[derive(Debug, Default)]
pub struct Commit {
    notices: Vec<Notice>,
}

impl Commit {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn warn(
        &mut self,
        target: impl Into<PathBuf>,
        recoverable: impl Into<PathBuf>,
        source: std::io::Error,
    ) {
        self.notices.push(Notice {
            target: target.into(),
            recoverable: recoverable.into(),
            source,
        });
    }

    pub(super) fn merge(mut self, mut other: Self) -> Self {
        self.notices.append(&mut other.notices);
        self
    }

    pub(super) fn follow<T>(self, result: Result<T, Error>) -> Result<(T, Self), Error> {
        match result {
            Ok(value) => Ok((value, self)),
            Err(operation) if self.notices.is_empty() => Err(operation),
            Err(operation) => Err(Error::Following {
                commit: self,
                operation: Box::new(operation),
            }),
        }
    }

    /// Return every cleanup warning retained after the commit point.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }
}

impl Notice {
    /// Return the installed artifact-tree path.
    #[must_use]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Return the exact recoverable path left on disk.
    #[must_use]
    pub fn recoverable(&self) -> &Path {
        &self.recoverable
    }

    /// Return the operating-system cleanup failure.
    #[must_use]
    pub const fn source(&self) -> &std::io::Error {
        &self.source
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "artifact tree committed at {}, but recoverable data remains at {}: {}",
            self.target.display(),
            self.recoverable.display(),
            self.source
        )
    }
}

impl fmt::Display for Commit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} committed cleanup warning(s)",
            self.notices.len()
        )?;
        for notice in &self.notices {
            write!(formatter, "; {notice}")?;
        }
        Ok(())
    }
}
