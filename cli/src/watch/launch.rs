//! Shared initial compilation and live-session launch for native hosts.

use std::path::PathBuf;
use std::time::Duration;

use rawr::{diagnostic, project};

use super::{Adapter, Session};

const DEBOUNCE: Duration = Duration::from_millis(30);

/// One initial generation paired with its running reload transport.
pub(crate) struct Launch {
    bootstrap: rawr::reload::installation::Bootstrap,
    source: diagnostic::Source,
    service: rawr_iced::reload::Service,
    timing: rawr::timing::Session,
    session: Session,
}

impl Launch {
    /// Compile, bind, watch, and start one exact project target.
    pub(crate) fn open(path: Option<PathBuf>) -> Result<Self, diagnostic::Report> {
        let target = match path {
            Some(path) => project::Target::discover(path),
            None => project::Target::current(),
        }
        .map_err(|error| diagnostic::Report::from_project(&error))?;
        let target_path = target_path(&target);
        let fallback = diagnostic::Source::new(
            target.source().path().to_string_lossy().into_owned(),
            target.source().text(),
        )
        .map_err(|error| report(diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error, None))?;

        let mut engine = rawr::Engine::new();
        rawr_iced::install(&mut engine).map_err(|error| {
            report(
                diagnostic::code::FRAMEWORK_FACADE,
                error,
                Some(fallback.clone()),
            )
        })?;
        let engine = rawr::Shared::new(engine);
        let timing = rawr::timing::Session::start();
        let candidate = engine
            .compile_package_generation_observed(&target, &timing)
            .map_err(|error| package_report(&error, fallback.clone()))?;
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let source = candidate.source().entry_source().diagnostic().clone();
        let adapter = Adapter::open(candidate.source(), DEBOUNCE).map_err(|error| {
            report(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                error,
                Some(source.clone()),
            )
        })?;
        snapshot.revalidate().map_err(|error| {
            report(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                error,
                Some(source.clone()),
            )
        })?;

        let mut runtime = rawr::host::runtime::Registry::new();
        let facades = rawr_iced::runtime_facades().map_err(|error| {
            report(
                diagnostic::code::FRAMEWORK_FACADE,
                error,
                Some(source.clone()),
            )
        })?;
        for facade in facades {
            runtime.install(facade).map_err(|error| {
                report(
                    diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                    error,
                    Some(source.clone()),
                )
            })?;
        }
        let bootstrap = candidate.bind(&engine, runtime).map_err(|error| {
            report(
                diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                error,
                Some(source.clone()),
            )
        })?;
        let (transport, service) = rawr_iced::reload::channel_observed(
            adapter.gate(),
            adapter.publication(),
            engine.clone(),
            timing.clone(),
        );
        let session = Session::start(target_path, engine, adapter, transport, timing.clone())
            .map_err(|error| {
                report(
                    diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                    error,
                    Some(source.clone()),
                )
            })?;

        Ok(Self {
            bootstrap,
            source,
            service,
            timing,
            session,
        })
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        rawr::reload::installation::Bootstrap,
        diagnostic::Source,
        rawr_iced::reload::Service,
        rawr::timing::Session,
        Session,
    ) {
        (
            self.bootstrap,
            self.source,
            self.service,
            self.timing,
            self.session,
        )
    }
}

pub(crate) fn native_report(
    message: impl std::fmt::Display,
    source: diagnostic::Source,
) -> diagnostic::Report {
    report(diagnostic::code::FRAMEWORK_NATIVE, message, Some(source))
}

fn target_path(target: &project::Target) -> PathBuf {
    match target {
        project::Target::File(source) => source.path().to_path_buf(),
        project::Target::Project(project) => project.root().as_path().to_path_buf(),
    }
}

pub(crate) fn package_report(
    error: &rawr::package::Error,
    fallback: diagnostic::Source,
) -> diagnostic::Report {
    error.report().unwrap_or_else(|report_error| {
        report(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            format!("cannot retain exact package diagnostic evidence: {report_error}"),
            Some(fallback),
        )
    })
}

pub(crate) fn report(
    code: diagnostic::code::Code,
    message: impl std::fmt::Display,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let builder = diagnostic::Report::error(code, message.to_string());
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::Launch;

    #[test]
    fn cli_launch_owns_and_joins_a_reload_session_without_opening_a_window() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cli-live-launch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/main.rawr"),
            r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("live") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
        )
        .unwrap();

        let launch = Launch::open(Some(root.clone())).unwrap();
        let (bootstrap, source, service, timing, session) = launch.into_parts();
        let host =
            rawr_iced::program::Host::bootstrap_observed(bootstrap, Some(source), timing).unwrap();
        assert!(host.adapter().last_good_output().is_some());
        drop(service);
        session.finish().unwrap();
        fs::remove_dir_all(root).ok();
    }
}
