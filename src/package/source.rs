//! Canonical source snapshots inside one package graph.

use std::path::{Path, PathBuf};

use crate::{diagnostic, ImmutableString};

/// An opaque source identity owned by one package graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

/// One canonical Rawr source snapshot in a package graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    id: Id,
    path: PathBuf,
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
    diagnostic: diagnostic::Source,
}

impl Id {
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

impl Source {
    pub(crate) fn new(
        id: Id,
        path: PathBuf,
        package: ImmutableString,
        modules: Box<[ImmutableString]>,
        text: String,
    ) -> Result<Self, diagnostic::Error> {
        let diagnostic = diagnostic::Source::new(path.to_string_lossy().into_owned(), text)?;
        Ok(Self {
            id,
            path,
            package,
            modules,
            diagnostic,
        })
    }

    /// Return this source's graph-local opaque identity.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Return the canonical filesystem path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the stable manifest-declared package identity.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Return the lexical module path from the package root.
    #[must_use]
    pub fn modules(&self) -> &[ImmutableString] {
        &self.modules
    }

    /// Return the exact retained UTF-8 source text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.diagnostic.text()
    }

    pub(crate) const fn diagnostic(&self) -> &diagnostic::Source {
        &self.diagnostic
    }
}
