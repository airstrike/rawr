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
        })))
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
