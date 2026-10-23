//! Shared compilation and native-launch preparation for Rawr iced applications.

use std::fmt;

use rawr::{application, diagnostic, host, program, project, timing, Engine, ParseError, Shared};

use crate::program::{Adapter, Application};

/// A compiler with the exact static iced facade installed.
pub struct Compiler {
    engine: Shared<Engine>,
}

/// A source target promoted through the complete application contract.
pub struct Checked {
    engine: Shared<Engine>,
    descriptor: application::Descriptor,
    source: diagnostic::Source,
    timing: Option<timing::Session>,
}

/// A checked application bound to the exact native iced runtime facade.
pub struct Prepared {
    adapter: Adapter,
    source: diagnostic::Source,
}

/// One checked or rejected application paired with immutable timing evidence.
pub struct Observed<T> {
    outcome: Result<T, Error>,
    timing: timing::Snapshot,
}

/// Failure while checking, preparing, or launching a Rawr iced application.
#[derive(Debug)]
pub enum Error {
    /// Exact diagnostic source evidence could not be retained.
    Diagnostic(diagnostic::Error),
    /// The static iced facade could not be built or installed.
    StaticFacade(host::Error),
    /// Source did not compile to a valid iced application descriptor.
    Compile(ParseError),
    /// A static package graph did not compile to a valid iced application descriptor.
    Package(rawr::package::Error),
    /// The validated descriptor did not belong to this compiler Engine.
    Launch(program::Error),
    /// The native iced runtime facade could not be built or installed.
    RuntimeFacade(host::Error),
    /// Exact native bindings did not satisfy the application launch.
    RuntimeBinding(host::runtime::Error),
    /// The bound application could not boot its state and initial view.
    Boot(crate::program::Error),
    /// The native iced event loop failed.
    Native(iced::Error),
}

impl Compiler {
    /// Build a compiler with the canonical static iced facade installed.
    pub fn new() -> Result<Self, Error> {
        let mut engine = Engine::new();
        crate::install(&mut engine).map_err(Error::StaticFacade)?;
        Ok(Self {
            engine: Shared::new(engine),
        })
    }

    /// Build a compiler whose Engine `configure` limits (rawr-play's
    /// operation, size and time budgets).
    pub fn configured(configure: impl FnOnce(&mut Engine)) -> Result<Self, Error> {
        let mut engine = Engine::new();
        crate::install(&mut engine).map_err(Error::StaticFacade)?;
        configure(&mut engine);
        Ok(Self {
            engine: Shared::new(engine),
        })
    }

    /// Check a discovered target through the complete application contract.
    pub fn check(&self, target: &project::Target) -> Result<Checked, Error> {
        let source = diagnostic::Source::new(
            target.source().path().to_string_lossy().into_owned(),
            target.source().text(),
        )
        .map_err(Error::Diagnostic)?;
        let descriptor = self
            .engine
            .compile_package_application_descriptor(target)
            .map_err(Error::Package)?;
        Ok(Checked {
            engine: self.engine.clone(),
            descriptor,
            source,
            timing: None,
        })
    }

    /// Check a discovered target and retain timing evidence on either outcome.
    #[must_use = "the observed outcome must be handled"]
    pub fn check_observed(&self, target: &project::Target) -> Observed<Checked> {
        let timing = timing::Session::start();
        let outcome = diagnostic::Source::new(
            target.source().path().to_string_lossy().into_owned(),
            target.source().text(),
        )
        .map_err(Error::Diagnostic)
        .and_then(|source| {
            self.engine
                .compile_package_application_descriptor_observed(target, &timing)
                .map_err(Error::Package)
                .map(|descriptor| Checked {
                    engine: self.engine.clone(),
                    descriptor,
                    source,
                    timing: Some(timing.clone()),
                })
        });
        Observed {
            outcome,
            timing: timing.snapshot(),
        }
    }

    /// [`Self::check_observed`] of a graph already loaded under `timing`:
    /// the host's layer decision and this check share that one parse.
    #[must_use = "the observed outcome must be handled"]
    pub fn check_loaded_observed(
        &self,
        target: &project::Target,
        graph: rawr::package::Graph,
        timing: timing::Session,
    ) -> Observed<Checked> {
        let outcome = diagnostic::Source::new(
            target.source().path().to_string_lossy().into_owned(),
            target.source().text(),
        )
        .map_err(Error::Diagnostic)
        .and_then(|source| {
            self.engine
                .compile_loaded_package_application_descriptor(graph, Some(&timing))
                .map_err(Error::Package)
                .map(|descriptor| Checked {
                    engine: self.engine.clone(),
                    descriptor,
                    source,
                    timing: Some(timing.clone()),
                })
        });
        Observed {
            outcome,
            timing: timing.snapshot(),
        }
    }

