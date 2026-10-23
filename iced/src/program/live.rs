//! Private iced message boundary for one reloadable application host.

use std::sync::Mutex;

use iced::widget::Widget as _;
use rawr::reload::scheduler::Admission;

use crate::{reload, runtime};

use super::{Draft, Error, Host, Transition};

pub(super) struct Application {
    initial: Mutex<Option<State>>,
    centered: bool,
}

struct State {
    host: Host,
    service: reload::Service,
    status: reload::status::State,
    pending_settle: Option<Owner>,
    agent: reload::agent::Session,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Owner {
    instance: rawr::application::Instance,
    generation: rawr::program::GenerationId,
}

#[derive(Clone)]
enum Message {
    Script(runtime::Message),
    Settle(Owner),
    Reload(reload::Wake),
    ReloadControl(reload::status::Control),
    Cargo(reload::cargo::Checked),
    Saved(crate::screenshot::Saved),
    Export(reload::export::Request),
    Agent(reload::agent::Call),
    AgentShot(reload::agent::Call, crate::screenshot::Saved),
}

const AGENT: reload::agent::Live<State, Message> = reload::agent::Live {
    session: |state| &mut state.agent,
    reset: |state| state.control(reload::status::Control::Reset),
    wait: |state, call| state.status.wait(call),
    next: Message::Agent,
    shot: Message::AgentShot,
};

impl Application {
    pub(super) fn new(host: Host, service: reload::Service) -> Self {
        let centered = host.centered();
        Self {
            initial: Mutex::new(Some(State {
                host,
                service,
                status: reload::status::State::new(),
                pending_settle: None,
                agent: reload::agent::Session::default(),
            })),
            centered,
        }
    }

    pub(super) fn run(self) -> iced::Result {
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

    fn boot_native(&self) -> (State, iced::Task<Message>) {
        let mut initial = match self.initial.lock() {
            Ok(initial) => initial,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(mut state) = initial.take() else {
            panic!("iced must boot a Rawr live application exactly once");
        };
        let task = state.host.activate().map(Message::Script);
        (state, task)
    }

    fn update_native(state: &mut State, message: Message) -> iced::Task<Message> {
        match message {
            Message::Script(message) => state.script(message),
            Message::Settle(owner) => state.settle(owner),
            Message::Reload(wake) => state.reload(wake),
            Message::ReloadControl(control) => state.control(control),
            Message::Cargo(checked) => {
                state.status.cargo(checked);
                iced::Task::none()
            }
            Message::Saved(saved) => {
                state.status.saved(saved);
                iced::Task::none()
            }
            Message::Export(request) => {
                request.answer(state.host.export_state());
                iced::Task::none()
            }
            Message::Agent(call) => AGENT.run(state, call),
            Message::AgentShot(call, saved) => {
                reload::agent::shot(&call, saved);
                iced::Task::none()
            }
        }
    }

    fn view_native(state: &State) -> iced::Element<'_, Message> {
        reload::agent::view(
            &state.agent,
            state.status.view(
                state.host.view().map(Message::Script).boxed(),
                Message::ReloadControl,
            ),
            Message::Agent,
        )
    }

    fn theme_native(state: &State) -> iced::Theme {
        state.host.theme()
    }

    fn title_native(state: &State) -> String {
        state.host.title().to_owned()
    }

    fn subscription_native(state: &State) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            state.host.subscription().map(Message::Script),
            state.service.subscription().map(Message::Reload),
            state.service.exports().map(Message::Export),
            state.service.cargo().map(Message::Cargo),
            reload::status::subscription().map(Message::ReloadControl),
            reload::agent::subscription().map(Message::Agent),
        ])
    }
}

impl State {
    fn script(&mut self, message: runtime::Message) -> iced::Task<Message> {
        let transition = match self.host.update_state(message) {
            Ok(transition) => transition,
            Err(_) => return iced::Task::none(),
        };
        let delivery = transition.delivery();
        let task = self.host.lower(transition).map(Message::Script);
        if delivery != super::Delivery::Applied {
            return task;
        }

        let owner = Owner {
            instance: self.host.adapter().instance(),
            generation: self.host.adapter().generation(),
        };
        if self.pending_settle == Some(owner) {
            task
        } else {
            self.pending_settle = Some(owner);
            iced::Task::batch([task, iced::Task::done(Message::Settle(owner))])
        }
    }

