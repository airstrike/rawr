//! Owned source snapshots attached directly to positioned labels.

use crate::{ImmutableString, Shared};

/// An opaque owned source snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source(Shared<Data>);

#[derive(Debug, Eq, PartialEq)]
struct Data {
    identity: ImmutableString,
    text: ImmutableString,
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
        })))
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
