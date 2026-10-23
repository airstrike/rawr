//! Narrow raw filesystem-event transport for exact current and staged inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use rawr::reload::scheduler;

use super::{input, native, notice};

/// One host-owned native watcher and shared neutral admission gate.
pub struct Adapter {
    control: Arc<Control>,
    gate: Arc<scheduler::Gate>,
    publication: Arc<rawr::reload::scheduler::publication::Slot>,
    filter: Arc<Filter>,
    notices: notice::Notices,
    wake: notice::Publisher,
    history: native::History,
}

/// Shared registration control for the watcher's current and staged input union.
pub struct Control {
    watch: Mutex<Watch>,
    filter: Arc<Filter>,
    gate: Arc<scheduler::Gate>,
}

/// One authority-free candidate input union retained only through final owner
/// admission or explicit retirement.
#[must_use = "staged watcher inputs must be committed or retired"]
pub struct StagedInputs {
    control: Weak<Control>,
    stamp: u64,
    active: bool,
}

struct Watch {
    watcher: notify::RecommendedWatcher,
    roots: BTreeMap<PathBuf, bool>,
    handler: Option<Handler>,
}

enum Filter {
    Publication(Arc<rawr::reload::scheduler::publication::Slot>),
    Direct(RwLock<Direct>),
}

struct Direct {
    current: rawr::reload::source::Inputs,
    staged: Vec<DirectEntry>,
    next: u64,
}

struct DirectEntry {
    stamp: u64,
    inputs: rawr::reload::source::Inputs,
}