    /// Check in-memory source through the same complete application contract.
    pub fn check_source(&self, source: impl AsRef<str>) -> Result<Checked, Error> {
        self.check_named_source("<memory>", source)
    }

    /// Check in-memory source and retain timing evidence on either outcome.
    #[must_use = "the observed outcome and timing evidence must be handled"]
    pub fn check_source_observed(&self, source: impl AsRef<str>) -> Observed<Checked> {
        self.check_named_source_observed("<memory>", source)
    }

    /// Check named in-memory source while retaining exact diagnostic identity.
    pub fn check_named_source(
        &self,
        identity: impl Into<rawr::ImmutableString>,
        source: impl AsRef<str>,
    ) -> Result<Checked, Error> {
        let source =
            diagnostic::Source::new(identity, source.as_ref()).map_err(Error::Diagnostic)?;
        let descriptor = self
            .engine
            .compile_application_descriptor_source(source.clone())
            .map_err(Error::Compile)?;
        Ok(Checked {
            engine: self.engine.clone(),
            descriptor,
            source,
            timing: None,
        })
    }

    /// Check named in-memory source as the root file of a crate, so its
    /// inline `mod` items are modules as in a project on disk.
    pub fn check_named_crate_source(
        &self,
        identity: impl Into<rawr::ImmutableString>,
        source: impl AsRef<str>,
    ) -> Result<Checked, Error> {
        let identity = identity.into();
        let source =
            diagnostic::Source::new(identity.clone(), source.as_ref()).map_err(Error::Diagnostic)?;
        let path = std::path::Path::new("/").join(identity.as_str());
        let snapshot = rawr::package::snapshot::Snapshot::new(
            path.clone(),
            vec![(path, source.text().into())],
        )
        .expect("one absolute file is a valid snapshot");
        let descriptor = rawr::package::Graph::load_snapshot(&self.engine, &snapshot)
            .and_then(|graph| {
                self.engine
                    .compile_loaded_package_application_descriptor(graph, None)
            })
            .map_err(Error::Package)?;
        Ok(Checked {
            engine: self.engine.clone(),
            descriptor,
            source,
            timing: None,
        })
    }

    /// Check named source and retain timing evidence on either outcome.
    #[must_use]
    pub fn check_named_source_observed(
        &self,
        identity: impl Into<rawr::ImmutableString>,
        source: impl AsRef<str>,
    ) -> Observed<Checked> {
        let timing = timing::Session::start();
        let outcome = diagnostic::Source::new(identity, source.as_ref())
            .map_err(Error::Diagnostic)
            .and_then(|source| {
                self.engine
                    .compile_application_descriptor_source_observed(source.clone(), &timing)
                    .map_err(Error::Compile)
                    .map(|descriptor| Checked {
                        engine: self.engine.clone(),
                        descriptor,
                        source,
                        timing: Some(timing.clone()),
                    })
            });
        Observed {
            outcome,
            timing: timing.snapshot(),
        }
    }
}

impl Checked {
    /// Return the exact compiler-normalized application descriptor.
    #[must_use]
    pub const fn descriptor(&self) -> &application::Descriptor {
        &self.descriptor
    }

    /// Return the exact source snapshot retained for later diagnostics.
    #[must_use]
    pub const fn source(&self) -> &diagnostic::Source {
        &self.source
    }

    /// Clone the immutable timing observations recorded so far.
    #[must_use]
    pub fn timing(&self) -> timing::Snapshot {
        self.timing
            .as_ref()
            .map_or_else(timing::Snapshot::default, timing::Session::snapshot)
    }

    /// Bind this checked application to the canonical native iced runtime.
    pub fn prepare(self) -> Result<Prepared, Error> {
        let source = self.source;
        let launch = self
            .engine
            .launch_application(self.descriptor)
            .map_err(Error::Launch)?;
        let mut registry = host::runtime::Registry::new();
        for facade in crate::runtime_facades().map_err(Error::RuntimeFacade)? {
            registry.install(facade).map_err(Error::RuntimeBinding)?;
        }
        let launch = launch.bind(registry).map_err(Error::RuntimeBinding)?;
        let adapter =
            Adapter::boot_with_source_and_timing(launch, Some(source.clone()), self.timing)
                .map_err(Error::Boot)?;
        Ok(Prepared { adapter, source })
    }
}

