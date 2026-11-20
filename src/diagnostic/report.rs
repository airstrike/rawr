//! Validated diagnostic proof bundles.

use crate::{ImmutableString, Span};
use std::fmt;

use super::{code::Code, Section, Source, Suggestion};

/// Stable high-level diagnostic family.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    /// Source grammar and tokenization.
    Parse,
    /// Name, item, and type resolution.
    Resolve,
    /// Framework application contracts and host bindings.
    Framework,
    /// Runtime evaluation and value failures.
    Runtime,
    /// Filesystem, project, and host-environment failures.
    Environment,
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse => formatter.write_str("parse"),
            Self::Resolve => formatter.write_str("resolve"),
            Self::Framework => formatter.write_str("framework"),
            Self::Runtime => formatter.write_str("runtime"),
            Self::Environment => formatter.write_str("environment"),
        }
    }
}

/// Canonical diagnostic severity.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    /// Compilation or execution cannot continue.
    Error,
    /// Action is recommended but the operation may continue.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => formatter.write_str("error"),
            Self::Warning => formatter.write_str("warning"),
        }
    }
}

/// Whether a positioned label is the report's primary or related evidence.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LabelKind {
    /// The exact source evidence that triggered the report.
    Primary,
    /// Related source evidence, such as an earlier declaration.
    Secondary,
}

impl fmt::Display for LabelKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => formatter.write_str("primary"),
            Self::Secondary => formatter.write_str("secondary"),
        }
    }
}

/// One exact source label retained by a report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    source: Source,
    span: Span,
    kind: LabelKind,
    message: Option<ImmutableString>,
}

/// One related position in an external source whose bytes are not retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalLabel {
    position: ExternalPosition,
    message: Option<ImmutableString>,
}

/// Exact external file, byte, line, and column evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalPosition {
    file: ImmutableString,
    byte_start: u32,
    byte_end: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
}

/// A canonical presentation-independent diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    code: Code,
    severity: Severity,
    message: ImmutableString,
    sources: Vec<Source>,
    labels: Vec<Label>,
    external_labels: Vec<ExternalLabel>,
    help: Option<ImmutableString>,
    notes: Vec<ImmutableString>,
    sections: Vec<Section>,
    suggestions: Vec<Suggestion>,
}

/// A report under validated construction.
#[derive(Debug)]
pub struct Builder {
    report: Report,
    has_primary: bool,
}

/// Invalid evidence rejected while constructing an opaque diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Report evidence could not reserve its exact retained capacity.
    Allocation,
    /// A source snapshot had no stable display identity.
    EmptySourceIdentity,
    /// An external position had no stable filename.
    EmptyExternalFile,
    /// A label used the sentinel no-position span.
    MissingSpan,
    /// A label ended before it started.
    ReversedSpan(Span),
    /// An external byte range ended before it started.
    ReversedExternalByteRange,
    /// An external line/column range ended before it started.
    ReversedExternalPosition,
    /// A label position falls outside its owned source snapshot.
    SpanOutsideSource {
        /// Stable source identity.
        source: ImmutableString,
        /// Rejected exact span.
        span: Span,
    },
    /// A report attempted to install a second primary label.
    MultiplePrimaryLabels,
}

impl Report {
    /// Start one error report using the category fixed by its stable code.
    #[must_use]
    pub fn error(code: Code, message: impl Into<ImmutableString>) -> Builder {
        Builder::new(code, Severity::Error, written(message.into()))
    }

    /// Start one warning report using the category fixed by its stable code.
    #[must_use]
    pub fn warning(code: Code, message: impl Into<ImmutableString>) -> Builder {
        Builder::new(code, Severity::Warning, written(message.into()))
    }

    /// Return this report's stable semantic code.
    #[must_use]
    pub const fn code(&self) -> Code {
        self.code
    }

    /// Return the semantic family fixed by the code.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.code.category()
    }

    /// Return the canonical severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Return the concise presentation-independent message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return all exact positioned labels in insertion order.
    #[must_use]
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// Return related external positions that intentionally own no source text.
    #[must_use]
    pub fn external_labels(&self) -> &[ExternalLabel] {
        &self.external_labels
    }

    /// Return every exact source snapshot retained by this report.
    ///
    /// Labels still own their Source directly; this list also retains known
    /// source context for diagnostics without a meaningful position.
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Return actionable help when available.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Return supplementary canonical notes.
    #[must_use]
    pub fn notes(&self) -> impl ExactSizeIterator<Item = &str> {
        self.notes.iter().map(AsRef::as_ref)
    }

    /// Return the notes and helps with places of their own, in the order
    /// they were given.
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Return the code the report suggests, in the order it was suggested.
    #[must_use]
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// The report with each label and edit `moved` gives a new place moved
    /// there: a text compiled from pieces of others reports where each
    /// piece is shown (`crate::splice`). A source no label or edit is left
    /// in is dropped.
    pub(crate) fn relabeled(
        &self,
        mut moved: impl FnMut(&Source, Span) -> Option<(Source, Span)>,
    ) -> Self {
        let mut report = self.clone();
        let mut left = Vec::new();
        let labels = report
            .labels
            .iter_mut()
            .chain(report.sections.iter_mut().flat_map(Section::labels_mut));
        for label in labels {
            if let Some((source, span)) = moved(&label.source, label.span) {
                left.push(label.source.clone());
                label.source = source;
                label.span = span;
            }
        }
        for edit in report
            .suggestions
            .iter_mut()
            .flat_map(Suggestion::edits_mut)
        {
            if let Some((source, span)) = moved(edit.source(), edit.span()) {
                left.push(edit.source().clone());
                edit.moved(source, span);
            }
        }
        report.sources.retain(|source| !left.contains(source));
        let placed = report
            .labels
            .iter()
            .chain(report.sections.iter().flat_map(Section::labels))
            .map(|label| &label.source)
            .chain(
                report
                    .suggestions
                    .iter()
                    .flat_map(Suggestion::edits)
                    .map(super::suggestion::Edit::source),
            );
        for source in placed {
            if !report.sources.contains(source) {
                report.sources.push(source.clone());
            }
        }
        report
    }
}