/// One watcher lifecycle notice for the owning session.
#[derive(Debug)]
pub enum Notice {
    /// A relevant current or staged input advanced the current revision.
    Changed(scheduler::notice::Observed),
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

#[derive(Clone)]
struct Handler {
    history: native::History,
    filter: Arc<Filter>,
    gate: Arc<scheduler::Gate>,
    notices: notice::Publisher,
}

impl Adapter {
    /// Open a raw watcher over one generation's exact resolved graph inputs.
    pub fn open(graph: &rawr::reload::source::Graph, debounce: Duration) -> Result<Self, Error> {
        let inputs = rawr::reload::source::Inputs::from_graph(graph);
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            inputs.clone(),
        ));
        Self::attach(Arc::new(scheduler::Gate::new(debounce)), publication)
    }

    /// Attach native observation to the host's exact admission gate and input
    /// publication. Current and staged inputs determine both relevance and
    /// initial transport coverage. The host must keep staging and registration
    /// ordered before candidate validation; attachment grants no admission.
    /// Startup or registration failure suspends this same gate.
    pub fn attach(
        gate: Arc<scheduler::Gate>,
        publication: Arc<rawr::reload::scheduler::publication::Slot>,
    ) -> Result<Self, Error> {
        Self::attach_with(gate, publication, true, |handler| {
            notify::recommended_watcher(move |event| handler.handle(event))
        })
    }

    fn attach_with(
        gate: Arc<scheduler::Gate>,
        publication: Arc<rawr::reload::scheduler::publication::Slot>,
        native: bool,
        start: impl FnOnce(Handler) -> notify::Result<notify::RecommendedWatcher>,
    ) -> Result<Self, Error> {
        let filter = Arc::new(Filter::Publication(Arc::clone(&publication)));
        let (notices, receiver) = notice::channel();
        let history = native::History::default();
        let handler = Handler {
            history: history.clone(),
            filter: Arc::clone(&filter),
            gate: Arc::clone(&gate),
            notices: notices.clone(),
        };
        let result = start(handler).map_err(Error::Start).and_then(|watcher| {
            Self::finish(
                publication,
                filter,
                Arc::clone(&gate),
                (notices, receiver),
                watcher,
                native,
                history,
            )
        });
        if result.is_err() {
            gate.suspend();
        }
        result
    }

    /// Open the generated-owner watcher over an already sealed Rawr/native
    /// input union. Replacements update this filter only after all new watcher
    /// roots have registered successfully.
    pub fn open_inputs(
        inputs: rawr::reload::source::Inputs,
        debounce: Duration,
    ) -> Result<Self, Error> {
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            inputs.clone(),
        ));
        let filter = Arc::new(Filter::Direct(RwLock::new(Direct {
            current: inputs.clone(),
            staged: Vec::new(),
            next: 0,
        })));
        let gate = Arc::new(scheduler::Gate::new(debounce));
        let (notices, receiver) = notice::channel();
        let history = native::History::default();
        let handler = Handler {
            history: history.clone(),
            filter: Arc::clone(&filter),
            gate: Arc::clone(&gate),
            notices: notices.clone(),
        };
        let watcher = notify::recommended_watcher(move |event| handler.handle(event))
            .map_err(Error::Start)?;
        Self::finish(
            publication,
            filter,
            gate,
            (notices, receiver),
            watcher,
            true,
            history,
        )
    }

    /// Open a watcher whose native callbacks are disabled for deterministic tests.
    #[cfg(feature = "test-support")]
    pub fn manual(graph: &rawr::reload::source::Graph, debounce: Duration) -> Result<Self, Error> {
        let inputs = rawr::reload::source::Inputs::from_graph(graph);
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            inputs.clone(),
        ));
        let filter = Arc::new(Filter::Publication(Arc::clone(&publication)));
        let gate = Arc::new(scheduler::Gate::new(debounce));
        let (notices, receiver) = notice::channel();
        let watcher = notify::recommended_watcher(|_event| {}).map_err(Error::Start)?;
        Self::finish(
            publication,
            filter,
            gate,
            (notices, receiver),
            watcher,
            false,
            native::History::default(),
        )
    }

    /// Open a direct input filter with deterministic callback delivery for tests.
    #[cfg(feature = "test-support")]
    pub fn manual_inputs(
        inputs: rawr::reload::source::Inputs,
        debounce: Duration,
    ) -> Result<Self, Error> {
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            inputs.clone(),
        ));
        let filter = Arc::new(Filter::Direct(RwLock::new(Direct {
            current: inputs,
            staged: Vec::new(),
            next: 0,
        })));
        let gate = Arc::new(scheduler::Gate::new(debounce));
        let channel = notice::channel();
        let watcher = notify::recommended_watcher(|_event| {}).map_err(Error::Start)?;
        Self::finish(
            publication,
            filter,
            gate,
            channel,
            watcher,
            false,
            native::History::default(),
        )
    }

    /// Deliver a deterministic data event through this watcher's real handler.
    #[cfg(feature = "test-support")]
    pub fn inject_modify(&self, path: &Path) {
        Handler {
            history: self.history.clone(),
            filter: Arc::clone(&self.filter),
            gate: Arc::clone(&self.gate),
            notices: self.wake.clone(),
        }
        .handle(Ok(notify::Event::new(notify::EventKind::Modify(
            notify::event::ModifyKind::Data(notify::event::DataChange::Content),
        ))
        .add_path(path.to_path_buf())));
    }

    fn finish(
        publication: Arc<rawr::reload::scheduler::publication::Slot>,
        filter: Arc<Filter>,
        gate: Arc<scheduler::Gate>,
        (wake, receiver): (notice::Publisher, notice::Notices),
        mut watcher: notify::RecommendedWatcher,
        native: bool,
        history: native::History,
    ) -> Result<Self, Error> {
        let (directories, trees) = filter.roots();
        let roots = transport_roots(
            directories.iter().map(PathBuf::as_path),
            trees.iter().map(PathBuf::as_path),
        )?;
        register_all(&mut watcher, &roots)?;
        let handler = native.then(|| Handler {
            history: history.clone(),
            filter: Arc::clone(&filter),
            gate: Arc::clone(&gate),
            notices: wake.clone(),
        });
        let control = Arc::new(Control {
            watch: Mutex::new(Watch {
                watcher,
                roots,
                handler,
            }),
            filter: Arc::clone(&filter),
            gate: Arc::clone(&gate),
        });
        Ok(Self {
            control,
            gate,
            publication,
            filter,
            notices: receiver,
            wake,
            history,
        })
    }

    /// Return watcher notices without granting candidate authority.
    #[must_use]
    pub const fn notices(&self) -> &notice::Notices {
        &self.notices
    }

    /// Return the newest outstanding request's debounce deadline.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.gate.deadline()
    }

    /// Start the newest outstanding request after its debounce deadline.
    #[must_use]
    pub fn start(&self, now: Instant) -> Option<scheduler::Request> {
        let request = self.gate.start(now)?;
        // Rearm after parent creation even if the candidate later fails to
        // compile, so subsequent children remain observable.
        for error in self.control.reconcile() {
            match error {
                Error::Watch { path, source } => {
                    self.gate.suspend();
                    self.wake.publish(Notice::Backend(source.add_path(path)));
                    return None;
                }
                error => eprintln!("Rawr input watcher warning: {error}"),
            }
        }
        Some(request)
    }

    /// Capture native invalidation history before full native validation.
    /// Retain this only with the owner successfully admitted by that work.
    #[must_use]
    pub fn native_baseline(&self) -> native::Baseline {
        let (paths, trees) = self.filter.native();
        self.history.capture(native::Stamp::capture(paths, trees))
    }

    /// Inspect observed native invalidation after the original request begins.
    /// This grants neither current Rust freshness nor admission authority:
    /// subsequent events still revoke the request through the shared gate.
    #[must_use]
    pub fn resident_eligible(&self, baseline: &native::Baseline) -> bool {
        self.gate.checkpoint().is_ok() && self.history.unchanged(baseline)
    }

    /// Capture the exact watcher event sequence before off-lock process work.
    pub fn checkpoint(&self) -> Result<rawr::reload::scheduler::Checkpoint, scheduler::Error> {
        self.gate.checkpoint()
    }

    /// Linearize one direct imported-owner slot transition against the watcher
    /// sequence captured by [`Self::checkpoint`].
    pub fn admit_checkpoint<T, U>(
        &self,
        checkpoint: rawr::reload::scheduler::Checkpoint,
        value: T,
        admit: impl FnOnce(T) -> U,
    ) -> scheduler::Admission<T, U> {
        self.gate.admit_checkpoint(checkpoint, value, admit)
    }

    /// Linearize one request-bound direct owner transition against the latest
    /// watcher revision. The callback must remain a short structural transition:
    /// no filesystem or store I/O, IPC, destruction, or watcher re-entry.
    pub fn admit_current<T, U>(
        &self,
        candidate: scheduler::Candidate<T>,
        admit: impl FnOnce(T) -> U,
    ) -> scheduler::Admission<T, U> {
        self.gate.admit_current(candidate, admit)
    }

    /// Share the exact admission gate advanced by this watcher.
    pub fn gate(&self) -> Arc<scheduler::Gate> {
        Arc::clone(&self.gate)
    }

    /// Share the publication slot used by the graph-owned input filter.
    pub fn publication(&self) -> Arc<rawr::reload::scheduler::publication::Slot> {
        Arc::clone(&self.publication)
    }

    /// Share off-gate watch registration and reconciliation control.
    pub fn control(&self) -> Arc<Control> {
        Arc::clone(&self.control)
    }

    /// Register a candidate filter's roots without changing current event
    /// relevance before the generated owner admits that candidate.
    pub fn prepare_inputs(
        &self,
        inputs: rawr::reload::source::Inputs,
    ) -> Result<StagedInputs, Error> {
        let transport = inputs.clone();
        let stamp = self.filter.stage(inputs)?;
        let staged = StagedInputs {
            control: Arc::downgrade(&self.control),
            stamp,
            active: true,
        };
        if let Err(error) = self.control.prepare(&transport) {
            drop(staged);
            return Err(error);
        }
        Ok(staged)
    }

    /// Publish one admitted generated-owner filter and retire roots no longer
    /// reachable from the last-good generation.
    pub fn commit_inputs(&self, mut staged: StagedInputs) -> Result<Vec<Error>, Error> {
        let Some(control) = staged.control.upgrade() else {
            return Err(Error::StaleStage);
        };
        if !Arc::ptr_eq(&control, &self.control) {
            return Err(Error::StaleStage);
        }
        self.filter.commit(staged.stamp)?;
        staged.active = false;
        Ok(self.control.reconcile())
    }
}

