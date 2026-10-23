//! Shared target discovery and application-contract compilation.

use std::fmt;
use std::path::{Path, PathBuf};

use rawr::diagnostic;

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
    validate_loaded(crate::base::load(path.as_ref()).map_err(Error::Diagnostic)?)
}

/// Check the command's one load through the shared application compiler.
pub(crate) fn validate_loaded(loaded: crate::base::Loaded) -> Result<Validated, Error> {
    let crate::base::Loaded {
        target,
        graph,
        timing,
        ..
    } = loaded;
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
        .check_loaded_observed(&target, graph, timing)
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

#[cfg(test)]
mod tests {
    use std::fs;

    use rawr::timing::Phase;

    #[test]
    fn an_iced_program_is_parsed_once_per_command() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cli-parse-once-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", include_str!("../../iced/examples/scripts/application_counter.rs"));
        let layered = || match crate::base::layer(Some(&root)) {
            Ok(crate::base::Program::Layered(loaded)) => loaded,
            _ => panic!("the counter brings in rawr-iced"),
        };
        let parses = |timing: rawr::timing::Snapshot| timing.aggregate(Phase::ParseItems).count();

        // `rawr check`: the decision's load is the one the check compiles.
        let validated = super::validate_loaded(layered()).unwrap();
        assert_eq!(parses(validated.checked.timing()), 1);

        // `rawr run`: and the one the first reload generation compiles.
        let launch = crate::watch::Launch::open(layered()).unwrap();
        let (bootstrap, _, service, timing, session) = launch.into_parts();
        assert_eq!(parses(timing.snapshot()), 1);
        drop((bootstrap, service));
        session.finish().unwrap();
        fs::remove_dir_all(root).ok();
    }
}
