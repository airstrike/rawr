//! Narrow raw filesystem-event transport for exact current and staged inputs.
//!
//! The native watcher's thread only stamps each event and sends it to the
//! adapter's inbox. The thread that owns the adapter classifies, observes
//! and admits. Every query that decides a start, an admission or a report
//! first takes every event already sent, so an event the OS delivered before
//! an admission always revokes it, and one delivered after it issues the
//! next revision.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use rawr::reload::scheduler;

use super::{input, native};

/// One owner's native watcher, its inbox and its admission gate.
pub struct Adapter {
    inbox: Receiver<Wake>,
    /// What the native callbacks and each [`Waker`] send on.
    sender: Sender<Wake>,
    watch: Watch,
    gate: scheduler::Gate,
    filter: Filter,
    history: native::History,
    /// This adapter's identity, which its staged inputs carry.
    origin: Arc<Origin>,
    /// The first terminal notice, until [`Adapter::wait`] reports it.
    failed: Option<Notice>,
    /// How many relevant events the tests' adapter observed.
    #[cfg(test)]
    observed: u64,
}

/// One adapter's identity.
struct Origin;

/// What wakes the owner.
enum Wake {
    /// One native event, stamped when the watcher's thread received it, so
    /// the debounce runs from the OS's delivery and not from the owner's
    /// next look.
    Event(Instant, notify::Result<notify::Event>),
    /// Something outside the watched files happened, such as the owner's
    /// program ending: the owner looks again.
    Nudge,
}

/// Wakes the adapter's owner from another thread, without a change of its
/// own: the owner then looks at whatever it waits on besides files.
#[derive(Clone)]
pub struct Waker(Sender<Wake>);

/// One authority-free candidate input union, kept relevant until the owner
/// commits or withdraws it, or the adapter's next start withdraws it.
#[must_use = "staged watcher inputs must be committed or withdrawn"]
pub struct StagedInputs {
    origin: Arc<Origin>,
    stamp: u64,
}

struct Watch {
    watcher: notify::RecommendedWatcher,
    roots: BTreeMap<PathBuf, bool>,
    /// What the watcher registered for `roots`: see [`registration`].
    registered: BTreeMap<PathBuf, bool>,
    /// Where each native watcher sends its events; `None` for a watcher
    /// whose native callbacks are disabled.
    deliver: Option<Sender<Wake>>,
}

enum Filter {
    /// One run's whole graph, fixed for the watcher's life: rawr-cli's
    /// watcher restarts the program at each relevant save. A save is
    /// source-only when every path it names is either no input or a Rawr
    /// source with no input or recursive root beneath it.
    Graph(rawr::reload::source::Inputs),
    Direct(Direct),
}

struct Direct {
    current: Arc<rawr::reload::source::Inputs>,
    staged: Vec<DirectEntry>,
    next: u64,
}

struct DirectEntry {
    stamp: u64,
    inputs: Arc<rawr::reload::source::Inputs>,
    /// Whether the filter held these inputs already when they were staged:
    /// otherwise they registered roots a withdrawal retires.
    held: Held,
}

/// One terminal watcher notice for the owning session.
#[derive(Debug)]
pub enum Notice {
    /// The native watcher reported an asynchronous backend failure.
    Backend(notify::Error),
    /// The deterministic scheduler could not issue the relevant revision.
    Schedule(scheduler::Error),
}

/// Failure while opening or reconciling the exact graph watcher.
#[derive(Debug)]
pub enum Error {
    /// The platform's recommended raw watcher could not start.
    Start(notify::Error),
    /// The OS refused the stream over the roots the watcher asked for.
    Stream {
        /// Each registered root, a directory watched whole or for its own
        /// entries, or a path watched by name.
        paths: Box<[PathBuf]>,
        /// The OS's refusal, as the native watcher reports it.
        source: notify::Error,
    },
    /// A canonical parent directory could not be registered.
    Watch {
        /// Canonical non-recursive transport directory.
        path: PathBuf,
        /// Native watcher failure.
        source: notify::Error,
    },
    /// A no-longer-relevant transport directory could not be removed.
    Unwatch {
        /// Canonical non-recursive transport directory.
        path: PathBuf,
        /// Native watcher failure.
        source: notify::Error,
    },
    /// Only a direct imported-owner watcher can replace its complete filter.
    FixedFilter,
    /// The direct staged-input sequence cannot issue another identity.
    StageExhausted,
    /// A staged input proof belongs to another watcher or is no longer live.
    StaleStage,
}

/// Whether a filter held staged inputs already, so the roots they need are
/// registered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Held {
    Already,
    New,
}

impl Held {
    const fn of(held: bool) -> Self {
        if held {
            Self::Already
        } else {
            Self::New
        }
    }
}

impl Adapter {
    /// Open a raw watcher over one generation's exact resolved graph inputs.
    pub fn open(graph: &rawr::reload::source::Graph, debounce: Duration) -> Result<Self, Error> {
        Self::with_filter(Filter::graph(graph), debounce, true)
    }

    /// Open the generated-owner watcher over an already sealed Rawr/native
    /// input union. Replacements update this filter only after all new watcher
    /// roots have registered successfully.
    pub fn open_inputs(
        inputs: rawr::reload::source::Inputs,
        debounce: Duration,
    ) -> Result<Self, Error> {
        Self::with_filter(Filter::direct(inputs), debounce, true)
    }

    /// Open a watcher whose native callbacks are disabled for deterministic tests.
    #[cfg(feature = "manual")]
    pub fn manual(graph: &rawr::reload::source::Graph, debounce: Duration) -> Result<Self, Error> {
        Self::with_filter(Filter::graph(graph), debounce, false)
    }

    /// Open a direct input filter with deterministic callback delivery for tests.
    #[cfg(feature = "manual")]
    pub fn manual_inputs(
        inputs: rawr::reload::source::Inputs,
        debounce: Duration,
    ) -> Result<Self, Error> {
        Self::with_filter(Filter::direct(inputs), debounce, false)
    }

