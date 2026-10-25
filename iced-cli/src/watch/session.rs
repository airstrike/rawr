//! Bounded latest-wins compilation and deterministic watcher shutdown.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rawr::reload::scheduler::Request;

use super::{Adapter, Notice};

const IDLE_POLL: Duration = Duration::from_millis(25);
const MAX_ACTIVE: usize = 2;

/// One live watcher/compiler session joined before its native host is retired.
pub(crate) struct Session {
    gate: Arc<rawr::reload::scheduler::Gate>,
    shutdown: mpsc::Sender<()>,
    supervisor: Option<thread::JoinHandle<bool>>,
}

/// Failure while starting or joining the bounded reload session.
#[derive(Debug)]
pub(crate) enum Error {
    Spawn(std::io::Error),
    Panicked,
}

struct Pool {
    active: usize,
    queued: Option<Request>,
}

struct Worker {
    id: u64,
    handle: thread::JoinHandle<()>,
}

struct Finished {
    id: u64,
    completion: rawr_iced::reload::Completion,
}

trait Work: Clone + Send + 'static {
    fn perform(&self, worker: u64, request: Request) -> rawr_iced::reload::Completion;
}

#[derive(Clone)]
struct Compiler {
    target: PathBuf,
    engine: rawr::Shared<rawr::Engine>,
    layer: Arc<Mutex<crate::base::Session>>,
    timing: rawr::timing::Session,
    control: Arc<super::Control>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    /// The program's parse cells, remembering: a reload parses only the
    /// sources whose text changed.
    cells: Option<Arc<Mutex<rawr::package::Cells>>>,
}

struct Supervisor<W> {
    work: W,
    adapter: Adapter,
    control: Arc<super::Control>,
    delivery: rawr_iced::reload::Sender,
    feedback: mpsc::Receiver<rawr_iced::reload::Feedback>,
    retry: rawr_iced::reload::Retry,
    shutdown: mpsc::Receiver<()>,
    finished: mpsc::Receiver<Finished>,
    returned: mpsc::Sender<Finished>,
    workers: BTreeMap<u64, Worker>,
    next: u64,
    pool: Pool,
    panicked: bool,
    /// The background `cargo check` of a Cargo crate, told of each save.
    cargo: Option<rawr_cli::cargo::Background>,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        target: PathBuf,
        engine: rawr::Shared<rawr::Engine>,
        layer: crate::base::Session,
        adapter: Adapter,
        transport: rawr_iced::reload::Transport,
        timing: rawr::timing::Session,
        cells: rawr::package::Cells,
        cargo: Option<rawr_cli::cargo::Background>,
    ) -> Result<Self, Error> {
        let gate = adapter.gate();
        let publication = adapter.publication();
        let control = adapter.control();
        let (delivery, feedback, retry) = transport.into_parts();
        let (shutdown, stopped) = mpsc::channel();
        let (returned, finished) = mpsc::channel();
        let work = Compiler {
            target,
            engine,
            layer: Arc::new(Mutex::new(layer)),
            timing,
            control: Arc::clone(&control),
            publication: Arc::clone(&publication),
            cells: Some(Arc::new(Mutex::new(cells.remember()))),
        };
        let supervisor = Supervisor {
            work,
            adapter,
            control,
            delivery,
            feedback,
            retry,
            shutdown: stopped,
            finished,
            returned,
            workers: BTreeMap::new(),
            next: 0,
            pool: Pool::new(),
            panicked: false,
            cargo,
        };
        let handle = thread::Builder::new()
            .name("rawr-reload-supervisor".into())
            .spawn(move || supervisor.run())
            .map_err(Error::Spawn)?;
        Ok(Self {
            gate,
            shutdown,
            supervisor: Some(handle),
        })
    }

    pub(crate) fn finish(mut self) -> Result<(), Error> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.gate.suspend();
        match self.shutdown.send(()) {
            Ok(()) | Err(_) => {}
        }
        let Some(supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        match supervisor.join() {
            Ok(false) => Ok(()),
            Ok(true) | Err(_) => Err(Error::Panicked),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.stop());
    }
}