/// A report's text as the program could write it: an item a macro's
/// `$crate` reached through a crate's hidden segment reads from that
/// crate's root (`host::path::public`), so no message names it (#563).
fn written(text: ImmutableString) -> ImmutableString {
    match crate::host::path::public(&text) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(public) => public.into(),
    }
}

impl Builder {
    fn new(code: Code, severity: Severity, message: ImmutableString) -> Self {
        Self {
            report: Report {
                code,
                severity,
                message,
                sources: Vec::new(),
                labels: Vec::new(),
                external_labels: Vec::new(),
                help: None,
                notes: Vec::new(),
                sections: Vec::new(),
                suggestions: Vec::new(),
            },
            has_primary: false,
        }
    }

    /// Attach the one primary source label.
    ///
    /// # Errors
    ///
    /// Returns an error for a second primary or a span outside its source.
    pub fn primary(
        self,
        source: Source,
        span: Span,
        message: impl Into<Option<ImmutableString>>,
    ) -> Result<Self, Error> {
        self.label(source, span, LabelKind::Primary, message.into())
    }

    /// Attach one related source label.
    ///
    /// # Errors
    ///
    /// Returns an error when the span falls outside its source snapshot.
    pub fn secondary(
        self,
        source: Source,
        span: Span,
        message: impl Into<Option<ImmutableString>>,
    ) -> Result<Self, Error> {
        self.label(source, span, LabelKind::Secondary, message.into())
    }

    /// Attach one related external position without claiming its source text.
    #[must_use]
    pub fn secondary_external(
        mut self,
        position: ExternalPosition,
        message: impl Into<Option<ImmutableString>>,
    ) -> Self {
        self.report.external_labels.push(ExternalLabel {
            position,
            message: message.into().map(written),
        });
        self
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn reserve_external_labels(&mut self, additional: usize) -> Result<(), Error> {
        self.report
            .external_labels
            .try_reserve_exact(additional)
            .map_err(|_| Error::Allocation)
    }

    /// Retain one exact source snapshot as report context.
    #[must_use]
    pub fn source(mut self, source: Source) -> Self {
        // An expansion's positions are its derive's, in the item's source.
        let source = match source.origin() {
            Some(origin) => origin.source.clone(),
            None => source,
        };
        if !self.report.sources.contains(&source) {
            self.report.sources.push(source);
        }
        self
    }

    fn label(
        mut self,
        source: Source,
        span: Span,
        kind: LabelKind,
        message: Option<ImmutableString>,
    ) -> Result<Self, Error> {
        let label = self.placed(Label::new(source, span, kind, message))?;
        if kind == LabelKind::Primary {
            if self.has_primary {
                return Err(Error::MultiplePrimaryLabels);
            }
            self.has_primary = true;
        }
        self.report.labels.push(label);
        Ok(self)
    }

    /// `label` where the report shows it, its source retained: a place a
    /// macro wrote is shown where the macro is called or derived.
    fn placed(&mut self, label: Label) -> Result<Label, Error> {
        let Label {
            source,
            span,
            kind,
            message,
        } = label;
        if span.is_none() {
            return Err(Error::MissingSpan);
        }
        if span.start() > span.end() {
            return Err(Error::ReversedSpan(span));
        }
        // A position in a derive's expansion is the derive's, as rustc
        // reports it, with rustc's note naming the macro.
        let (source, span) = match source.origin() {
            Some(origin) => {
                let note: ImmutableString = format!(
                    "this error originates in the derive macro `{}`",
                    origin.name
                )
                .into();
                if !self.report.notes.contains(&note) {
                    self.report.notes.push(note);
                }
                (origin.source.clone(), origin.span)
            }
            None => (source, span),
        };
        // A position in the expansion of a call among the items is the
        // call's, with rustc's note naming the macro.
        let span = match source.expansion_at(span.start()) {
            Some(expansion) => {
                let note: ImmutableString = format!(
                    "this error originates in the macro `{}` (in Nightly builds, run with -Z \
                     macro-backtrace for more info)",
                    expansion.name
                )
                .into();
                if !self.report.notes.contains(&note) {
                    self.report.notes.push(note);
                }
                expansion.call
            }
            None => span,
        };
        // A span of tokens a macro expansion made from one written token
        // can run past the written one's line: it starts where they were
        // written.
        let span = if source.contains(span.start()) && !source.contains(span.end()) {
            Span::new(span.start(), span.start())
        } else {
            span
        };
        if !source.contains(span.start()) || !source.contains(span.end()) {
            return Err(Error::SpanOutsideSource {
                source: source.identity().into(),
                span,
            });
        }
        if !self.report.sources.contains(&source) {
            self.report.sources.push(source.clone());
        }
        Ok(Label {
            source,
            span,
            kind,
            message: message.map(written),
        })
    }

    /// Attach actionable help.
    #[must_use]
    pub fn help(mut self, help: impl Into<ImmutableString>) -> Self {
        self.report.help = Some(written(help.into()));
        self
    }

    /// Attach a supplementary canonical note.
    #[must_use]
    pub fn note(mut self, note: impl Into<ImmutableString>) -> Self {
        self.report.notes.push(written(note.into()));
        self
    }

    /// Attach a note or help with places of its own, which rustc shows as
    /// its own section after the notes.
    ///
    /// # Errors
    ///
    /// Returns an error for a place with no position, a reversed one, or
    /// one outside its source.
    pub fn section(mut self, section: Section) -> Result<Self, Error> {
        let section = section.written(written, |label| self.placed(label))?;
        self.report.sections.push(section);
        Ok(self)
    }

    /// Suggest code, as rustc does under a `help:`. A suggestion that edits
    /// code a macro wrote is dropped, as rustc suggests nothing there.
    #[must_use]
    pub fn suggestion(mut self, suggestion: Suggestion) -> Self {
        if suggestion
            .edits()
            .iter()
            .any(super::suggestion::Edit::expanded)
        {
            return self;
        }
        for edit in suggestion.edits() {
            self = self.source(edit.source().clone());
        }
        self.report.suggestions.push(suggestion.written(written));
        self
    }

    /// Finish the validated proof bundle.
    #[must_use]
    pub fn finish(self) -> Report {
        self.report
    }
}

impl Label {
    pub(super) const fn new(
        source: Source,
        span: Span,
        kind: LabelKind,
        message: Option<ImmutableString>,
    ) -> Self {
        Self {
            source,
            span,
            kind,
            message,
        }
    }

