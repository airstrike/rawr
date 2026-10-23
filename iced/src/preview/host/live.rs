//! Private iced message boundary for one reloadable preview host.

use std::sync::Mutex;

use iced::widget::Widget as _;
use rawr::reload::scheduler::Admission;

use crate::reload;

use super::{Host, Message};

pub(super) struct Application {
    initial: Mutex<Option<State>>,
    centered: bool,
}

struct State {
    host: Host,
    service: reload::Service,
    status: reload::status::State,
    agent: reload::agent::Session,
}

#[derive(Clone)]
enum LiveMessage {
    Host(Message),
    Reload(reload::Wake),
    ReloadControl(reload::status::Control),
    Cargo(reload::cargo::Checked),
    Saved(crate::screenshot::Saved),
    Export(reload::export::Request),
    Agent(reload::agent::Call),
    AgentShot(reload::agent::Call, crate::screenshot::Saved),
}

// iced's `debug` feature requires `Debug` of an application's message.
impl std::fmt::Debug for LiveMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Host(..) => "Host",
            Self::Reload(..) => "Reload",
            Self::ReloadControl(..) => "ReloadControl",
            Self::Cargo(..) => "Cargo",
            Self::Saved(..) => "Saved",
            Self::Export(..) => "Export",
            Self::Agent(..) => "Agent",
            Self::AgentShot(..) => "AgentShot",
        })
    }
}

const AGENT: reload::agent::Live<State, LiveMessage> = reload::agent::Live {
    session: |state| &mut state.agent,
    reset: |state| state.control(reload::status::Control::Reset),
    wait: |state, call| state.status.wait(call),
    next: LiveMessage::Agent,
    shot: LiveMessage::AgentShot,
};

impl Application {
    fn new(host: Host, service: reload::Service) -> Self {
        let centered = host.centered();
        Self {
            centered,
            initial: Mutex::new(Some(State {
                host,
                service,
                status: reload::status::State::new(),
                agent: reload::agent::Session::default(),
            })),
        }
    }

    fn run(self) -> iced::Result {
        let centered = self.centered;
        let application = iced::application(
            move || self.boot_native(),
            Self::update_native,
            Self::view_native,
        )
        .title(Self::title_native)
        .theme(Self::theme_native)
        .subscription(Self::subscription_native);
        if centered {
            application.centered().run()
        } else {
            application.run()
        }
    }

    fn title_native(state: &State) -> String {
        state.host.title()
    }

    fn boot_native(&self) -> (State, iced::Task<LiveMessage>) {
        let mut initial = match self.initial.lock() {
            Ok(initial) => initial,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(mut state) = initial.take() else {
            panic!("iced must boot a Rawr live preview host exactly once");
        };
        let (pending, ready) = state.host.take_boot();
        let task = iced::Task::batch([state.host.lower(pending), ready]).map(LiveMessage::Host);
        (state, task)
    }

    fn update_native(state: &mut State, message: LiveMessage) -> iced::Task<LiveMessage> {
        match message {
            LiveMessage::Host(super::Message::Control(super::Control::Screenshot)) => {
                state.control(reload::status::Control::Screenshot)
            }
            LiveMessage::Host(message) => {
                Host::update_state_native(&mut state.host, message).map(LiveMessage::Host)
            }
            LiveMessage::Saved(saved) => {
                state.status.saved(saved);
                iced::Task::none()
            }
            LiveMessage::Reload(wake) => state.reload(wake),
            LiveMessage::ReloadControl(control) => state.control(control),
            LiveMessage::Cargo(checked) => {
                state.status.cargo(checked);
                iced::Task::none()
            }
            LiveMessage::Export(request) => {
                request.answer(state.host.export_state());
                iced::Task::none()
            }
            LiveMessage::Agent(call) => AGENT.run(state, call),
            LiveMessage::AgentShot(call, saved) => {
                reload::agent::shot(&call, saved);
                iced::Task::none()
            }
        }
    }

    fn view_native(state: &State) -> iced::Element<'_, LiveMessage> {
        reload::agent::view(
            &state.agent,
            state.status.view(
                Host::view_native(&state.host).map(LiveMessage::Host).boxed(),
                LiveMessage::ReloadControl,
            ),
            LiveMessage::Agent,
        )
    }

    fn theme_native(state: &State) -> iced::Theme {
        Host::theme_native(&state.host)
    }