impl<W: Work> Supervisor<W> {
    fn run(mut self) -> bool {
        loop {
            if self.shutdown.try_recv().is_ok() {
                break;
            }
            self.drain_finished();
            self.drain_feedback();
            let now = Instant::now();
            if self.retry.take() {
                match self.adapter.gate().observe(now) {
                    Ok(observed) => {
                        self.pool.revoke_queued();
                        self.delivery
                            .checking(self.adapter.gate().as_ref(), observed);
                    }
                    Err(rawr::reload::scheduler::Error::Suspended) => {}
                    Err(error) => {
                        let report = environment_report(error);
                        eprintln!("{}", rawr::diagnostic::render(&report));
                        self.adapter.gate().suspend();
                        self.terminate(report);
                        break;
                    }
                }
            }
            if let Some(request) = self.adapter.start(now) {
                self.delivery
                    .reloading(self.adapter.gate().as_ref(), &request);
                self.schedule(request);
            }
            let timeout = self.adapter.deadline().map_or(IDLE_POLL, |deadline| {
                deadline.saturating_duration_since(now)
            });
            match self.adapter.notices().recv_timeout(timeout.min(IDLE_POLL)) {
                Ok(Notice::Changed(observed)) => {
                    if let Some(cargo) = &self.cargo {
                        cargo.saved();
                    }
                    self.pool.revoke_queued();
                    self.delivery
                        .checking(self.adapter.gate().as_ref(), observed);
                }
                Ok(Notice::Backend(error)) => {
                    let error = environment_report(format!("Rawr reload watcher stopped: {error}"));
                    eprintln!("{}", rawr::diagnostic::render(&error));
                    self.adapter.gate().suspend();
                    self.terminate(error);
                    break;
                }
                Ok(Notice::Schedule(error)) => {
                    let error =
                        environment_report(format!("Rawr reload scheduler stopped: {error}"));
                    eprintln!("{}", rawr::diagnostic::render(&error));
                    self.adapter.gate().suspend();
                    self.terminate(error);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let error =
                        environment_report("Rawr reload watcher event channel disconnected");
                    eprintln!("{}", rawr::diagnostic::render(&error));
                    self.adapter.gate().suspend();
                    self.terminate(error);
                    break;
                }
            }
        }
        self.stop_accepting();
        for (_, worker) in std::mem::take(&mut self.workers) {
            self.panicked |= worker.handle.join().is_err();
        }
        self.retire_finished();
        self.drain_feedback();
        self.panicked
    }

    fn schedule(&mut self, request: Request) {
        if let Some(request) = self.pool.schedule(request) {
            self.spawn(request);
        }
    }

    fn spawn(&mut self, request: Request) {
        let id = self.next;
        let Some(next) = self.next.checked_add(1) else {
            let error = environment_report("Rawr reload worker sequence is exhausted");
            eprintln!("{}", rawr::diagnostic::render(&error));
            self.adapter.gate().suspend();
            self.terminate(error);
            self.pool.queued = None;
            self.pool.active = self.pool.active.saturating_sub(1);
            return;
        };
        self.next = next;
        let work = self.work.clone();
        let returned = self.returned.clone();
        let builder = thread::Builder::new().name(format!("rawr-reload-{id}"));
        match builder.spawn(move || {
            let finished = Finished {
                id,
                completion: work.perform(id, request),
            };
            drop(returned.send(finished));
        }) {
            Ok(handle) => {
                self.workers.insert(id, Worker { id, handle });
            }
            Err(error) => {
                let error =
                    environment_report(format!("cannot start Rawr reload compiler: {error}"));
                eprintln!("{}", rawr::diagnostic::render(&error));
                self.adapter.gate().suspend();
                self.terminate(error);
                self.pool.finished();
            }
        }
    }

    fn drain_finished(&mut self) {
        while let Ok(finished) = self.finished.try_recv() {
            let _ = self.finish_one(finished);
        }
    }

    fn finish_one(&mut self, finished: Finished) -> bool {
        let Finished { id, completion } = finished;
        if let Some(worker) = self.workers.remove(&id) {
            debug_assert_eq!(worker.id, id);
            self.panicked |= worker.handle.join().is_err();
        }
        let published = match self.delivery.send(self.adapter.gate().as_ref(), completion) {
            Ok(true) => true,
            Ok(false) => {
                self.reconcile();
                false
            }
            Err(completion) => {
                drop(completion);
                self.reconcile();
                false
            }
        };
        if let Some(request) = self.pool.finished() {
            self.spawn(request);
        }
        published
    }

    fn stop_accepting(&mut self) {
        self.adapter.gate().suspend();
        self.pool.queued = None;
        self.delivery.shutdown();
    }