impl Control {
    /// Register a successor over the full transport union while retaining the
    /// incumbent. No registration occurs when the union is unchanged.
    pub fn prepare(&self, inputs: &rawr::reload::source::Inputs) -> Result<(), Error> {
        let result = (|| {
            let added = transport_roots(
                inputs.paths().iter().filter_map(|path| path.parent()),
                inputs.trees().iter().map(PathBuf::as_path),
            )?;
            let mut watch = lock(&self.watch);
            let mut desired = watch.roots.clone();
            for (path, recursive) in added {
                desired
                    .entry(path)
                    .and_modify(|current| *current |= recursive)
                    .or_insert(recursive);
            }
            watch.replace(desired)
        })();
        if result.is_err() {
            self.gate.suspend();
        }
        result
    }

    /// Reconcile the union without mutating registrations on the incumbent.
    pub fn reconcile(&self) -> Vec<Error> {
        let result = (|| {
            let mut watch = lock(&self.watch);
            let (directories, trees) = self.filter.roots();
            let desired = transport_roots(
                directories.iter().map(PathBuf::as_path),
                trees.iter().map(PathBuf::as_path),
            )?;
            watch.replace(desired)
        })();
        match result {
            Ok(()) => Vec::new(),
            Err(error) => {
                self.gate.suspend();
                vec![error]
            }
        }
    }
}

impl Watch {
    fn replace(&mut self, desired: BTreeMap<PathBuf, bool>) -> Result<(), Error> {
        let handler = self.handler.clone();
        self.replace_with(desired, || match handler {
            Some(handler) => notify::recommended_watcher(move |event| handler.handle(event)),
            None => notify::recommended_watcher(|_event| {}),
        })
    }

    fn replace_with(
        &mut self,
        desired: BTreeMap<PathBuf, bool>,
        start: impl FnOnce() -> notify::Result<notify::RecommendedWatcher>,
    ) -> Result<(), Error> {
        if self.roots == desired {
            return Ok(());
        }
        // The incumbent remains alive through every fallible successor step.
        // Native lifecycle work never runs under the scheduler admission gate.
        let mut successor = start().map_err(Error::Start)?;
        register_all(&mut successor, &desired)?;
        // The successor cannot see events queued before its SinceNow origin.
        // Deliver those incumbent callbacks before retiring it. This lock is
        // registration-only; callbacks use the gate/filter, never this mutex.
        #[cfg(target_os = "macos")]
        self.watcher.drain().map_err(Error::Start)?;
        let retired = std::mem::replace(&mut self.watcher, successor);
        self.roots = desired;
        drop(retired);
        Ok(())
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
    registration.commit().map_err(Error::Start)
}

fn mode(recursive: bool) -> RecursiveMode {
    if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    }
}

fn transport_roots<'a>(
    directories: impl Iterator<Item = &'a Path>,
    trees: impl Iterator<Item = &'a Path>,
) -> Result<BTreeMap<PathBuf, bool>, Error> {
    // Sibling files repeat their parent: inspect each distinct path once.
    let mut distinct = BTreeMap::<&Path, bool>::new();
    for (path, recursive) in directories
        .map(|path| (path, false))
        .chain(trees.map(|path| (path, true)))
    {
        *distinct.entry(path).or_default() |= recursive;
    }
    let mut roots = BTreeMap::new();
    for (path, recursive) in distinct {
        let root = existing_directory(path)?;
        // A missing recursive input needs only its next component's creation
        // event; recursively watching a distant ancestor could cover the disk.
        let recursive = recursive && root.as_path() == path;
        roots
            .entry(root)
            .and_modify(|current| *current |= recursive)
            .or_insert(recursive);
    }
    Ok(roots)
}