    /// Take a deterministic data event as the native watcher would deliver it.
    #[cfg(feature = "manual")]
    pub fn inject_modify(&mut self, path: &Path) {
        self.take(
            Instant::now(),
            Ok(
                notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Content,
                )))
                .add_path(path.to_path_buf()),
            ),
        );
    }

    /// Observe and start one request at once, for deterministic tests.
    #[cfg(feature = "manual")]
    pub fn observe_and_start(
        &mut self,
        now: Instant,
    ) -> Result<Option<scheduler::Request>, scheduler::Error> {
        self.drain();
        self.gate.observe_and_start(now)
    }

    fn with_filter(filter: Filter, debounce: Duration, native: bool) -> Result<Self, Error> {
        let (sender, inbox) = mpsc::channel();
        let deliver = native.then(|| sender.clone());
        let mut watcher = start(deliver.clone()).map_err(Error::Start)?;
        let roots = filter.wanted().roots()?;
        let registered = registration(&roots, &filter.watched());
        register_all(&mut watcher, &registered)?;
        Ok(Self {
            inbox,
            sender,
            watch: Watch {
                watcher,
                roots,
                registered,
                deliver,
            },
            gate: scheduler::Gate::new(debounce),
            filter,
            history: native::History::default(),
            origin: Arc::new(Origin),
            failed: None,
            #[cfg(test)]
            observed: 0,
        })
    }

    /// A handle that wakes this adapter's owner from another thread.
    #[must_use]
    pub fn waker(&self) -> Waker {
        Waker(self.sender.clone())
    }

    /// Wait for the next wake: until the debounce of a pending save ends
    /// while one is pending, and for as long as it takes otherwise. Every
    /// event already sent is taken before this returns. The first terminal
    /// notice is returned once, before any wait.
    pub fn wait(&mut self) -> Result<(), Notice> {
        if let Some(notice) = self.failed.take() {
            return Err(notice);
        }
        let wake = match self.gate.deadline() {
            Some(deadline) => {
                match self
                    .inbox
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(wake) => Some(wake),
                    // The debounce ended: the owner starts the save.
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => {
                        unreachable!("the adapter holds a sender of its own inbox")
                    }
                }
            }
            None => Some(
                self.inbox
                    .recv()
                    .expect("the adapter holds a sender of its own inbox"),
            ),
        };
        if let Some(wake) = wake {
            self.receive(wake);
        }
        self.drain();
        self.failed.take().map_or(Ok(()), Err)
    }

    /// Take every wake already sent.
    fn drain(&mut self) {
        while let Ok(wake) = self.inbox.try_recv() {
            self.receive(wake);
        }
    }

    fn receive(&mut self, wake: Wake) {
        match wake {
            Wake::Event(at, event) => self.take(at, event),
            Wake::Nudge => {}
        }
    }

    /// Classify one native event delivered at `at`, and observe it when it
    /// is relevant.
    fn take(&mut self, at: Instant, event: notify::Result<notify::Event>) {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                self.gate.suspend();
                self.fail(Notice::Backend(error));
                return;
            }
        };
        let change = input::Change::from_event(&event);
        // Creation of an absent input's parent is relevant even though the
        // new directory is not itself an exact input file.
        let change = match (event.kind, change) {
            (notify::EventKind::Create(_), input::Change::Exact(paths)) => {
                input::Change::Tree(paths)
            }
            (_, change) => change,
        };
        let (relevant, source_only) = self.filter.classify(&change);
        if !source_only {
            self.history.roots_moved();
        }
        if matches!(change, input::Change::Rescan) {
            self.history.rescan();
        } else if relevant && !source_only {
            self.history.invalidate(
                change
                    .known_paths()
                    .and_then(<[_]>::first)
                    .map(PathBuf::as_path),
            );
        }
        if !relevant {
            return;
        }
        match self.gate.observe(at) {
            Ok(()) => {
                #[cfg(test)]
                {
                    self.observed += 1;
                }
            }
            Err(scheduler::Error::Suspended) => {}
            Err(error) => self.fail(Notice::Schedule(error)),
        }
    }

    /// Keep the first terminal notice for the owner's next wait.
    fn fail(&mut self, notice: Notice) {
        if self.failed.is_none() {
            self.failed = Some(notice);
        }
    }

    /// Return the newest outstanding request's debounce deadline.
    #[must_use]
    pub fn deadline(&mut self) -> Option<Instant> {
        self.drain();
        self.gate.deadline()
    }

    /// Observe a revision with no file event, as a save does: the owner
    /// attempts the program again.
    pub fn observe(&mut self, now: Instant) -> Result<(), scheduler::Error> {
        self.drain();
        self.gate.observe(now)
    }

    /// Start the newest outstanding request after its debounce deadline.
    /// Inputs still staged then belong to a finished attempt, and are
    /// withdrawn.
    #[must_use]
    pub fn start(&mut self, now: Instant) -> Option<scheduler::Request> {
        self.drain();
        let retired = self.filter.withdraw_all() == Some(Held::New);
        let request = self.gate.start(now);
        // Rearm after parent creation even if the candidate later fails to
        // compile, so subsequent children remain observable. Only an event
        // that may have created or removed a directory moves the roots, and
        // only inputs staged with roots of their own leave some behind: a
        // save that writes a Rawr source derives none (#1604).
        let moved = request.is_some() && self.history.take_roots_moved();
        if !moved && !retired {
            return request;
        }
        let errors = self.reconcile();
        if !errors.is_empty() {
            self.history.roots_moved();
        }
        for error in errors {
            match error {
                Error::Watch { path, source } => {
                    self.gate.suspend();
                    self.fail(Notice::Backend(source.add_path(path)));
                    return None;
                }
                error => eprintln!("warning: file watcher: {error}"),
            }
        }
        request
    }

    /// Whether `request` is still the newest revision, after every event
    /// already sent.
    #[must_use]
    pub fn is_current(&mut self, request: &scheduler::Request) -> bool {
        self.drain();
        self.gate.request_is_current(request)
    }

    /// Capture native invalidation history before full native validation.
    /// Retain this only with the owner successfully admitted by that work.
    #[must_use]
    pub fn native_baseline(&mut self) -> native::Baseline {
        self.drain();
        let (paths, trees) = self.filter.native();
        self.history.capture(native::Stamp::capture(paths, trees))
    }

    /// Inspect observed native invalidation after the original request begins.
    /// This grants neither current Rust freshness nor admission authority:
    /// subsequent events still revoke the request through the gate.
    #[must_use]
    pub fn resident_eligible(&mut self, baseline: &native::Baseline) -> bool {
        self.resident_change(baseline).is_none()
    }

    /// The watcher's word that no native input changed since `baseline`,
    /// `None` when one may have.
    #[must_use]
    pub fn unchanged(&mut self, baseline: &native::Baseline) -> Option<native::Unchanged> {
        self.resident_eligible(baseline)
            .then_some(native::Unchanged(()))
    }

    /// Why a save can't take the resident route against `baseline`, `None`
    /// when it can (see [`Self::resident_eligible`]).
    #[must_use]
    pub fn resident_change(&mut self, baseline: &native::Baseline) -> Option<native::Change> {
        self.drain();
        if self.gate.checkpoint().is_err() {
            return Some(native::Change::Suspended);
        }
        self.history.change(baseline)
    }

    /// Capture the exact watcher event sequence before process work.
    pub fn checkpoint(&mut self) -> Result<rawr::reload::scheduler::Checkpoint, scheduler::Error> {
        self.drain();
        self.gate.checkpoint()
    }

    /// Admit one direct imported-owner slot transition only if no event was
    /// observed since [`Self::checkpoint`], after every event already sent.
    pub fn admit_checkpoint<T, U>(
        &mut self,
        checkpoint: rawr::reload::scheduler::Checkpoint,
        value: T,
        admit: impl FnOnce(T) -> U,
    ) -> scheduler::Admission<T, U> {
        self.drain();
        self.gate.admit_checkpoint(checkpoint, value, admit)
    }

    /// Admit one request-bound direct owner transition only while its
    /// revision is the newest, after every event already sent. The callback
    /// must remain a short structural transition: no filesystem or store
    /// I/O, IPC, destruction, or watcher re-entry.
    pub fn admit_current<T, U>(
        &mut self,
        candidate: scheduler::Candidate<T>,
        admit: impl FnOnce(T) -> U,
    ) -> scheduler::Admission<T, U> {
        self.drain();
        self.gate.admit_current(candidate, admit)
    }

    /// Register a candidate filter's roots without changing current event
    /// relevance before the generated owner admits that candidate. Inputs
    /// the filter already holds, current or staged, register nothing: a
    /// save that changes only a source's text stages no root (#1604).
    pub fn prepare_inputs(
        &mut self,
        inputs: impl Into<Arc<rawr::reload::source::Inputs>>,
    ) -> Result<StagedInputs, Error> {
        let inputs = inputs.into();
        let (stamp, held) = self.filter.stage(Arc::clone(&inputs))?;
        if held == Held::New
            && let Err(error) = self.prepare(&inputs)
        {
            self.filter.withdraw(stamp);
            drop(self.reconcile());
            return Err(error);
        }
        Ok(StagedInputs {
            origin: Arc::clone(&self.origin),
            stamp,
        })
    }

    /// Publish one admitted generated-owner filter and retire roots no longer
    /// reachable from the last-good generation. When the admitted inputs
    /// are the very ones already current, every root still stands.
    pub fn commit_inputs(&mut self, staged: StagedInputs) -> Result<Vec<Error>, Error> {
        if !Arc::ptr_eq(&staged.origin, &self.origin) {
            return Err(Error::StaleStage);
        }
        if self.filter.commit(staged.stamp)? == Held::Already {
            return Ok(Vec::new());
        }
        Ok(self.reconcile())
    }

    /// Withdraw inputs an attempt staged and will not commit, and retire the
    /// roots only they needed.
    pub fn withdraw(&mut self, staged: StagedInputs) -> Vec<Error> {
        if !Arc::ptr_eq(&staged.origin, &self.origin) {
            return Vec::new();
        }
        match self.filter.withdraw(staged.stamp) {
            Some(Held::New) => self.reconcile(),
            Some(Held::Already) | None => Vec::new(),
        }
    }

    /// Register a successor over the full transport union while retaining the
    /// incumbent. No registration occurs when the union is unchanged.
    fn prepare(&mut self, inputs: &rawr::reload::source::Inputs) -> Result<(), Error> {
        let result = (|| {
            let mut wanted = Wanted::default();
            wanted.add(inputs);
            let added = wanted.roots()?;
            let mut desired = self.watch.roots.clone();
            for (path, recursive) in added {
                desired
                    .entry(path)
                    .and_modify(|current| *current |= recursive)
                    .or_insert(recursive);
            }
            self.watch.replace(desired, &self.filter.watched())
        })();
        match result {
            Ok(replaced) => {
                if replaced {
                    self.history.registered();
                }
                Ok(())
            }
            Err(error) => {
                self.gate.suspend();
                Err(error)
            }
        }
    }

    /// Reconcile the union without mutating registrations on the incumbent.
    fn reconcile(&mut self) -> Vec<Error> {
        let result = self
            .filter
            .wanted()
            .roots()
            .and_then(|desired| self.watch.replace(desired, &self.filter.watched()));
        match result {
            Ok(replaced) => {
                if replaced {
                    self.history.registered();
                }
                Vec::new()
            }
            Err(error) => {
                self.gate.suspend();
                vec![error]
            }
        }
    }
}

