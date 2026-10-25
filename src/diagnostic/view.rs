//! Stable read-only data views for editor and serialization consumers.

use super::{Category, ExternalLabel, Label, LabelKind, Report, Severity, Source};

/// Serializable read-only projection of a report.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportView<'a> {
    code: &'a str,
    category: Category,
    severity: Severity,
    message: &'a str,
    sources: Vec<SourceView<'a>>,
    labels: Vec<LabelView<'a>>,
    external_labels: Vec<ExternalLabelView<'a>>,
    help: Option<&'a str>,
    notes: Vec<&'a str>,
}

/// Serializable read-only projection of one label.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LabelView<'a> {
    source: SourceView<'a>,
    span: SpanView,
    kind: LabelKind,
    message: Option<&'a str>,
}

/// Serializable read-only projection of one external position label.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalLabelView<'a> {
    file: &'a str,
    byte_start: u32,
    byte_end: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
    message: Option<&'a str>,
}

/// Serializable read-only projection of an owned source snapshot.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceView<'a> {
    identity: &'a str,
    text: &'a str,
}

/// Serializable source span independent of Rawr's compact internal layout.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpanView {
    start: PositionView,
    end: PositionView,
}

/// Serializable line/column source position.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionView {
    line: Option<usize>,
    column: Option<usize>,
}

impl Report {
    /// Project canonical report data into the stable read-only editor view.
    #[must_use]
    pub fn view(&self) -> ReportView<'_> {
        ReportView {
            code: self.code().as_str(),
            category: self.category(),
            severity: self.severity(),
            message: self.message(),
            sources: self.sources().iter().map(Source::view).collect(),
            labels: self.labels().iter().map(Label::view).collect(),
            external_labels: self
                .external_labels()
                .iter()
                .map(ExternalLabel::view)
                .collect(),
            help: self.help(),
            notes: self.notes().collect(),
        }
    }
}

impl Label {
    fn view(&self) -> LabelView<'_> {
        LabelView {
            source: self.source().view(),
            span: SpanView {
                start: PositionView::new(self.span().start()),
                end: PositionView::new(self.span().end()),
            },
            kind: self.kind(),
            message: self.message(),
        }
    }
}

impl ExternalLabel {
    fn view(&self) -> ExternalLabelView<'_> {
        let ((line_start, column_start), (line_end, column_end)) = self.position().positions();
        let bytes = self.position().byte_range();
        ExternalLabelView {
            file: self.position().file(),
            byte_start: bytes.start,
            byte_end: bytes.end,
            line_start,
            column_start,
            line_end,
            column_end,
            message: self.message(),
        }
    }
}

impl Source {
    fn view(&self) -> SourceView<'_> {
        SourceView {
            identity: self.identity(),
            text: self.text(),
        }
    }
}

impl PositionView {
    fn new(position: crate::Position) -> Self {
        Self {
            line: position.line(),
            column: position.position(),
        }
    }
}

impl<'a> ReportView<'a> {
    /// Return the stable code.
    #[must_use]
    pub const fn code(&self) -> &'a str {
        self.code
    }

    /// Return the semantic family.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    /// Return the severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Return the concise message.
    #[must_use]
    pub const fn message(&self) -> &'a str {
        self.message
    }

    /// Return exact labels.
    #[must_use]
    pub fn labels(&self) -> &[LabelView<'a>] {
        &self.labels
    }

    /// Return exact external position labels.
    #[must_use]
    pub fn external_labels(&self) -> &[ExternalLabelView<'a>] {
        &self.external_labels
    }

    /// Return every retained source snapshot.
    #[must_use]
    pub fn sources(&self) -> &[SourceView<'a>] {
        &self.sources
    }

    /// Return actionable help.
    #[must_use]
    pub const fn help(&self) -> Option<&'a str> {
        self.help
    }

    /// Return canonical notes.
    #[must_use]
    pub fn notes(&self) -> &[&'a str] {
        &self.notes
    }
}

impl<'a> LabelView<'a> {
    /// Return the owned source projection.
    #[must_use]
    pub const fn source(&self) -> SourceView<'a> {
        self.source
    }

    /// Return the exact span projection.
    #[must_use]
    pub const fn span(&self) -> SpanView {
        self.span
    }

    /// Return the label kind.
    #[must_use]
    pub const fn kind(&self) -> LabelKind {
        self.kind
    }

    /// Return the label message.
    #[must_use]
    pub const fn message(&self) -> Option<&'a str> {
        self.message
    }
}

impl<'a> ExternalLabelView<'a> {
    /// Return the external filename.
    #[must_use]
    pub const fn file(self) -> &'a str {
        self.file
    }

    /// Return the half-open external byte range.
    #[must_use]
    pub const fn byte_range(self) -> std::ops::Range<u32> {
        self.byte_start..self.byte_end
    }

    /// Return one-based line and zero-based column endpoints.
    #[must_use]
    pub const fn positions(self) -> ((u32, u32), (u32, u32)) {
        (
            (self.line_start, self.column_start),
            (self.line_end, self.column_end),
        )
    }

    /// Return the related-evidence message.
    #[must_use]
    pub const fn message(self) -> Option<&'a str> {
        self.message
    }
}

impl<'a> SourceView<'a> {
    /// Return the source identity.
    #[must_use]
    pub const fn identity(self) -> &'a str {
        self.identity
    }

    /// Return the exact source text.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
}

impl SpanView {
    /// Return the start position.
    #[must_use]
    pub const fn start(self) -> PositionView {
        self.start
    }

    /// Return the end position.
    #[must_use]
    pub const fn end(self) -> PositionView {
        self.end
    }
}

impl PositionView {
    /// Return the one-based line.
    #[must_use]
    pub const fn line(self) -> Option<usize> {
        self.line
    }

    /// Return the one-based character column, or `None` at beginning-of-line.
    #[must_use]
    pub const fn column(self) -> Option<usize> {
        self.column
    }
}
