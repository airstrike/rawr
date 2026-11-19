//! Owned source snapshots attached directly to positioned labels.

use crate::{ImmutableString, Shared};

/// An opaque owned source snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source(Shared<Data>);

#[derive(Debug, Eq, PartialEq)]
struct Data {
    identity: ImmutableString,
    text: ImmutableString,
    origin: Option<Origin>,
    /// The lines past the file's own where macro calls among its items
    /// are expanded.
    expansions: Box<[Expansion]>,
}

/// Where a macro's expansion was asked for: every position in an expansion
/// is reported there, as rustc reports an error in a derive's output at
/// the derive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Origin {
    pub(crate) source: Source,
    pub(crate) span: crate::Span,
    /// The macro, as the program writes it (`Serialize`).
    pub(crate) name: ImmutableString,
}

/// Lines a source holds past its file's own, where the expansion of a
/// macro call among its items is laid out (`parser::item_macro`): every
/// position on them is reported at the call, as rustc reports an error in
/// a macro's output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Expansion {
    /// The first and the last of the lines.
    pub(crate) lines: (usize, usize),
    /// The call, in the file's own lines.
    pub(crate) call: crate::Span,
    /// The call where it is written: on the lines of the expansion that
    /// holds it, for a call an expansion makes.
    pub(crate) site: crate::Span,
    /// The macro, as the call writes it.
    pub(crate) name: ImmutableString,
    /// Each call of a location macro (`line!()`, `column!()`) in the
    /// laid-out text: where its path starts there, and where the token
    /// that starts it came from, which the lowering reads it at
    /// (`macros::Table::written`).
    pub(crate) locations: Box<[(crate::Position, crate::Position)]>,
}

impl Source {
    /// Retain one exact source identity and text snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the display identity is empty.
    pub fn new(
        identity: impl Into<ImmutableString>,
        text: impl Into<ImmutableString>,
    ) -> Result<Self, super::Error> {
        let identity = identity.into();
        if identity.is_empty() {
            return Err(super::Error::EmptySourceIdentity);
        }
        Ok(Self(Shared::new(Data {
            identity,
            text: text.into(),
            origin: None,
            expansions: Box::default(),
        })))
    }

    /// The expansion `text` of the macro `origin` names, kept at
    /// `identity`, whose positions are all reported at `origin`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn expansion(
        identity: impl Into<ImmutableString>,
        text: impl Into<ImmutableString>,
        origin: Origin,
    ) -> Result<Self, super::Error> {
        let identity = identity.into();
        if identity.is_empty() {
            return Err(super::Error::EmptySourceIdentity);
        }
        Ok(Self(Shared::new(Data {
            identity,
            text: text.into(),
            origin: Some(origin),
            expansions: Box::default(),
        })))
    }

    /// This source with its `text` past the file's own lines, which holds
    /// the `expansions` of the calls among its items.
    pub(crate) fn with_expansions(
        &self,
        text: impl Into<ImmutableString>,
        expansions: Box<[Expansion]>,
    ) -> Self {
        Self(Shared::new(Data {
            identity: self.0.identity.clone(),
            text: text.into(),
            origin: self.0.origin.clone(),
            expansions,
        }))
    }

    /// The expansion laid out on the line of `position`, if any.
    pub(crate) fn expansion_at(&self, position: crate::Position) -> Option<&Expansion> {
        let line = position.line()?;
        self.0
            .expansions
            .iter()
            .find(|expansion| (expansion.lines.0..=expansion.lines.1).contains(&line))
    }

    /// Where this expansion was asked for, when it is a macro's expansion.
    pub(crate) fn origin(&self) -> Option<&Origin> {
        self.0.origin.as_ref()
    }

    /// Return the stable human-facing source identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.0.identity
    }

    /// Return the exact retained source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0.text
    }

    pub(super) fn contains(&self, position: crate::Position) -> bool {
        let Some(line_number) = position.line() else {
            return false;
        };
        let Some(line) = self.text().split('\n').nth(line_number.saturating_sub(1)) else {
            return false;
        };
        let column = position.position().unwrap_or(0);
        // Token spans are half-open and parser EOF cursors may sit one
        // character beyond the final scalar value on a line.
        let maximum = line.chars().count() + 1;
        column <= maximum
    }
}