    fn subscription_native(state: &State) -> iced::Subscription<LiveMessage> {
        iced::Subscription::batch([
            Host::subscription_native(&state.host).map(LiveMessage::Host),
            Host::shortcuts().map(LiveMessage::Host),
            state.service.subscription().map(LiveMessage::Reload),
            state.service.exports().map(LiveMessage::Export),
            state.service.cargo().map(LiveMessage::Cargo),
            reload::status::subscription().map(LiveMessage::ReloadControl),
            reload::agent::subscription().map(LiveMessage::Agent),
        ])
    }
}

impl State {
    fn reload(&mut self, wake: reload::Wake) -> iced::Task<LiveMessage> {
        let Some(completion) = self.service.take_latest(wake) else {
            return iced::Task::none();
        };
        let candidate = match completion.into_input() {
            reload::Input::Revision(rawr::reload::scheduler::mailbox::Revision::Notice(notice)) => {
                self.status.progress(notice.into_inner());
                return iced::Task::none();
            }
            reload::Input::Revision(rawr::reload::scheduler::mailbox::Revision::Candidate(
                candidate,
            )) => match candidate.map_result(|outcome| match outcome {
                reload::Outcome::Candidate(candidate, staged) => Ok((*candidate, staged)),
                reload::Outcome::Failed(error) => Err(error),
            }) {
                Ok(candidate) => candidate,
                Err(candidate) => {
                    if let Some(report) = self.service.failed_candidate(candidate) {
                        self.status.failed(report);
                    }
                    return iced::Task::none();
                }
            },
            reload::Input::Terminal(error) => {
                self.status.terminal(error.clone());
                self.service.failed(error);
                return iced::Task::none();
            }
            reload::Input::Exit => {
                self.service.exiting();
                return iced::exit();
            }
        };
        let source = self.host.source();
        let runtime = match self.service.runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                let report = error.report(source);
                let candidate = candidate.map(|(generation, staged)| {
                    drop(staged);
                    drop(generation);
                    report
                });
                if let Some(report) = self.service.failed_candidate(candidate) {
                    self.status.failed(report);
                }
                return iced::Task::none();
            }
        };
        match self.host.install_observed(
            self.service.gate(),
            self.service.publication(),
            candidate,
            self.service.engine(),
            runtime,
            self.service.timing(),
        ) {
            Ok(Admission::Current(outcome)) => {
                let evidence = outcome.evidence().to_vec();
                let (task, inputs) = outcome.into_parts();
                let Some(inputs) = inputs else {
                    let report = crate::reload::report(
                        rawr::diagnostic::code::FRAMEWORK_NATIVE,
                        "live preview installation omitted input retirement",
                        source,
                    );
                    self.status.failed(report.clone());
                    self.service.failed(report);
                    return iced::Task::none();
                };
                self.service.installed(inputs);
                self.status.installed(&evidence, self.host.prelude_note());
                task.map(LiveMessage::Host)
            }
            Ok(Admission::Obsolete(())) => {
                self.service.settled();
                iced::Task::none()
            }
            Err(error) => {
                let report = error.report(source);
                self.status.failed(report.clone());
                self.service.failed(report);
                iced::Task::none()
            }
        }
    }

    fn control(&mut self, control: reload::status::Control) -> iced::Task<LiveMessage> {
        match control {
            reload::status::Control::Refresh => {
                if self.status.can_refresh() {
                    self.service.retry();
                }
                iced::Task::none()
            }
            reload::status::Control::Retry => {
                if self.status.can_retry() {
                    self.service.retry();
                }
                iced::Task::none()
            }
            reload::status::Control::Copy => {
                self.status.copy().map_or_else(iced::Task::none, |text| {
                    iced::clipboard::write(text).discard()
                })
            }
            reload::status::Control::Screenshot => {
                let (name, chrome) = self.host.screenshot_subject();
                crate::screenshot::capture(name, chrome, LiveMessage::Saved)
            }
            reload::status::Control::Dismiss => {
                self.status.dismiss();
                iced::Task::none()
            }
            reload::status::Control::Reset => {
                let pending = self.host.reset();
                self.host.lower(pending).map(LiveMessage::Host)
            }
        }
    }
}

impl Host {
    fn source(&self) -> Option<rawr::diagnostic::Source> {
        Some(self.source.clone())
    }

