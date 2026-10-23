//! Source-qualified failures from static package graph construction.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::{diagnostic, ImmutableString, ParseError, Span};

/// Stable package-compilation failure family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    /// A Rawr source failed to parse.
    Parse,
    /// The crate's `Cargo.toml` is malformed or incomplete.
    Manifest,
    /// A canonical package or source resource could not be loaded.
    Resource,
    /// A declared file-module graph is invalid.
    Module,
    /// Static names or item headers failed aggregate semantic resolution.
    Resolution,
    /// A resolved package graph failed body lowering or application checking.
    Compilation,
}

/// A typed package-graph failure retaining every exact source label.
#[derive(Debug)]
pub struct Error {
    sources: Box<[diagnostic::Source]>,
    kind: Kind,
    /// The Rust path a failure missed, and whether it lies below a wholesale
    /// crate re-export. An import demanding it may resolve it.
    rust_demand: Option<(Box<str>, bool)>,
}

#[derive(Debug)]
enum Kind {
    Parse {
        source: usize,
        error: ParseError,
    },
    Manifest {
        source: usize,
        message: ImmutableString,
        span: Span,
    },
    Resource {
        message: ImmutableString,
        source: io::Error,
        primary: Option<Label>,
    },
    Module {
        message: ImmutableString,
        primary: Label,
        secondary: Option<Label>,
    },
    Resolution {
        primary: usize,
        secondary: Option<usize>,
        error: crate::resolve::Error,
    },
    Compilation {
        evidence: crate::resolve::LoweringEvidence,
        error: ParseError,
    },
    #[cfg(feature = "rust-import")]
    Import(super::import::Error),
    #[cfg(feature = "rust-import")]
    ImportDiagnostic(diagnostic::Report),
    Diagnostic(diagnostic::Error),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Label {
    pub(crate) source: usize,
    pub(crate) span: Span,
}

impl Error {
    pub(crate) fn parse(
        sources: Vec<diagnostic::Source>,
        source: usize,
        error: ParseError,
    ) -> Self {
        Self {
            sources: sources.into_boxed_slice(),
            kind: Kind::Parse { source, error },
            rust_demand: None,
        }
    }

    pub(crate) fn manifest(
        sources: Vec<diagnostic::Source>,
        source: usize,
        message: ImmutableString,
        span: Span,
    ) -> Self {
        Self {
            sources: sources.into_boxed_slice(),
            kind: Kind::Manifest {
                source,
                message,
                span,
            },
            rust_demand: None,
        }
    }

    pub(crate) fn resource(
        sources: Vec<diagnostic::Source>,
        message: ImmutableString,
        source: io::Error,
        primary: Option<Label>,
    ) -> Self {
        Self {
            sources: sources.into_boxed_slice(),
            kind: Kind::Resource {
                message,
                source,
                primary,
            },
            rust_demand: None,
        }
    }

    pub(crate) fn module(
        sources: Vec<diagnostic::Source>,
        message: ImmutableString,
        primary: Label,
        secondary: Option<Label>,
    ) -> Self {
        Self {
            sources: sources.into_boxed_slice(),
            kind: Kind::Module {
                message,
                primary,
                secondary,
            },
            rust_demand: None,
        }
    }

    pub(crate) fn diagnostic(source: diagnostic::Error) -> Self {
        Self {
            sources: Box::default(),
            kind: Kind::Diagnostic(source),
            rust_demand: None,
        }
    }

    pub(crate) fn resolution(
        sources: Box<[diagnostic::Source]>,
        failure: crate::resolve::package::Failure,
    ) -> Self {
        Self {
            sources,
            kind: Kind::Resolution {
                primary: failure.primary.index(),
                secondary: failure.secondary.map(crate::package::source::Id::index),
                error: failure.error,
            },
            rust_demand: None,
        }
    }

