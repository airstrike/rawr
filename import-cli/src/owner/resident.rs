//! The short Rawr edit path, before Cargo, extraction or symbolic compilation.

use std::path::PathBuf;
use std::sync::Arc;

use rawr::reload::source;
use rawr_import::{process::resident, supervisor::snapshot::attempt::Scope};

#[cfg(test)]
mod tests;

/// The exact `Cargo.toml` the admitted native owner consumed, and what
/// native code sees of the marked modules when the crate root is native
/// (D501). Comparing them conservatively routes any declaration edit, and
/// any edit that changes a trampoline, to preparation. The last field is a
/// marked root's async `main` entry (D513.4).
#[derive(Clone, Eq, PartialEq)]
pub struct Manifests(
    Box<[(PathBuf, Box<str>)]>,
    Option<rawr::package::Shape>,
    Option<Box<str>>,
);

impl Manifests {
    pub fn from_graph(graph: &rawr::package::Graph) -> Self {
        Self(
            graph
                .entry_package()
                .cargo_manifest()
                .map(|(path, text)| (path.to_path_buf(), text.into()))
                .into_iter()
                .collect(),
            graph.island_shape(),
            graph.entry_package().entry().map(Box::from),
        )
    }

    /// Whether every save restarts the program: an async `main` runs on
    /// its runtime's thread, and a new generation can't reach it there.
    pub fn restarts_on_save(&self) -> bool {
        self.1.is_none() && self.2.is_some()
    }

    /// What changed from these manifests to `next`, in one line; `None`
    /// when nothing did.
    fn change(&self, next: &Self) -> Option<String> {
        if self.0 != next.0 {
            return Some("`Cargo.toml` changed".to_owned());
        }
        match (&self.1, &next.1) {
            (Some(shape), Some(next_shape)) => {
                if let Some(change) = shape.change(next_shape) {
                    return Some(change);
                }
            }
            (None, None) => {}
            _ => return Some("whether the crate root is native changed".to_owned()),
        }
        (self.2 != next.2).then(|| "`main`'s async entry changed".to_owned())
    }

    /// The reason a marked `main` restarts when its next preparation is
    /// `next`, as `rawr run` says it in one line.
    pub fn restart_line(&self, next: &Self) -> String {
        if self.0 == next.0 {
            "the source changed".to_owned()
        } else {
            "`Cargo.toml` changed".to_owned()
        }
    }

    /// Whether the program's crate root is native and calls marked
    /// modules (D501).
    pub fn has_islands(&self) -> bool {
        self.1.is_some()
    }

    /// Why a program with native islands restarts when its next
    /// preparation is `next`, in one line. `None` for a program without
    /// islands, whose host says what a new runner means. When what native
    /// code sees of the marked modules is the same, the new runner's own
    /// reason is `native`'s: the input or native use that changed.
    pub fn restart_reason(
        &self,
        next: &Self,
        native: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        // An async `main` runs on its runtime's own thread: a save restarts
        // it with the new code (D500.1, D513.7).
        if self.1.is_none() && self.2.is_some() {
            return Some(if next.2.is_some() {
                "the source changed".to_owned()
            } else {
                "`main` is no longer async".to_owned()
            });
        }
        let (Some(shape), Some(next_shape)) = (&self.1, &next.1) else {
            return None;
        };
        if self.0 != next.0 {
            return Some("`Cargo.toml` changed".to_owned());
        }
        Some(
            shape
                .change(next_shape)
                .or_else(native)
                // A new derive needs a new native build only for a native
                // use of its impl, as a marked call at a native bound.
                .or_else(|| shape.derived(next_shape))
                // No runner runs to compare with, or an earlier native
                // build is still owed.
                .unwrap_or_else(|| "the program needs a new native build".to_owned()),
        )
    }
}

/// Why a save takes the full preparation instead of compiling in the
/// running program, as `rawr run` says it in one line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Reason {
    /// The running program asked for a refresh.
    Refresh,
    /// The native build the running program was checked against may no
    /// longer stand.
    Native(rawr_watch::native::Change),
    /// What native code sees of the program changed, as this line says.
    Manifests(String),
    /// An earlier save's new native build is still owed.
    Owed,
    /// The running program's compiler found a native use its build doesn't
    /// cover.
    Uncovered,
    /// The running program compiles no save in place yet.
    Unstarted,
}

