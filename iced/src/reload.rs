//! Revisioned candidate transport into the iced-owned live host.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

pub mod agent;
pub mod cargo;
pub mod export;
mod mailbox;
pub mod status;

use mailbox::Inbox;
pub use mailbox::Sender;

/// One revision-bound compiled candidate with staged exact inputs.
#[must_use = "a live completion must be delivered or retired"]
pub struct Completion(Input);

/// Payload-free empty-to-full edge delivered through iced's native proxy.
#[must_use = "a reload wake must pull the newest shared completion"]
#[derive(Clone)]
pub struct Wake(());

pub(crate) enum Input {
    Revision(rawr::reload::scheduler::mailbox::Revision<Outcome, status::Phase>),
    Terminal(rawr::diagnostic::Report),
    Exit,
}

pub(crate) enum Outcome {
    Candidate(
        Box<rawr::reload::Candidate>,
        rawr::reload::scheduler::publication::Staged,
    ),
    Failed(rawr::diagnostic::Report),
}

/// Post-admission work returned to the watcher/session owner.
#[must_use = "watch publication feedback must be reconciled"]
pub enum Feedback {
    /// The host and exact inputs committed; the prior inputs can be reconciled.
    Installed(rawr::reload::scheduler::publication::Retired),
    /// A candidate settled without changing the current publication.
    Settled,
    /// Candidate preparation failed without changing the current publication.
    Failed(rawr::diagnostic::Report),
    /// The iced owner accepted an authenticated graceful-exit request.
    Exiting,
}

/// Worker-side channel endpoint and post-admission feedback receiver.
pub struct Transport {
    sender: Sender,
    feedback: mpsc::Receiver<Feedback>,
    retry: Retry,
    exporter: export::Exporter,
    cargo: cargo::Sender,
}

/// One coalesced UI-to-supervisor retry request.
pub struct Retry(Arc<AtomicBool>);

/// Iced-owned revision gate, publication proof, compiler origin, and inbox.
pub struct Service {
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    engine: rawr::Shared<rawr::Engine>,
    inbox: Inbox,
    feedback: mpsc::Sender<Feedback>,
    retry: Arc<AtomicBool>,
    retiring: bool,
    timing: Option<rawr::timing::Session>,
    runtime: Provider,
    exports: export::Requests,
    cargo: cargo::Inbox,
}

enum Provider {
    Static,
    /// The runner's one registry, built when it started.
    Generated(rawr::host::runtime::Registry),
}

pub(crate) enum Runtime {
    Facade(rawr::host::Error),
    Registry(rawr::host::runtime::Error),
}

/// Create the sole worker/UI transport for one live host session.
#[must_use]
pub fn channel(
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    engine: rawr::Shared<rawr::Engine>,
) -> (Transport, Service) {
    channel_with_timing(gate, publication, engine, None, Provider::Static)
}

/// Create a live transport with one explicit host observation session.
#[doc(hidden)]
#[must_use]
pub fn channel_observed(
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    engine: rawr::Shared<rawr::Engine>,
    timing: rawr::timing::Session,
) -> (Transport, Service) {
    channel_with_timing(gate, publication, engine, Some(timing), Provider::Static)
}

/// Create a live generated-owner transport whose every hot registry is the
/// next generation of the runner's one registry, built when it started.
#[doc(hidden)]
#[must_use]
pub fn channel_generated(
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    engine: rawr::Shared<rawr::Engine>,
    runtime: rawr::host::runtime::Registry,
) -> (Transport, Service) {
    channel_with_timing(gate, publication, engine, None, Provider::Generated(runtime))
}