    fn drain_feedback(&mut self) {
        while let Ok(feedback) = self.feedback.try_recv() {
            match feedback {
                rawr_iced::reload::Feedback::Installed(retired) => {
                    self.reconcile();
                    retired.release();
                }
                rawr_iced::reload::Feedback::Settled => self.reconcile(),
                rawr_iced::reload::Feedback::Failed(error) => {
                    eprintln!(
                        "Rawr reload kept the last good generation:\n{}",
                        rawr::diagnostic::render(&error)
                    );
                    self.reconcile();
                }
                rawr_iced::reload::Feedback::Exiting => self.reconcile(),
            }
        }
    }

    fn retire_finished(&mut self) {
        while let Ok(finished) = self.finished.try_recv() {
            drop(finished.completion);
            let queued = self.pool.finished();
            drop(queued);
        }
    }

    fn reconcile(&self) {
        for error in self.control.reconcile() {
            eprintln!("Rawr reload watcher warning: {error}");
        }
    }

    fn terminate(&self, error: rawr::diagnostic::Report) {
        if let Err(completion) = self.delivery.terminate(error) {
            drop(completion);
        }
    }
}

impl Work for Compiler {
    fn perform(&self, _worker: u64, request: Request) -> rawr_iced::reload::Completion {
        compile(
            self.target.clone(),
            self.engine.clone(),
            &self.layer,
            Arc::clone(&self.control),
            Arc::clone(&self.publication),
            Some(&self.timing),
            self.cells.as_deref(),
            request,
        )
    }
}

impl Pool {
    const fn new() -> Self {
        Self {
            active: 0,
            queued: None,
        }
    }

    fn schedule(&mut self, request: Request) -> Option<Request> {
        if self.active < MAX_ACTIVE {
            self.active += 1;
            Some(request)
        } else {
            self.queued = Some(request);
            None
        }
    }

    fn revoke_queued(&mut self) {
        self.queued = None;
    }

    fn finished(&mut self) -> Option<Request> {
        self.active = self.active.saturating_sub(1);
        match self.queued.take() {
            Some(request) => {
                self.active += 1;
                Some(request)
            }
            None => None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile(
    target: PathBuf,
    engine: rawr::Shared<rawr::Engine>,
    layer: &Mutex<crate::base::Session>,
    control: Arc<super::Control>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    timing: Option<&rawr::timing::Session>,
    cells: Option<&Mutex<rawr::package::Cells>>,
    request: Request,
) -> rawr_iced::reload::Completion {
    let authority = request.complete(());
    let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let target = rawr::project::Target::discover(target)
            .map_err(|error| rawr::diagnostic::Report::from_project(&error))?;
        let fallback = rawr::diagnostic::Source::new(
            target.source().path().to_string_lossy().into_owned(),
            target.source().text(),
        )
        .map_err(environment_report)?;
        // One parse per reload: the layer's facts and the compile share it.
        let load = || match cells {
            Some(cells) => rawr::package::Graph::load_with_cells(
                &engine,
                &target,
                &cells
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            ),
            None => rawr::package::Graph::load(&engine, &target),
        };
        let graph = match timing {
            Some(timing) => timing.observe_result(rawr::timing::Phase::ParseItems, load),
            None => load(),
        }
        .map_err(|error| super::launch::package_report(&error, fallback.clone()))?;
        layer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .judge(&graph)?;
        engine
            .compile_loaded_package_generation(graph, timing)
            .map_err(|error| super::launch::package_report(&error, fallback))
    }));
    let candidate = match compiled {
        Ok(Ok(candidate)) => candidate,
        Ok(Err(error)) => return rawr_iced::reload::Completion::failed(authority.map(|()| error)),
        Err(_) => {
            return rawr_iced::reload::Completion::failed(
                authority.map(|()| environment_report("Rawr reload compiler worker panicked")),
            );
        }
    };
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let candidate = authority
        .map(|()| (candidate, snapshot))
        .stage_inputs(&publication);
    let watched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        control.prepare(candidate.value().1.inputs())
    }));
    match watched {
        Ok(Ok(())) => rawr_iced::reload::Completion::new(candidate),
        Ok(Err(error)) => {
            rawr_iced::reload::Completion::failed(candidate.map(|(generation, staged)| {
                let source = generation.source().entry_source().diagnostic().clone();
                drop(staged);
                drop(generation);
                super::launch::report(
                    rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                    error,
                    Some(source),
                )
            }))
        }
        Err(_) => rawr_iced::reload::Completion::failed(candidate.map(|(generation, staged)| {
            let source = generation.source().entry_source().diagnostic().clone();
            drop(staged);
            drop(generation);
            super::launch::report(
                rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                "Rawr reload watcher registration panicked",
                Some(source),
            )
        })),
    }
}

