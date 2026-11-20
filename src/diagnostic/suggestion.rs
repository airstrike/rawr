//! Code a report suggests: edits to its sources, which rustc shows as the
//! source they make, or beside the code one changes.

use crate::{ImmutableString, Span};

use super::{Error, Source};

/// How rustc shows a suggestion.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Style {
    /// Beside the code it changes, as a `help:` label, when it is the
    /// report's one suggestion, one edit of one line, with a message of
    /// fewer than ten words; otherwise as [`Style::Verbose`] (rustc's
    /// `span_suggestion`).
    Inline,
    /// Under its own `help:` line, as the source its edits make (rustc's
    /// `span_suggestion_verbose`).
    Verbose,
}

/// One replacement of a span of a source's text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    source: Source,
    span: Span,
    replacement: ImmutableString,
}

/// Code a report suggests: its message and the edits that make it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    message: ImmutableString,
    style: Style,
    edits: Vec<Edit>,
}

impl Edit {
    /// Replace `span` of `source` with `replacement`: an empty span
    /// inserts, an empty replacement removes.
    ///
    /// # Errors
    ///
    /// Returns an error for a span with no position, a reversed one, or one
    /// outside its source.
    pub fn new(
        source: Source,
        span: Span,
        replacement: impl Into<ImmutableString>,
    ) -> Result<Self, Error> {
        if span.is_none() {
            return Err(Error::MissingSpan);
        }
        if span.start() > span.end() {
            return Err(Error::ReversedSpan(span));
        }
        if !source.contains(span.start()) || !source.contains(span.end()) {
            return Err(Error::SpanOutsideSource {
                source: source.identity().into(),
                span,
            });
        }
        Ok(Self {
            source,
            span,
            replacement: replacement.into(),
        })
    }

    /// Return the source the edit changes.
    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// Return the span it replaces.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Return the text it puts there.
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// Whether the edit falls in code a macro wrote, where rustc suggests
    /// nothing.
    pub(super) fn expanded(&self) -> bool {
        self.source.origin().is_some() || self.source.expansion_at(self.span.start()).is_some()
    }

    pub(super) fn moved(&mut self, source: Source, span: Span) {
        self.source = source;
        self.span = span;
    }
}

impl Suggestion {
    /// A suggestion of `edit`, shown in `style` under `message`.
    #[must_use]
    pub fn new(message: impl Into<ImmutableString>, style: Style, edit: Edit) -> Self {
        Self {
            message: message.into(),
            style,
            edits: vec![edit],
        }
    }

    /// The suggestion with `edit` made too.
    #[must_use]
    pub fn edit(mut self, edit: Edit) -> Self {
        self.edits.push(edit);
        self
    }

    /// Return what the suggestion says.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return how it is shown.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Return its edits, in the order they were made.
    #[must_use]
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub(super) fn edits_mut(&mut self) -> &mut [Edit] {
        &mut self.edits
    }

    pub(super) fn written(
        mut self,
        written: impl FnOnce(ImmutableString) -> ImmutableString,
    ) -> Self {
        self.message = written(self.message);
        self
    }

    /// The `help:` label rustc writes beside the code instead of the
    /// suggestion's own section, and the edit it is written at: for a
    /// report's only suggestion that is [`Style::Inline`], one edit of
    /// one line, under a message of fewer than ten words.
    pub(super) fn inline(suggestions: &[Self]) -> Option<(&Edit, String)> {
        let [suggestion] = suggestions else {
            return None;
        };
        let [edit] = suggestion.edits.as_slice() else {
            return None;
        };
        if suggestion.style != Style::Inline
            || suggestion.message.split_whitespace().count() >= 10
            || edit.replacement.contains('\n')
        {
            return None;
        }
        // A removal shows no code.
        let label = if edit.replacement.is_empty() {
            format!("help: {}", suggestion.message)
        } else {
            format!("help: {}: `{}`", suggestion.message, edit.replacement)
        };
        Some((edit, label))
    }
}