fn channel_with_timing(
    gate: Arc<rawr::reload::scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    engine: rawr::Shared<rawr::Engine>,
    timing: Option<rawr::timing::Session>,
    runtime: Provider,
) -> (Transport, Service) {
    let (sender, inbox) = mailbox::channel();
    let (feedback, returned) = mpsc::channel();
    let retry = Arc::new(AtomicBool::new(false));
    let exporter = export::Exporter::default();
    let exports = exporter.requests();
    let (cargo, checks) = cargo::channel();
    (
        Transport {
            sender,
            feedback: returned,
            retry: Retry(Arc::clone(&retry)),
            exporter,
            cargo,
        },
        Service {
            gate,
            publication,
            engine,
            inbox,
            feedback,
            retry,
            retiring: false,
            timing,
            runtime,
            exports,
            cargo: checks,
        },
    )
}

impl Completion {
    /// Seal one staged candidate for delivery to the iced owner.
    #[must_use = "the completion must be delivered or retired"]
    pub fn new(
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
    ) -> Self {
        Self(Input::Revision(
            rawr::reload::scheduler::mailbox::Revision::Candidate(
                candidate
                    .map(|(candidate, staged)| Outcome::Candidate(Box::new(candidate), staged)),
            ),
        ))
    }

    /// Wake the sole iced owner with one contained transport failure.
    #[must_use = "the failed completion must be delivered or retired"]
    pub fn failed(candidate: rawr::reload::scheduler::Candidate<rawr::diagnostic::Report>) -> Self {
        Self(Input::Revision(
            rawr::reload::scheduler::mailbox::Revision::Candidate(candidate.map(Outcome::Failed)),
        ))
    }

    pub(crate) fn into_input(self) -> Input {
        self.0
    }

    /// Borrow a structured worker failure without exposing Candidate authority.
    #[doc(hidden)]
    #[must_use]
    pub fn report(&self) -> Option<&rawr::diagnostic::Report> {
        match &self.0 {
            Input::Revision(rawr::reload::scheduler::mailbox::Revision::Candidate(candidate)) => {
                match candidate.value() {
                    Outcome::Failed(report) => Some(report),
                    Outcome::Candidate(_, _) => None,
                }
            }
            Input::Revision(rawr::reload::scheduler::mailbox::Revision::Notice(_))
            | Input::Terminal(_)
            | Input::Exit => None,
        }
    }
}

impl Sender {
    /// Publish one revision only if it is still current at mailbox admission.
    ///
    /// `Ok(false)` means an inverted worker completed after a newer revision.
    /// A closed mailbox returns the still-owned completion to the caller.
    pub fn send(
        &self,
        gate: &rawr::reload::scheduler::Gate,
        completion: Completion,
    ) -> Result<bool, Completion> {
        let candidate = match completion.0 {
            Input::Revision(rawr::reload::scheduler::mailbox::Revision::Candidate(candidate)) => {
                candidate
            }
            input => return Err(Completion(input)),
        };
        self.publish(gate, candidate).map_err(|candidate| {
            Completion(Input::Revision(
                rawr::reload::scheduler::mailbox::Revision::Candidate(candidate),
            ))
        })
    }

    /// Publish Checking for one exact observed revision.
    pub fn checking(
        &self,
        gate: &rawr::reload::scheduler::Gate,
        observed: rawr::reload::scheduler::notice::Observed,
    ) -> bool {
        self.publish_notice(gate, observed, status::Phase::Checking)
    }

    /// Publish Reloading for one exact started request without consuming it.
    pub fn reloading(
        &self,
        gate: &rawr::reload::scheduler::Gate,
        request: &rawr::reload::scheduler::Request,
    ) -> bool {
        self.publish_started(gate, request, status::Phase::Reloading)
    }

    /// Replace any pending revision with one explicit terminal failure.
    pub fn terminate(&self, error: rawr::diagnostic::Report) -> Result<(), Completion> {
        self.publish_terminal_input(Input::Terminal(error))
            .map_err(Completion)
    }

    /// Replace pending work with one authenticated graceful UI exit.
    pub fn exit(&self) -> Result<(), Completion> {
        self.publish_terminal_input(Input::Exit).map_err(Completion)
    }
}

impl Transport {
    /// The worker's end of State export requests.
    #[must_use]
    pub fn exporter(&self) -> export::Exporter {
        self.exporter.clone()
    }

