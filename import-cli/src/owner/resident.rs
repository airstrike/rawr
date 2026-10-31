//! The short Rawr edit path, before Cargo, extraction or symbolic compilation.

use std::path::PathBuf;

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
    /// islands, whose host says what a new runner means.
    pub fn restart_reason(&self, next: &Self) -> Option<String> {
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
                .unwrap_or_else(|| "native code changed".to_owned()),
        )
    }
}

/// One native generation's watched identities and their derived inputs.
pub(super) type NativeInputs = (Box<[rawr_import::input::proof::Watch]>, source::Inputs);

pub(super) enum Attempt {
    Settled,
    /// The runner committed the new generation compiled against it.
    Swapped,
    Prepare(Option<rawr_watch::StagedInputs>),
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
    native_inputs: &mut Option<NativeInputs>,
    scope: &Scope,
    diagnostic: &rawr::diagnostic::Source,
) -> Result<Attempt, rawr::diagnostic::Report> {
    let native_current = adapter.resident_eligible(native_baseline);
    let capture = (|| {
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), target)
            .map_err(|error| crate::report::package(&error, diagnostic.clone()))?;
        // The resident route uses the same limits profile.
        let limits = graph.rust_limits();
        let eligible =
            !refresh_required && native_current && Manifests::from_graph(&graph) == *baseline;
        let graph = source::Graph::from_package(&graph);
        let snapshot = source::Snapshot::from_graph(&graph);
        let expected = snapshot
            .imported(&limits)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let bytes = snapshot
            .imported_transport(expected, &limits)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let watch = lane
            .native_watch()
            .map_err(|error| super::framework(error, diagnostic.clone()))?
            .collect::<Box<[_]>>();
        // No native event since the baseline: the native part derived for
        // these exact identities still stands; a Rawr edit derives only its own.
        if !native_current || native_inputs.as_ref().is_none_or(|(key, _)| *key != watch) {
            let derived = source::Inputs::imported(watch.iter().cloned());
            *native_inputs = Some((watch, derived));
        }
        let native = &native_inputs.as_ref().expect("derived above").1;
        let inputs = source::Inputs::from_graph(&graph).with_native(native);
        let observation = inputs
            .encode_observation()
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        // Register before the initial source check; this preserves complete
        // current+staged watch coverage throughout candidate work.
        let staged = adapter
            .prepare_inputs(inputs)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        let freshness = scope
            .observe(&bytes, &limits)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        Ok((bytes, freshness, observation, staged, eligible))
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
    let (bytes, freshness, observation, staged, eligible) = captured;
    if !adapter.gate().request_is_current(watch_request) {
        lane.cancel_compile(started)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        return Ok(Attempt::Settled);
    }
    if !eligible || started.is_none() {
        // A previous failed native replacement can suppress HotBegin while the
        // last-good child remains live. Its next cold attempt still needs the
        // newly discovered Rawr roots observed throughout full preparation.
        lane.cancel_compile(started)
            .map_err(|error| super::framework(error, diagnostic.clone()))?;
        return Ok(Attempt::Prepare(Some(staged)));
    }
    let started = started.expect("eligible resident attempts retain the original HotBegin");
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
                rawr_import::process::say(format_args!(
                    "Rawr imported-owner watcher warning: {warning}"
                ));
            }
            Ok(Attempt::Swapped)
        }
        resident::Outcome::NeedsPreparation if adapter.gate().request_is_current(watch_request) => {
            Ok(Attempt::Prepare(Some(staged)))
        }
        resident::Outcome::NeedsPreparation => Ok(Attempt::Settled),
        resident::Outcome::Obsolete => obsolete(adapter, watch_request, diagnostic),
        resident::Outcome::Source(reason) | resident::Outcome::Resource(reason) => {
            if !adapter.gate().request_is_current(watch_request) {
                return Ok(Attempt::Settled);
            }
            Err(super::framework(reason, diagnostic.clone()))
        }
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