impl std::fmt::Display for Reason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refresh => formatter.write_str("the program asked for a refresh"),
            Self::Native(change) => write!(formatter, "{change}"),
            Self::Manifests(change) => formatter.write_str(change),
            Self::Owed => formatter.write_str("an earlier save's native build is still owed"),
            Self::Uncovered => formatter.write_str(
                "the running program's native build doesn't cover this save's native uses",
            ),
            Self::Unstarted => formatter.write_str("the running program can't compile a save yet"),
        }
    }
}

impl Reason {
    /// The first reason, in the order `attempt` checks them, that a save
    /// can't take the resident route; `None` when it can.
    fn of(
        refresh_required: bool,
        native: Option<rawr_watch::native::Change>,
        baseline: &Manifests,
        next: &Manifests,
    ) -> Option<Self> {
        if refresh_required {
            return Some(Self::Refresh);
        }
        if let Some(change) = native {
            return Some(Self::Native(change));
        }
        baseline.change(next).map(Self::Manifests)
    }
}

/// What a resident save watches: the running native build's inputs,
/// derived once for its proof, joined with the marked sources of the last
/// save. A save that changes only a marked source's text joins, encodes and
/// stages nothing again (#1604).
#[derive(Default)]
pub(super) struct Watched(Option<Native>);

/// One native generation's inputs and the last join with them.
struct Native {
    /// The running native build's input proof, which names its watch.
    proof: rawr_import::input::fingerprint::Fingerprint,
    inputs: source::Inputs,
    joined: Option<Joined>,
}

/// The marked sources of one save joined with the native inputs, and the
/// observation the runner checks them as.
struct Joined {
    marked: source::Inputs,
    inputs: Arc<source::Inputs>,
    observation: Arc<[u8]>,
}

impl Watched {
    /// The native inputs of the build `proof` names, derived from `watch`
    /// unless these are its own and no native event came since.
    fn native<I>(
        &mut self,
        proof: rawr_import::input::fingerprint::Fingerprint,
        current: bool,
        project: &std::path::Path,
        watch: impl FnOnce() -> Result<I, crate::native::Error>,
    ) -> Result<&mut Native, crate::native::Error>
    where
        I: Iterator<Item = rawr_import::input::proof::Watch>,
    {
        if !current || self.0.as_ref().is_none_or(|native| native.proof != proof) {
            let outputs = rawr_import::location::outputs_here(project);
            let inputs = source::Inputs::imported(watch()?)
                .outside(&outputs)
                .with_checkouts(&rawr_import::location::checkouts_here());
            self.0 = Some(Native {
                proof,
                inputs,
                joined: None,
            });
        }
        Ok(self.0.as_mut().expect("derived above"))
    }
}

impl Native {
    /// `marked` joined with these native inputs: the last save's join when
    /// its marked sources are the same.
    fn join(&mut self, marked: source::Inputs) -> Result<&Joined, source::observation::Error> {
        if self
            .joined
            .as_ref()
            .is_none_or(|joined| joined.marked != marked)
        {
            let inputs = marked.clone().with_native(&self.inputs);
            let observation = inputs.encode_observation()?.into();
            self.joined = Some(Joined {
                marked,
                inputs: Arc::new(inputs),
                observation,
            });
        }
        Ok(self.joined.as_ref().expect("joined above"))
    }
}

/// What this process had done when a save began: the sources it parsed,
/// Cargo's resolutions, the API tables it read, the facades it registered
/// and the watcher roots it derived. A save of a marked file parses that
/// file again and needs none of the rest (#1604).
pub(super) struct Work {
    parsed: usize,
    api: rawr_import::progress::Work,
    registered: usize,
    roots: usize,
}

impl Work {
    /// What this process has done so far, its parses through `cells`.
    pub(super) fn now(cells: &rawr::package::Cells) -> Self {
        Self {
            parsed: cells.counts().parsed,
            api: rawr_import::progress::work(),
            registered: rawr::Engine::registered_facades(),
            roots: rawr_watch::derived_roots(),
        }
    }