fn existing_directory(path: &Path) -> Result<PathBuf, Error> {
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

impl Filter {
    #[cfg(test)]
    fn relevant(&self, change: &input::Change) -> bool {
        self.classify(change).0
    }

    fn classify(&self, change: &input::Change) -> (bool, bool) {
        match self {
            Self::Publication(publication) => {
                let inputs = publication.read();
                let relevant = change.relevant(&inputs)
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
                                inputs.affects(path)
                                    || inputs.trees().iter().any(|tree| tree.starts_with(path))
                            }
                            _ => false,
                        };
                        !relevant || inputs.is_source_only(path)
                    })
                });
                (relevant, source_only)
            }
            Self::Direct(state) => {
                let state = read(state);
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

    fn roots(&self) -> (BTreeSet<PathBuf>, BTreeSet<PathBuf>) {
        match self {
            Self::Publication(publication) => {
                let read = publication.read();
                (
                    read.parents().into_vec().into_iter().collect(),
                    read.trees().into_vec().into_iter().collect(),
                )
            }
            Self::Direct(state) => {
                let state = read(state);
                let mut directories = state
                    .current
                    .paths()
                    .iter()
                    .filter_map(|path| path.parent().map(Path::to_path_buf))
                    .collect::<BTreeSet<_>>();
                let mut trees = state
                    .current
                    .trees()
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                for entry in &state.staged {
                    directories.extend(
                        entry
                            .inputs
                            .paths()
                            .iter()
                            .filter_map(|path| path.parent().map(Path::to_path_buf)),
                    );
                    trees.extend(entry.inputs.trees().iter().cloned());
                }
                (directories, trees)
            }
        }
    }

    /// Every non-source input path and recursive root, current and staged.
    fn native(&self) -> (Box<[PathBuf]>, Box<[PathBuf]>) {
        match self {
            Self::Publication(publication) => {
                let read = publication.read();
                (read.native_paths(), read.trees())
            }
            Self::Direct(state) => {
                let state = read(state);
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

    fn stage(&self, inputs: rawr::reload::source::Inputs) -> Result<u64, Error> {
        let Self::Direct(state) = self else {
            return Err(Error::FixedFilter);
        };
        let mut state = write(state);
        let stamp = state.next;
        state.next = state.next.checked_add(1).ok_or(Error::StageExhausted)?;
        state.staged.push(DirectEntry { stamp, inputs });
        Ok(stamp)
    }

    fn commit(&self, stamp: u64) -> Result<(), Error> {
        let Self::Direct(state) = self else {
            return Err(Error::FixedFilter);
        };
        let mut state = write(state);
        let index = state
            .staged
            .iter()
            .position(|entry| entry.stamp == stamp)
            .ok_or(Error::StaleStage)?;
        let entry = state.staged.swap_remove(index);
        state.current = entry.inputs;
        Ok(())
    }

    fn withdraw(&self, stamp: u64) {
        let Self::Direct(state) = self else {
            return;
        };
        let mut state = write(state);
        if let Some(index) = state.staged.iter().position(|entry| entry.stamp == stamp) {
            drop(state.staged.swap_remove(index));
        }
    }
}

impl Drop for StagedInputs {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Some(control) = self.control.upgrade() else {
            return;
        };
        control.filter.withdraw(self.stamp);
        drop(control.reconcile());
    }
}

impl Handler {
    fn handle(&self, event: notify::Result<notify::Event>) {
        self.handle_at(event, Instant::now());
    }

    fn handle_at(&self, event: notify::Result<notify::Event>, now: Instant) {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                self.gate.suspend();
                self.send(Notice::Backend(error));
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
        // Serialize observation with coalescing so concurrently delivered
        // callbacks cannot replace a newer revision with an older notice.
        self.notices.publish_with(|| {
            match self.gate.observe_if_notice(now, || {
                let (relevant, source_only) = self.filter.classify(&change);
                if matches!(change, input::Change::Rescan) {
                    self.history.rescan();
                } else if relevant && !source_only {
                    self.history.invalidate();
                }
                relevant
            }) {
                Ok(Some(observed)) => Some(Notice::Changed(observed)),
                Ok(None) | Err(scheduler::Error::Suspended) => None,
                Err(error) => Some(Notice::Schedule(error)),
            }
        });
    }

    fn send(&self, notice: Notice) {
        self.notices.publish(notice);
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn read<T>(value: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match value.read() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write<T>(value: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match value.write() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "cannot start Rawr source watcher: {error}"),
            Self::Watch { path, source } => write!(
                formatter,
                "cannot watch resolved input directory `{}`: {source}",
                path.display()
            ),
            Self::Unwatch { path, source } => write!(
                formatter,
                "cannot retire input directory watch `{}`: {source}",
                path.display()
            ),
            Self::FixedFilter => formatter.write_str(
                "the existing publication-owned watcher filter cannot be replaced directly",
            ),
            Self::StageExhausted => {
                formatter.write_str("the imported-owner watcher stage sequence is exhausted")
            }
            Self::StaleStage => {
                formatter.write_str("the imported-owner watcher input stage is no longer current")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error)
            | Self::Watch { source: error, .. }
            | Self::Unwatch { source: error, .. } => Some(error),
            Self::FixedFilter | Self::StageExhausted | Self::StaleStage => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::notice;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use notify::event::{AccessKind, ModifyKind};
    use notify::{Event, EventKind};

    use super::{Adapter, Direct, Filter, Handler, Notice};

    #[test]
    fn unchanged_roots_do_not_construct_a_successor() {
        let fixture = Fixture::new("unchanged-successor");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (adapter, _) = attached(gate, Arc::clone(&fixture.publication));
        let mut watch = super::lock(&adapter.control.watch);
        let desired = watch.roots.clone();
        watch
            .replace_with(desired, || panic!("unchanged roots restarted observation"))
            .unwrap();
    }

    #[test]
    fn failed_successor_preserves_incumbent_roots() {
        let fixture = Fixture::new("failed-successor");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (adapter, _) = attached(gate, Arc::clone(&fixture.publication));
        let mut watch = super::lock(&adapter.control.watch);
        let original = watch.roots.clone();
        let mut desired = original.clone();
        desired.insert(fixture.root.join("missing"), false);
        assert!(watch
            .replace_with(desired.clone(), || Err(notify::Error::generic(
                "start failed"
            )))
            .is_err());
        assert_eq!(watch.roots, original);
        assert!(watch
            .replace_with(desired, || notify::recommended_watcher(|_| {}))
            .is_err());
        assert_eq!(watch.roots, original);
    }

    #[test]
    fn attachment_revokes_the_supplied_gate_before_notice_consumption() {
        let fixture = Fixture::new("attached-revocation");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (adapter, handler) = attached(Arc::clone(&gate), Arc::clone(&fixture.publication));
        assert!(Arc::ptr_eq(&gate, &adapter.gate()));
        assert!(Arc::ptr_eq(&fixture.publication, &adapter.publication()));
        let now = Instant::now();
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete("candidate");
        handler.handle_at(
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone())),
            now,
        );
        assert!(matches!(
            gate.admit_current(candidate, |_| panic!("revoked candidate cannot install")),
            rawr::reload::scheduler::Admission::Obsolete("candidate")
        ));
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Changed(_))
        ));
    }

    #[test]
    fn attachment_uses_supplied_current_and_staged_publication() {
        let current = Fixture::new("attached-current");
        let mut next = Fixture::new("attached-next");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let now = Instant::now();
        gate.observe(now).unwrap();
        let staged = gate
            .start(now)
            .unwrap()
            .complete(((), next.snapshot.take().unwrap()))
            .stage_inputs(&current.publication);
        let (adapter, handler) = attached(Arc::clone(&gate), Arc::clone(&current.publication));
        assert!(super::lock(&adapter.control.watch)
            .roots
            .contains_key(next.source.parent().unwrap()));
        assert!(super::lock(&adapter.control.watch)
            .roots
            .contains_key(current.source.parent().unwrap()));
        let change = |path| Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path));
        handler.handle_at(change(current.source.clone()), now);
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Changed(_))
        ));
        handler.handle_at(change(next.source.clone()), now);
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Changed(_))
        ));
        handler.handle_at(change(next.root.join("unrelated.rs")), now);
        assert!(adapter.notices().try_recv().is_err());

        drop(staged);
        assert!(adapter.control.reconcile().is_empty());
        assert!(!super::lock(&adapter.control.watch)
            .roots
            .contains_key(next.source.parent().unwrap()));
        handler.handle_at(change(next.source.clone()), now);
        assert!(adapter.notices().try_recv().is_err());
        handler.handle_at(change(current.source.clone()), now);
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Changed(_))
        ));
    }

    #[test]
    fn attachment_backend_failure_suspends_the_supplied_gate() {
        let fixture = Fixture::new("attached-failure");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (adapter, handler) = attached(Arc::clone(&gate), Arc::clone(&fixture.publication));
        let now = Instant::now();
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete("candidate");
        handler.handle_at(Err(notify::Error::generic("backend failed")), now);
        assert!(matches!(
            gate.checkpoint(),
            Err(rawr::reload::scheduler::Error::Suspended)
        ));
        assert!(matches!(
            gate.admit_current(candidate, |_| panic!("suspended candidate cannot install")),
            rawr::reload::scheduler::Admission::Obsolete("candidate")
        ));
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Backend(_))
        ));
    }

    #[test]
    fn attachment_start_failure_suspends_the_supplied_gate() {
        let fixture = Fixture::new("attached-start-failure");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let result = Adapter::attach_with(
            Arc::clone(&gate),
            Arc::clone(&fixture.publication),
            false,
            |_| Err(notify::Error::generic("cannot start backend")),
        );
        assert!(matches!(result, Err(super::Error::Start(_))));
        assert!(matches!(
            gate.checkpoint(),
            Err(rawr::reload::scheduler::Error::Suspended)
        ));
    }

    fn attached(
        gate: Arc<rawr::reload::scheduler::Gate>,
        publication: Arc<rawr::reload::scheduler::publication::Slot>,
    ) -> (Adapter, Handler) {
        let mut callback = None;
        let adapter = Adapter::attach_with(gate, publication, false, |handler| {
            callback = Some(handler);
            // Use the production attachment and registration path while leaving
            // callback timing entirely under the test's control.
            notify::recommended_watcher(|_event| {})
        })
        .unwrap();
        (adapter, callback.unwrap())
    }

    #[test]
    fn direct_filter_observes_the_staged_union_until_abort_or_commit() {
        let fixture = Fixture::new("staged-union");
        let current = fixture.publication.read().current().clone();
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
        let filter = Filter::Direct(std::sync::RwLock::new(Direct {
            current,
            staged: Vec::new(),
            next: 0,
        }));
        let change = super::input::Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(candidate_path.clone()),
        );

        assert!(!filter.relevant(&change));
        let aborted = filter.stage(candidate.clone()).expect("candidate stages");
        assert!(filter.relevant(&change));
        assert!(filter.roots().0.contains(&candidate_root));
        filter.withdraw(aborted);
        assert!(!filter.relevant(&change));
        assert!(!filter.roots().0.contains(&candidate_root));

        let committed = filter.stage(candidate).expect("candidate restages");
        filter.commit(committed).expect("candidate commits");
        assert!(filter.relevant(&change));
        assert!(filter.roots().0.contains(&candidate_root));
    }

    #[test]
    fn relevant_callback_revokes_immediately_before_the_owner_reads_its_wake() {
        let fixture = Fixture::new("revocation");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let now = Instant::now();
        gate.observe(now).unwrap();
        let obsolete = gate.start(now).unwrap();
        let (notices, receiver) = notice::channel();
        let handler = Handler {
            history: super::native::History::default(),
            filter: Arc::new(Filter::Publication(Arc::clone(&fixture.publication))),
            gate: Arc::clone(&gate),
            notices,
        };

        handler.handle_at(
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone())),
            now,
        );

        assert!(gate.start(now).is_some());
        assert!(matches!(receiver.try_recv(), Ok(Notice::Changed(_))));
        drop(obsolete);
    }

    #[test]
    fn access_stays_quiet_and_backend_failure_wakes_the_owner_once() {
        let fixture = Fixture::new("failure");
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let (notices, receiver) = notice::channel();
        let handler = Handler {
            history: super::native::History::default(),
            filter: Arc::new(Filter::Publication(Arc::clone(&fixture.publication))),
            gate: Arc::clone(&gate),
            notices,
        };
        let now = Instant::now();

        handler.handle_at(
            Ok(Event::new(EventKind::Access(AccessKind::Read)).add_path(fixture.source.clone())),
            now,
        );
        assert!(gate.deadline().is_none());
        assert!(receiver.try_recv().is_err());

        gate.observe(now).unwrap();
        let obsolete = gate.start(now).unwrap();
        handler.handle_at(Err(notify::Error::generic("watch failed")), now);
        assert!(gate.deadline().is_none());
        assert!(matches!(receiver.try_recv(), Ok(Notice::Backend(_))));
        assert!(receiver.try_recv().is_err());

        handler.handle_at(
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(fixture.source.clone())),
            now,
        );
        assert!(gate.deadline().is_none());
        assert!(gate.start(now).is_none());
        assert!(receiver.try_recv().is_err());
        drop(obsolete);
    }

    #[test]
    fn imported_missing_input_below_an_absent_directory_opens_its_watcher() {
        let fixture = Fixture::new("missing-imported-parent");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let missing = root.join("absent/config.toml");
        let inputs = fixture
            .publication
            .read()
            .current()
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
        let current = fixture.publication.read().current().clone();
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
        let adapter = direct(current.clone());
        let first_stage = adapter.prepare_inputs(candidate(first.clone())).unwrap();
        let second_stage = adapter.prepare_inputs(candidate(second.clone())).unwrap();
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&first),
            Some(&true)
        );
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&second),
            Some(&true)
        );
        drop(first_stage);
        assert!(!super::lock(&adapter.control.watch)
            .roots
            .contains_key(&first));
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&second),
            Some(&true)
        );
        assert!(adapter.commit_inputs(second_stage).unwrap().is_empty());
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&second),
            Some(&true)
        );
        let replacement = adapter.prepare_inputs(current).unwrap();
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&second),
            Some(&true)
        );
        assert!(adapter.commit_inputs(replacement).unwrap().is_empty());
        assert!(!super::lock(&adapter.control.watch)
            .roots
            .contains_key(&second));
    }

    #[test]
    fn transport_roots_deduplicate_absent_parents_without_recursive_ancestor_watch() {
        let fixture = Fixture::new("transport");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let absent = root.join("missing/nested");
        let other = root.join("another");
        let roots = super::transport_roots(
            [absent.as_path(), other.as_path()].into_iter(),
            [absent.as_path()].into_iter(),
        )
        .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(&root), Some(&false));

        fs::create_dir_all(&absent).unwrap();
        let roots = super::transport_roots(
            [absent.as_path()].into_iter(),
            [absent.as_path()].into_iter(),
        )
        .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(&absent), Some(&true));
    }

    #[test]
    fn transport_roots_merge_a_repeated_parent_and_its_tree_before_inspection() {
        let fixture = Fixture::new("transport-repeated");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let parents = vec![nested.as_path(); 64];
        let roots = super::transport_roots(
            parents.iter().copied().chain([root.as_path()]),
            [nested.as_path()].into_iter(),
        )
        .unwrap();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots.get(&nested), Some(&true));
        assert_eq!(roots.get(&root), Some(&false));
    }

    #[test]
    fn absent_config_creation_rearms_before_a_candidate_can_fail() {
        let fixture = Fixture::new("absent-config");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let config = root.join(".cargo/nested/config.toml");
        let inputs = fixture
            .publication
            .read()
            .current()
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Exact(config.clone())]);
        assert!(inputs.contains(&config));
        assert!(!inputs.contains(&root.join(".cargo")));
        let adapter = direct(inputs.clone());
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&root),
            Some(&false)
        );
        let (notices, receiver) = notice::channel();
        let handler = Handler {
            history: adapter.history.clone(),
            filter: Arc::clone(&adapter.filter),
            gate: Arc::clone(&adapter.gate),
            notices,
        };
        let now = Instant::now();
        fs::create_dir(root.join("unrelated")).unwrap();
        handler.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(root.join("unrelated")),
            ),
            now,
        );
        assert!(receiver.try_recv().is_err());
        assert!(adapter.start(now).is_none());

        for parent in [root.join(".cargo"), root.join(".cargo/nested")] {
            fs::create_dir(&parent).unwrap();
            handler.handle_at(
                Ok(
                    Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                        .add_path(parent.clone()),
                ),
                now,
            );
            assert!(matches!(receiver.try_recv(), Ok(Notice::Changed(_))));
            let candidate = adapter
                .start(now)
                .expect("parent creation queues a candidate");
            assert_eq!(
                super::lock(&adapter.control.watch).roots.get(&parent),
                Some(&false)
            );
            // Failing the candidate must not undo the newly armed transport.
            drop(candidate);
        }
        fs::write(&config, "[build]\njobs = 1\n").unwrap();
        handler.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::File))
                    .add_path(config.clone()),
            ),
            now,
        );
        assert!(matches!(receiver.try_recv(), Ok(Notice::Changed(_))));
        assert!(adapter.start(now).is_some());
        let staged = adapter.prepare_inputs(inputs).unwrap();
        assert!(adapter.commit_inputs(staged).unwrap().is_empty());
        assert!(!super::lock(&adapter.control.watch)
            .roots
            .contains_key(&root));
    }

    #[test]
    fn absent_recursive_input_promotes_only_after_its_root_exists() {
        let fixture = Fixture::new("absent-tree");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let tree = root.join("generated/tree");
        let inputs = fixture
            .publication
            .read()
            .current()
            .clone()
            .with_imported([rawr_import::input::proof::Watch::Tree(tree.clone())]);
        let adapter = direct(inputs.clone());
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&root),
            Some(&false)
        );
        let (notices, receiver) = notice::channel();
        let handler = Handler {
            history: adapter.history.clone(),
            filter: Arc::clone(&adapter.filter),
            gate: Arc::clone(&adapter.gate),
            notices,
        };
        let now = Instant::now();
        fs::create_dir_all(&tree).unwrap();
        handler.handle_at(
            Ok(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(root.join("generated")),
            ),
            now,
        );
        assert!(matches!(receiver.try_recv(), Ok(Notice::Changed(_))));
        assert!(adapter.start(now).is_some());
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&tree),
            Some(&true)
        );
        // Publication filters also retain ancestor relevance for tree inputs.
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(inputs));
        let filter = Filter::Publication(publication);
        assert!(filter.relevant(&super::input::Change::Tree(Box::from([
            root.join("generated")
        ]))));
    }

    #[test]
    fn publication_rearms_from_exact_parents_after_directory_creation() {
        let fixture = Fixture::new("publication-parent");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let parent = root.join(".cargo/nested");
        let inputs = fixture.publication.read().current().clone().with_imported([
            rawr_import::input::proof::Watch::Exact(parent.join("config.toml")),
        ]);
        let adapter = transport(inputs, true);
        assert!(adapter.filter.roots().0.contains(&parent));
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&root),
            Some(&false)
        );
        fs::create_dir_all(&parent).unwrap();
        assert!(adapter.control.reconcile().is_empty());
        assert_eq!(
            super::lock(&adapter.control.watch).roots.get(&parent),
            Some(&false)
        );
    }

    fn direct(inputs: rawr::reload::source::Inputs) -> super::Adapter {
        transport(inputs, false)
    }

    fn callback(adapter: &Adapter) -> Handler {
        Handler {
            history: adapter.history.clone(),
            filter: Arc::clone(&adapter.filter),
            gate: Arc::clone(&adapter.gate),
            notices: adapter.wake.clone(),
        }
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
        let adapter = direct(fixture.publication.read().current().clone());
        let handler = callback(&adapter);
        let baseline = adapter.native_baseline();
        let now = Instant::now();
        adapter.gate.observe(now).unwrap();
        let candidate = adapter.start(now).unwrap().complete(());
        handler.handle_at(Ok(source_event(&fixture.source)), now);
        assert!(adapter.resident_eligible(&baseline));
        assert!(matches!(
            adapter.admit_current(candidate, |_| panic!("source event must revoke")),
            rawr::reload::scheduler::Admission::Obsolete(())
        ));
        let foreign = direct(fixture.publication.read().current().clone());
        assert!(!foreign.resident_eligible(&baseline));
    }

    #[test]
    fn resident_native_invalidation_survives_notice_coalescing_and_input_commit() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-watermark");
        let native = fixture.source.with_file_name("native.rs");
        let inputs = fixture
            .publication
            .read()
            .current()
            .clone()
            .with_imported([Watch::Exact(native.clone())]);
        let adapter = direct(inputs.clone());
        let handler = callback(&adapter);
        let before_validation = adapter.native_baseline();
        let now = Instant::now();
        let staged = adapter.prepare_inputs(inputs.clone()).unwrap();
        handler.handle_at(Ok(source_event(&native)), now);
        handler.handle_at(Ok(source_event(&fixture.source)), now);
        assert!(matches!(
            adapter.notices().try_recv(),
            Ok(Notice::Changed(_))
        ));
        assert!(adapter.notices().try_recv().is_err());
        assert!(!adapter.resident_eligible(&before_validation));
        adapter.commit_inputs(staged).unwrap();
        assert!(!adapter.resident_eligible(&before_validation));

        // A new full validation can retain a new baseline, but an event after
        // capture and before adoption/commit must remain visible.
        let before_next_validation = adapter.native_baseline();
        let staged = adapter.prepare_inputs(inputs).unwrap();
        assert!(adapter.resident_eligible(&before_next_validation));
        handler.handle_at(Ok(source_event(&native)), now);
        adapter.commit_inputs(staged).unwrap();
        assert!(!adapter.resident_eligible(&before_next_validation));

        let admitted = adapter.native_baseline();
        let candidate = adapter.start(now).unwrap().complete(());
        assert!(adapter.resident_eligible(&admitted));
        handler.handle_at(Ok(source_event(&native)), now);
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
            .publication
            .read()
            .current()
            .clone()
            .with_imported([Watch::Exact(native.clone())]);
        let adapter = direct(inputs);
        let handler = callback(&adapter);
        let now = Instant::now();
        let attribute = |path: &std::path::Path| {
            Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::Extended,
            )))
            .add_path(path.to_path_buf())
        };
        // Cargo tags its registry and git caches on every invocation.
        let baseline = adapter.native_baseline();
        handler.handle_at(Ok(attribute(&cache)), now);
        assert!(adapter.resident_eligible(&baseline));
        assert!(adapter.notices().try_recv().is_err());

        handler.handle_at(Ok(attribute(&native)), now);
        assert!(!adapter.resident_eligible(&baseline));
    }

    #[test]
    fn resident_unknown_pathless_rescan_and_ancestor_events_fail_closed() {
        let fixture = Fixture::new("resident-unknown");
        let adapter = direct(fixture.publication.read().current().clone());
        let handler = callback(&adapter);
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
            handler.handle_at(Ok(event), now);
            assert_eq!(adapter.resident_eligible(&baseline), dropped);
            assert!(matches!(
                adapter.admit_current(candidate, |_| panic!("uncertain event must revoke")),
                rawr::reload::scheduler::Admission::Obsolete(())
            ));
        }
        let baseline = adapter.native_baseline();
        handler.handle_at(Err(notify::Error::generic("watcher failed")), now);
        assert!(!adapter.resident_eligible(&baseline));
    }

    #[test]
    fn resident_staged_source_role_cannot_hide_current_or_staged_native_overlap() {
        use rawr_import::input::proof::Watch;
        let fixture = Fixture::new("resident-overlap");
        let source = fixture.publication.read().current().clone();
        let native = source
            .clone()
            .with_imported([Watch::Exact(fixture.source.clone())]);
        let change = super::input::Change::from_event(&source_event(&fixture.source));
        for (current, candidate) in [(source.clone(), native.clone()), (native, source)] {
            let direct = Filter::Direct(std::sync::RwLock::new(Direct {
                current: current.clone(),
                staged: Vec::new(),
                next: 0,
            }));
            direct.stage(candidate.clone()).unwrap();
            assert_eq!(direct.classify(&change), (true, false));
            let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(current));
            let _staged = publication.register(candidate);
            assert_eq!(
                Filter::Publication(publication).classify(&change),
                (true, false)
            );
        }
    }

    #[test]
    fn resident_rename_ignores_unrelated_save_temp_paths_in_both_filters() {
        let fixture = Fixture::new("resident-save-temp");
        for published in [false, true] {
            let adapter = transport(fixture.publication.read().current().clone(), published);
            let handler = callback(&adapter);
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
            handler.handle_at(Ok(event), now);
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
        let current = fixture.publication.read().current().clone();
        let native = fixture.source.with_file_name("native");
        for published in [false, true] {
            let adapter = transport(current.clone(), published);
            let handler = callback(&adapter);
            let inputs = current.clone().with_imported([
                Watch::Exact(native.join("exact.rs")),
                Watch::Tree(native.join("tree")),
            ]);
            let (_observation, _staged) = if published {
                (Some(adapter.publication.register(inputs)), None)
            } else {
                (None, Some(adapter.prepare_inputs(inputs).unwrap()))
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
                handler.handle_at(Ok(event), Instant::now());
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
                    .publication
                    .read()
                    .current()
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let adapter = transport(inputs, published);
                let handler = callback(&adapter);
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
                handler.handle_at(Ok(event), now);
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
                    .publication
                    .read()
                    .current()
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let adapter = transport(inputs, published);
                let handler = callback(&adapter);
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
                handler.handle_at(Ok(event), now);
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
                    .publication
                    .read()
                    .current()
                    .clone()
                    .with_imported([Watch::Exact(native.clone())]);
                let adapter = transport(inputs, published);
                let handler = callback(&adapter);
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
                    handler.handle_at(Ok(event), now);
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
        let publication = Filter::Publication(Arc::clone(&fixture.publication));
        assert_eq!(publication.classify(&change), (false, true));
    }

    fn transport(inputs: rawr::reload::source::Inputs, published: bool) -> super::Adapter {
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            inputs.clone(),
        ));
        let filter = Arc::new(if published {
            Filter::Publication(Arc::clone(&publication))
        } else {
            Filter::Direct(std::sync::RwLock::new(Direct {
                current: inputs.clone(),
                staged: Vec::new(),
                next: 0,
            }))
        });
        let gate = Arc::new(rawr::reload::scheduler::Gate::new(Duration::ZERO));
        let channel = notice::channel();
        // Native registration is real; callback delivery is deterministic.
        let watcher = notify::recommended_watcher(|_event| {}).unwrap();
        super::Adapter::finish(
            publication,
            filter,
            gate,
            channel,
            watcher,
            false,
            super::native::History::default(),
        )
        .unwrap()
    }

    struct Fixture {
        root: std::path::PathBuf,
        source: std::path::PathBuf,
        publication: Arc<rawr::reload::scheduler::publication::Slot>,
        snapshot: Option<rawr::reload::source::Snapshot>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-watch-adapter-{}-{:?}-{name}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(
                root.join("src/main.rs"),
                r#"// rawr
use iced::Element;
use iced::widget::text;
#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("watch") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
            )
            .unwrap();
            // One explicit file: no Cargo.toml, so the watched inputs are
            // the source alone.
            let target = rawr::project::Target::discover(root.join("src/main.rs")).unwrap();
            let mut engine = rawr::Engine::new();
            rawr_iced::install(&mut engine).unwrap();
            let candidate = engine.compile_package_generation(&target).unwrap();
            let source = candidate.source().entry_source().path().to_path_buf();
            let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
                rawr::reload::source::Inputs::from_graph(candidate.source()),
            ));
            Self {
                root,
                source,
                publication,
                snapshot: Some(rawr::reload::source::Snapshot::from_graph(
                    candidate.source(),
                )),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }
}
