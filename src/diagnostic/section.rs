//! A note or help with places of its own, which rustc shows as its own
//! section after the report's snippet: its `note:` or `help:` line, then
//! the source lines its labels mark.

use crate::{ImmutableString, Span};

use super::{Label, LabelKind, Source};

/// The line a section starts with.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Level {
    /// `note:`, something the report's reader should know.
    Note,
    /// `help:`, something the reader could do.
    Help,
}

/// A note or help and the places it marks: one primary, `^` under it, and
/// any related ones, `-` under them, each with its own label or none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    level: Level,
    message: ImmutableString,
    labels: Vec<Label>,
}

impl Section {
    /// A `note:` section saying `message`.
    #[must_use]
    pub fn note(message: impl Into<ImmutableString>) -> Self {
        Self::new(Level::Note, message.into())
    }

    /// A `help:` section saying `message`.
    #[must_use]
    pub fn help(message: impl Into<ImmutableString>) -> Self {
        Self::new(Level::Help, message.into())
    }

    /// A section at `level` saying `message`, with no places yet.
    #[must_use]
    pub const fn new(level: Level, message: ImmutableString) -> Self {
        Self {
            level,
            message,
            labels: Vec::new(),
        }
    }

    /// The section with `span` of `source` as the place it is about. A
    /// report checks the place when it takes the section
    /// ([`super::Builder::section`]).
    #[must_use]
    pub fn primary(
        self,
        source: Source,
        span: Span,
        message: impl Into<Option<ImmutableString>>,
    ) -> Self {
        self.label(source, span, LabelKind::Primary, message.into())
    }

    /// The section with `span` of `source` as a place related to it.
    #[must_use]
    pub fn secondary(
        self,
        source: Source,
        span: Span,
        message: impl Into<Option<ImmutableString>>,
    ) -> Self {
        self.label(source, span, LabelKind::Secondary, message.into())
    }

    fn label(
        mut self,
        source: Source,
        span: Span,
        kind: LabelKind,
        message: Option<ImmutableString>,
    ) -> Self {
        self.labels.push(Label::new(source, span, kind, message));
        self
    }

    /// Return the line it starts with.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    /// Return what it says.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return the places it marks, in the order they were given.
    #[must_use]
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub(super) fn labels_mut(&mut self) -> &mut [Label] {
        &mut self.labels
    }

    /// The section, its message and labels as `written` writes them, and
    /// its labels those `placed` gives.
    pub(super) fn written(
        mut self,
        written: impl Fn(ImmutableString) -> ImmutableString,
        placed: impl FnMut(Label) -> Result<Label, super::Error>,
    ) -> Result<Self, super::Error> {
        self.message = written(self.message);
        self.labels = self
            .labels
            .into_iter()
            .map(placed)
            .collect::<Result<_, _>>()?;
        Ok(self)
    }
}