    /// With `RAWR_VERBOSE`, say what the save since `self` did.
    pub(super) fn tell(self, cells: &rawr::package::Cells) {
        if !rawr_cli::cells::verbose() {
            return;
        }
        let now = Self::now(cells);
        eprintln!(
            "rawr-import: a save parsed {} sources, Cargo resolutions {}, API tables loaded {}, facades registered {}, watch roots staged {}",
            now.parsed - self.parsed,
            now.api.resolved() - self.api.resolved(),
            now.api.loaded() - self.api.loaded(),
            now.registered - self.registered,
            now.roots - self.roots,
        );
    }
}

pub(super) enum Attempt {
    Settled,
    /// The runner committed the new generation compiled against it.
    Swapped,
    /// The save takes the full preparation, for this reason.
    Prepare(Option<rawr_watch::StagedInputs>, Reason),
    /// The running program can't take the save, for this reason (D553.6):
    /// it restarts, with no preparation first.
    Restart(Box<str>),
}

#[allow(clippy::too_many_arguments)]
pub(super) fn attempt(
    lane: &mut super::Lane,
    adapter: &rawr_watch::Adapter,
    watch_request: &rawr::reload::scheduler::Request,
    started: Option<rawr_import::process::hot::Started>,
    revision: rawr_import::supervisor::Revision,
    target: &rawr::project::Target,
    baseline: &Manifests,
    native_baseline: &rawr_watch::native::Baseline,
    refresh_required: bool,
    watched: &mut Watched,
    scope: &Scope,
    cells: &rawr::package::Cells,
    diagnostic: &rawr::diagnostic::Source,
) -> Result<Attempt, rawr::diagnostic::Report> {
    let native_change = adapter.resident_change(native_baseline);
    let native_current = native_change.is_none();
    let capture = (|| {
        let load = || {
            rawr::package::Graph::load_with_cells(&rawr::Engine::new(), target, cells)
                .map_err(|error| crate::report::package(&error, diagnostic.clone()))
        };
        let mut graph = load()?;
        // An edit to an item a crate derive expanded expands it again here,
        // so its expansion's source changes in place and an edit to its
        // bodies alone swaps in (#1040). One that fails prepares, which
        // expands it again and says why.
        if crate::plain::refresh_expansions(&graph).unwrap_or(false) {
            graph = load()?;
        }
        // The resident route uses the same limits profile.
        let limits = graph.rust_limits();
        let reason = Reason::of(
            refresh_required,
            native_change,
            baseline,
            &Manifests::from_graph(&graph),
        );
        let project = graph.entry_package().root().to_path_buf();
        let graph = source::Graph::from_package(&graph);
        let snapshot = source::Snapshot::from_graph(&graph);
        let expected = snapshot
            .imported(&limits)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let bytes = snapshot
            .imported_transport(expected, &limits)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        // No native event since the baseline: the native part derived for
        // the running build's proof still stands; a Rawr edit derives only
        // its own, and one that keeps the marked sources' set reuses the
        // last join whole.
        let proof = lane
            .native_fingerprint()
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let joined = watched
            .native(proof, native_current, &project, || lane.native_watch())
            .map_err(|error| super::framework(error, diagnostic.clone()))?
            .join(source::Inputs::from_graph(&graph))
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let observation = Arc::clone(&joined.observation);
        // Register before the initial source check; this preserves complete
        // current+staged watch coverage throughout candidate work.
        let staged = adapter
            .prepare_inputs(Arc::clone(&joined.inputs))
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let freshness = scope
            .observe(&bytes)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        Ok((bytes, freshness, observation, staged, reason))
    })();
    let captured = match capture {
        Ok(captured) => captured,
        Err(error) => {
            lane.cancel_compile(started)
                .map_err(|error| super::framework(error, diagnostic.clone()))?;
            if !adapter.gate().request_is_current(watch_request) {
                return Ok(Attempt::Settled);
            }
            return Err(error);
        }
    };
    let (bytes, freshness, observation, staged, reason) = captured;
    if !adapter.gate().request_is_current(watch_request) {
        lane.cancel_compile(started)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        return Ok(Attempt::Settled);
    }
    // A previous failed native replacement can suppress HotBegin while the
    // last-good child remains live. Its next cold attempt still needs the
    // newly discovered Rawr roots observed throughout full preparation.
    let started = match (reason, started) {
        (None, Some(started)) => started,
        (reason, started) => {
            lane.cancel_compile(started)
                .map_err(|error| super::framework(error, diagnostic.clone()))?;
            return Ok(Attempt::Prepare(
                Some(staged),
                reason.unwrap_or(Reason::Owed),
            ));
        }
    };
    let outcome = match lane.prepare_resident(started, revision, bytes, freshness, &observation) {
        Ok(outcome) => outcome,
        Err(error) => return failed(error, adapter, watch_request, diagnostic),
    };
    match outcome {
        resident::Outcome::Prepared(prepared) => {
            if !adapter.resident_eligible(native_baseline)
                || !adapter.gate().request_is_current(watch_request)
            {
                lane.abort_resident(prepared)
                    .map_err(|error| super::framework(error, diagnostic.clone()))?;
                return Ok(Attempt::Settled);
            }
            if let Err(error) = lane.commit_resident(prepared) {
                return failed(error, adapter, watch_request, diagnostic);
            }
            for warning in adapter
                .commit_inputs(staged)
                .map_err(|error| super::framework(error, diagnostic.clone()))?
            {
                rawr_import::process::say(format_args!("warning: file watcher: {warning}"));
            }
            Ok(Attempt::Swapped)
        }
        resident::Outcome::NeedsPreparation if adapter.gate().request_is_current(watch_request) => {
            Ok(Attempt::Prepare(Some(staged), Reason::Uncovered))
        }
        resident::Outcome::Unstarted if adapter.gate().request_is_current(watch_request) => {
            Ok(Attempt::Prepare(Some(staged), Reason::Unstarted))
        }
        resident::Outcome::Refused(reason) if adapter.gate().request_is_current(watch_request) => {
            Ok(Attempt::Restart(reason))
        }
        resident::Outcome::NeedsPreparation
        | resident::Outcome::Unstarted
        | resident::Outcome::Refused(_) => Ok(Attempt::Settled),
        resident::Outcome::Obsolete => obsolete(adapter, watch_request, diagnostic),
        _ if !adapter.gate().request_is_current(watch_request) => Ok(Attempt::Settled),
        // The error the runner's compile found, in its own file, as the
        // owner's own compile reports it (#397).
        resident::Outcome::Source(report) => Err(rawr::diagnostic::Report::decode(&report)
            .unwrap_or_else(|error| super::framework(error, diagnostic.clone()))),
        resident::Outcome::Resource(reason) => Err(super::framework(reason, diagnostic.clone())),
    }
}