    /// Where `cargo check` results go to be shown in the window.
    #[must_use]
    pub fn cargo(&self) -> cargo::Sender {
        self.cargo.clone()
    }

    /// Split this session endpoint into candidate delivery and feedback.
    #[must_use]
    pub fn into_parts(self) -> (Sender, mpsc::Receiver<Feedback>, Retry) {
        (self.sender, self.feedback, self.retry)
    }
}

impl Service {
    pub(crate) fn gate(&self) -> &rawr::reload::scheduler::Gate {
        &self.gate
    }

    pub(crate) fn publication(&self) -> &rawr::reload::scheduler::publication::Slot {
        &self.publication
    }

    pub(crate) const fn engine(&self) -> &rawr::Shared<rawr::Engine> {
        &self.engine
    }

    pub(crate) const fn timing(&self) -> Option<&rawr::timing::Session> {
        self.timing.as_ref()
    }

    pub(crate) fn runtime(&self) -> Result<rawr::host::runtime::Registry, Runtime> {
        match &self.runtime {
            Provider::Static => {
                let mut runtime = rawr::host::runtime::Registry::new();
                for facade in crate::runtime_facades().map_err(Runtime::Facade)? {
                    runtime.install(facade).map_err(Runtime::Registry)?;
                }
                Ok(runtime)
            }
            Provider::Generated(runtime) => Ok(runtime.next_generation()),
        }
    }

    pub(crate) fn subscription(&self) -> iced::Subscription<Wake> {
        iced::Subscription::run_with(self.inbox.clone(), mailbox::messages)
    }

    /// Each finished `cargo check`.
    pub(crate) fn cargo(&self) -> iced::Subscription<cargo::Checked> {
        iced::Subscription::run_with(self.cargo.clone(), cargo::messages)
    }

    /// Pending State export requests.
    pub(crate) fn exports(&self) -> iced::Subscription<export::Request> {
        self.exports.subscription()
    }

    pub(crate) fn take_latest(&self, wake: Wake) -> Option<Completion> {
        self.inbox.take_latest(wake)
    }

    pub(crate) fn installed(&self, inputs: rawr::reload::scheduler::publication::Retired) {
        self.send(Feedback::Installed(inputs));
    }

    pub(crate) fn settled(&self) {
        self.send(Feedback::Settled);
    }

    pub(crate) fn failed(&self, report: rawr::diagnostic::Report) {
        self.send(Feedback::Failed(report));
    }

    pub(crate) fn exiting(&mut self) {
        // Exit and UI controls are serialized by the owner. Close acceptance
        // before acknowledging, so the transport can drain the final request.
        self.retiring = true;
        self.send(Feedback::Exiting);
    }

    pub(crate) fn failed_candidate(
        &self,
        candidate: rawr::reload::scheduler::Candidate<rawr::diagnostic::Report>,
    ) -> Option<rawr::diagnostic::Report> {
        match self.gate.retain_current(candidate) {
            rawr::reload::scheduler::Admission::Current(candidate) => {
                let report = candidate.value().clone();
                drop(candidate);
                self.failed(report.clone());
                Some(report)
            }
            rawr::reload::scheduler::Admission::Obsolete(report) => {
                drop(report);
                self.settled();
                None
            }
        }
    }

    pub(crate) fn retry(&self) {
        if !self.retiring {
            self.retry.store(true, Ordering::Release);
        }
    }

    fn send(&self, feedback: Feedback) {
        drop(self.feedback.send(feedback));
    }

    #[cfg(test)]
    pub(crate) fn take_wake(&self) -> Option<Wake> {
        use iced::futures::Stream as _;

        let mut messages = mailbox::messages(&self.inbox);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::Pin::new(&mut messages).poll_next(&mut context) {
            std::task::Poll::Ready(wake) => wake,
            std::task::Poll::Pending => None,
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.inbox.close();
    }
}

impl Retry {
    /// Take one or more coalesced retry requests.
    #[must_use]
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

impl fmt::Debug for Completion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Completion(<private>)")
    }
}