    pub(crate) fn compilation(
        sources: Box<[diagnostic::Source]>,
        evidence: crate::resolve::LoweringEvidence,
        error: ParseError,
    ) -> Self {
        Self {
            sources,
            kind: Kind::Compilation { evidence, error },
            rust_demand: None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn import(error: super::import::Error) -> Self {
        Self {
            sources: Box::default(),
            kind: Kind::Import(error),
            rust_demand: None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn import_diagnostic(report: diagnostic::Report) -> Self {
        Self {
            sources: Box::default(),
            kind: Kind::ImportDiagnostic(report),
            rust_demand: None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn with_rust_demand(mut self, path: &str, through_alias: bool) -> Self {
        self.rust_demand = Some((path.into(), through_alias));
        self
    }

    /// The Rust path this failure missed below a wholesale crate
    /// re-export. Importing again with this path demanded may resolve it.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_demand(&self) -> Option<&str> {
        self.rust_demand
            .as_ref()
            .filter(|(_, through_alias)| *through_alias)
            .map(|(path, _)| &**path)
    }

    /// The Rust path below a dependency alias this failure
    /// missed (an unknown import, type or member). When that dependency was
    /// extracted from demand, importing again with the path demanded may
    /// resolve it; a dependency extracted whole has no more to give.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_miss(&self) -> Option<&str> {
        self.rust_demand.as_ref().map(|(path, _)| &**path)
    }

    /// Classify this failure without inspecting its display text.
    #[must_use]
    pub const fn category(&self) -> Category {
        match self.kind {
            Kind::Parse { .. } => Category::Parse,
            Kind::Manifest { .. } => Category::Manifest,
            Kind::Resource { .. } | Kind::Diagnostic(_) => Category::Resource,
            Kind::Module { .. } => Category::Module,
            Kind::Resolution { .. } => Category::Resolution,
            Kind::Compilation { .. } => Category::Compilation,
            #[cfg(feature = "rust-import")]
            Kind::Import(_) => Category::Resource,
            #[cfg(feature = "rust-import")]
            Kind::ImportDiagnostic(_) => Category::Resolution,
        }
    }

    /// Transform this semantic failure into the canonical report contract.
    pub fn report(&self) -> Result<diagnostic::Report, diagnostic::Error> {
        match &self.kind {
            Kind::Parse { source, error } => {
                diagnostic::Report::from_parse(self.sources[*source].clone(), error)
            }
            Kind::Manifest {
                source,
                message,
                span,
            } => Ok(diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_PACKAGE_MANIFEST,
                message.clone(),
            )
            .primary(self.sources[*source].clone(), *span, None)?
            .finish()),
            Kind::Resource {
                message, primary, ..
            } => {
                let builder = diagnostic::Report::error(
                    diagnostic::code::ENVIRONMENT_PACKAGE_RESOURCE,
                    message.clone(),
                );
                Ok(match primary {
                    Some(label) => builder
                        .primary(self.sources[label.source].clone(), label.span, None)?
                        .finish(),
                    None => builder.finish(),
                })
            }
            Kind::Module {
                message,
                primary,
                secondary,
            } => labels(
                diagnostic::Report::error(diagnostic::code::RESOLVE_MODULE_GRAPH, message.clone()),
                &self.sources,
                *primary,
                *secondary,
            ),
            Kind::Resolution {
                primary,
                secondary,
                error,
            } => diagnostic::Report::from_resolution(
                self.sources[*primary].clone(),
                secondary.map(|source| self.sources[source].clone()),
                error,
            ),
            Kind::Compilation { evidence, error } => match evidence {
                crate::resolve::LoweringEvidence::Original(source) => {
                    diagnostic::Report::from_parse(self.sources[source.index()].clone(), error)
                }
                crate::resolve::LoweringEvidence::Generated { source, span } => {
                    diagnostic::Report::from_parse_at(
                        self.sources[source.index()].clone(),
                        error,
                        *span,
                    )
                }
                crate::resolve::LoweringEvidence::Unmapped(reason) => Ok(
                    diagnostic::Report::from_parse_unpositioned(error, reason.clone()),
                ),
            },
            #[cfg(feature = "rust-import")]
            Kind::Import(error) => Ok(diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_PACKAGE_RESOURCE,
                error.to_string(),
            )
            .finish()),
            #[cfg(feature = "rust-import")]
            Kind::ImportDiagnostic(report) => Ok(report.clone()),
            Kind::Diagnostic(error) => Err(error.clone()),
        }
    }
}

fn labels(
    builder: diagnostic::Builder,
    sources: &[diagnostic::Source],
    primary: Label,
    secondary: Option<Label>,
) -> Result<diagnostic::Report, diagnostic::Error> {
    let mut builder = builder.primary(sources[primary.source].clone(), primary.span, None)?;
    if let Some(secondary) = secondary {
        builder = builder.secondary(
            sources[secondary.source].clone(),
            secondary.span,
            Some("related declaration is here".into()),
        )?;
    }
    Ok(builder.finish())
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            Kind::Parse { error, .. } => error.fmt(formatter),
            Kind::Manifest { message, .. }
            | Kind::Resource { message, .. }
            | Kind::Module { message, .. } => message.fmt(formatter),
            Kind::Resolution { error, .. } => error.fmt(formatter),
            Kind::Compilation { error, .. } => error.fmt(formatter),
            #[cfg(feature = "rust-import")]
            Kind::Import(error) => error.fmt(formatter),
            #[cfg(feature = "rust-import")]
            Kind::ImportDiagnostic(report) => report.fmt(formatter),
            Kind::Diagnostic(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            Kind::Parse { error, .. } => Some(error),
            Kind::Resource { source, .. } => Some(source),
            Kind::Diagnostic(error) => Some(error),
            Kind::Resolution { error, .. } => Some(error),
            Kind::Compilation { error, .. } => Some(error),
            #[cfg(feature = "rust-import")]
            Kind::Import(error) => Some(error),
            #[cfg(feature = "rust-import")]
            Kind::ImportDiagnostic(_) => None,
            Kind::Manifest { .. } | Kind::Module { .. } => None,
        }
    }
}

pub(crate) fn resource_message(action: &str, path: PathBuf, source: &io::Error) -> ImmutableString {
    format!("cannot {action} `{}`: {source}", path.display()).into()
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::diagnostic::Source;
    use crate::{ParseError, ParseErrorType, Position};

    #[test]
    fn unmapped_aggregate_failure_does_not_claim_the_entry_or_dependency_source() {
        let sources = vec![
            Source::new("app/src/main.rs", "fn main() {}\n")
                .expect("entry fixture has a stable identity"),
            Source::new("app/src/view.rs", "fn view() {}\n")
                .expect("dependency fixture has a stable identity"),
        ]
        .into_boxed_slice();
        let error = ParseError(
            Box::new(ParseErrorType::UnexpectedEOF),
            Position::new(19, 7),
        );
        let failure = Error::compilation(
            sources,
            crate::resolve::LoweringEvidence::Unmapped(
                "generated diagnostic has no origin proof".into(),
            ),
            error,
        );

        let report = failure
            .report()
            .expect("unmapped compiler failure remains a valid unpositioned report");

        assert!(report.sources().is_empty());
        assert!(report.labels().is_empty());
        assert_eq!(
            report.notes().collect::<Vec<_>>(),
            vec!["generated diagnostic has no origin proof"]
        );
    }
}