fn environment_report(message: impl fmt::Display) -> rawr::diagnostic::Report {
    super::launch::report(
        rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
        message,
        None,
    )
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "cannot start Rawr reload session: {error}"),
            Self::Panicked => formatter.write_str("Rawr reload thread panicked during shutdown"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Panicked => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rawr::reload::scheduler::Gate;
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::{mpsc, Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::{compile, Compiler, Pool, Supervisor, Work, MAX_ACTIVE};

    #[derive(Clone)]
    struct Controlled {
        compiler: Compiler,
        releases: Arc<(Mutex<BTreeSet<u64>>, Condvar)>,
        started: mpsc::Sender<u64>,
    }

    struct Harness {
        root: std::path::PathBuf,
        supervisor: Supervisor<Controlled>,
        service: rawr_iced::reload::Service,
        releases: Arc<(Mutex<BTreeSet<u64>>, Condvar)>,
        started: mpsc::Receiver<u64>,
        timing: rawr::timing::Session,
    }

    impl Work for Controlled {
        fn perform(
            &self,
            worker: u64,
            request: rawr::reload::scheduler::Request,
        ) -> rawr_iced::reload::Completion {
            self.started
                .send(worker)
                .expect("the deterministic worker harness must remain connected");
            let (releases, changed) = self.releases.as_ref();
            let mut releases = releases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while !releases.remove(&worker) {
                releases = changed
                    .wait(releases)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            drop(releases);
            self.compiler.perform(worker, request)
        }
    }

    impl Harness {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-supervisor-stress-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            crate::write_marked(&root, "src/main.rs", source(label));

            let mut compiler = rawr::Engine::new();
            rawr_iced::install(&mut compiler).unwrap();
            let engine = rawr::Shared::new(compiler);
            let target = rawr::project::Target::discover(&root).unwrap();
            let first = engine.compile_package_generation(&target).unwrap();
            let adapter = super::super::Adapter::open(first.source(), Duration::ZERO).unwrap();
            let gate = adapter.gate();
            let publication = adapter.publication();
            let control = adapter.control();
            let timing = rawr::timing::Session::start();
            let (transport, service) = rawr_iced::reload::channel_observed(
                gate,
                Arc::clone(&publication),
                engine.clone(),
                timing.clone(),
            );
            let (delivery, feedback, retry) = transport.into_parts();
            let (_shutdown, stopped) = mpsc::channel();
            let (returned, finished) = mpsc::channel();
            let compiler = Compiler {
                target: root.clone(),
                engine,
                layer: Arc::new(Mutex::new(iced_session())),
                timing: timing.clone(),
                control: Arc::clone(&control),
                publication,
                cells: None,
            };
            let releases = Arc::new((Mutex::new(BTreeSet::new()), Condvar::new()));
            let (started, starts) = mpsc::channel();
            let work = Controlled {
                compiler,
                releases: Arc::clone(&releases),
                started,
            };
            let supervisor = Supervisor {
                work,
                adapter,
                control,
                delivery,
                feedback,
                retry,
                shutdown: stopped,
                finished,
                returned,
                workers: std::collections::BTreeMap::new(),
                next: 0,
                pool: Pool::new(),
                panicked: false,
                cargo: None,
            };
            Self {
                root,
                supervisor,
                service,
                releases,
                started: starts,
                timing,
            }
        }

        fn schedule(&mut self, now: Instant) {
            let gate = self.supervisor.adapter.gate();
            gate.observe(now).unwrap();
            let request = gate.start(now).unwrap();
            self.supervisor.schedule(request);
        }

        fn release(&self, worker: u64) {
            let (releases, changed) = self.releases.as_ref();
            releases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(worker);
            changed.notify_all();
        }

        fn finish_one(&mut self) -> bool {
            let finished = self
                .supervisor
                .finished
                .recv()
                .expect("a released deterministic worker must finish");
            self.supervisor.finish_one(finished)
        }

        fn cleanup(mut self) {
            self.supervisor.stop_accepting();
            drop(self.service);
            fs::remove_dir_all(self.root).ok();
        }
    }

    #[test]
    fn pool_limits_active_work_and_keeps_only_the_newest_queue() {
        let gate = Gate::new(Duration::ZERO);
        let now = Instant::now();
        let request = || {
            gate.observe(now).unwrap();
            gate.start(now).unwrap()
        };
        let mut pool = Pool::new();
        assert!(pool.schedule(request()).is_some());
        assert!(pool.schedule(request()).is_some());
        assert_eq!(pool.active, MAX_ACTIVE);
        assert!(pool.schedule(request()).is_none());
        let newest = request();
        assert!(pool.schedule(newest).is_none());
        assert!(pool.queued.is_some());
        let queued = pool.finished().unwrap();
        assert!(matches!(
            gate.retain_current(queued.complete(())),
            rawr::reload::scheduler::Admission::Current(_)
        ));
        assert_eq!(pool.active, MAX_ACTIVE);
    }

    #[test]
    fn relevant_event_discards_obsolete_queued_work_without_touching_active_jobs() {
        let gate = Gate::new(Duration::ZERO);
        let now = Instant::now();
        let request = || {
            gate.observe(now).unwrap();
            gate.start(now).unwrap()
        };
        let mut pool = Pool::new();
        let _ = pool.schedule(request());
        let _ = pool.schedule(request());
        let _ = pool.schedule(request());
        pool.revoke_queued();
        assert!(pool.queued.is_none());
        assert!(pool.finished().is_none());
        assert_eq!(pool.active, 1);
    }

    #[test]
    fn newest_on_disk_recompile_rejects_the_prior_structured_failure_without_sleeping() {
        let root = std::env::temp_dir().join(format!(
            "rawr-retry-newest-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Initial"));

        let mut compiler = rawr::Engine::new();
        rawr_iced::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let target = rawr::project::Target::discover(&root).unwrap();
        let first = engine.compile_package_generation(&target).unwrap();
        let adapter = super::super::Adapter::manual(first.source(), Duration::ZERO).unwrap();
        let gate = adapter.gate();
        let publication = adapter.publication();
        let control = adapter.control();
        let (transport, service) =
            rawr_iced::reload::channel(Arc::clone(&gate), Arc::clone(&publication), engine.clone());
        let (delivery, _feedback, _retry) = transport.into_parts();
        let now = Instant::now();

        crate::write_marked(&root, "src/main.rs", "pub fn main( {");
        let observed = gate.observe(now).unwrap();
        assert!(delivery.checking(&gate, observed));
        let request = gate.start(now).unwrap();
        assert!(delivery.reloading(&gate, &request));
        let layer = Mutex::new(iced_session());
        let invalid = compile(
            root.clone(),
            engine.clone(),
            &layer,
            Arc::clone(&control),
            Arc::clone(&publication),
            None,
            None,
            request,
        );
        assert_eq!(
            invalid.report().map(rawr::diagnostic::Report::category),
            Some(rawr::diagnostic::Category::Parse)
        );

        crate::write_marked(&root, "src/main.rs", source("Newest"));
        let observed = gate.observe(now).unwrap();
        assert!(delivery.checking(&gate, observed));
        let request = gate.start(now).unwrap();
        assert!(delivery.reloading(&gate, &request));
        let newest = compile(
            root.clone(),
            engine,
            &layer,
            control,
            publication,
            None,
            None,
            request,
        );
        assert!(newest.report().is_none());

        assert!(!delivery.send(&gate, invalid).unwrap());
        assert!(delivery.send(&gate, newest).unwrap());
        drop(service);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn twelve_revision_burst_bounds_real_workers_and_publishes_only_newest_rawr_compile() {
        let mut harness = Harness::new("Burst");
        let now = Instant::now();
        for _ in 0..12 {
            harness.schedule(now);
        }

        let mut active = [
            harness.started.recv().unwrap(),
            harness.started.recv().unwrap(),
        ];
        active.sort_unstable();
        assert_eq!(active, [0, 1]);
        assert_eq!(harness.supervisor.pool.active, MAX_ACTIVE);
        assert_eq!(harness.supervisor.workers.len(), MAX_ACTIVE);
        assert!(harness.supervisor.pool.queued.is_some());

        harness.release(1);
        assert!(!harness.finish_one());
        assert_eq!(harness.started.recv().unwrap(), 2);
        assert_eq!(harness.supervisor.pool.active, MAX_ACTIVE);
        assert_eq!(harness.supervisor.workers.len(), MAX_ACTIVE);

        harness.release(2);
        assert!(harness.finish_one());
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 1);

        harness.release(0);
        assert!(!harness.finish_one());
        assert!(harness.supervisor.workers.is_empty());
        assert_eq!(harness.supervisor.pool.active, 0);
        assert!(harness.started.try_recv().is_err());
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 1);
        assert_eq!(
            harness
                .timing
                .snapshot()
                .aggregate(rawr::timing::Phase::ParseItems)
                .count(),
            3
        );
        assert_eq!(
            harness
                .timing
                .snapshot()
                .aggregate(rawr::timing::Phase::Resolve)
                .count(),
            3
        );

        harness.supervisor.stop_accepting();
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 0);
        harness.cleanup();
    }

    #[test]
    fn fast_second_rawr_worker_publishes_before_slow_first_without_stale_replacement() {
        let mut harness = Harness::new("Inversion");
        let now = Instant::now();
        harness.schedule(now);
        harness.schedule(now);
        let mut active = [
            harness.started.recv().unwrap(),
            harness.started.recv().unwrap(),
        ];
        active.sort_unstable();
        assert_eq!(active, [0, 1]);

        harness.release(1);
        assert!(harness.finish_one());
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 1);
        harness.release(0);
        assert!(!harness.finish_one());
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 1);
        assert!(harness.supervisor.workers.is_empty());

        harness.supervisor.stop_accepting();
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 0);
        harness.cleanup();
    }

    #[test]
    fn sticky_shutdown_retires_blocked_rawr_workers_without_starting_queue_or_publishing() {
        let mut harness = Harness::new("Shutdown");
        let now = Instant::now();
        harness.schedule(now);
        harness.schedule(now);
        harness.schedule(now);
        let mut active = [
            harness.started.recv().unwrap(),
            harness.started.recv().unwrap(),
        ];
        active.sort_unstable();
        assert_eq!(active, [0, 1]);
        assert!(harness.supervisor.pool.queued.is_some());

        harness.supervisor.stop_accepting();
        assert!(harness.supervisor.pool.queued.is_none());
        harness.release(0);
        harness.release(1);
        assert!(!harness.finish_one());
        assert!(!harness.finish_one());

        assert!(harness.supervisor.workers.is_empty());
        assert_eq!(harness.supervisor.pool.active, 0);
        assert!(harness.started.try_recv().is_err());
        assert_eq!(harness.supervisor.adapter.publication().read().staged(), 0);
        assert!(harness.supervisor.feedback.try_recv().is_err());
        harness.cleanup();
    }

    fn iced_session() -> crate::base::Session {
        crate::base::session(rawr::layer::Sheet::default())
    }

    #[test]
    fn a_reload_that_drops_iced_asks_for_a_restart_and_keeps_the_layer_otherwise() {
        let root = std::env::temp_dir().join(format!(
            "rawr-reload-layer-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", source("Initial"));

        let loaded = crate::base::load(Some(&root)).unwrap();
        let mut compiler = rawr::Engine::new();
        rawr_iced::install(&mut compiler).unwrap();
        let engine = rawr::Shared::new(compiler);
        let first = engine
            .compile_loaded_package_generation(loaded.graph, None)
            .unwrap();
        let layer = Mutex::new(crate::base::session(loaded.sheet));
        let adapter = super::super::Adapter::manual(first.source(), Duration::ZERO).unwrap();
        let gate = adapter.gate();
        let publication = adapter.publication();
        let control = adapter.control();
        let (transport, service) =
            rawr_iced::reload::channel(Arc::clone(&gate), Arc::clone(&publication), engine.clone());
        let (delivery, _feedback, _retry) = transport.into_parts();
        let reload = |text: &str| {
            crate::write_marked(&root, "src/main.rs", text);
            let now = Instant::now();
            let observed = gate.observe(now).unwrap();
            assert!(delivery.checking(&gate, observed));
            let request = gate.start(now).unwrap();
            assert!(delivery.reloading(&gate, &request));
            let completion = compile(
                root.clone(),
                engine.clone(),
                &layer,
                Arc::clone(&control),
                Arc::clone(&publication),
                None,
                None,
                request,
            );
            let report = completion.report().cloned();
            assert!(delivery.send(&gate, completion).is_ok());
            report
        };

        // An edit that keeps iced reloads as before.
        assert_eq!(reload(&source("Edited")), None);
        // Taking iced out: the session cannot become a base program.
        let dropped = reload(r#"fn main() { println!("plain"); }"#).unwrap();
        assert!(
            dropped
                .message()
                .contains("this program no longer uses `iced`"),
            "{dropped:?}"
        );
        // Bringing it back reloads again.
        assert_eq!(reload(&source("Back")), None);
        drop(service);
        fs::remove_dir_all(root).ok();
    }

    fn source(label: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<Message> {{ text("{label}") }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }
}