    fn settle(&mut self, owner: Owner) -> iced::Task<Message> {
        if self.pending_settle == Some(owner) {
            self.pending_settle = None;
            let _ = self.host.settle(owner.instance, owner.generation);
        }
        iced::Task::none()
    }

    fn reload(&mut self, wake: reload::Wake) -> iced::Task<Message> {
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
                self.status.installed(outcome.evidence());
                let (task, inputs) = outcome.into_parts();
                self.service.installed(inputs);
                task.map(Message::Script)
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

    fn control(&mut self, control: reload::status::Control) -> iced::Task<Message> {
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
                crate::screenshot::capture(self.host.title().to_owned(), 0.0, Message::Saved)
            }
            reload::status::Control::Dismiss => {
                self.status.dismiss();
                iced::Task::none()
            }
            reload::status::Control::Reset => match self.host.reset() {
                Ok(task) => task.map(Message::Script),
                Err(error) => {
                    self.status.failed(error.report(self.host.source()));
                    iced::Task::none()
                }
            },
        }
    }
}

impl Host {
    fn source(&self) -> Option<rawr::diagnostic::Source> {
        Some(
            self.slot
                .generation()
                .source()
                .entry_source()
                .diagnostic()
                .clone(),
        )
    }

    fn activate(&mut self) -> iced::Task<runtime::Message> {
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        adapter
            .authority(&context)
            .map_or_else(iced::Task::none, |authority| {
                adapter.activate_prepared(&context, &authority)
            })
    }

    fn lower(&mut self, transition: Transition) -> iced::Task<runtime::Message> {
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        let plan = transition.into_plan();
        match adapter.authority(&context) {
            Some(authority) => adapter
                .lower_task_authority(plan, &authority)
                .unwrap_or_else(|error| {
                    adapter.record_task_error(Error::Task(std::sync::Arc::new(error)));
                    iced::Task::none()
                }),
            None => iced::Task::none(),
        }
    }

    fn theme(&self) -> iced::Theme {
        self.slot.state().theme.native().clone()
    }

    fn title(&self) -> &str {
        self.slot.state().title()
    }

    fn centered(&self) -> bool {
        self.slot.state().centered()
    }

    fn subscription(&self) -> iced::Subscription<runtime::Message> {
        let context = self.slot.context();
        let adapter = self.slot.state();
        match adapter.authority(&context) {
            Some(authority) => {
                adapter.lower_subscription_authority(&authority, std::convert::identity)
            }
            None => iced::Subscription::none(),
        }
    }

    fn reset(&mut self) -> Result<iced::Task<runtime::Message>, super::install::Error> {
        let next = {
            let context = self.slot.context();
            let previous = self.slot.state();
            let authority = previous
                .authority(&context)
                .ok_or(super::install::Error::Application)?;
            let launch = authority.borrow();
            let mut draft = Draft::boot(&launch, previous.source.clone(), self.timing.clone())
                .map_err(super::install::Error::Adapter)?;
            draft.timing = None;
            if let Some(entry) = draft.diagnostics.first() {
                return Err(super::install::Error::Candidate(entry.report().clone()));
            }
            let startup = std::mem::take(&mut draft.startup);
            let prepared = startup
                .prepare_application(&launch)
                .map_err(super::install::Error::Task)?;
            let mut adapter = draft.install(authority.generation());
            adapter.prepared = Some(prepared);
            adapter
        };
        let old = {
            let mut mutation = self.slot.mutation();
            let (adapter, _context) = mutation.parts();
            std::mem::replace(adapter, next)
        };
        drop(old);
        Ok(self.activate())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use iced_test::futures::futures::StreamExt;
    use rawr::reload::scheduler::publication;

    use super::{Application, State};
    use crate::reload;

    #[test]
    fn retirement_ignores_refresh_and_retry_controls_queued_after_exit() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-retirement-{}-{:?}",
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
        let host = super::Host::bootstrap_observed(
            candidate.bind(&engine, registry()).unwrap(),
            Some(source),
            rawr::timing::Session::start(),
        )
        .unwrap();
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (transport, service) = reload::channel(gate, publication, engine);
        let (delivery, feedback, retry) = transport.into_parts();
        let mut state = State {
            host,
            service,
            status: reload::status::State::new(),
            pending_settle: None,
            agent: reload::agent::Session::default(),
        };
        let refresh = super::Message::ReloadControl(reload::status::Control::Refresh);
        let _task = Application::update_native(&mut state, refresh.clone());
        delivery.exit().unwrap();
        let wake = state.service.take_wake().unwrap();
        let _task = Application::update_native(&mut state, super::Message::Reload(wake));
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
            super::Message::ReloadControl(reload::status::Control::Retry),
        );
        assert!(!retry.take());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_control_bursts_apply_every_message_and_settle_once_without_sleeping() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-form-burst-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", form_source());

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
        let host =
            super::Host::bootstrap_observed(bootstrap, Some(diagnostic_source), timing.clone())
                .unwrap();
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (_transport, service) =
            reload::channel_observed(gate, publication, engine, timing.clone());
        let mut state = State {
            host,
            service,
            status: reload::status::State::new(),
            pending_settle: None,
            agent: reload::agent::Session::default(),
        };
        let input = "x".repeat(256);
        let mut simulator = iced_test::simulator(Application::view_native(&state));
        simulator.click("Burst input").unwrap();
        let _ = simulator.typewrite(&input);
        let messages = simulator.into_messages().collect::<Vec<_>>();
        assert_eq!(messages.len(), input.len());
        let before = timing
            .snapshot()
            .aggregate(rawr::timing::Phase::ViewEvaluation)
            .count();