    pub(crate) fn run(self, service: reload::Service) -> iced::Result {
        Application::new(self, service).run()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use iced_test::futures::futures::StreamExt;
    use rawr::reload::scheduler::publication;

    use super::{Application, Host, State};
    use crate::reload;

    #[test]
    fn the_message_is_debug_for_iceds_debug_feature() {
        fn debug<T: std::fmt::Debug>() {}
        debug::<super::LiveMessage>();
    }

    #[test]
    fn retirement_ignores_refresh_and_retry_controls_queued_after_exit() {
        let root = std::env::temp_dir().join(format!(
            "rawr-preview-retirement-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Retiring"));
        let mut compiler = rawr::Engine::new();
        crate::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let target = rawr::project::Target::discover(&root).unwrap();
        let candidate = engine.compile_package_generation(&target).unwrap();
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(candidate.source()),
        ));
        let source = candidate.source().entry_source().diagnostic().clone();
        let host =
            Host::bootstrap(candidate.bind(&engine, registry()).unwrap(), source, None).unwrap();
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (transport, service) = reload::channel(gate, publication, engine);
        let (delivery, feedback, retry) = transport.into_parts();
        let mut state = State {
            host,
            service,
            status: reload::status::State::new(),
            agent: reload::agent::Session::default(),
        };
        let refresh = super::LiveMessage::ReloadControl(reload::status::Control::Refresh);
        let _task = Application::update_native(&mut state, refresh.clone());
        delivery.exit().unwrap();
        let wake = state.service.take_wake().unwrap();
        let _task = Application::update_native(&mut state, super::LiveMessage::Reload(wake));
        assert!(matches!(feedback.try_recv(), Ok(reload::Feedback::Exiting)));
        assert!(retry.take());
        assert!(!retry.take());
        let _task = Application::update_native(&mut state, refresh);
        assert!(!retry.take());
        state.status.failed(
            rawr::diagnostic::Report::error(rawr::diagnostic::code::FRAMEWORK_NATIVE, "retry")
                .finish(),
        );
        assert!(state.status.can_retry());
        let _task = Application::update_native(
            &mut state,
            super::LiveMessage::ReloadControl(reload::status::Control::Retry),
        );
        assert!(!retry.take());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_preview_reload_message_installs_without_sleeping() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-preview-message-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Old"));