impl fmt::Debug for Wake {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Wake")
    }
}

impl fmt::Debug for Sender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Sender(<private>)")
    }
}

impl fmt::Debug for Feedback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Installed(_) => formatter.write_str("Installed(<private>)"),
            Self::Settled => formatter.write_str("Settled"),
            Self::Failed(_) => formatter.write_str("Failed(<private>)"),
            Self::Exiting => formatter.write_str("Exiting"),
        }
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Transport(<private>)")
    }
}

impl fmt::Debug for Retry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Retry(<private>)")
    }
}

impl fmt::Debug for Service {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Service(<private>)")
    }
}

impl fmt::Display for Runtime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facade(error) => error.fmt(formatter),
            Self::Registry(error) => error.fmt(formatter),
        }
    }
}

impl Runtime {
    pub(crate) fn report(
        &self,
        source: Option<rawr::diagnostic::Source>,
    ) -> rawr::diagnostic::Report {
        report(
            rawr::diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
            self,
            source,
        )
    }
}

pub(crate) fn report(
    code: rawr::diagnostic::code::Code,
    message: impl fmt::Display,
    source: Option<rawr::diagnostic::Source>,
) -> rawr::diagnostic::Report {
    let builder = rawr::diagnostic::Report::error(code, message.to_string());
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake as WakeTrait, Waker};
    use std::time::{Duration, Instant};

    use iced::futures::Stream;
    use rawr::reload::scheduler::{self, publication};

    use super::{mailbox, Completion, Input, Outcome, Wake};

    #[test]
    fn polled_edge_without_handler_keeps_one_generation_and_withdraws_replaced_stages() {
        let fixture = Fixture::new("bounded");
        let gate = scheduler::Gate::new(Duration::ZERO);
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(fixture.first.source()),
        ));
        let (sender, inbox) = mailbox::channel();
        let mut messages = mailbox::messages(&inbox);
        let waker = Waker::noop();
        let mut wake = None;
        let mut edges = 0;
        let now = Instant::now();

        gate.observe(now).unwrap();
        let slow_failure = gate
            .start(now)
            .unwrap()
            .complete(failure("obsolete compile failure"));

        for _ in 0..12 {
            let generation = fixture
                .engine
                .compile_package_generation(&fixture.target)
                .unwrap();
            let snapshot = rawr::reload::source::Snapshot::from_graph(generation.source());
            gate.observe(now).unwrap();
            let candidate = gate
                .start(now)
                .unwrap()
                .complete((generation, snapshot))
                .stage_inputs(&publication);

            assert!(sender.send(&gate, Completion::new(candidate)).unwrap());
            assert_eq!(sender.pending(), 1);
            assert_eq!(publication.read().staged(), 1);
            match poll(&mut messages, waker) {
                Poll::Ready(Some(edge)) => {
                    edges += 1;
                    wake = Some(edge);
                }
                Poll::Pending => {}
                Poll::Ready(None) => panic!("an open mailbox must retain its edge stream"),
            }
        }

        assert!(!sender
            .send(&gate, Completion::failed(slow_failure))
            .unwrap());
        assert_eq!(sender.pending(), 1);
        assert_eq!(publication.read().staged(), 1);

        assert_eq!(edges, 1);
        let completion = inbox.take_latest(wake.unwrap()).unwrap();
        assert!(matches!(
            completion.into_input(),
            Input::Revision(scheduler::mailbox::Revision::Candidate(candidate))
                if matches!(candidate.value(), Outcome::Candidate(_, _))
        ));
        assert_eq!(sender.pending(), 0);
        assert_eq!(publication.read().staged(), 0);

        sender.shutdown();
        assert_eq!(sender.pending(), 0);
        assert_eq!(publication.read().staged(), 0);
    }

    #[test]
    fn current_failure_replaces_obsolete_pending_and_slow_old_finish_stays_out() {
        let gate = scheduler::Gate::new(Duration::ZERO);
        let (sender, inbox) = mailbox::channel();
        let mut messages = mailbox::messages(&inbox);
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wakes));
        let now = Instant::now();

        assert!(matches!(poll(&mut messages, &waker), Poll::Pending));

        gate.observe(now).unwrap();
        let slow_old = gate.start(now).unwrap().complete(failure("slow old"));
        gate.observe(now).unwrap();
        let pending_old = gate.start(now).unwrap().complete(failure("pending old"));
        assert!(sender.send(&gate, Completion::failed(pending_old)).unwrap());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);

        gate.observe(now).unwrap();
        let newest = gate.start(now).unwrap().complete(failure("newest failure"));
        assert!(sender.send(&gate, Completion::failed(newest)).unwrap());
        assert_eq!(sender.pending(), 1);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(!sender.send(&gate, Completion::failed(slow_old)).unwrap());

        let Poll::Ready(Some(wake)) = poll(&mut messages, &waker) else {
            panic!("the newest current failure must be delivered");
        };
        let completion = inbox.take_latest(wake).unwrap();
        let Input::Revision(scheduler::mailbox::Revision::Candidate(candidate)) =
            completion.into_input()
        else {
            panic!("a worker failure must retain its revision");
        };
        assert!(matches!(
            candidate.value(),
            Outcome::Failed(error) if error.message() == "newest failure"
        ));
        assert!(matches!(poll(&mut messages, &waker), Poll::Pending));
    }

    #[test]
    fn checking_and_reloading_share_one_edge_and_keep_only_the_latest_notice() {
        let gate = scheduler::Gate::new(Duration::ZERO);
        let (sender, inbox) = mailbox::channel();
        let mut messages = mailbox::messages(&inbox);
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wakes));
        let now = Instant::now();

        assert!(matches!(poll(&mut messages, &waker), Poll::Pending));
        let observed = gate.observe(now).unwrap();
        assert!(sender.checking(&gate, observed));
        assert_eq!(sender.pending(), 1);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);

        let request = gate.start(now).unwrap();
        assert!(sender.reloading(&gate, &request));
        assert_eq!(sender.pending(), 1);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);

        let Poll::Ready(Some(wake)) = poll(&mut messages, &waker) else {
            panic!("the first progress publication must emit one edge");
        };
        let completion = inbox.take_latest(wake).unwrap();
        let Input::Revision(scheduler::mailbox::Revision::Notice(notice)) = completion.into_input()
        else {
            panic!("progress must remain non-install notice authority");
        };
        assert_eq!(notice.into_inner(), super::status::Phase::Reloading);
        drop(request);
    }

    #[test]
    fn another_gate_rejects_progress_failure_and_success_publication() {
        let fixture = Fixture::new("cross-gate");
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(fixture.first.source()),
        ));
        let (sender, _inbox) = mailbox::channel();
        let now = Instant::now();

        let origin = scheduler::Gate::new(Duration::ZERO);
        let foreign = scheduler::Gate::new(Duration::ZERO);
        let observed = origin.observe(now).unwrap();
        let request = origin.start(now).unwrap();
        foreign.observe(now).unwrap();
        assert!(!sender.checking(&foreign, observed));
        assert!(!sender.reloading(&foreign, &request));
        assert!(!sender
            .send(
                &foreign,
                Completion::failed(request.complete(failure("foreign failure"))),
            )
            .unwrap());
        assert_eq!(sender.pending(), 0);

        let origin = scheduler::Gate::new(Duration::ZERO);
        let foreign = scheduler::Gate::new(Duration::ZERO);
        origin.observe(now).unwrap();
        foreign.observe(now).unwrap();
        let generation = fixture
            .engine
            .compile_package_generation(&fixture.target)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(generation.source());
        let candidate = origin
            .start(now)
            .unwrap()
            .complete((generation, snapshot))
            .stage_inputs(&publication);
        assert_eq!(publication.read().staged(), 1);
        assert!(!sender.send(&foreign, Completion::new(candidate)).unwrap());
        assert_eq!(sender.pending(), 0);
        assert_eq!(publication.read().staged(), 0);
    }

    #[test]
    fn retry_clicks_coalesce_without_a_command_queue() {
        let fixture = Fixture::new("retry");
        let gate = Arc::new(scheduler::Gate::new(Duration::ZERO));
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(fixture.first.source()),
        ));
        let (transport, service) = super::channel(gate, publication, fixture.engine.clone());
        let (_sender, _feedback, retry) = transport.into_parts();

        for _ in 0..12 {
            service.retry();
        }
        assert!(retry.take());
        assert!(!retry.take());
    }

    #[test]
    fn candidate_then_terminal_uses_the_same_wake_and_pulls_terminal() {
        let gate = scheduler::Gate::new(Duration::ZERO);
        let (sender, inbox) = mailbox::channel();
        let mut messages = mailbox::messages(&inbox);
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wakes));
        let now = Instant::now();

        assert!(matches!(poll(&mut messages, &waker), Poll::Pending));
        gate.observe(now).unwrap();
        let pending = gate
            .start(now)
            .unwrap()
            .complete(failure("obsolete failure"));
        assert!(sender.send(&gate, Completion::failed(pending)).unwrap());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);

        sender.terminate(failure("watcher stopped")).unwrap();
        assert_eq!(sender.pending(), 1);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        let Poll::Ready(Some(wake)) = poll(&mut messages, &waker) else {
            panic!("terminal failure must replace the pending revision");
        };
        let completion = inbox.take_latest(wake).unwrap();
        let Input::Terminal(error) = completion.into_input() else {
            panic!("terminal failure must not be queued behind obsolete work");
        };
        assert_eq!(error.message(), "watcher stopped");
        assert!(matches!(poll(&mut messages, &waker), Poll::Ready(None)));
    }

    #[test]
    fn close_before_wake_handler_retires_pending_stage_and_makes_wake_stale() {
        let fixture = Fixture::new("close");
        let gate = scheduler::Gate::new(Duration::ZERO);
        let publication = Arc::new(publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(fixture.first.source()),
        ));
        let (sender, inbox) = mailbox::channel();
        let mut messages = mailbox::messages(&inbox);
        let now = Instant::now();

        let generation = fixture
            .engine
            .compile_package_generation(&fixture.target)
            .unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(generation.source());
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete((generation, snapshot))
            .stage_inputs(&publication);
        assert!(sender.send(&gate, Completion::new(candidate)).unwrap());
        let Poll::Ready(Some(wake)) = poll(&mut messages, Waker::noop()) else {
            panic!("the first full transition must emit one edge wake");
        };
        assert_eq!(sender.pending(), 1);
        assert_eq!(publication.read().staged(), 1);

        inbox.close();

        assert_eq!(sender.pending(), 0);
        assert_eq!(publication.read().staged(), 0);
        assert!(inbox.take_latest(wake).is_none());
    }

    fn poll(messages: &mut mailbox::Messages, waker: &Waker) -> Poll<Option<Wake>> {
        Pin::new(messages).poll_next(&mut Context::from_waker(waker))
    }

    fn failure(message: &str) -> rawr::diagnostic::Report {
        rawr::diagnostic::Report::error(
            rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            message.to_owned(),
        )
        .finish()
    }

    struct Wakes(AtomicUsize);

    impl WakeTrait for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Fixture {
        root: std::path::PathBuf,
        target: rawr::project::Target,
        engine: rawr::Shared<rawr::Engine>,
        first: rawr::reload::Candidate,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-reload-mailbox-{}-{:?}-{name}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            crate::write_marked(&root, "src/main.rs", source());
            let target = rawr::project::Target::discover(&root).unwrap();
            let mut compiler = rawr::Engine::new();
            crate::install(&mut compiler).unwrap();
            let engine = rawr::Shared::new(compiler);
            let first = engine.compile_package_generation(&target).unwrap();
            Self {
                root,
                target,
                engine,
                first,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    fn source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("bounded") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }
}
