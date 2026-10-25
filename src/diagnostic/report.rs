//! Validated diagnostic proof bundles.

use crate::{ImmutableString, Span};
use std::fmt;

use super::{code::Code, Source};

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
        Builder::new(code, Severity::Error, message.into())
    }

    /// Start one warning report using the category fixed by its stable code.
    #[must_use]
    pub fn warning(code: Code, message: impl Into<ImmutableString>) -> Builder {
        Builder::new(code, Severity::Warning, message.into())
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
            message: message.into(),
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
        if kind == LabelKind::Primary {
            if self.has_primary {
                return Err(Error::MultiplePrimaryLabels);
            }
            self.has_primary = true;
        }
        self = self.source(source.clone());
        self.report.labels.push(Label {
            source,
            span,
            kind,
            message,
        });
        Ok(self)
    }

    /// Attach actionable help.
    #[must_use]
    pub fn help(mut self, help: impl Into<ImmutableString>) -> Self {
        self.report.help = Some(help.into());
        self
    }

    /// Attach a supplementary canonical note.
    #[must_use]
    pub fn note(mut self, note: impl Into<ImmutableString>) -> Self {
        self.report.notes.push(note.into());
        self
    }

    /// Finish the validated proof bundle.
    #[must_use]
    pub fn finish(self) -> Report {
        self.report
    }
}

impl Label {
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
            "{}[{}]: {}",
            self.severity, self.code, self.message
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