    /// Return the label's owned source snapshot.
    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// Return the exact source span.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Return whether this is primary or related evidence.
    #[must_use]
    pub const fn kind(&self) -> LabelKind {
        self.kind
    }

    /// Return the concise label message when supplied.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl ExternalPosition {
    /// Validate an exact external position independently of source text.
    pub fn checked(
        file: impl Into<ImmutableString>,
        byte_start: u32,
        byte_end: u32,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Result<Self, Error> {
        let file = file.into();
        if file.is_empty() {
            return Err(Error::EmptyExternalFile);
        }
        if byte_start > byte_end {
            return Err(Error::ReversedExternalByteRange);
        }
        if start > end {
            return Err(Error::ReversedExternalPosition);
        }
        Ok(Self {
            file,
            byte_start,
            byte_end,
            line_start: start.0,
            column_start: start.1,
            line_end: end.0,
            column_end: end.1,
        })
    }

    /// Return the external filename retained by the importer.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Return the half-open external byte range.
    #[must_use]
    pub const fn byte_range(&self) -> std::ops::Range<u32> {
        self.byte_start..self.byte_end
    }

    /// Return one-based line and zero-based column endpoints.
    #[must_use]
    pub const fn positions(&self) -> ((u32, u32), (u32, u32)) {
        (
            (self.line_start, self.column_start),
            (self.line_end, self.column_end),
        )
    }
}

impl ExternalLabel {
    /// Return the exact external position proof.
    #[must_use]
    pub const fn position(&self) -> &ExternalPosition {
        &self.position
    }

    /// Return the concise related-evidence message.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}{}: {}",
            self.severity,
            self.code.tag(),
            self.message
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation => formatter.write_str("cannot allocate diagnostic evidence"),
            Self::EmptySourceIdentity => formatter.write_str("diagnostic source identity is empty"),
            Self::EmptyExternalFile => formatter.write_str("external diagnostic filename is empty"),
            Self::MissingSpan => formatter.write_str("diagnostic label has no source span"),
            Self::ReversedSpan(span) => write!(formatter, "diagnostic span `{span}` is reversed"),
            Self::ReversedExternalByteRange => {
                formatter.write_str("external diagnostic byte range is reversed")
            }
            Self::ReversedExternalPosition => {
                formatter.write_str("external diagnostic line/column range is reversed")
            }
            Self::SpanOutsideSource { source, span } => write!(
                formatter,
                "diagnostic span `{span}` falls outside source `{source}`"
            ),
            Self::MultiplePrimaryLabels => {
                formatter.write_str("diagnostic report has more than one primary label")
            }
        }
    }
}

impl std::error::Error for Error {}
