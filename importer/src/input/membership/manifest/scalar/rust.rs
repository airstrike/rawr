//! Cargo RustVersion's PartialVersion conversion, before compatibility checks.

use std::fmt;

use super::{partial, Error};

/// A validated Rust version without prerelease or build fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Rust {
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Partial(partial::Rejection),
    Prerelease,
    BuildMetadata,
}

impl fmt::Display for Rust {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.major)?;
        if let Some(minor) = self.minor {
            write!(formatter, ".{minor}")?;
        }
        if let Some(patch) = self.patch {
            write!(formatter, ".{patch}")?;
        }
        Ok(())
    }
}

impl Rust {
    pub(crate) fn parse(text: &str) -> Result<Self, Error> {
        let value = partial::parse(text).map_err(|error| Error::Rust(Rejection::Partial(error)))?;
        if value.pre().is_some() {
            return Err(Error::Rust(Rejection::Prerelease));
        }
        if value.build().is_some() {
            return Err(Error::Rust(Rejection::BuildMetadata));
        }
        Ok(Self {
            major: value.major(),
            minor: value.minor(),
            patch: value.patch(),
        })
    }

    pub(crate) fn major(&self) -> u64 {
        self.major
    }

    pub(crate) fn minor(&self) -> Option<u64> {
        self.minor
    }

    pub(crate) fn patch(&self) -> Option<u64> {
        self.patch
    }
}