impl Prepared {
    /// Return the booted adapter without entering the native event loop.
    #[must_use]
    pub const fn adapter(&self) -> &Adapter {
        &self.adapter
    }

    /// Consume preparation into the booted adapter for headless experiments.
    #[must_use]
    pub fn into_adapter(self) -> Adapter {
        self.adapter
    }

    /// Return the exact source snapshot retained for runtime diagnostics.
    #[must_use]
    pub const fn source(&self) -> &diagnostic::Source {
        &self.source
    }

    /// Clone the immutable compiler and runtime timing observations so far.
    #[must_use]
    pub fn timing(&self) -> timing::Snapshot {
        self.adapter.timing()
    }

    /// Open the native iced window and run until it closes.
    pub fn run(self) -> Result<(), Error> {
        Application::from_adapter(self.adapter)
            .run()
            .map_err(Error::Native)
    }
}

impl<T> Observed<T> {
    /// Borrow the successful value or failure without discarding timing.
    #[must_use = "the observed outcome must be handled"]
    pub fn outcome(&self) -> Result<&T, &Error> {
        self.outcome.as_ref()
    }

    /// Return the immutable timing evidence recorded before the outcome.
    #[must_use]
    pub const fn timing(&self) -> &timing::Snapshot {
        &self.timing
    }

    /// Consume this observation into the existing result-shaped API.
    pub fn into_result(self) -> Result<T, Error> {
        self.outcome
    }

    /// Consume this observation into its sibling outcome and timing values.
    #[must_use = "the observed outcome and timing evidence must be handled"]
    pub fn into_parts(self) -> (Result<T, Error>, timing::Snapshot) {
        (self.outcome, self.timing)
    }
}

impl Error {
    /// Transform this framework failure into the canonical shared report.
    #[must_use]
    pub fn report(&self, source: Option<diagnostic::Source>) -> diagnostic::Report {
        let fallback_source = source.clone();
        let report = match self {
            Self::Compile(error) => source
                .ok_or(diagnostic::Error::EmptySourceIdentity)
                .and_then(|source| diagnostic::Report::from_parse(source, error)),
            Self::Package(error) => error.report(),
            Self::Launch(error) => diagnostic::Report::from_program(source, error),
            Self::Boot(error) => Ok(error.report(source)),
            Self::StaticFacade(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_FACADE,
                format!("cannot install the static iced facade: {error}"),
                source,
            )),
            Self::RuntimeFacade(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_FACADE,
                format!("cannot build the native iced facade: {error}"),
                source,
            )),
            Self::RuntimeBinding(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                format!("cannot bind the native iced facade: {error}"),
                source,
            )),
            Self::Native(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_NATIVE,
                format!("native iced runtime failed: {error}"),
                source,
            )),
            Self::Diagnostic(error) => Ok(report_with_source(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                error.to_string(),
                source,
            )),
        };
        report.unwrap_or_else(|error| {
            let builder = diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                format!("cannot retain exact diagnostic evidence: {error}"),
            );
            match fallback_source {
                Some(source) => builder.source(source).finish(),
                None => builder.finish(),
            }
        })
    }
}