impl Waker {
    /// Wake the owner.
    pub fn wake(&self) {
        // A send fails only once the adapter is gone, and with it the owner.
        let _ = self.0.send(Wake::Nudge);
    }

    /// Wake the owner when the child process `pid` ends, without reaping
    /// it: the owner reaps it as it does on any wake. A thread waits for the
    /// end in the kernel.
    #[cfg(unix)]
    pub fn wake_on_exit(&self, pid: u32) {
        let waker = self.clone();
        let spawned = std::thread::Builder::new()
            .name("rawr-exit".into())
            .spawn(move || {
                super::exit::wait(pid);
                waker.wake();
            });
        // A thread that can't start leaves the owner to see the end at its
        // next wake from a file.
        drop(spawned);
    }
}

/// Start a native watcher that sends each event to `deliver`, or drops it.
fn start(deliver: Option<Sender<Wake>>) -> notify::Result<notify::RecommendedWatcher> {
    match deliver {
        Some(sender) => notify::recommended_watcher(move |event| {
            // A send fails only once the adapter is gone.
            let _ = sender.send(Wake::Event(Instant::now(), event));
        }),
        None => notify::recommended_watcher(|_event| {}),
    }
}

impl Watch {
    /// Register `desired`, and say whether a successor replaced the
    /// incumbent watcher.
    fn replace(
        &mut self,
        desired: BTreeMap<PathBuf, bool>,
        exact: &BTreeSet<PathBuf>,
    ) -> Result<bool, Error> {
        let deliver = self.deliver.clone();
        self.replace_with(desired, exact, || start(deliver))
    }

    fn replace_with(
        &mut self,
        desired: BTreeMap<PathBuf, bool>,
        exact: &BTreeSet<PathBuf>,
        start: impl FnOnce() -> notify::Result<notify::RecommendedWatcher>,
    ) -> Result<bool, Error> {
        let registered = registration(&desired, exact);
        if self.registered == registered {
            self.roots = desired;
            return Ok(false);
        }
        // The incumbent remains alive through every fallible successor step.
        let mut successor = start().map_err(Error::Start)?;
        register_all(&mut successor, &registered)?;
        // The successor cannot see events queued before its SinceNow origin.
        // Deliver those incumbent events to the inbox before retiring it.
        #[cfg(target_os = "macos")]
        self.watcher.drain().map_err(Error::Start)?;
        let retired = std::mem::replace(&mut self.watcher, successor);
        self.roots = desired;
        self.registered = registered;
        drop(retired);
        Ok(self.deliver.is_some())
    }
}

fn register_all(
    watcher: &mut notify::RecommendedWatcher,
    roots: &BTreeMap<PathBuf, bool>,
) -> Result<(), Error> {
    if roots.is_empty() {
        return Ok(());
    }
    let mut registration = watcher.paths_mut();
    for (path, recursive) in roots {
        registration
            .add(path, mode(*recursive))
            .map_err(|source| Error::Watch {
                path: path.clone(),
                source,
            })?;
    }
    registration.commit().map_err(|source| Error::Stream {
        paths: roots.keys().cloned().collect(),
        source,
    })
}

/// The paths the native watcher registers for `roots`. FSEvents watches a
/// directory's whole subtree, whatever the mode: a parent directory watched
/// for its own entries (a package root beside its `target`, a home, `/` for
/// a missing Cargo config) would deliver every event below it, rawr's own
/// writes under `target/rawr` among them, and under a busy disk the stream
/// lags seconds and drops into rescans. So only a recursive root registers
/// as a directory; every other watched path, an input file or a missing
/// tree, registers by name, which FSEvents reports whether or not it exists
/// yet.
#[cfg(target_os = "macos")]
fn registration(
    roots: &BTreeMap<PathBuf, bool>,
    watched: &BTreeSet<PathBuf>,
) -> BTreeMap<PathBuf, bool> {
    let trees = roots
        .iter()
        .filter(|(_, recursive)| **recursive)
        .map(|(root, _)| root)
        .collect::<Vec<_>>();
    let mut registered = trees
        .iter()
        .map(|root| ((*root).clone(), true))
        .collect::<BTreeMap<_, _>>();
    for path in watched {
        if !trees.iter().any(|tree| path.starts_with(tree)) {
            registered.insert(path.clone(), false);
        }
    }
    registered
}

#[cfg(not(target_os = "macos"))]
fn registration(
    roots: &BTreeMap<PathBuf, bool>,
    _watched: &BTreeSet<PathBuf>,
) -> BTreeMap<PathBuf, bool> {
    roots.clone()
}

fn mode(recursive: bool) -> RecursiveMode {
    if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    }
}

/// What the watcher's roots derive from: each input's directory, whether a
/// manifest lies in it, the recursive trees, and the directories nothing
/// under which is watched.
#[derive(Default)]
struct Wanted {
    directories: BTreeMap<PathBuf, bool>,
    trees: BTreeSet<PathBuf>,
    unwatched: BTreeSet<PathBuf>,
}

impl Wanted {
    fn add(&mut self, inputs: &rawr::reload::source::Inputs) {
        self.add_paths(inputs.paths(), inputs.trees(), inputs.unwatched());
    }

    fn add_paths(&mut self, paths: &[PathBuf], trees: &[PathBuf], unwatched: &[PathBuf]) {
        // Sibling files repeat their parent: keep each directory once.
        for path in paths {
            let Some(parent) = path.parent() else {
                continue;
            };
            let manifest = path
                .file_name()
                .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock");
            match self.directories.get_mut(parent) {
                Some(current) => *current |= manifest,
                None => {
                    self.directories.insert(parent.to_path_buf(), manifest);
                }
            }
        }
        self.trees.extend(trees.iter().cloned());
        self.unwatched.extend(unwatched.iter().cloned());
    }

    /// The directories the native watcher watches, a handful however many
    /// files the inputs hold. A manifest's directory is watched for its own
    /// entries, since it holds the package's `target`; so is any directory
    /// with no manifest's directory above it (a Cargo config's, a home's).
    /// Any other input directory below a manifest's (the package's `src/`,
    /// a marked file's or a native module's directory, a path dependency's
    /// `src/`) is watched whole and stands for every directory beneath it.
    /// Nothing under an unwatched directory is watched, and no recursive
    /// root holds one: a dependency from the registry or git arrives as a
    /// change to `Cargo.lock`. The filter drops each event on a path that
    /// is no input.
    fn roots(&self) -> Result<BTreeMap<PathBuf, bool>, Error> {
        let under = |path: &Path| self.unwatched.iter().any(|dir| path.starts_with(dir));
        let holds = |path: &Path| self.unwatched.iter().any(|dir| dir.starts_with(path));
        let manifests = self
            .directories
            .iter()
            .filter(|(directory, manifest)| **manifest && !under(directory))
            .map(|(directory, _)| directory.as_path())
            .collect::<BTreeSet<_>>();
        let mut wanted = BTreeMap::<&Path, bool>::new();
        for (directory, manifest) in &self.directories {
            if under(directory) {
                continue;
            }
            let recursive = !*manifest
                && !holds(directory)
                && directory
                    .ancestors()
                    .skip(1)
                    .any(|ancestor| manifests.contains(ancestor));
            *wanted.entry(directory).or_default() |= recursive;
        }
        for tree in &self.trees {
            if !under(tree) {
                *wanted.entry(tree).or_default() = true;
            }
        }
        let whole = wanted
            .iter()
            .filter(|(_, recursive)| **recursive)
            .map(|(directory, _)| *directory)
            .collect::<Vec<_>>();
        let mut roots = BTreeMap::new();
        for (path, recursive) in wanted {
            if whole
                .iter()
                .any(|tree| path != *tree && path.starts_with(tree))
            {
                continue;
            }
            let root = existing_directory(path)?;
            // A missing recursive input needs only its next component's
            // creation event; recursively watching a distant ancestor could
            // cover the disk.
            let recursive = recursive && root.as_path() == path;
            roots
                .entry(root)
                .and_modify(|current| *current |= recursive)
                .or_insert(recursive);
        }
        Ok(roots)
    }
}

/// How many roots this process's watchers derived from their inputs, each
/// by finding the directory that exists for it.
static DERIVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many watcher roots this process derived from inputs so far: a save
/// that changes only a marked source's text derives none (#1604).
#[must_use]
pub fn derived_roots() -> usize {
    DERIVED.load(std::sync::atomic::Ordering::Relaxed)
}

