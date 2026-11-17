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
    /// How long the file's own text is, which the diagnostic's text holds
    /// first, before the expansions laid out after it.
    file: usize,
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
        let file = diagnostic.text().len();
        Ok(Self {
            id,
            path,
            package,
            modules,
            diagnostic,
            file,
        })
    }

    /// A derive's expansion `text`, kept at `path`, read as part of the
    /// module `modules` its item is in, whose positions are reported at
    /// `origin` (#593).
    #[cfg(feature = "rust-import")]
    pub(crate) fn expansion(
        id: Id,
        path: PathBuf,
        package: ImmutableString,
        modules: Box<[ImmutableString]>,
        text: String,
        origin: diagnostic::source::Origin,
    ) -> Result<Self, diagnostic::Error> {
        let diagnostic =
            diagnostic::Source::expansion(path.to_string_lossy().into_owned(), text, origin)?;
        let file = diagnostic.text().len();
        Ok(Self {
            id,
            path,
            package,
            modules,
            diagnostic,
            file,
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
        &self.diagnostic.text()[..self.file]
    }

    /// The file's text with the expansions of the macro calls among its
    /// items laid out after it, which their items' positions are in.
    pub(crate) fn expanded_text(&self) -> &str {
        self.diagnostic.text()
    }

    /// Lay out `expansions` after the file's text: `text` is the file's
    /// with them after it, as its parse gives it.
    pub(crate) fn expand(&mut self, text: &str, expansions: Box<[diagnostic::source::Expansion]>) {
        self.diagnostic = self.diagnostic.with_expansions(text, expansions);
    }

    pub(crate) const fn diagnostic(&self) -> &diagnostic::Source {
        &self.diagnostic
    }
}
