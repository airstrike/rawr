//! Integrated native app/preview Catalog preparation and execution.

use std::fmt;
use std::path::{Path, PathBuf};

use rawr::{diagnostic, project};

/// Failure while discovering, preparing, or running the preview host.
#[derive(Debug)]
pub enum Error {
    /// Canonical diagnostic shared with compiler, host, and editor consumers.
    Diagnostic(diagnostic::Report),
}

/// Discover, check, bind, and boot a preview Catalog without opening a window.
pub fn prepare(path: Option<PathBuf>) -> Result<rawr_iced::preview::Prepared, Error> {
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| Error::Diagnostic(diagnostic::Report::from_project(&error)))?;
    let source = diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| {
        Error::Diagnostic(
            diagnostic::Report::error(diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error.to_string())
                .finish(),
        )
    })?;
    let compiler = rawr_iced::preview::Compiler::new()
        .map_err(|error| Error::Diagnostic(error.report(Some(source.clone()))))?;
    compiler
        .check_observed(&target)
        .and_then(rawr_iced::preview::Checked::prepare)
        .map_err(|error| Error::Diagnostic(error.report(Some(source))))
}

/// Resolve `rawr preview [path] [name]` and remember `name` for the host.
///
/// A sole argument that is not an existing path (nor a `.rs` file or
/// `Cargo.toml`) names a preview, `name` or `group/name`, in the current
/// project. Returns the project path to open.
pub fn select(path: Option<PathBuf>, name: Option<String>) -> Result<Option<PathBuf>, Error> {
    let (path, name) = split(path, name);
    if let Some(name) = name {
        let root = root(path.as_ref())?;
        rawr_iced::control::remember(&root, &name).map_err(|error| {
            Error::Diagnostic(
                diagnostic::Report::error(
                    diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                    format!(
                        "cannot remember preview `{name}` in {}: {error}",
                        root.display()
                    ),
                )
                .finish(),
            )
        })?;
    }
    Ok(path)
}

fn split(path: Option<PathBuf>, name: Option<String>) -> (Option<PathBuf>, Option<String>) {
    match (path, name) {
        (Some(path), None)
            if !path.exists()
                && path
                    .extension()
                    .is_none_or(|extension| extension != "rs" && extension != "rawr")
                && path
                    .file_name()
                    .is_none_or(|name| name != "Cargo.toml" && name != "rawr.toml") =>
        {
            (None, Some(path.to_string_lossy().into_owned()))
        }
        selection => selection,
    }
}

/// The project root `rawr preview` remembers its preview under.
pub(crate) fn root(path: Option<&PathBuf>) -> Result<PathBuf, Error> {
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| Error::Diagnostic(diagnostic::Report::from_project(&error)))?;
    Ok(target
        .project()
        .map(|project| project.root().as_path().to_path_buf())
        .or_else(|| target.source().path().parent().map(Path::to_path_buf))
        .unwrap_or_default())
}

/// Prepare and run one integrated native preview selector.
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
    let loaded = crate::base::load(path.as_ref()).map_err(Error::Diagnostic)?;
    #[cfg(feature = "native-runner")]
    if loaded.graph.declares_rust_import() {
        return crate::import::owner::run(
            &loaded.target,
            Some(loaded.graph),
            rawr_import::process::Lifecycle::Preview,
            #[cfg(feature = "import-baseline")]
            profile,
        )
        .map(|_| ())
        .map_err(Error::Diagnostic);
    }
    #[cfg(feature = "import-baseline")]
    if profile.is_some() {
        return Err(Error::Diagnostic(
            diagnostic::Report::error(
                diagnostic::code::FRAMEWORK_NATIVE,
                "private profile selection requires a declared Rust import",
            )
            .finish(),
        ));
    }
    let root = root(path.as_ref())?;
    let launch = crate::watch::Launch::open(loaded).map_err(Error::Diagnostic)?;
    let (bootstrap, source, service, timing, session) = launch.into_parts();
    let control = rawr_iced::control::Host::bootstrap_observed(bootstrap, source.clone(), timing)
        .map_err(Error::Diagnostic)?
        .into_preview(&root);
    let native = control
        .run_live(service)
        .map_err(|error| Error::Diagnostic(crate::watch::native_report(error, source.clone())));
    let shutdown = session
        .finish()
        .map_err(|error| Error::Diagnostic(crate::watch::native_report(error, source)));
    native.and(shutdown)
}

impl Error {
    /// Return the canonical diagnostic report.
    #[must_use]
    pub const fn report(&self) -> &diagnostic::Report {
        match self {
            Self::Diagnostic(report) => report,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.report().fmt(formatter)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn preview_preparation_binds_and_boots_without_opening_a_window() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../iced/examples/preview_metadata.rawr");
        let source = std::fs::canonicalize(source).unwrap();
        assert_eq!(
            super::split(Some(PathBuf::from("kit/dial")), None),
            (None, Some("kit/dial".to_owned())),
            "a missing path names a preview"
        );
        assert_eq!(
            super::split(Some(source.clone()), None),
            (Some(source.clone()), None)
        );
        assert!(super::split(Some(PathBuf::from("gone.rawr")), None)
            .1
            .is_none());

        let prepared = super::prepare(Some(source.clone())).unwrap();

        assert_eq!(prepared.source().identity(), source.to_string_lossy());
    }
}
