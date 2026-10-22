//! Source text retained by the Rawr compiler.

use crate::{ImmutableString, Position, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The source text associated with one parsed compilation unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Map {
    text: ImmutableString,
}

impl Map {
    /// Retain source text for a compilation unit.
    #[must_use]
    pub fn new(text: impl Into<ImmutableString>) -> Self {
        Self { text: text.into() }
    }

    /// Access the retained source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn without(&self, spans: impl IntoIterator<Item = Span>) -> String {
        let spans: Vec<_> = spans.into_iter().collect();
        let mut masked = String::with_capacity(self.text.len());
        let mut line = 1_u16;
        let mut column = 0_u16;

        for character in self.text.chars() {
            if character == '\n' {
                masked.push(character);
                line = line.saturating_add(1);
                column = 0;
                continue;
            }

            column = column.saturating_add(1);
            let position = Position::new(line, column);
            let is_item = spans
                .iter()
                .any(|span| position >= span.start() && position < span.end());

            masked.push(if is_item { ' ' } else { character });
        }

        masked
    }
}
