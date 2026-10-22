//! Deterministic snapshot command over one exact bound preview Catalog.

use std::fmt;
use std::path::PathBuf;

use rawr::{diagnostic, project};

use crate::command;

/// Failure while discovering, proving, rendering, or storing snapshots.
#[derive(Debug)]
pub struct Error {
    report: diagnostic::Report,
}

/// Discover, compile, bind, and execute one complete snapshot generation.
pub fn execute(
    mode: command::Mode,
    path: Option<PathBuf>,
) -> Result<rawr_iced::snapshot::Outcome, Error> {
    execute_transform(
        mode,
        path,
        #[cfg(feature = "import-baseline")]
        None,
    )
}

#[cfg(feature = "import-baseline")]
pub(crate) fn execute_with_profile(
    mode: command::Mode,
    path: Option<PathBuf>,
    profile: Option<rawr_iced::profile::Fixture>,
) -> Result<rawr_iced::snapshot::Outcome, Error> {
    execute_transform(mode, path, profile)
}

fn execute_transform(
    mode: command::Mode,
    path: Option<PathBuf>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<rawr_iced::snapshot::Outcome, Error> {
    #[cfg(feature = "native-runner")]
    if let Some(target) = crate::import::declared(path.as_ref()).map_err(Error::from_report)? {
        return crate::import::owner::snapshot(
            &target,
            mode,
            #[cfg(feature = "import-baseline")]
            profile,
        )
        .map_err(Error::from_report);
    }
    #[cfg(feature = "import-baseline")]
    if profile.is_some() {
        return Err(Error::from_report(
            diagnostic::Report::error(
                diagnostic::code::FRAMEWORK_NATIVE,
                "private profile selection requires a declared Rust import",
            )
            .finish(),
        ));
    }
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| Error {
        report: diagnostic::Report::from_project(&error),
    })?;
    let source = diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| Error {
        report: diagnostic::Report::error(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            error.to_string(),
        )
        .finish(),
    })?;
    let compiler = rawr_iced::preview::Compiler::new().map_err(|error| Error {
        report: error.report(Some(source.clone())),
    })?;
    let checked = compiler.check_observed(&target).map_err(|error| Error {
        report: error.report(Some(source.clone())),
    })?;
    let suite = checked
        .snapshots()
        .map_err(|error| from_snapshot(error, &source))?;
    suite
        .execute(match mode {
            command::Mode::Check => rawr_iced::snapshot::Mode::Check,
            command::Mode::Update => rawr_iced::snapshot::Mode::Update,
        })
        .map_err(|error| from_snapshot(error, &source))
}

impl Error {
    pub(crate) const fn from_report(report: diagnostic::Report) -> Self {
        Self { report }
    }

    /// Return the canonical diagnostic report.
    #[must_use]
    pub const fn report(&self) -> &diagnostic::Report {
        &self.report
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.report.fmt(formatter)
    }
}

impl std::error::Error for Error {}

fn from_snapshot(error: rawr_iced::snapshot::Error, source: &diagnostic::Source) -> Error {
    let report = match error {
        rawr_iced::snapshot::Error::Preview(error) => error.report(Some(source.clone())),
        error => diagnostic::Report::error(diagnostic::code::FRAMEWORK_SNAPSHOT, error.to_string())
            .source(source.clone())
            .finish(),
    };
    Error { report }
}