fn report_with_source(
    code: diagnostic::code::Code,
    message: impl Into<rawr::ImmutableString>,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let builder = diagnostic::Report::error(code, message);
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic(error) => error.fmt(formatter),
            Self::StaticFacade(error) => {
                write!(formatter, "cannot install the static iced facade: {error}")
            }
            Self::Compile(error) => error.fmt(formatter),
            Self::Package(error) => error.fmt(formatter),
            Self::Launch(error) => write!(
                formatter,
                "cannot authorize the checked application: {error}"
            ),
            Self::RuntimeFacade(error) => {
                write!(formatter, "cannot build the native iced facade: {error}")
            }
            Self::RuntimeBinding(error) => {
                write!(formatter, "cannot bind the native iced facade: {error}")
            }
            Self::Boot(error) => write!(formatter, "cannot boot the checked application: {error}"),
            Self::Native(error) => write!(formatter, "native iced runtime failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Diagnostic(error) => Some(error),
            Self::StaticFacade(error) | Self::RuntimeFacade(error) => Some(error),
            Self::Compile(error) => Some(error),
            Self::Package(error) => Some(error),
            Self::Launch(error) => Some(error),
            Self::RuntimeBinding(error) => Some(error),
            Self::Boot(error) => Some(error),
            Self::Native(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use rawr::timing::{Outcome, Phase};

    use super::Compiler;

    #[test]
    fn checked_counter_prepares_without_opening_a_window() {
        let checked = Compiler::new()
            .unwrap()
            .check_source_observed(include_str!("../examples/scripts/application_counter.rs"))
            .into_result()
            .unwrap();
        let prepared = checked.prepare().unwrap();

        assert!(prepared.adapter().last_good_output().is_some());
        assert!(prepared.adapter().diagnostics().is_empty());
        for phase in [
            Phase::ParseItems,
            Phase::Resolve,
            Phase::Lower,
            Phase::ApplicationContract,
            Phase::ViewEvaluation,
        ] {
            assert_eq!(prepared.timing().aggregate(phase).count(), 1);
        }
        assert_eq!(
            prepared
                .timing()
                .aggregate(Phase::DescriptorLowering)
                .count(),
            0
        );
    }

    #[test]
    fn ordinary_check_and_view_paths_do_not_start_an_observation_session() {
        let checked = Compiler::new()
            .unwrap()
            .check_source(include_str!("../examples/scripts/application_counter.rs"))
            .unwrap();
        assert!(checked.timing().attempts().is_empty());

        let prepared = checked.prepare().unwrap();
        drop(prepared.adapter().view());
        assert!(prepared.timing().attempts().is_empty());
    }

    #[test]
    fn observed_compile_records_the_single_pipeline_in_semantic_order() {
        let observed = Compiler::new()
            .unwrap()
            .check_source_observed(include_str!("../examples/scripts/application_counter.rs"));

        assert!(observed.outcome().is_ok());
        let attempts = observed.timing().attempts();
        assert_eq!(attempts.len(), 4);
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| (attempt.phase(), attempt.outcome()))
                .collect::<Vec<_>>(),
            vec![
                (Phase::ParseItems, Outcome::Success),
                (Phase::Resolve, Outcome::Success),
                (Phase::Lower, Outcome::Success),
                (Phase::ApplicationContract, Outcome::Success),
            ]
        );
        for phase in [
            Phase::ParseItems,
            Phase::Resolve,
            Phase::Lower,
            Phase::ApplicationContract,
        ] {
            assert_eq!(observed.timing().aggregate(phase).count(), 1);
        }
    }

    #[test]
    fn failed_compile_retains_only_started_phases_and_the_exact_failure() {
        let compiler = Compiler::new().unwrap();
        let parse_failure = compiler.check_source_observed("fn");
        assert!(parse_failure.outcome().is_err());
        assert_eq!(parse_failure.timing().attempts().len(), 1);
        assert_eq!(
            parse_failure.timing().attempts()[0].phase(),
            Phase::ParseItems
        );
        assert_eq!(
            parse_failure.timing().attempts()[0].outcome(),
            Outcome::Failure
        );

        let resolve_failure =
            compiler.check_source_observed("struct Duplicate {}\nstruct Duplicate {}");
        assert!(resolve_failure.outcome().is_err());
        assert_eq!(
            resolve_failure
                .timing()
                .attempts()
                .iter()
                .map(|attempt| (attempt.phase(), attempt.outcome()))
                .collect::<Vec<_>>(),
            vec![
                (Phase::ParseItems, Outcome::Success),
                (Phase::Resolve, Outcome::Failure),
            ]
        );

        let lower_failure =
            compiler.check_source_observed("pub fn pending() { if true { 40 + 2 } }");
        assert!(lower_failure.outcome().is_err());
        assert_eq!(
            lower_failure
                .timing()
                .attempts()
                .iter()
                .map(|attempt| (attempt.phase(), attempt.outcome()))
                .collect::<Vec<_>>(),
            vec![
                (Phase::ParseItems, Outcome::Success),
                (Phase::Resolve, Outcome::Success),
                (Phase::Lower, Outcome::Failure),
            ]
        );

        let contract_failure = compiler.check_source_observed("40 + 2");
        assert!(contract_failure.outcome().is_err());
        assert_eq!(
            contract_failure
                .timing()
                .attempts()
                .iter()
                .map(|attempt| (attempt.phase(), attempt.outcome()))
                .collect::<Vec<_>>(),
            vec![
                (Phase::ParseItems, Outcome::Success),
                (Phase::Resolve, Outcome::Success),
                (Phase::Lower, Outcome::Success),
                (Phase::ApplicationContract, Outcome::Failure),
            ]
        );
    }
}
