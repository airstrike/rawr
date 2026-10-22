//! Shared target discovery and application-contract compilation.

use std::fmt;
use std::path::{Path, PathBuf};

use rawr::{diagnostic, project};

/// A discovered source checked through the complete iced application contract.
pub struct Validated {
    source: PathBuf,
    checked: rawr_iced::application::Checked,
}

/// Failure before native runtime preparation.
#[derive(Debug)]
pub enum Error {
    /// Canonical diagnostic describing the failed transformation.
    Diagnostic(diagnostic::Report),
}

impl Validated {
    /// Return the canonical source path checked by this proof bundle.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Bind this validated target to the canonical native iced runtime.
    pub fn prepare(self) -> Result<rawr_iced::application::Prepared, Error> {
        let source = self.checked.source().clone();
        self.checked
            .prepare()
            .map_err(|error| Error::Diagnostic(error.report(Some(source))))
    }
}

/// Discover and check a target through the shared application compiler.
pub fn validate(path: Option<PathBuf>) -> Result<Validated, Error> {
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| Error::Diagnostic(diagnostic::Report::from_project(&error)))?;
    let source = target.source().path().to_path_buf();
    let diagnostic_source = diagnostic::Source::new(
        source.to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| {
        Error::Diagnostic(
            diagnostic::Report::error(diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error.to_string())
                .finish(),
        )
    })?;
    let compiler = rawr_iced::application::Compiler::new()
        .map_err(|error| Error::Diagnostic(error.report(Some(diagnostic_source.clone()))))?;
    let checked = compiler
        .check_observed(&target)
        .into_result()
        .map_err(|error| Error::Diagnostic(error.report(Some(diagnostic_source))))?;
    Ok(Validated { source, checked })
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic(report) => report.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Diagnostic(..) => None,
        }
    }
}

impl Error {
    pub(crate) const fn from_report(report: diagnostic::Report) -> Self {
        Self::Diagnostic(report)
    }

    /// Return the canonical diagnostic report.
    #[must_use]
    pub const fn report(&self) -> &diagnostic::Report {
        match self {
            Self::Diagnostic(report) => report,
        }
    }
}