        let outputs = messages
            .into_iter()
            .flat_map(|message| task_messages(Application::update_native(&mut state, message)))
            .collect::<Vec<_>>();

        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before
        );
        assert_eq!(outputs.len(), 1);
        assert!(matches!(outputs[0], super::Message::Settle(_)));
        let _ = Application::update_native(&mut state, outputs.into_iter().next().unwrap());
        assert!(state.pending_settle.is_none());
        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before + 1
        );
        {
            let mut simulator = iced_test::simulator(Application::view_native(&state));
            simulator.find(input).unwrap();
            simulator.find("256").unwrap();
        }

        let before = timing
            .snapshot()
            .aggregate(rawr::timing::Phase::ViewEvaluation)
            .count();
        let messages = {
            let mut simulator = iced_test::simulator(Application::view_native(&state));
            for index in 0..256 {
                simulator.point_at(iced::Point::new(
                    if index % 2 == 0 { 256.0 } else { 768.0 },
                    8.0,
                ));
                let _ = simulator.simulate(iced_test::simulator::click());
            }
            simulator.into_messages().collect::<Vec<_>>()
        };
        assert_eq!(messages.len(), 256);

        let outputs = messages
            .into_iter()
            .flat_map(|message| task_messages(Application::update_native(&mut state, message)))
            .collect::<Vec<_>>();

        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before
        );
        assert_eq!(outputs.len(), 1);
        assert!(matches!(outputs[0], super::Message::Settle(_)));
        let _ = Application::update_native(&mut state, outputs.into_iter().next().unwrap());
        assert!(state.pending_settle.is_none());
        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before + 1
        );
        let mut simulator = iced_test::simulator(Application::view_native(&state));
        simulator.find("75").unwrap();
        simulator.find("256").unwrap();

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn private_live_message_installs_new_callbacks_without_sleeping() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-message-{}-{:?}",
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
        let host =
            super::Host::bootstrap_observed(bootstrap, Some(diagnostic_source), timing.clone())
                .unwrap();
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
            pending_settle: None,
            agent: reload::agent::Session::default(),
        };
        let healthy = state.host.generation();
        assert!(!retry.take());
        for _ in 0..3 {
            let _task = Application::update_native(
                &mut state,
                super::Message::ReloadControl(reload::status::Control::Refresh),
            );
        }
        assert!(retry.take());
        assert!(!retry.take());
        assert!(Arc::ptr_eq(&state.host.generation(), &healthy));
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
        assert!(matches!(outputs[0], super::Message::Settle(_)));
        let _ = Application::update_native(&mut state, outputs.pop().unwrap());
        assert_eq!(
            timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before_batch + 1
        );
        let before_view = timing.snapshot();
        drop(Application::view_native(&state));
        drop(Application::view_native(&state));
        let after_view = timing.snapshot();
        assert_eq!(
            after_view
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            before_view
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count()
        );
        assert_eq!(
            after_view
                .aggregate(rawr::timing::Phase::DescriptorLowering)
                .count(),
            before_view
                .aggregate(rawr::timing::Phase::DescriptorLowering)
                .count()
                + 2
        );

        let now = Instant::now();
        let observed = gate.observe(now).unwrap();
        assert!(delivery.checking(&gate, observed));
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert_eq!(state.status.phase(), reload::status::Phase::Checking);
        iced_test::simulator(Application::view_native(&state))
            .find("Rawr · Checking…")
            .unwrap();
        let request = gate.start(now).unwrap();
        assert!(delivery.reloading(&gate, &request));
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert_eq!(state.status.phase(), reload::status::Phase::Reloading);
        drop(request);

        let mut wake = None;
        for index in 0..12 {
            crate::write_marked(&root, "src/main.rs", source(&format!("New {index}")));
            let target = rawr::project::Target::discover(&root).unwrap();
            let candidate = engine
                .compile_package_generation_observed(&target, &timing)
                .unwrap();
            let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
            gate.observe(now).unwrap();
            let candidate = gate
                .start(now)
                .unwrap()
                .complete((candidate, snapshot))
                .stage_inputs(&publication);
            assert!(delivery
                .send(&gate, reload::Completion::new(candidate))
                .unwrap());
            match state.service.take_wake() {
                Some(edge) => {
                    assert!(wake.is_none());
                    wake = Some(edge);
                }
                None => assert!(wake.is_some()),
            }
            assert_eq!(publication.read().staged(), 1);
        }

        let wake = wake.unwrap();
        let _task = state.reload(wake);
        assert_eq!(publication.read().staged(), 0);

        iced_test::simulator(state.host.view())
            .find("New 11")
            .unwrap();
        let reload::Feedback::Installed(inputs) = feedback.try_recv().unwrap() else {
            panic!("live host must report exact installed input retirement");
        };
        inputs.release();
        assert!(feedback.try_recv().is_err());
        assert_eq!(
            timing
                .snapshot()
                .aggregate_outcome(
                    rawr::timing::Phase::Installation,
                    rawr::timing::Outcome::Success,
                )
                .count(),
            1
        );
        let last_good_generation = state.host.generation();
        let last_good_instance = state.host.adapter().instance();
        let report = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::PARSE_SYNTAX,
            "invalid newest source",
        )
        .finish();
        gate.observe(now).unwrap();
        let failure = gate.start(now).unwrap().complete(report.clone());
        assert!(delivery
            .send(&gate, reload::Completion::failed(failure))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        let reload::Feedback::Failed(error) = feedback.try_recv().unwrap() else {
            panic!("transport failure must wake and return through the sole iced owner");
        };
        assert_eq!(error, report);
        assert!(Arc::ptr_eq(&state.host.generation(), &last_good_generation));
        assert_eq!(state.host.adapter().instance(), last_good_instance);
        iced_test::simulator(Application::view_native(&state))
            .find("invalid newest source")
            .unwrap();
        for control in ["Retry", "Copy", "Reset"] {
            iced_test::simulator(Application::view_native(&state))
                .find(control)
                .unwrap();
        }
        iced_test::simulator(state.host.view())
            .find("New 11")
            .unwrap();

        gate.observe(now).unwrap();
        let repeated = gate.start(now).unwrap().complete(report.clone());
        assert!(delivery
            .send(&gate, reload::Completion::failed(repeated))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert_eq!(state.status.reports().len(), 1);
        assert!(matches!(
            feedback.try_recv(),
            Ok(reload::Feedback::Failed(_))
        ));

        crate::write_marked(&root, "src/main.rs", failing_source("candidate view failed"));
        let target = rawr::project::Target::discover(&root).unwrap();
        let failing = engine
            .compile_package_generation_observed(&target, &timing)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(failing.source());
        gate.observe(now).unwrap();
        let failing = gate
            .start(now)
            .unwrap()
            .complete((failing, snapshot))
            .stage_inputs(&publication);
        assert!(delivery
            .send(&gate, reload::Completion::new(failing))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert!(Arc::ptr_eq(&state.host.generation(), &last_good_generation));
        assert_eq!(state.host.adapter().instance(), last_good_instance);
        iced_test::simulator(state.host.view())
            .find("New 11")
            .unwrap();
        let reload::Feedback::Failed(prepare_report) = feedback.try_recv().unwrap() else {
            panic!("prepare failure must return its canonical report");
        };
        assert!(rawr::diagnostic::render(&prepare_report).contains("candidate view failed"));
        iced_test::simulator(Application::view_native(&state))
            .find(prepare_report.message())
            .unwrap();

        crate::write_marked(&root, "src/main.rs", source("Obsolete"));
        let target = rawr::project::Target::discover(&root).unwrap();
        let obsolete = engine
            .compile_package_generation_observed(&target, &timing)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(obsolete.source());
        gate.observe(now).unwrap();
        let obsolete = gate
            .start(now)
            .unwrap()
            .complete((obsolete, snapshot))
            .stage_inputs(&publication);
        assert!(delivery
            .send(&gate, reload::Completion::new(obsolete))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        gate.observe(now).unwrap();
        let reports = state.status.reports().len();
        let _task = state.reload(wake);
        assert_eq!(state.status.reports().len(), reports);
        assert!(Arc::ptr_eq(&state.host.generation(), &last_good_generation));
        assert_eq!(state.host.adapter().instance(), last_good_instance);
        assert!(matches!(feedback.try_recv(), Ok(reload::Feedback::Settled)));

        let _task = state.control(reload::status::Control::Retry);
        assert!(retry.take());
        assert!(!retry.take());

        let reset_generation = state.host.generation();
        let mut reset_instance = state.host.adapter().instance();
        for _ in 0..3 {
            let _task = state.control(reload::status::Control::Reset);
            assert!(Arc::ptr_eq(&state.host.generation(), &reset_generation));
            assert_ne!(state.host.adapter().instance(), reset_instance);
            assert_eq!(state.status.phase(), reload::status::Phase::Error);
            reset_instance = state.host.adapter().instance();
        }

        crate::write_marked(&root, "src/main.rs", source("Recovered"));
        let target = rawr::project::Target::discover(&root).unwrap();
        let recovered = engine
            .compile_package_generation_observed(&target, &timing)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(recovered.source());
        gate.observe(now).unwrap();
        let recovered = gate
            .start(now)
            .unwrap()
            .complete((recovered, snapshot))
            .stage_inputs(&publication);
        assert!(delivery
            .send(&gate, reload::Completion::new(recovered))
            .unwrap());
        let wake = state.service.take_wake().unwrap();
        let _task = state.reload(wake);
        assert_eq!(state.status.phase(), reload::status::Phase::Ready);
        assert_eq!(state.status.reports().len(), 0);
        iced_test::simulator(state.host.view())
            .find("Recovered")
            .unwrap();
        let reload::Feedback::Installed(inputs) = feedback.try_recv().unwrap() else {
            panic!("valid recovery must install and clear the reload layer");
        };
        inputs.release();

        for (cycle, (invalid_source, expected_code)) in [
            ("fn view( {\n", rawr::diagnostic::code::PARSE_MISSING_TOKEN),
            (
                "struct Counter { value: Missing }\n",
                rawr::diagnostic::code::RESOLVE_UNKNOWN_TYPE,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let retained = state.host.generation();
            crate::write_marked(&root, "src/main.rs", invalid_source);
            let target = rawr::project::Target::discover(&root).unwrap();
            let invalid = engine
                .compile_package_generation_observed(&target, &timing)
                .unwrap_err();
            let invalid = invalid
                .report()
                .expect("parse and resolution failures must carry canonical reports");
            assert_eq!(invalid.code(), expected_code);
            gate.observe(now).unwrap();
            let failed = gate.start(now).unwrap().complete(invalid);
            assert!(delivery
                .send(&gate, reload::Completion::failed(failed))
                .unwrap());
            let wake = state.service.take_wake().unwrap();
            let _task = state.reload(wake);
            assert_eq!(state.status.phase(), reload::status::Phase::Error);
            assert!(Arc::ptr_eq(&state.host.generation(), &retained));
            assert!(matches!(
                feedback.try_recv(),
                Ok(reload::Feedback::Failed(_))
            ));

            let label = format!("Recovery cycle {cycle}");
            crate::write_marked(&root, "src/main.rs", source(&label));
            let target = rawr::project::Target::discover(&root).unwrap();
            let recovered = engine
                .compile_package_generation_observed(&target, &timing)
                .unwrap();
            let snapshot = rawr::reload::source::Snapshot::from_graph(recovered.source());
            gate.observe(now).unwrap();
            let recovered = gate
                .start(now)
                .unwrap()
                .complete((recovered, snapshot))
                .stage_inputs(&publication);
            assert!(delivery
                .send(&gate, reload::Completion::new(recovered))
                .unwrap());
            let wake = state.service.take_wake().unwrap();
            let _task = state.reload(wake);
            assert_eq!(state.status.phase(), reload::status::Phase::Ready);
            iced_test::simulator(state.host.view())
                .find(label.clone())
                .unwrap();
            let reload::Feedback::Installed(inputs) = feedback.try_recv().unwrap() else {
                panic!("every valid recovery cycle must install exactly once");
            };
            inputs.release();
        }

        let terminal = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            "reload watcher stopped",
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
        assert_eq!(
            timing.aggregate(rawr::timing::Phase::ParseItems).count(),
            20
        );
        assert_eq!(timing.aggregate(rawr::timing::Phase::Resolve).count(), 19);
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::CandidateValidation)
                .count(),
            6
        );
        assert_eq!(
            timing
                .aggregate_outcome(
                    rawr::timing::Phase::CandidateValidation,
                    rawr::timing::Outcome::Failure,
                )
                .count(),
            1
        );
        assert_eq!(
            timing.aggregate(rawr::timing::Phase::Installation).count(),
            5
        );
        assert_eq!(
            timing
                .aggregate_outcome(
                    rawr::timing::Phase::Installation,
                    rawr::timing::Outcome::Success,
                )
                .count(),
            4
        );
        assert_eq!(
            timing
                .aggregate_outcome(
                    rawr::timing::Phase::Installation,
                    rawr::timing::Outcome::Obsolete,
                )
                .count(),
            1
        );
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::EffectRetirement)
                .count(),
            4
        );
        assert_eq!(
            timing
                .aggregate(rawr::timing::Phase::ViewEvaluation)
                .count(),
            12
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
    fn application_publication_keeps_imported_observation_after_hot_install() {
        let root = std::env::temp_dir().join(format!(
            "rawr-application-full-publication-{}-{:?}",
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
        let mut host =
            super::Host::bootstrap_observed(bootstrap, Some(diagnostic_source), timing.clone())
                .unwrap();
        let previous = host.generation();
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
        assert!(!Arc::ptr_eq(&host.generation(), &previous));
        assert!(publication.read().current().contains(&native));
        assert_eq!(publication.read().staged(), 0);
        drop(installed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn observed_application_records_current_publication_rejection_as_install_failure() {
        let root = std::env::temp_dir().join(format!(
            "rawr-live-install-rejection-{}-{:?}",
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
        let mut host =
            super::Host::bootstrap_observed(bootstrap, Some(diagnostic_source), timing.clone())
                .unwrap();
        let retained = host.generation();
        let instance = host.adapter().instance();

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
        assert!(Arc::ptr_eq(&host.generation(), &retained));
        assert_eq!(host.adapter().instance(), instance);
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
    fn view(&self) -> Element<Message> {{
        column![button("Tick").on_press(Message::Tick), text("{label}")]
    }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }

    fn form_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::{column, slider, text, text_input};

#[derive(Default)]
struct State {
    value: String,
    deliveries: i64,
    slider: i64,
    slider_deliveries: i64,
}
#[derive(Debug, Clone)] enum Message { Input(String), Slide(i64) }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Input(value) => {
                self.value = value;
                self.deliveries += 1;
            },
            Message::Slide(value) => {
                self.slider = value;
                self.slider_deliveries += 1;
            },
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            slider(0..=100, self.slider, Message::Slide).step(5),
            text_input("Burst input", self.value).on_input(Message::Input),
            text(self.value),
            text(self.deliveries),
            text(self.slider),
            text(self.slider_deliveries),
        ]
    }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }

    fn task_messages(task: iced::Task<super::Message>) -> Vec<super::Message> {
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

    fn failing_source(message: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<Message> {{ throw "{message}" }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }
}