fn existing_directory(path: &Path) -> Result<PathBuf, Error> {
    DERIVED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for candidate in path.ancestors() {
        match std::fs::metadata(candidate) {
            Ok(metadata) if metadata.is_dir() => return Ok(candidate.to_path_buf()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(source) => {
                return Err(Error::Watch {
                    path: candidate.to_path_buf(),
                    source: notify::Error::io(source),
                })
            }
        }
    }
    Err(Error::Watch {
        path: path.to_path_buf(),
        source: notify::Error::generic("input has no existing directory ancestor"),
    })
}

/// Whether a graph filter's `inputs` give `path` only a Rawr source role:
/// it is an input, a source alone, and no input or recursive root lies
/// beneath it.
fn graph_source_only(inputs: &rawr::reload::source::Inputs, path: &Path) -> bool {
    inputs.contains(path)
        && inputs.is_source_only(path)
        && !inputs
            .paths()
            .iter()
            .any(|input| input.as_path() != path && input.starts_with(path))
        && !inputs.trees().iter().any(|tree| tree.starts_with(path))
}

impl Filter {
    fn graph(graph: &rawr::reload::source::Graph) -> Self {
        let inputs = rawr::reload::source::Inputs::from_graph(graph);
        Self::Graph(inputs)
    }

    fn direct(inputs: rawr::reload::source::Inputs) -> Self {
        Self::Direct(Direct {
            current: Arc::new(inputs),
            staged: Vec::new(),
            next: 0,
        })
    }

    #[cfg(test)]
    fn relevant(&self, change: &input::Change) -> bool {
        self.classify(change).0
    }

    fn classify(&self, change: &input::Change) -> (bool, bool) {
        match self {
            Self::Graph(inputs) => {
                let relevant = change.relevant(inputs)
                    || match change {
                        input::Change::Tree(paths) | input::Change::Unknown(paths) => paths
                            .iter()
                            .any(|path| inputs.trees().iter().any(|tree| tree.starts_with(path))),
                        _ => false,
                    };
                let source_only = change.known_paths().is_some_and(|paths| {
                    paths.iter().all(|path| {
                        let relevant = match change {
                            input::Change::Exact(_) | input::Change::Metadata(_) => {
                                inputs.contains(path)
                            }
                            input::Change::Tree(_) => {
                                input::affects(inputs, path)
                                    || inputs.trees().iter().any(|tree| tree.starts_with(path))
                            }
                            _ => false,
                        };
                        !relevant || graph_source_only(inputs, path)
                    })
                });
                (relevant, source_only)
            }
            Self::Direct(state) => {
                let relevant = change.relevant_inputs(&state.current)
                    || state
                        .staged
                        .iter()
                        .any(|entry| change.relevant_inputs(&entry.inputs));
                let source_only = change.known_paths().is_some_and(|paths| {
                    paths.iter().all(|path| {
                        for inputs in std::iter::once(&state.current)
                            .chain(state.staged.iter().map(|entry| &entry.inputs))
                        {
                            let descendants = !matches!(change, input::Change::Metadata(_))
                                && inputs
                                    .paths()
                                    .iter()
                                    .any(|input| input != path && input.starts_with(path));
                            let trees = inputs.trees().iter().any(|tree| tree.starts_with(path));
                            let relevant = inputs.contains(path)
                                || descendants
                                || matches!(change, input::Change::Tree(_)) && trees;
                            if relevant && (!inputs.is_source_only(path) || descendants || trees) {
                                return false;
                            }
                        }
                        true
                    })
                });
                (relevant, source_only)
            }
        }
    }

    fn wanted(&self) -> Wanted {
        let mut wanted = Wanted::default();
        match self {
            Self::Graph(inputs) => wanted.add(inputs),
            Self::Direct(state) => {
                for inputs in std::iter::once(&state.current)
                    .chain(state.staged.iter().map(|entry| &entry.inputs))
                {
                    wanted.add(inputs);
                }
            }
        }
        wanted
    }

    /// Every exact input path and recursive root, current and staged, that
    /// lies under no unwatched directory: what the watcher stands for, by
    /// path.
    fn watched(&self) -> BTreeSet<PathBuf> {
        let kept = |unwatched: &[PathBuf], path: &PathBuf| {
            !unwatched
                .iter()
                .any(|directory| path.starts_with(directory))
        };
        match self {
            Self::Graph(inputs) => inputs
                .paths()
                .iter()
                .chain(inputs.trees())
                .filter(|path| kept(inputs.unwatched(), path))
                .cloned()
                .collect(),
            Self::Direct(state) => {
                let inputs = || {
                    std::iter::once(&state.current)
                        .chain(state.staged.iter().map(|entry| &entry.inputs))
                };
                let unwatched = inputs()
                    .flat_map(|inputs| inputs.unwatched().iter().cloned())
                    .collect::<Vec<_>>();
                inputs()
                    .flat_map(|inputs| inputs.paths().iter().chain(inputs.trees()))
                    .filter(|path| kept(&unwatched, path))
                    .cloned()
                    .collect()
            }
        }
    }

    /// Every non-source input path and recursive root, current and staged.
    fn native(&self) -> (Box<[PathBuf]>, Box<[PathBuf]>) {
        match self {
            Self::Graph(inputs) => (
                inputs
                    .paths()
                    .iter()
                    .filter(|path| !inputs.is_source_only(path))
                    .cloned()
                    .collect(),
                inputs.trees().into(),
            ),
            Self::Direct(state) => {
                let mut paths = BTreeSet::new();
                let mut trees = BTreeSet::new();
                for inputs in std::iter::once(&state.current)
                    .chain(state.staged.iter().map(|entry| &entry.inputs))
                {
                    paths.extend(
                        inputs
                            .paths()
                            .iter()
                            .filter(|path| !inputs.is_source_only(path))
                            .cloned(),
                    );
                    trees.extend(inputs.trees().iter().cloned());
                }
                (paths.into_iter().collect(), trees.into_iter().collect())
            }
        }
    }

    /// Stage `inputs`, and say whether the filter held these inputs
    /// already, the very ones current or staged or the current ones' equal,
    /// whose roots then stand.
    fn stage(&mut self, inputs: Arc<rawr::reload::source::Inputs>) -> Result<(u64, Held), Error> {
        let Self::Direct(state) = self else {
            return Err(Error::FixedFilter);
        };
        let held = Held::of(
            std::iter::once(&state.current)
                .chain(state.staged.iter().map(|entry| &entry.inputs))
                .any(|held| Arc::ptr_eq(held, &inputs))
                || state.current == inputs,
        );
        let stamp = state.next;
        state.next = state.next.checked_add(1).ok_or(Error::StageExhausted)?;
        state.staged.push(DirectEntry {
            stamp,
            inputs,
            held,
        });
        Ok((stamp, held))
    }

    /// Make the staged inputs current, and say whether they were the inputs
    /// current already.
    fn commit(&mut self, stamp: u64) -> Result<Held, Error> {
        let Self::Direct(state) = self else {
            return Err(Error::FixedFilter);
        };
        let index = state
            .staged
            .iter()
            .position(|entry| entry.stamp == stamp)
            .ok_or(Error::StaleStage)?;
        let entry = state.staged.swap_remove(index);
        // `Arc`'s equality compares the pointers first.
        let held = Held::of(state.current == entry.inputs);
        state.current = entry.inputs;
        Ok(held)
    }

    /// Withdraw the inputs staged as `stamp`, and say whether the filter
    /// held them already when they were staged; `None` when none are.
    fn withdraw(&mut self, stamp: u64) -> Option<Held> {
        let Self::Direct(state) = self else {
            return None;
        };
        let index = state.staged.iter().position(|entry| entry.stamp == stamp)?;
        Some(state.staged.swap_remove(index).held)
    }

    /// Withdraw every staged input, and say whether any registered roots
    /// of its own (`New`); `None` when none were staged.
    fn withdraw_all(&mut self) -> Option<Held> {
        let Self::Direct(state) = self else {
            return None;
        };
        state
            .staged
            .drain(..)
            .map(|entry| entry.held)
            .reduce(|held, next| if next == Held::New { next } else { held })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "cannot start the file watcher: {error}"),
            Self::Stream { paths, source } => {
                write!(
                    formatter,
                    "cannot start the file watcher: {source}; it asked for {} paths:",
                    paths.len()
                )?;
                for path in paths {
                    write!(formatter, "\n  {}", path.display())?;
                }
                Ok(())
            }
            Self::Watch { path, source } => write!(
                formatter,
                "cannot watch directory `{}`: {source}",
                path.display()
            ),
            Self::Unwatch { path, source } => write!(
                formatter,
                "cannot stop watching directory `{}`: {source}",
                path.display()
            ),
            Self::FixedFilter => {
                formatter.write_str("cannot replace the file watcher's fixed filter")
            }
            Self::StageExhausted => {
                formatter.write_str("the file watcher ran out of stage numbers")
            }
            Self::StaleStage => {
                formatter.write_str("the file watcher's input stage is out of date")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error)
            | Self::Stream { source: error, .. }
            | Self::Watch { source: error, .. }
            | Self::Unwatch { source: error, .. } => Some(error),
            Self::FixedFilter | Self::StageExhausted | Self::StaleStage => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use notify::event::{AccessKind, ModifyKind};
    use notify::{Event, EventKind};

    use super::{Adapter, Filter, Notice};

    impl Adapter {
        /// Take `event` as the inbox delivers it, stamped `now`.
        fn handle_at(&mut self, event: notify::Result<Event>, now: Instant) {
            self.take(now, event);
        }

        /// Whether a relevant event revoked older work since the last call.
        fn changed(&mut self) -> bool {
            std::mem::take(&mut self.observed) > 0
        }
    }

    #[test]
    fn unchanged_roots_do_not_construct_a_successor() {
        let fixture = Fixture::new("unchanged-successor");
        let mut adapter = transport(fixture.inputs.clone(), true);
        let watch = &mut adapter.watch;
        let exact = adapter.filter.watched();
        let desired = watch.roots.clone();
        watch
            .replace_with(desired, &exact, || {
                panic!("unchanged roots restarted observation")
            })
            .unwrap();
    }

    #[test]
    fn failed_successor_preserves_incumbent_roots() {
        let fixture = Fixture::new("failed-successor");
        let mut adapter = transport(fixture.inputs.clone(), true);
        let exact = adapter.filter.watched();
        let watch = &mut adapter.watch;
        let original = watch.roots.clone();
        let mut desired = original.clone();
        // No backend registers a missing tree. FSEvents registers only trees
        // as directories, and a missing path below an existing one by name.
        desired.insert(std::path::PathBuf::from("rawr-absent-root/missing"), true);
        assert!(watch
            .replace_with(desired.clone(), &exact, || Err(notify::Error::generic(
                "start failed"
            )))
            .is_err());
        assert_eq!(watch.roots, original);
        assert!(watch
            .replace_with(desired, &exact, || notify::recommended_watcher(|_| {}))
            .is_err());
        assert_eq!(watch.roots, original);
    }

    #[test]
    fn direct_filter_observes_the_staged_union_until_abort_or_commit() {
        let fixture = Fixture::new("staged-union");
        let current = fixture.inputs.clone();
        let candidate_root = fixture.root.join("candidate");
        fs::create_dir_all(&candidate_root).expect("candidate directory opens");
        let candidate_root =
            fs::canonicalize(candidate_root).expect("candidate directory canonicalizes");
        let candidate_path = candidate_root.join("candidate.rs");
        fs::write(&candidate_path, "candidate").expect("candidate source writes");
        let candidate_path =
            fs::canonicalize(candidate_path).expect("candidate source canonicalizes");
        let candidate = current
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Exact(
                candidate_path.clone(),
            )]);
        let mut filter = Filter::direct(current);
        let change = super::input::Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(candidate_path.clone()),
        );

        assert!(!filter.relevant(&change));
        let (aborted, _) = filter
            .stage(Arc::new(candidate.clone()))
            .expect("candidate stages");
        assert!(filter.relevant(&change));
        assert!(filter.wanted().directories.contains_key(&candidate_root));
        filter.withdraw(aborted);
        assert!(!filter.relevant(&change));
        assert!(!filter.wanted().directories.contains_key(&candidate_root));

        let (committed, _) = filter
            .stage(Arc::new(candidate))
            .expect("candidate restages");
        filter.commit(committed).expect("candidate commits");
        assert!(filter.relevant(&change));
        assert!(filter.wanted().directories.contains_key(&candidate_root));
    }

    #[test]
    fn relevant_event_revokes_before_the_owner_admits() {
        let fixture = Fixture::new("revocation");
        let mut adapter = transport(fixture.inputs.clone(), true);
        let now = Instant::now();
        adapter.gate.observe(now).unwrap();
        let obsolete = adapter.gate.start(now).unwrap().complete("obsolete");
        // The native thread's send: the owner has not looked yet.
        adapter
            .sender
            .send(super::Wake::Event(
                now,
                Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone())),
            ))
            .unwrap();
        assert!(matches!(
            adapter.admit_current(obsolete, |_| panic!("a delivered event revokes")),
            rawr::reload::scheduler::Admission::Obsolete("obsolete")
        ));
        assert!(adapter.start(now).is_some());
    }

    #[test]
    fn a_wake_without_a_change_starts_nothing() {
        let fixture = Fixture::new("nudge");
        let mut adapter = transport(fixture.inputs.clone(), true);
        adapter.waker().wake();
        assert!(adapter.wait().is_ok());
        assert!(adapter.start(Instant::now()).is_none());
    }

    #[test]
    fn access_stays_quiet_and_backend_failure_wakes_the_owner_once() {
        let fixture = Fixture::new("failure");
        let mut adapter = transport(fixture.inputs.clone(), true);
        let now = Instant::now();

        adapter.handle_at(
            Ok(Event::new(EventKind::Access(AccessKind::Read)).add_path(fixture.source.clone())),
            now,
        );
        assert!(adapter.gate.deadline().is_none());
        assert!(!adapter.changed());

        adapter.gate.observe(now).unwrap();
        let obsolete = adapter.gate.start(now).unwrap();
        adapter.handle_at(Err(notify::Error::generic("watch failed")), now);
        assert!(adapter.gate.deadline().is_none());
        assert!(matches!(adapter.wait(), Err(Notice::Backend(_))));

        adapter.handle_at(
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone())),
            now,
        );
        assert!(adapter.gate.deadline().is_none());
        assert!(adapter.start(now).is_none());
        assert!(!adapter.changed());
        assert!(!adapter.is_current(&obsolete));
    }

    #[test]
    fn imported_missing_input_below_an_absent_directory_opens_its_watcher() {
        let fixture = Fixture::new("missing-imported-parent");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let missing = root.join("absent/config.toml");
        let inputs = fixture
            .inputs
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Exact(missing)]);

        assert!(!root.join("absent").exists());
        let adapter = Adapter::open_inputs(inputs, Duration::ZERO)
            .expect("the watcher registers the nearest existing transport directory");
        drop(adapter);
    }

    #[test]
    fn staged_transport_is_retained_until_abort_or_commit() {
        let fixture = Fixture::new("staged-transport");
        let current = fixture.inputs.clone();
        let root = fs::canonicalize(&fixture.root).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let candidate = |path: std::path::PathBuf| {
            current
                .clone()
                .with_imported([rawr_import::input::proof::Watch::Tree(path)])
        };
        let mut adapter = direct(current.clone());
        let first_stage = adapter.prepare_inputs(candidate(first.clone())).unwrap();
        let second_stage = adapter.prepare_inputs(candidate(second.clone())).unwrap();
        assert_eq!(adapter.watch.roots.get(&first), Some(&true));
        assert_eq!(adapter.watch.roots.get(&second), Some(&true));
        assert!(adapter.withdraw(first_stage).is_empty());
        assert!(!adapter.watch.roots.contains_key(&first));
        assert_eq!(adapter.watch.roots.get(&second), Some(&true));
        assert!(adapter.commit_inputs(second_stage).unwrap().is_empty());
        assert_eq!(adapter.watch.roots.get(&second), Some(&true));
        let replacement = adapter.prepare_inputs(current).unwrap();
        assert_eq!(adapter.watch.roots.get(&second), Some(&true));
        assert!(adapter.commit_inputs(replacement).unwrap().is_empty());
        assert!(!adapter.watch.roots.contains_key(&second));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fsevents_register_each_input_by_name_and_only_trees_as_directories() {
        use std::collections::{BTreeMap, BTreeSet};
        use std::path::PathBuf;
        let roots = BTreeMap::from([
            (PathBuf::from("/"), false),
            (PathBuf::from("/work/app"), false),
            (PathBuf::from("/work/app/src"), false),
            (PathBuf::from("/work"), false),
            (PathBuf::from("/registry/dep"), true),
        ]);
        let watched = BTreeSet::from([
            PathBuf::from("/.cargo/config.toml"),
            PathBuf::from("/registry/dep"),
            PathBuf::from("/registry/dep/src/lib.rs"),
            PathBuf::from("/work/app/Cargo.toml"),
            PathBuf::from("/work/app/src/main.rs"),
            PathBuf::from("/work/absent/tree"),
        ]);
        assert_eq!(
            super::registration(&roots, &watched),
            BTreeMap::from([
                (PathBuf::from("/.cargo/config.toml"), false),
                (PathBuf::from("/registry/dep"), true),
                (PathBuf::from("/work/absent/tree"), false),
                (PathBuf::from("/work/app/Cargo.toml"), false),
                (PathBuf::from("/work/app/src/main.rs"), false),
            ])
        );
    }

    /// A package beside its `target`, whose build script reads the whole
    /// package and whose build wrote rawr's copy of the marked file and an
    /// `OUT_DIR`: the watcher stands for the package's inputs by path, and
    /// nothing under `target` reaches it.
    #[test]
    fn a_package_s_watched_set_names_its_inputs_and_leaves_out_its_target() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("package-target");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let target = root.join("target");
        for (path, text) in [
            ("Cargo.toml", "[package]\nname = \"app\"\n"),
            ("build.rs", "fn main() {}\n"),
            ("assets/logo.txt", "logo\n"),
            ("target/rawr/crate/src/main.rs", MAIN),
            ("target/debug/build/app-1/out/icons.rs", "\n"),
            ("target/debug/deps/libapp-1.rmeta", ""),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let native = rawr::reload::source::Inputs::imported([
            Watch::Tree(root.clone()),
            Watch::Exact(root.join("Cargo.toml")),
            Watch::Exact(target.join("rawr/crate/src/main.rs")),
            Watch::Exact(target.join("debug/deps/libapp-1.rmeta")),
            Watch::Tree(target.join("debug/build/app-1/out")),
        ])
        .outside(std::slice::from_ref(&target));
        let inputs = fixture.inputs.clone().with_native(&native);
        let adapter = Adapter::open_inputs(inputs, Duration::ZERO).unwrap();

        let watched = adapter.filter.watched();
        for input in ["Cargo.toml", "build.rs", "src/main.rs", "assets/logo.txt"] {
            let input = root.join(input);
            assert!(
                watched.iter().any(|path| input.starts_with(path)),
                "{} is not watched: {watched:?}",
                input.display()
            );
        }
        let registered = adapter.watch.registered.clone();
        assert!(
            watched
                .iter()
                .chain(registered.keys())
                .all(|path| !path.starts_with(&target)),
            "{watched:?}\n{registered:?}"
        );
        // A directory registered recursively, or on FSEvents at all, covers
        // its subtree: neither may hold `target`.
        assert!(
            registered.iter().all(|(path, recursive)| {
                !target.starts_with(path) || !*recursive && !cfg!(target_os = "macos")
            }),
            "{registered:?}"
        );
        assert!(
            !adapter
                .filter
                .classify(&crate::input::Change::from_event(&source_event(
                    &target.join("rawr/crate/src/main.rs")
                )))
                .0
        );
    }

    #[test]
    fn transport_roots_deduplicate_absent_parents_without_recursive_ancestor_watch() {
        let fixture = Fixture::new("transport");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let absent = root.join("missing/nested");
        let other = root.join("another");
        let mut wanted = super::Wanted::default();
        wanted.add_paths(
            &[absent.join("a.rs"), other.join("b.rs")],
            std::slice::from_ref(&absent),
            &[],
        );
        let roots = wanted.roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(&root), Some(&false));

        fs::create_dir_all(&absent).unwrap();
        let mut wanted = super::Wanted::default();
        wanted.add_paths(&[absent.join("a.rs")], std::slice::from_ref(&absent), &[]);
        let roots = wanted.roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(&absent), Some(&true));
    }

    #[test]
    fn transport_roots_merge_a_repeated_parent_and_its_tree_before_inspection() {
        let fixture = Fixture::new("transport-repeated");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let paths = (0..64)
            .map(|index| nested.join(format!("{index}.rs")))
            .chain([root.join("top.rs")])
            .collect::<Vec<_>>();
        let mut wanted = super::Wanted::default();
        wanted.add_paths(&paths, std::slice::from_ref(&nested), &[]);
        let roots = wanted.roots().unwrap();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots.get(&nested), Some(&true));
        assert_eq!(roots.get(&root), Some(&false));
    }

    /// A package of a few thousand input files under a few directories,
    /// beside a registry checkout of a few thousand more: the watcher
    /// registers the package's manifest directory and its `src/`, a path
    /// dependency's two, and nothing of the registry's. An event on a file
    /// under a root that is no input is dropped; one on an input arrives.
    #[test]
    fn a_big_package_registers_its_few_directories_and_none_of_the_registry_s() {
        use std::collections::BTreeMap;
        use std::path::{Path, PathBuf};
        let fixture = Fixture::new("big-package");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let app = root.join("app");
        let dependency = root.join("dependency");
        let registry = root.join("home/registry/src/index-1");
        let target = app.join("target");
        let mut written = Vec::new();
        let mut write = |path: PathBuf| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "\n").unwrap();
            written.push(path);
        };
        for manifest in ["Cargo.toml", "Cargo.lock"] {
            write(app.join(manifest));
        }
        write(app.join("src/main.rs"));
        write(dependency.join("Cargo.toml"));
        for index in 0..3000 {
            write(app.join(format!("src/module{}/file{index}.rs", index % 4)));
            write(dependency.join(format!("src/file{index}.rs")));
            write(registry.join(format!("crate{}/src/file{index}.rs", index % 40)));
        }
        fs::create_dir_all(target.join("debug")).unwrap();
        let notes = app.join("src/notes.txt");
        fs::write(&notes, "\n").unwrap();
        let native = rawr::reload::source::Inputs::imported(
            written
                .iter()
                .cloned()
                .map(rawr_import::input::proof::Watch::Exact),
        )
        .outside(std::slice::from_ref(&target))
        .with_checkouts(&[root.join("home/registry/src")]);
        let adapter = Adapter::open_inputs(native, Duration::ZERO).unwrap();

        let watch = &adapter.watch;
        assert_eq!(
            watch.roots,
            BTreeMap::from([
                (app.clone(), false),
                (app.join("src"), true),
                (dependency.clone(), false),
                (dependency.join("src"), true),
            ])
        );
        let expected = if cfg!(target_os = "macos") {
            BTreeMap::from([
                (app.join("Cargo.lock"), false),
                (app.join("Cargo.toml"), false),
                (app.join("src"), true),
                (dependency.join("Cargo.toml"), false),
                (dependency.join("src"), true),
            ])
        } else {
            watch.roots.clone()
        };
        assert_eq!(watch.registered, expected);

        let relevant = |path: &Path| {
            adapter
                .filter
                .relevant(&crate::input::Change::from_event(&source_event(path)))
        };
        assert!(!relevant(&notes));
        assert!(!relevant(&target.join("debug/app")));
        assert!(relevant(&app.join("src/module1/file1.rs")));
        assert!(relevant(&dependency.join("src/file7.rs")));
        assert!(relevant(&app.join("Cargo.lock")));
    }

    #[test]
    fn absent_config_creation_rearms_before_a_candidate_can_fail() {
        let fixture = Fixture::new("absent-config");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let config = root.join(".cargo/nested/config.toml");
        let inputs = fixture
            .inputs
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Exact(config.clone())]);
        assert!(inputs.contains(&config));
        assert!(!inputs.contains(&root.join(".cargo")));
        let mut adapter = direct(inputs.clone());
        assert_eq!(adapter.watch.roots.get(&root), Some(&false));
        let now = Instant::now();
        fs::create_dir(root.join("unrelated")).unwrap();
        adapter.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(root.join("unrelated")),
            ),
            now,
        );
        assert!(!adapter.changed());
        assert!(adapter.start(now).is_none());

        for parent in [root.join(".cargo"), root.join(".cargo/nested")] {
            fs::create_dir(&parent).unwrap();
            adapter.handle_at(
                Ok(
                    Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                        .add_path(parent.clone()),
                ),
                now,
            );
            assert!(adapter.changed());
            let candidate = adapter
                .start(now)
                .expect("parent creation queues a candidate");
            assert_eq!(adapter.watch.roots.get(&parent), Some(&false));
            // Failing the candidate must not undo the newly armed transport.
            drop(candidate);
        }
        fs::write(&config, "[build]\njobs = 1\n").unwrap();
        adapter.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::File))
                    .add_path(config.clone()),
            ),
            now,
        );
        assert!(adapter.changed());
        assert!(adapter.start(now).is_some());
        let staged = adapter.prepare_inputs(inputs).unwrap();
        assert!(adapter.commit_inputs(staged).unwrap().is_empty());
        assert!(!adapter.watch.roots.contains_key(&root));
    }

    #[test]
    fn absent_recursive_input_promotes_only_after_its_root_exists() {
        let fixture = Fixture::new("absent-tree");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let tree = root.join("generated/tree");
        let inputs = fixture
            .inputs
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Tree(tree.clone())]);
        let mut adapter = direct(inputs.clone());
        assert_eq!(adapter.watch.roots.get(&root), Some(&false));
        let now = Instant::now();
        fs::create_dir_all(&tree).unwrap();
        adapter.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(root.join("generated")),
            ),
            now,
        );
        assert!(adapter.changed());
        assert!(adapter.start(now).is_some());
        assert_eq!(adapter.watch.roots.get(&tree), Some(&true));
        // Graph filters also retain ancestor relevance for tree inputs.
        let filter = Filter::Graph(inputs);
        assert!(filter.relevant(&super::input::Change::Tree(Box::from([
            root.join("generated")
        ]))));
    }

    #[test]
    fn publication_rearms_from_exact_parents_after_directory_creation() {
        let fixture = Fixture::new("publication-parent");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let parent = root.join(".cargo/nested");
        let inputs =
            fixture
                .inputs
                .clone()
                .with_imported([rawr_import::input::proof::Watch::Exact(
                    parent.join("config.toml"),
                )]);
        let mut adapter = transport(inputs, true);
        assert!(adapter.filter.wanted().directories.contains_key(&parent));
        assert_eq!(adapter.watch.roots.get(&root), Some(&false));
        fs::create_dir_all(&parent).unwrap();
        assert!(adapter.reconcile().is_empty());
        assert_eq!(adapter.watch.roots.get(&parent), Some(&false));
    }

    fn direct(inputs: rawr::reload::source::Inputs) -> super::Adapter {
        transport(inputs, false)
    }

    fn source_event(path: &std::path::Path) -> Event {
        Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(path.to_path_buf())
    }

    #[test]
    fn resident_source_edit_stays_eligible_but_revokes_the_original_request() {
        let fixture = Fixture::new("resident-source");
        let mut adapter = direct(fixture.inputs.clone());
        let baseline = adapter.native_baseline();
        let now = Instant::now();
        adapter.gate.observe(now).unwrap();
        let candidate = adapter.start(now).unwrap().complete(());
        adapter.handle_at(Ok(source_event(&fixture.source)), now);
        assert!(adapter.resident_eligible(&baseline));
        assert!(matches!(
            adapter.admit_current(candidate, |_| panic!("source event must revoke")),
            rawr::reload::scheduler::Admission::Obsolete(())
        ));
        let mut foreign = direct(fixture.inputs.clone());
        assert!(!foreign.resident_eligible(&baseline));
    }

    #[test]
    fn resident_native_invalidation_survives_notice_coalescing_and_input_commit() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-watermark");
        let native = fixture.source.with_file_name("native.rs");
        let inputs = fixture
            .inputs
            .clone()
            .with_imported([Watch::Exact(native.clone())]);
        let mut adapter = direct(inputs.clone());
        let before_validation = adapter.native_baseline();
        let now = Instant::now();
        let staged = adapter.prepare_inputs(inputs.clone()).unwrap();
        adapter.handle_at(Ok(source_event(&native)), now);
        adapter.handle_at(Ok(source_event(&fixture.source)), now);
        assert!(adapter.changed());
        assert!(!adapter.changed());
        assert!(!adapter.resident_eligible(&before_validation));
        adapter.commit_inputs(staged).unwrap();
        assert!(!adapter.resident_eligible(&before_validation));

        // A new full validation can retain a new baseline, but an event after
        // capture and before adoption/commit must remain visible.
        let before_next_validation = adapter.native_baseline();
        let staged = adapter.prepare_inputs(inputs).unwrap();
        assert!(adapter.resident_eligible(&before_next_validation));
        adapter.handle_at(Ok(source_event(&native)), now);
        adapter.commit_inputs(staged).unwrap();
        assert!(!adapter.resident_eligible(&before_next_validation));

        let admitted = adapter.native_baseline();
        let candidate = adapter.start(now).unwrap().complete(());
        assert!(adapter.resident_eligible(&admitted));
        adapter.handle_at(Ok(source_event(&native)), now);
        assert!(matches!(
            adapter.admit_current(candidate, |_| panic!("later native event must revoke")),
            rawr::reload::scheduler::Admission::Obsolete(())
        ));
        assert!(!adapter.resident_eligible(&admitted));
    }

    #[test]
    fn ancestor_attribute_change_keeps_resident_but_input_attribute_change_does_not() {
        use notify::event::MetadataKind;
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-ancestor-attribute");
        let cache = fixture.source.parent().unwrap().join("registry");
        let native = cache.join("src/crate/lib.rs");
        let inputs = fixture
            .inputs
            .clone()
            .with_imported([Watch::Exact(native.clone())]);
        let mut adapter = direct(inputs);
        let now = Instant::now();
        let attribute = |path: &std::path::Path| {
            Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::Extended,
            )))
            .add_path(path.to_path_buf())
        };
        // Cargo tags its registry and git caches on every invocation.
        let baseline = adapter.native_baseline();
        adapter.handle_at(Ok(attribute(&cache)), now);
        assert!(adapter.resident_eligible(&baseline));
        assert!(!adapter.changed());

        adapter.handle_at(Ok(attribute(&native)), now);
        assert!(!adapter.resident_eligible(&baseline));
    }

    #[test]
    fn resident_unknown_pathless_rescan_and_ancestor_events_fail_closed() {
        let fixture = Fixture::new("resident-unknown");
        let mut adapter = direct(fixture.inputs.clone());
        let now = Instant::now();
        // Dropped-event rescans re-stamp the untouched native inputs instead
        // of assuming a change; every uncertain event still revokes the request.
        for (event, dropped) in [
            (
                Event::new(EventKind::Any).add_path(fixture.source.clone()),
                true,
            ),
            (
                Event::new(EventKind::Other).add_path(fixture.source.clone()),
                true,
            ),
            (
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone()),
                false,
            ),
            (
                Event::new(EventKind::Modify(ModifyKind::Other)).add_path(fixture.source.clone()),
                false,
            ),
            (
                Event::new(EventKind::Create(notify::event::CreateKind::File)),
                true,
            ),
            (
                source_event(&fixture.source).set_flag(notify::event::Flag::Rescan),
                true,
            ),
            (
                Event::new(EventKind::Remove(notify::event::RemoveKind::Folder))
                    .add_path(fixture.source.parent().unwrap().to_path_buf()),
                false,
            ),
        ] {
            let baseline = adapter.native_baseline();
            adapter.gate.observe(now).unwrap();
            let candidate = adapter.start(now).unwrap().complete(());
            adapter.handle_at(Ok(event), now);
            assert_eq!(adapter.resident_eligible(&baseline), dropped);
            assert!(matches!(
                adapter.admit_current(candidate, |_| panic!("uncertain event must revoke")),
                rawr::reload::scheduler::Admission::Obsolete(())
            ));
        }
        let baseline = adapter.native_baseline();
        adapter.handle_at(Err(notify::Error::generic("watcher failed")), now);
        assert!(!adapter.resident_eligible(&baseline));
    }

    #[test]
    fn resident_staged_source_role_cannot_hide_current_or_staged_native_overlap() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-overlap");
        let source = fixture.inputs.clone();
        let native = source
            .clone()
            .with_imported([Watch::Exact(fixture.source.clone())]);
        let change = super::input::Change::from_event(&source_event(&fixture.source));
        for (current, candidate) in [(source.clone(), native.clone()), (native, source)] {
            let mut direct = Filter::direct(current.clone());
            direct.stage(Arc::new(candidate.clone())).unwrap();
            assert_eq!(direct.classify(&change), (true, false));
        }
    }

    #[test]
    fn resident_rename_ignores_unrelated_save_temp_paths_in_both_filters() {
        let fixture = Fixture::new("resident-save-temp");
        for published in [false, true] {
            let mut adapter = transport(fixture.inputs.clone(), published);
            let baseline = adapter.native_baseline();
            let now = Instant::now();
            adapter.gate.observe(now).unwrap();
            let candidate = adapter.start(now).unwrap().complete(());
            let event = Event::new(EventKind::Modify(ModifyKind::Name(
                notify::event::RenameMode::Both,
            )))
            .add_path(fixture.source.with_file_name("save-temp"))
            .add_path(fixture.source.clone());
            assert_eq!(
                adapter
                    .filter
                    .classify(&super::input::Change::from_event(&event)),
                (true, true)
            );
            adapter.handle_at(Ok(event), now);
            assert!(adapter.resident_eligible(&baseline));
            assert!(matches!(
                adapter.admit_current(candidate, |_| panic!("rename must revoke old request")),
                rawr::reload::scheduler::Admission::Obsolete(())
            ));
        }
    }

    #[test]
    fn resident_rename_keeps_mixed_native_paths_dirty_in_both_filters() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-mixed-rename");
        let current = fixture.inputs.clone();
        let native = fixture.source.with_file_name("native");
        for published in [false, true] {
            let inputs = current.clone().with_imported([
                Watch::Exact(native.join("exact.rs")),
                Watch::Tree(native.join("tree")),
            ]);
            // A graph filter is fixed, so it holds the native inputs from
            // the start; a direct one stages them.
            let (mut adapter, _staged) = if published {
                (transport(inputs, true), None)
            } else {
                let mut adapter = transport(current.clone(), false);
                let staged = adapter.prepare_inputs(inputs).unwrap();
                (adapter, Some(staged))
            };
            // Exact native files, recursive members, and ancestors of a staged
            // native root all remain relevant even alongside a Rawr endpoint.
            for path in [
                native.join("exact.rs"),
                native.join("tree/new.rs"),
                native.clone(),
            ] {
                let baseline = adapter.native_baseline();
                let event = Event::new(EventKind::Modify(ModifyKind::Name(
                    notify::event::RenameMode::Both,
                )))
                .add_path(path)
                .add_path(fixture.source.clone());
                assert_eq!(
                    adapter
                        .filter
                        .classify(&super::input::Change::from_event(&event)),
                    (true, false)
                );
                adapter.handle_at(Ok(event), Instant::now());
                assert!(!adapter.resident_eligible(&baseline));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn resident_symlink_replacement_preserves_native_identity_in_both_filters() {
        use rawr_import::input::proof::Watch;
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new("resident-native-symlink");
        let native = fixture.source.with_file_name("native.rs");
        let temporary = fixture.source.with_file_name("replace-native");
        let unrelated = fixture.source.with_file_name("unrelated.data");
        fs::write(&unrelated, "unrelated").unwrap();
        for published in [false, true] {
            for target in [&fixture.source, &unrelated] {
                fs::write(&native, "native").unwrap();
                let inputs = fixture
                    .inputs
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let mut adapter = transport(inputs, published);
                let baseline = adapter.native_baseline();
                let now = Instant::now();
                adapter.gate.observe(now).unwrap();
                let candidate = adapter.start(now).unwrap().complete(());

                symlink(target, &temporary).unwrap();
                fs::rename(&temporary, &native).unwrap();
                let event = Event::new(EventKind::Modify(ModifyKind::Name(
                    notify::event::RenameMode::Both,
                )))
                .add_path(temporary.clone())
                .add_path(native.clone());
                assert_eq!(
                    adapter
                        .filter
                        .classify(&super::input::Change::from_event(&event)),
                    (true, false)
                );
                adapter.handle_at(Ok(event), now);
                assert!(!adapter.resident_eligible(&baseline));
                assert!(matches!(
                    adapter.admit_current(candidate, |_| panic!("native symlink must revoke")),
                    rawr::reload::scheduler::Admission::Obsolete(())
                ));
                fs::remove_file(&native).unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn resident_symlink_ancestor_preserves_native_identity_through_event_aliases() {
        use rawr_import::input::proof::Watch;
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new("resident-native-ancestor-symlink");
        let parent = fixture.source.parent().unwrap();
        let directory = parent.join("native-directory");
        let retired = parent.join("retired-directory");
        let native = directory.join("main.rs");
        let unrelated = parent.join("unrelated-directory");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("main.rs"), "unrelated").unwrap();
        let alias = parent.join("event-alias");
        symlink(parent, &alias).unwrap();

        for published in [false, true] {
            for target in [parent, unrelated.as_path()] {
                fs::create_dir(&directory).unwrap();
                fs::write(&native, "native").unwrap();
                let inputs = fixture
                    .inputs
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let mut adapter = transport(inputs, published);
                let baseline = adapter.native_baseline();
                let now = Instant::now();
                adapter.gate.observe(now).unwrap();
                let candidate = adapter.start(now).unwrap().complete(());

                fs::rename(&directory, &retired).unwrap();
                symlink(target, &directory).unwrap();
                let event = source_event(&alias.join("native-directory/main.rs"));
                assert_eq!(
                    adapter
                        .filter
                        .classify(&super::input::Change::from_event(&event)),
                    (true, false)
                );
                adapter.handle_at(Ok(event), now);
                assert!(!adapter.resident_eligible(&baseline));
                assert!(matches!(
                    adapter.admit_current(candidate, |_| panic!("ancestor symlink must revoke")),
                    rawr::reload::scheduler::Admission::Obsolete(())
                ));
                fs::remove_file(&directory).unwrap();
                fs::remove_file(retired.join("main.rs")).unwrap();
                fs::remove_dir(&retired).unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn resident_symlink_directory_data_and_metadata_revoke_exact_descendants() {
        use rawr_import::input::proof::Watch;
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new("resident-directory-identity");
        let directory = fixture.source.with_file_name("native-directory");
        let retired = fixture.source.with_file_name("retired-directory");
        let native = directory.join("native.rs");
        let unrelated = fixture.source.with_file_name("unrelated.data");
        fs::write(&unrelated, "unrelated").unwrap();

        for published in [false, true] {
            for target in [&fixture.source, &unrelated] {
                fs::create_dir(&directory).unwrap();
                fs::write(&native, "native").unwrap();
                let inputs = fixture
                    .inputs
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let mut adapter = transport(inputs, published);
                fs::rename(&directory, &retired).unwrap();
                symlink(target, &directory).unwrap();

                for modification in [
                    ModifyKind::Data(notify::event::DataChange::Content),
                    ModifyKind::Metadata(notify::event::MetadataKind::Any),
                ] {
                    let baseline = adapter.native_baseline();
                    let now = Instant::now();
                    adapter.gate.observe(now).unwrap();
                    // Registration already covers the identity replaced here;
                    // start directly so reconciliation does not obscure event
                    // classification with a now-invalid directory transport.
                    let candidate = adapter.gate.start(now).unwrap().complete(());
                    let event =
                        Event::new(EventKind::Modify(modification)).add_path(directory.clone());
                    assert_eq!(
                        adapter
                            .filter
                            .classify(&super::input::Change::from_event(&event)),
                        (true, false)
                    );
                    adapter.handle_at(Ok(event), now);
                    assert!(!adapter.resident_eligible(&baseline));
                    assert!(matches!(
                        adapter
                            .admit_current(candidate, |_| panic!("directory identity must revoke")),
                        rawr::reload::scheduler::Admission::Obsolete(())
                    ));
                }
                fs::remove_file(&directory).unwrap();
                fs::remove_file(retired.join("native.rs")).unwrap();
                fs::remove_dir(&retired).unwrap();
            }
        }
    }

    #[test]
    fn canonical_parent_metadata_keeps_existing_publication_relevance() {
        let fixture = Fixture::new("canonical-parent-metadata");
        let change = super::input::Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Metadata(
                notify::event::MetadataKind::Any,
            )))
            .add_path(fixture.source.parent().unwrap().to_path_buf()),
        );
        assert!(matches!(change, super::input::Change::Metadata(_)));
        let graph = Filter::Graph(fixture.inputs.clone());
        assert_eq!(graph.classify(&change), (false, true));
    }

    fn transport(inputs: rawr::reload::source::Inputs, published: bool) -> super::Adapter {
        let filter = if published {
            Filter::Graph(inputs)
        } else {
            Filter::direct(inputs)
        };
        // Native registration is real; delivery is the test's.
        Adapter::with_filter(filter, Duration::ZERO, false).unwrap()
    }

    const MAIN: &str = "// rawr\nfn main() {\n    println!(\"watch\");\n}\n";

    struct Fixture {
        root: std::path::PathBuf,
        source: std::path::PathBuf,
        inputs: rawr::reload::source::Inputs,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-watch-adapter-{}-{:?}-{name}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/main.rs"), MAIN).unwrap();
            // One explicit file: no Cargo.toml, so the watched inputs are
            // the source alone.
            let entry = fs::canonicalize(root.join("src/main.rs")).unwrap();
            let snapshot =
                rawr::package::snapshot::Snapshot::new(entry.clone(), vec![(entry, MAIN.into())])
                    .unwrap();
            let engine = rawr::Engine::new();
            let (candidate, _ast) = engine
                .compile_package_snapshot_generation_with_top_level(&snapshot, None)
                .unwrap();
            let source = candidate.source().entry_source().path().to_path_buf();
            let inputs = rawr::reload::source::Inputs::from_graph(candidate.source());
            Self {
                root,
                source,
                inputs,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }
}