        let mut compiler = rawr::Engine::new();
        crate::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let timing = rawr::timing::Session::start();
        let target = rawr::project::Target::discover(&root).unwrap();
        let first = engine
            .compile_package_generation_observed(&target, &timing)
            .unwrap();
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(first.source()),
        ));
        let diagnostic_source = first.source().entry_source().diagnostic().clone();
        let bootstrap = first.bind(&engine, registry()).unwrap();
        let mut host = Host::bootstrap(bootstrap, diagnostic_source, Some(timing.clone())).unwrap();
        let key = host.control_previews()[0].key().clone();
        host.select_control(&key).unwrap();
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (transport, service) = reload::channel_observed(
            Arc::clone(&gate),
            Arc::clone(&publication),
            engine.clone(),
            timing.clone(),
        );
        let (delivery, feedback, retry) = transport.into_parts();
        let mut state = State {
            host,
            service,
            status: reload::status::State::new(),
            agent: reload::agent::Session::default(),
        };
        let healthy = state.host.generation().unwrap();
        assert!(!retry.take());
        for _ in 0..3 {
            let _task = Application::update_native(
                &mut state,
                super::LiveMessage::ReloadControl(reload::status::Control::Refresh),
            );
        }
        assert!(retry.take());
        assert!(!retry.take());
        assert!(Arc::ptr_eq(&state.host.generation().unwrap(), &healthy));
        assert_eq!(state.status.phase(), reload::status::Phase::Ready);
        let mut simulator = iced_test::simulator(Application::view_native(&state));
        simulator.click("Tick").unwrap();
        let script = simulator.into_messages().next().unwrap();
        let before_batch = timing
            .snapshot()
            .aggregate(rawr::timing::Phase::ViewEvaluation)
            .count();
        let mut tasks = Vec::new();
        for _ in 0..100 {
            tasks.push(Application::update_native(&mut state, script.clone()));
        }
        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before_batch
        );
        let mut outputs = tasks
            .into_iter()
            .flat_map(task_messages)
            .collect::<Vec<_>>();
        assert_eq!(outputs.len(), 1);
        assert!(matches!(
            outputs[0],
            super::LiveMessage::Host(super::super::Message::Settle(_))
        ));
        let _ = Application::update_native(&mut state, outputs.pop().unwrap());
        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before_batch + 1
        );

        crate::write_marked(&root, "src/main.rs", source("New"));
        let target = rawr::project::Target::discover(&root).unwrap();
        let candidate = engine
            .compile_package_generation_observed(&target, &timing)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let now = Instant::now();
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete((candidate, snapshot))
            .stage_inputs(&publication);
        assert!(delivery
            .send(&gate, reload::Completion::new(candidate))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);

        iced_test::simulator(Host::view_native(&state.host))
            .find("New")
            .unwrap();
        let reload::Feedback::Installed(inputs) = feedback.try_recv().unwrap() else {
            panic!("live preview host must report installed input retirement");
        };
        inputs.release();

        let generation = state.host.generation().unwrap();
        let application = state.host.application.as_ref().unwrap().instance();
        let preview = state.host.preview.as_ref().unwrap().instance;
        let report = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::PARSE_SYNTAX,
            "invalid preview source",
        )
        .finish();
        gate.observe(now).unwrap();
        let failed = gate.start(now).unwrap().complete(report.clone());
        assert!(delivery
            .send(&gate, reload::Completion::failed(failed))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert!(Arc::ptr_eq(&state.host.generation().unwrap(), &generation));
        assert_eq!(
            state.host.application.as_ref().unwrap().instance(),
            application
        );
        assert_eq!(state.host.preview.as_ref().unwrap().instance, preview);
        assert_eq!(state.status.phase(), reload::status::Phase::Error);
        iced_test::simulator(Application::view_native(&state))
            .find("invalid preview source")
            .unwrap();

        let _task = state.control(reload::status::Control::Reset);
        assert!(Arc::ptr_eq(&state.host.generation().unwrap(), &generation));
        assert_eq!(
            state.host.application.as_ref().unwrap().instance(),
            application
        );
        assert_ne!(state.host.preview.as_ref().unwrap().instance, preview);
        assert_eq!(state.status.phase(), reload::status::Phase::Error);
        assert!(matches!(
            feedback.try_recv(),
            Ok(reload::Feedback::Failed(error)) if error == report
        ));

        let terminal = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            "preview watcher stopped",
        )
        .finish();
        delivery.terminate(terminal.clone()).unwrap();
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert!(!state.status.can_retry());
        assert_eq!(
            state.status.copy(),
            Some(rawr::diagnostic::render(&terminal))
        );
        iced_test::simulator(Application::view_native(&state))
            .find("Rawr · Error · Reload stopped · Last good still running")
            .unwrap();
        let mut terminal_ui = iced_test::simulator(Application::view_native(&state));
        terminal_ui.find("Retry").unwrap();
        terminal_ui.click("Retry").unwrap();
        assert!(terminal_ui.into_messages().next().is_none());
        for control in ["Retry", "Copy", "Reset"] {
            iced_test::simulator(Application::view_native(&state))
                .find(control)
                .unwrap();
        }
        let _task = state.control(reload::status::Control::Retry);
        assert!(!retry.take());
        let _task = state.control(reload::status::Control::Refresh);
        assert!(!retry.take());
        let _task = state.control(reload::status::Control::Reset);
        assert_eq!(
            state.status.copy(),
            Some(rawr::diagnostic::render(&terminal))
        );
        assert!(matches!(
            feedback.try_recv(),
            Ok(reload::Feedback::Failed(error)) if error == terminal
        ));
        let timing = timing.snapshot();
        assert_eq!(timing.aggregate(rawr::timing::Phase::ParseItems).count(), 2);
        assert_eq!(timing.aggregate(rawr::timing::Phase::Resolve).count(), 2);
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::CandidateValidation)
                .count(),
            1
        );
        assert_eq!(
            timing.aggregate(rawr::timing::Phase::Installation).count(),
            1
        );
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::EffectRetirement)
                .count(),
            1
        );
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            7
        );
        assert!(
            timing
                .aggregate(rawr::timing::Phase::DescriptorLowering)
                .count()
                > 0
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn preview_publication_keeps_imported_observation_after_hot_install() {
        let root = std::env::temp_dir().join(format!(
            "rawr-preview-full-publication-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Before"));
        let mut compiler = rawr::Engine::new();
        crate::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let timing = rawr::timing::Session::start();
        let target = rawr::project::Target::discover(&root).unwrap();
        let first = engine.compile_package_generation(&target).unwrap();
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(first.source()),
        ));
        let diagnostic_source = first.source().entry_source().diagnostic().clone();
        let bootstrap = first.bind(&engine, registry()).unwrap();
        let mut host = Host::bootstrap(bootstrap, diagnostic_source, Some(timing.clone())).unwrap();
        let previous = host.generation().unwrap();
        let gate = rawr::reload::scheduler::Gate::new(Duration::ZERO);
        let now = Instant::now();
        gate.observe(now).unwrap();
        let request = gate.start(now).unwrap();
        crate::write_marked(&root, "src/main.rs", source("After"));
        let candidate = engine.compile_package_generation(&target).unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let native = root.join("missing/native.rs");
        let inputs = rawr::reload::source::Inputs::from_graph(candidate.source())
            .with_imported([rawr_import::input::proof::Watch::Exact(native.clone())]);
        let observed = publication.register(inputs);
        assert!(!publication.read().current().contains(&native));
        let candidate = request.complete((candidate, observed.bind(snapshot).unwrap()));
        let installed = host
            .install_observed(
                &gate,
                &publication,
                candidate,
                &engine,
                registry(),
                Some(&timing),
            )
            .unwrap();
        assert!(matches!(
            installed,
            rawr::reload::scheduler::Admission::Current(_)
        ));
        assert!(!Arc::ptr_eq(&host.generation().unwrap(), &previous));
        assert!(publication.read().current().contains(&native));
        assert_eq!(publication.read().staged(), 0);
        drop(installed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn observed_preview_records_current_publication_rejection_as_install_failure() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-preview-install-rejection-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Retained"));

        let mut compiler = rawr::Engine::new();
        crate::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let timing = rawr::timing::Session::start();
        let target = rawr::project::Target::discover(&root).unwrap();
        let first = engine.compile_package_generation(&target).unwrap();
        let origin = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(first.source()),
        ));
        let origin_paths = origin.read().current().paths().to_vec();
        let foreign =
            publication::Slot::new(rawr::reload::source::Inputs::from_graph(first.source()));
        let foreign_paths = foreign.read().current().paths().to_vec();
        let diagnostic_source = first.source().entry_source().diagnostic().clone();
        let bootstrap = first.bind(&engine, registry()).unwrap();
        let mut host = Host::bootstrap(bootstrap, diagnostic_source, Some(timing.clone())).unwrap();
        let retained = host.generation().unwrap();
        let instance = host.application.as_ref().unwrap().instance();

        crate::write_marked(&root, "src/main.rs", source("Rejected"));
        let target = rawr::project::Target::discover(&root).unwrap();
        let candidate = engine.compile_package_generation(&target).unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let now = Instant::now();
        let gate = rawr::reload::scheduler::Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete((candidate, snapshot))
            .stage_inputs(&origin);
        assert_eq!(origin.read().staged(), 1);

        assert!(matches!(
            host.install_observed(
                &gate,
                &foreign,
                candidate,
                &engine,
                registry(),
                Some(&timing),
            ),
            Err(super::super::install::Error::Stale)
        ));
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert_eq!(origin.read().staged(), 0);
        assert_eq!(origin.read().current().paths(), origin_paths);
        assert_eq!(foreign.read().current().paths(), foreign_paths);
        let timing = timing.snapshot();
        assert_eq!(
            timing
                .aggregate_outcome(
                    rawr::timing::Phase::Installation,
                    rawr::timing::Outcome::Failure,
                )
                .count(),
            1
        );
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::EffectRetirement)
                .count(),
            0
        );
        fs::remove_dir_all(root).ok();
    }

    fn registry() -> rawr::host::runtime::Registry {
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry
    }

    fn source(label: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::{{button, column, text}};

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    #[preview(name = "Default")]
    fn view(&self) -> Element<Message> {{
        column![button("Tick").on_press(Message::Tick), text("{label}")]
    }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }

    fn task_messages(task: iced::Task<super::LiveMessage>) -> Vec<super::LiveMessage> {
        let Some(mut stream) = iced_test::runtime::task::into_stream(task) else {
            return Vec::new();
        };
        iced_test::futures::futures::executor::block_on(async move {
            let mut messages = Vec::new();
            while let Some(action) = stream.next().await {
                match action {
                    iced_test::runtime::Action::Output(message) => messages.push(message),
                    _ => panic!("expected a task output"),
                }
            }
            messages
        })
    }
}