/// The runner's own watcher revoked its request, possibly for the very write
/// this owner already scheduled. Unless a newer request supersedes this one,
/// nothing else would retry: re-arm so the current bytes are compiled again.
fn obsolete(
    adapter: &rawr_watch::Adapter,
    request: &rawr::reload::scheduler::Request,
    diagnostic: &rawr::diagnostic::Source,
) -> Result<Attempt, rawr::diagnostic::Report> {
    if adapter.gate().request_is_current(request) {
        adapter
            .gate()
            .observe(std::time::Instant::now())
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
    }
    Ok(Attempt::Settled)
}

fn failed(
    error: crate::native::Error,
    adapter: &rawr_watch::Adapter,
    request: &rawr::reload::scheduler::Request,
    diagnostic: &rawr::diagnostic::Source,
) -> Result<Attempt, rawr::diagnostic::Report> {
    use crate::native::Error;
    use rawr_import::process::RunnerError;
    // The runner's own watcher revoked the commit: same race as `obsolete`.
    if matches!(error, Error::Process(RunnerError::HotSuperseded)) {
        return obsolete(adapter, request, diagnostic);
    }
    // These errors follow proven no-install/abort completion. Never suppress
    // uncertain cleanup, lost ownership, or an unexpected protocol failure.
    let consumed = matches!(
        error,
        Error::Process(
            RunnerError::Node(_) | RunnerError::Snapshot(_) | RunnerError::HotConsumedRejected(_)
        )
    );
    if consumed && !adapter.gate().request_is_current(request) {
        return Ok(Attempt::Settled);
    }
    Err(super::framework(error, diagnostic.clone()))
}
