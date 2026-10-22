//! One-shot native application preparation and execution.

use std::fmt;
use std::path::PathBuf;

use crate::compile;
use rawr::diagnostic;

/// Failure while preparing or running the native application.
#[derive(Debug)]
pub enum Error {
    /// Project discovery or static application checking failed.
    Compile(compile::Error),
    /// Native preparation or event-loop execution failed.
    Application {
        /// Canonical report shared with host/editor consumers.
        report: diagnostic::Report,
    },
    /// Live initial compilation, watching, or reload-session shutdown failed.
    Live(diagnostic::Report),
}

/// Discover, check, bind, and boot an application without opening a window.
pub fn prepare(path: Option<PathBuf>) -> Result<rawr_iced::application::Prepared, Error> {
    compile::validate(path)
        .map_err(Error::Compile)?
        .prepare()
        .map_err(Error::Compile)
}

/// Prepare and run one application in the native iced event loop.
pub fn execute(path: Option<PathBuf>) -> Result<(), Error> {
    execute_transform(
        path,
        #[cfg(feature = "import-baseline")]
        None,
    )
}

#[cfg(feature = "import-baseline")]
pub(crate) fn execute_with_profile(
    path: Option<PathBuf>,
    profile: Option<rawr_iced::profile::Fixture>,
) -> Result<(), Error> {
    execute_transform(path, profile)
}

fn execute_transform(
    path: Option<PathBuf>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<(), Error> {
    #[cfg(feature = "native-runner")]
    if let Some(target) = crate::import::declared(path.as_ref()).map_err(Error::Live)? {
        return crate::import::owner::run(
            &target,
            rawr_import::process::Lifecycle::Run,
            #[cfg(feature = "import-baseline")]
            profile,
        )
        .map_err(Error::Live);
    }
    #[cfg(feature = "import-baseline")]
    if profile.is_some() {
        return Err(Error::Live(profile_report()));
    }
    let launch = crate::watch::Launch::open(path).map_err(Error::Live)?;
    let (bootstrap, source, service, timing, session) = launch.into_parts();
    let previews = {
        let preparation = bootstrap.preparation();
        let catalog = preparation.catalog();
        catalog.application().is_some() && !catalog.previews().is_empty()
    };
    let native = if previews {
        // An app that declares previews runs under the preview footer.
        rawr_iced::control::Host::bootstrap_observed(bootstrap, source.clone(), timing)
            .map_err(Error::Live)?
            .into_run()
            .run_live(service)
    } else {
        rawr_iced::program::Host::bootstrap_observed(bootstrap, Some(source.clone()), timing)
            .map_err(|error| Error::Live(crate::watch::native_report(error, source.clone())))?
            .run(service)
    }
    .map_err(|error| Error::Application {
        report: crate::watch::native_report(error, source.clone()),
    });
    let shutdown = session
        .finish()
        .map_err(|error| Error::Live(crate::watch::native_report(error, source)));
    native.and(shutdown)
}

#[cfg(feature = "import-baseline")]
fn profile_report() -> diagnostic::Report {
    diagnostic::Report::error(
        diagnostic::code::FRAMEWORK_NATIVE,
        "private profile selection requires a declared Rust import",
    )
    .finish()
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(error) => error.fmt(formatter),
            Self::Application { report } => report.fmt(formatter),
            Self::Live(report) => report.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compile(error) => Some(error),
            Self::Application { .. } | Self::Live(_) => None,
        }
    }
}

impl Error {
    /// Return the canonical diagnostic report.
    #[must_use]
    pub const fn report(&self) -> &diagnostic::Report {
        match self {
            Self::Compile(error) => error.report(),
            Self::Application { report } => report,
            Self::Live(report) => report,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn run_preparation_boots_the_exact_counter_without_opening_a_window() {
        let source = temporary_source();
        fs::write(
            &source,
            include_str!("../../iced/examples/application_counter.rawr"),
        )
        .unwrap();

        let prepared = super::prepare(Some(source.clone())).unwrap();

        assert!(prepared.adapter().last_good_output().is_some());
        assert!(prepared.adapter().diagnostics().is_empty());
        fs::remove_file(source).unwrap();
    }

    fn temporary_source() -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rawr-cli-run-{}-{id}.rawr", std::process::id()))
    }
}
