//! One status line on stderr while a cold launch builds its Rust dependencies.
//!
//! The line carries one overall bar for the whole launch. Each stage owns a
//! weight: the previous cold launch's stage durations for this project (kept in
//! `<project>/target/rawr/cache/progress`), else [`DEFAULT_WEIGHTS`]. Within a stage,
//! progress is 90% of the crate fraction (finished crates over the unit-graph
//! total, else over the previous launch's count for that stage), with the gap
//! to the stage end closed by a time fraction (elapsed over the previous
//! launch's stage duration, else an easing toward [`DEFAULT_SECONDS`]). It never
//! reaches the stage end on time alone, and a slow tail of large crates still
//! advances. Counts come from Cargo's own output: one stdout
//! JSON line per finished unit (`--message-format json`), or one `Compiling`
//! stderr line per crate. The shown percent never goes backwards. A launch whose
//! first stage settles within [`QUIET`] prints nothing, so warm launches stay
//! silent; once shown, the line stays up until the window appears or the launch
//! fails.
//!
//! A stage begins where its work does: a Cargo child of its kind, the
//! runner's generation (building runner) and the runner's build, a hit
//! included (starting). The line names the stage, or the step of it the
//! launch named last, such as the check's "reading API" (#1763).
//!
//! A check shows its phase on the same line instead: the phase's name, and
//! for extraction the units Cargo has finished of the planned total. It has
//! no bar and no clock, and is redrawn only when the phase or its count moves.

use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STAGES: [&str; 4] = [
    "resolving dependencies",
    "building API",
    "building runner",
    "starting",
];
/// First-launch stage weights: resolve, build API, build runner, start.
const DEFAULT_WEIGHTS: [f64; 4] = [0.10, 0.45, 0.40, 0.05];
/// First-launch stage time estimates, in seconds, when no record exists.
const DEFAULT_SECONDS: [f64; 4] = [20.0, 150.0, 120.0, 10.0];
/// Crates alone carry a stage this far; time closes the rest.
const CRATE_SHARE: f64 = 0.9;
/// Time alone never carries a stage past this.
const TIME_CAP: f64 = 0.98;
const QUIET: Duration = Duration::from_secs(1);
const TICK: Duration = Duration::from_millis(250);

/// One stage as a launch saw it: crates finished and how long it took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Record {
    crates: usize,
    millis: u64,
}

struct State {
    launch: Option<Instant>,
    stage: usize,
    since: Option<Instant>,
    lines: usize,
    compiling: usize,
    units: Option<usize>,
    tick: usize,
    drawn: bool,
    announced: usize,
    notes: Vec<String>,
    file: Option<PathBuf>,
    previous: Option<[Record; 4]>,
    current: [Record; 4],
    compiled: bool,
    shown: f64,
    /// Whether this process shows a check's phases.
    phases: bool,
    phase: Option<&'static str>,
    /// The step of its stage a launch named last, which its line says in
    /// place of the stage's name (#1763).
    step: Option<&'static str>,
}

/// The phase whose line counts Cargo's finished units.
const EXTRACTING: &str = "extracting";

const RECORD: Record = Record {
    crates: 0,
    millis: 0,
};

static STATE: Mutex<State> = Mutex::new(State::new());
/// How many lines [`note`] and [`say`] have printed in this process.
static SAID: AtomicUsize = AtomicUsize::new(0);

/// Who hears each stage and each 5% step of a launch beside the status
/// line, such as a supervising `rawr mcp` (set by the command).
static OBSERVER: Mutex<Option<Observer>> = Mutex::new(None);

/// Hears a stage's name and the launch's percent.
type Observer = fn(&str, u8);

/// What the observer heard last: the stage and the 5% step.
static OBSERVED: Mutex<Option<(usize, u8)>> = Mutex::new(None);

/// Tell `observer` each launch's stage and percent as they move.
pub fn listen(observer: fn(&str, u8)) {
    *OBSERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(observer);
}

fn report(state: &mut State) {
    let Some(observer) = *OBSERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    else {
        return;
    };
    let percent = (state.percent(Instant::now()) * 100.0).floor() as u8;
    let step = (state.stage, percent / 5);
    let mut observed = OBSERVED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *observed != Some(step) {
        *observed = Some(step);
        observer(STAGES[state.stage.clamp(1, STAGES.len()) - 1], percent);
    }
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Start showing progress for one launch of the project at `project` until
/// [`finish`].
pub fn begin(project: &Path) {
    let file = crate::location::rawr_directory_here(project).join("cache/progress");
    let previous = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| parse(&text));
    let mut state = lock();
    let notes = std::mem::take(&mut state.notes);
    *state = State::new();
    state.notes = notes;
    state.start(Instant::now(), Some(file), previous);
    drop(state);
    std::thread::spawn(|| loop {
        std::thread::sleep(TICK);
        let mut state = lock();
        if state.launch.is_none() {
            return;
        }
        draw(&mut state);
        report(&mut state);
    });
}

/// Clear the status line: the window is up (`success`), or the launch failed.
/// A successful launch that compiled anything records its stages for the next.
pub fn finish(success: bool) {
    let mut state = lock();
    if state.launch.is_some() {
        state.close(Instant::now());
        if let Some(file) = state.file.as_ref().filter(|_| success && state.compiled) {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, format(&state.current));
        }
    }
    clear(&mut state);
    state.launch = None;
    state.phases = false;
    state.phase = None;
    state.step = None;
}

/// Show a check's phases on the status line until [`finish`].
pub fn begin_phases() {
    lock().phases = true;
}

/// Enter `phase` of a check. During a launch, the phase is the step of
/// its stage the line names; outside a check or a launch, nothing is
/// shown.
pub fn phase(phase: &'static str) {
    mark(phase);
    let mut state = lock();
    if state.launch.is_some() {
        if state.step != Some(phase) {
            state.step = Some(phase);
            draw(&mut state);
        }
        return;
    }
    if !state.phases || state.phase == Some(phase) {
        return;
    }
    state.phase = Some(phase);
    if phase == EXTRACTING {
        state.lines = 0;
    }
    draw(&mut state);
}

/// Print `line` once per process, above the status line.
pub fn note(key: &str, line: &str) {
    let mut state = lock();
    if state.notes.iter().any(|seen| seen == key) {
        return;
    }
    state.notes.push(key.into());
    say_above(&mut state, line);
}

/// Print `line` above the status line, every time.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn say(line: &str) {
    say_above(&mut lock(), line);
}

/// How many times this process ran Cargo's resolution of the program's
/// dependencies, built them with the nightly, and ran the probe that
/// extracts their API ([`work`]).
static RESOLVED: AtomicUsize = AtomicUsize::new(0);
static BUILT: AtomicUsize = AtomicUsize::new(0);
static PROBED: AtomicUsize = AtomicUsize::new(0);
/// How many API tables this process read from the store.
static LOADED: AtomicUsize = AtomicUsize::new(0);

/// Count one run of Cargo's resolution.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn resolved() {
    RESOLVED.fetch_add(1, Ordering::Relaxed);
}

/// Count one nightly build of the dependencies.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn built() {
    BUILT.fetch_add(1, Ordering::Relaxed);
}

/// Count one run of the probe.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn probed() {
    PROBED.fetch_add(1, Ordering::Relaxed);
    count_round(|round| &mut round.probed, 1);
}

/// How many units this process's runner builds compiled: the artifacts
/// Cargo reported that it did not find fresh ([`runner_units`]).
static RUNNER_UNITS: AtomicUsize = AtomicUsize::new(0);
/// How many times this process ran Cargo to build a runner.
static RUNNER_BUILDS: AtomicUsize = AtomicUsize::new(0);

/// Count one runner build and the units it compiled, from its JSON
/// `messages`. With `RAWR_VERBOSE`, say which units it compiled, in
/// Cargo's order, and the time from its start to its last artifact,
/// `took` (#1795).
pub(crate) fn runner_built(messages: &[u8], took: Duration) {
    RUNNER_BUILDS.fetch_add(1, Ordering::Relaxed);
    let compiled = compiled_units(messages);
    RUNNER_UNITS.fetch_add(compiled.len(), Ordering::Relaxed);
    if !verbose() {
        return;
    }
    say(&format!(
        "rawr-import: the runner build compiled {} units in {:.1} s: {}",
        compiled.len(),
        took.as_secs_f64(),
        compiled.join(", "),
    ));
}

/// Count the units one nightly check compiled, from its JSON `messages`,
/// toward the current round. With `RAWR_VERBOSE`, say which, in Cargo's
/// order, and the time from its start to its end, `took` (#1815).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn nightly_checked(messages: &[u8], took: Duration) {
    let compiled = compiled_units(messages);
    count_round(|round| &mut round.compiled, compiled.len());
    if verbose() {
        say(&format!(
            "rawr-import: the nightly check compiled {} units in {:.1} s: {}",
            compiled.len(),
            took.as_secs_f64(),
            compiled.join(", "),
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long one run of the probe took, `took`.
#[cfg(feature = "rustc-driver")]
pub(crate) fn probe_ran(took: Duration) {
    if verbose() {
        say(&format!(
            "rawr-import: the probe ran in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

thread_local! {
    /// Whether this thread rendered a runner's sources since its last
    /// generation said so ([`rendered`]).
    static RENDERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Mark the runner's sources as rendered by this thread's generation, not
/// kept from an earlier rendering (#1821).
pub(crate) fn rendered() {
    RENDERED.set(true);
}

/// With `RAWR_VERBOSE`, say how long generating a runner's sources took,
/// `took`, and how many of its `files` it rendered: all of them, or none
/// when it kept an earlier rendering of the same inputs (#1816, #1821).
pub(crate) fn runner_generated(took: Duration, files: usize) {
    let rendered = if RENDERED.replace(false) { files } else { 0 };
    if verbose() {
        say(&format!(
            "rawr-import: the runner's sources were generated in {:.1} s: rendered {rendered} of {files} files",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long staging a runner's sources for its
/// build took, `took`, and how many of its `files` it wrote, `written`:
/// the others were there as they are (#1816).
pub(crate) fn runner_staged(took: Duration, written: usize, files: usize) {
    if verbose() {
        say(&format!(
            "rawr-import: the runner's sources were staged in {:.1} s: wrote {written} of {files} files",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long the API graph a warm start deferred
/// took to build off the start's path, `took` (#1815).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn deferred_graph(took: Duration) {
    if verbose() {
        say(&format!(
            "rawr-import: the API graph a start deferred built in the background in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long reading and settling a replayed API's
/// fragments took off the replay's path, `took` (#1815).
#[cfg(feature = "rustc-driver")]
pub(crate) fn kept_reads(took: Duration) {
    if verbose() {
        say(&format!(
            "rawr-import: the API's fragments were read and settled in the background in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long a round waited, `took`, for the API
/// graph a background build held (#1815).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn waited_for_graph(took: Duration) {
    if verbose() && !UNCOUNTED.get() {
        say(&format!(
            "rawr-import: the round waited {:.1} s for the API graph a background build held",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long a round took, `took`, to build the API
/// graph again from its stored product (#1815).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn rebuilt_graph(took: Duration) {
    if verbose() && !UNCOUNTED.get() {
        say(&format!(
            "rawr-import: the round built the API graph from the store in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

/// The names of the units Cargo's JSON `messages` say it compiled: each
/// artifact it did not find fresh.
fn compiled_units(messages: &[u8]) -> Vec<String> {
    messages
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            count(line, b"\"reason\":\"compiler-artifact\"") > 0
                && count(line, b"\"fresh\":false") > 0
        })
        .map(|line| {
            serde_json::from_slice::<serde_json::Value>(line)
                .ok()
                .and_then(|message| message["target"]["name"].as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect()
}

/// Whether `RAWR_VERBOSE` asks the process to say what it did.
pub(crate) fn verbose() -> bool {
    std::env::var_os("RAWR_VERBOSE").is_some_and(|value| !value.is_empty() && value != "0")
}

/// How many units this process's runner builds compiled: a warm start's
/// build finds every unit fresh and compiles none.
#[must_use]
pub fn runner_units() -> usize {
    RUNNER_UNITS.load(Ordering::Relaxed)
}

/// How many times this process ran Cargo to build a runner.
#[must_use]
pub fn runner_builds() -> usize {
    RUNNER_BUILDS.load(Ordering::Relaxed)
}

/// How many times this process walked a runner's read-set closure, each
/// walk visiting every entry of it: a check of the entries' current
/// inputs, or of their storage alone (#1760).
static READ_SET_WALKS: AtomicUsize = AtomicUsize::new(0);

/// Count one walk of a runner's read-set closure.
pub(crate) fn walked_read_set() {
    READ_SET_WALKS.fetch_add(1, Ordering::Relaxed);
}

/// How many times this process walked a runner's read-set closure: a
/// start that needs no native build walks it once (#1760).
#[must_use]
pub fn read_set_walks() -> usize {
    READ_SET_WALKS.load(Ordering::Relaxed)
}

/// Count one API table read from the store.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn loaded() {
    LOADED.fetch_add(1, Ordering::Relaxed);
}

/// What this process has done to read its dependencies' API so far.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Work {
    resolved: usize,
    built: usize,
    probed: usize,
    loaded: usize,
}

impl Work {
    /// How many times Cargo resolved the dependencies.
    #[must_use]
    pub const fn resolved(self) -> usize {
        self.resolved
    }

    /// How many times the nightly built them.
    #[must_use]
    pub const fn built(self) -> usize {
        self.built
    }

    /// How many times the probe extracted their API.
    #[must_use]
    pub const fn probed(self) -> usize {
        self.probed
    }

    /// How many API tables were read from the store.
    #[must_use]
    pub const fn loaded(self) -> usize {
        self.loaded
    }
}

impl std::fmt::Display for Work {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Cargo resolutions {}, nightly builds {}, probes {}",
            self.resolved, self.built, self.probed
        )
    }
}

/// What this process has done to read its dependencies' API so far.
#[must_use]
pub fn work() -> Work {
    Work {
        resolved: RESOLVED.load(Ordering::Relaxed),
        built: BUILT.load(Ordering::Relaxed),
        probed: PROBED.load(Ordering::Relaxed),
        loaded: LOADED.load(Ordering::Relaxed),
    }
}

/// What one round of a check did at each stage: how many resolution
/// harnesses it bound, how many units the nightly compiled, how many build
/// outputs it copied into a resolution bundle and how many bundles it
/// sealed, whether the probe ran, how many definitions the round read from
/// the API, how many compiler sidecars it canonicalized into an API, how
/// many definitions it classified, how many classified paths it generated
/// bindings for, and how many of the program's sources it parsed. A stage a cache
/// answered counts nothing. Last, what its replays of a stored Cargo view
/// did: how many parsed the view's metadata, how many read and hashed its
/// recorded inputs again, and how many took what this process kept (#1759).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Round {
    harnessed: usize,
    compiled: usize,
    copied: usize,
    sealed: usize,
    probed: usize,
    read: usize,
    canonicalized: usize,
    classified: usize,
    generated: usize,
    parsed: usize,
    views: Views,
}

/// What one round's replays of a stored Cargo view did ([`View`]).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Views {
    parsed: usize,
    compared: usize,
    kept: usize,
}

impl std::fmt::Display for Round {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "harnesses {}, compiled {}, copied {}, sealed {}, probes {}, read {}, \
             canonicalized {}, classified {}, generated {}, parsed {}, \
             Cargo views parsed {}, compared {}, kept {}",
            self.harnessed,
            self.compiled,
            self.copied,
            self.sealed,
            self.probed,
            self.read,
            self.canonicalized,
            self.classified,
            self.generated,
            self.parsed,
            self.views.parsed,
            self.views.compared,
            self.views.kept,
        )
    }
}

/// Each round this process began ([`begin_round`]), in order.
static ROUNDS: Mutex<Vec<Round>> = Mutex::new(Vec::new());

/// Begin one round of a check: the stages that follow count toward it.
pub fn begin_round() {
    rounds_lock().push(Round::default());
}

/// When each phase of a round began ([`phase`]): the round's index, the
/// phase and the time.
static MARKS: Mutex<Vec<(usize, &'static str, Instant)>> = Mutex::new(Vec::new());

/// Mark `phase` as begun in the current round, unless it is the phase the
/// round is in; outside a round, or [`uncounted`], nothing is marked.
fn mark(phase: &'static str) {
    if UNCOUNTED.get() {
        return;
    }
    let Some(round) = rounds_lock().len().checked_sub(1) else {
        return;
    };
    let mut marks = MARKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = marks
        .last()
        .filter(|(index, _, _)| *index == round)
        .map(|(_, current, _)| *current);
    if current != Some(phase) {
        marks.push((round, phase, Instant::now()));
    }
}

/// How long each phase of the round at `index` took, in order: the last
/// one until now (#1815).
#[must_use]
pub fn phases(index: usize) -> Phases {
    let marks = MARKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let marks = marks
        .iter()
        .filter(|(round, _, _)| *round == index)
        .collect::<Vec<_>>();
    let now = Instant::now();
    Phases(
        marks
            .iter()
            .enumerate()
            .map(|(at, (_, phase, began))| {
                let ended = marks.get(at + 1).map_or(now, |(_, _, next)| *next);
                (*phase, ended.duration_since(*began))
            })
            .collect(),
    )
}

/// The phases of one round and how long each took ([`phases`]).
pub struct Phases(Vec<(&'static str, Duration)>);

impl std::fmt::Display for Phases {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (at, (phase, took)) in self.0.iter().enumerate() {
            if at > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{phase} {:.1} s", took.as_secs_f64())?;
        }
        Ok(())
    }
}

/// The rounds this process began, each with what its stages did.
#[must_use]
pub fn rounds() -> Vec<Round> {
    rounds_lock().clone()
}

fn rounds_lock() -> std::sync::MutexGuard<'static, Vec<Round>> {
    ROUNDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

thread_local! {
    /// Whether this thread's work counts toward no round ([`uncounted`]).
    static UNCOUNTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `work` on this thread counting toward no round: what a process
/// does off a round's path, as it reads ahead for the next save, is no
/// stage of whichever round is current when it lands.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn uncounted<T>(work: impl FnOnce() -> T) -> T {
    UNCOUNTED.set(true);
    let done = work();
    UNCOUNTED.set(false);
    done
}

/// Add to the current round's count of one stage; outside a round, or
/// [`uncounted`], nothing is counted.
fn count_round(stage: impl FnOnce(&mut Round) -> &mut usize, amount: usize) {
    if UNCOUNTED.get() {
        return;
    }
    if let Some(round) = rounds_lock().last_mut() {
        *stage(round) += amount;
    }
}

/// Count `definitions` read from the API in this round: those a
/// canonicalization joined and settled, or those decoded from a stored
/// API. A graph built from the last settlement reads none (#1786).
pub(crate) fn read(definitions: usize) {
    count_round(|round| &mut round.read, definitions);
}

/// Count the compiler `sidecars` canonicalized into an API in this round:
/// those read anew, as a sidecar whose read the process kept is not (#1776).
pub(crate) fn canonicalized(sidecars: usize) {
    count_round(|round| &mut round.canonicalized, sidecars);
}

/// Count `definitions` classified in this round.
pub(crate) fn classified(definitions: usize) {
    count_round(|round| &mut round.classified, definitions);
}

/// Count the classified paths bindings were generated for in this round.
pub(crate) fn generated(outcomes: usize) {
    count_round(|round| &mut round.generated, outcomes);
}

/// Count one resolution harness bound in this round.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn harnessed() {
    count_round(|round| &mut round.harnessed, 1);
}

/// Count one build output copied into a resolution bundle in this round.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn copied() {
    count_round(|round| &mut round.copied, 1);
}

/// Count one resolution bundle sealed in this round.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn sealed() {
    count_round(|round| &mut round.sealed, 1);
}

/// Count `sources` of the program parsed in this round.
pub fn parsed(sources: usize) {
    count_round(|round| &mut round.parsed, sources);
}

/// What one replay of a stored Cargo view did ([`Round`]).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum View {
    /// It parsed the view's metadata and unit graph.
    Parsed,
    /// It took the parse this process kept, and read and hashed the
    /// recorded inputs again.
    Compared,
    /// It took the parse and the comparison this process kept.
    Kept,
}

/// Count one replay of a stored Cargo view in this round.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn view(view: View) {
    count_round(
        |round| match view {
            View::Parsed => &mut round.views.parsed,
            View::Compared => &mut round.views.compared,
            View::Kept => &mut round.views.kept,
        },
        1,
    );
}

/// How many lines this process has printed above the status line: a
/// command that did nothing printed none.
pub fn said() -> usize {
    SAID.load(Ordering::Relaxed)
}

fn say_above(state: &mut State, line: &str) {
    SAID.fetch_add(1, Ordering::Relaxed);
    let shown = state.drawn;
    clear(state);
    crate::process::say(line);
    if shown {
        state.drawn = true;
        draw(state);
    }
}

/// Enter `stage` (1-based); stages only move forward.
pub(crate) fn stage(stage: usize) {
    let mut state = lock();
    state.advance(stage, Instant::now());
    let launching = state.launch.is_some();
    drop(state);
    // The build that feeds extraction is a check's extracting phase; a
    // launch's line names it by its stage.
    if stage == 2 && !launching {
        phase(EXTRACTING);
    }
}

/// Record the planned unit count from a `--unit-graph` listing.
pub(crate) fn plan(unit_graph: &[u8]) {
    let units = count(unit_graph, b"\"pkg_id\"");
    if units > 0 {
        lock().units = Some(units);
    }
}

/// Observe one chunk of a building Cargo child's stdout or stderr.
pub(crate) fn observe(stdout: bool, chunk: &[u8]) {
    let mut state = lock();
    if stdout {
        let lines = count(chunk, b"\n");
        state.lines += lines;
        if lines > 0 && state.phase == Some(EXTRACTING) {
            draw(&mut state);
        }
    } else {
        state.compiling += count(chunk, b"Compiling ");
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

impl State {
    const fn new() -> Self {
        Self {
            launch: None,
            stage: 0,
            since: None,
            lines: 0,
            compiling: 0,
            units: None,
            tick: 0,
            drawn: false,
            announced: 0,
            notes: Vec::new(),
            file: None,
            previous: None,
            current: [RECORD; 4],
            compiled: false,
            shown: 0.0,
            phases: false,
            phase: None,
            step: None,
        }
    }

    /// What the line says the launch is doing: the step of its stage it
    /// named last, else the stage.
    fn label(&self) -> &'static str {
        self.step
            .unwrap_or(STAGES[self.stage.clamp(1, STAGES.len()) - 1])
    }

    fn start(&mut self, now: Instant, file: Option<PathBuf>, previous: Option<[Record; 4]>) {
        self.launch = Some(now);
        self.stage = 1;
        self.since = Some(now);
        self.file = file;
        self.previous = previous;
    }

    /// Close the current stage's record at `now`.
    fn close(&mut self, now: Instant) {
        let Some(since) = self.since else { return };
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        self.current[index] = Record {
            crates: self.lines.max(self.compiling),
            millis: u64::try_from(now.saturating_duration_since(since).as_millis())
                .unwrap_or(u64::MAX),
        };
        self.compiled |= self.compiling > 0;
    }

    fn advance(&mut self, stage: usize, now: Instant) {
        if self.launch.is_some() && stage > self.stage && stage <= STAGES.len() {
            self.close(now);
            self.stage = stage;
            self.since = Some(now);
            self.lines = 0;
            self.compiling = 0;
            self.step = None;
        }
    }

    /// Fraction of the current stage done at `now`, in `[0, 1)`.
    fn fraction(&self, now: Instant) -> f64 {
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        let previous = self.previous.map(|records| records[index]);
        let total = (self.stage == 2)
            .then_some(self.units)
            .flatten()
            .or(previous.map(|record| record.crates))
            .filter(|total| *total > 0);
        // Cargo may report more finished units than the plan listed; cap rather
        // than dropping the bar.
        let crates = total.map_or(0.0, |total| {
            (self.lines.max(self.compiling) as f64 / total as f64).min(1.0) * CRATE_SHARE
        });
        let elapsed = self.since.map_or(0.0, |since| {
            now.saturating_duration_since(since).as_secs_f64()
        });
        let time = match previous.filter(|record| record.millis > 0) {
            Some(record) => (elapsed / (record.millis as f64 / 1000.0)).min(1.0),
            None => 1.0 - (-elapsed / DEFAULT_SECONDS[index]).exp(),
        };
        // At least max(crates, time × cap), and moving whenever time passes.
        crates + (TIME_CAP - crates).max(0.0) * time
    }

    /// Overall launch fraction at `now`, never lower than any shown before.
    fn percent(&mut self, now: Instant) -> f64 {
        let overall = overall(self.stage, self.fraction(now), &weights(self.previous));
        self.shown = self.shown.max(overall).min(1.0);
        self.shown
    }
}

/// Stage weights from the previous launch's durations, else the defaults.
fn weights(previous: Option<[Record; 4]>) -> [f64; 4] {
    let Some(records) = previous else {
        return DEFAULT_WEIGHTS;
    };
    let sum: u64 = records.iter().map(|record| record.millis).sum();
    if sum == 0 {
        return DEFAULT_WEIGHTS;
    }
    records.map(|record| record.millis as f64 / sum as f64)
}

/// Completed stage weights plus the current stage's weight times `fraction`.
fn overall(stage: usize, fraction: f64, weights: &[f64; 4]) -> f64 {
    let index = stage.clamp(1, STAGES.len()) - 1;
    let before: f64 = weights[..index].iter().sum();
    (before + weights[index] * fraction.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

fn format(records: &[Record; 4]) -> String {
    records
        .iter()
        .map(|record| format!("{} {}\n", record.crates, record.millis))
        .collect()
}

fn parse(text: &str) -> Option<[Record; 4]> {
    let mut records = [RECORD; 4];
    let mut lines = text.lines();
    for record in &mut records {
        let mut fields = lines.next()?.split_whitespace();
        *record = Record {
            crates: fields.next()?.parse().ok()?,
            millis: fields.next()?.parse().ok()?,
        };
    }
    Some(records)
}

fn draw(state: &mut State) {
    let (Some(launch), Some(since)) = (state.launch, state.since) else {
        draw_phase(state);
        return;
    };
    if !state.drawn && since.elapsed() < QUIET {
        return;
    }
    // A program that has the screen draws it alone.
    if crate::process::terminal::raw() {
        return;
    }
    let percent = state.percent(Instant::now());
    let mut stderr = std::io::stderr().lock();
    if stderr.is_terminal() {
        let style = Style::terminal(&stderr);
        state.tick = state.tick.wrapping_add(1);
        let line = render(state.label(), percent, launch.elapsed(), state.tick, &style);
        let _ = write!(stderr, "\r\x1b[2K{line}");
        state.drawn = true;
    } else if state.announced < state.stage {
        let style = Style {
            width: None,
            unicode: false,
            color: false,
            truecolor: false,
        };
        let line = render(state.label(), percent, launch.elapsed(), 0, &style);
        let _ = writeln!(stderr, "{line}");
        state.announced = state.stage;
    }
    let _ = stderr.flush();
}

/// Draw a check's phase in place. A phase line is for a person at a
/// terminal; a log or a test reading stderr sees none of it.
fn draw_phase(state: &mut State) {
    let Some(phase) = state.phase.filter(|_| state.phases) else {
        return;
    };
    if crate::process::terminal::raw() {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    if !stderr.is_terminal() {
        return;
    }
    let count = (phase == EXTRACTING).then_some((state.lines, state.units));
    let line = render_phase(phase, count, &Style::terminal(&stderr));
    let _ = write!(stderr, "\r\x1b[2K{line}");
    let _ = stderr.flush();
    state.drawn = true;
}

fn clear(state: &mut State) {
    // While a program has the screen, the line stays until it is restored.
    if state.drawn && !crate::process::terminal::raw() {
        state.drawn = false;
        let _ = write!(std::io::stderr(), "\r\x1b[2K");
    }
}

/// Clear a finished launch's line that a program's screen kept.
pub(crate) fn clear_left() {
    let mut state = lock();
    if state.launch.is_none() {
        clear(&mut state);
    }
}

/// How one status line is drawn: `width` is `None` for plain (non-terminal) lines.
struct Style {
    width: Option<usize>,
    unicode: bool,
    color: bool,
    truecolor: bool,
}

impl Style {
    /// The style of the terminal `stderr` is.
    fn terminal(stderr: &std::io::StderrLock<'_>) -> Self {
        let width = rustix::termios::tcgetwinsize(stderr).map_or(80, |size| size.ws_col);
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
            .is_some_and(|value| value.to_ascii_lowercase().replace('-', "").contains("utf8"));
        Self {
            width: Some(usize::from(if width == 0 { 80 } else { width })),
            unicode: utf8,
            color: std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
            truecolor: palette::truecolor(),
        }
    }
}

/// The CLI's palette: the site's two reds (site/src/styles/tokens.css's
/// `--red` in its light and dark themes) and grays.
pub mod palette {
    /// One terminal color: 24-bit RGB with a 256-color fallback.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Color {
        pub rgb: (u8, u8, u8),
        pub ansi256: u8,
    }

    /// #E4002B: the one colored swatch (spinner, bar fill).
    pub const RED: Color = Color {
        rgb: (228, 0, 43),
        ansi256: 160,
    };
    /// #F87171: the `rawr` mark and percentages.
    pub const LIGHT_RED: Color = Color {
        rgb: (248, 113, 113),
        ansi256: 203,
    };
    /// #C8C8C8: labels.
    pub const LIGHT_GRAY: Color = Color {
        rgb: (200, 200, 200),
        ansi256: 251,
    };
    /// #8A8A8A: secondary figures such as elapsed time.
    pub const MID_GRAY: Color = Color {
        rgb: (138, 138, 138),
        ansi256: 245,
    };
    /// #4A4A4A: tracks and empty space.
    pub const DARK_GRAY: Color = Color {
        rgb: (74, 74, 74),
        ansi256: 239,
    };

    impl Color {
        /// Wrap `text` in this foreground color (`bold` optional), then reset.
        #[must_use]
        pub fn paint(self, text: &str, bold: bool, truecolor: bool) -> String {
            let weight = if bold { "1;" } else { "" };
            let (r, g, b) = self.rgb;
            if truecolor {
                format!("\x1b[{weight}38;2;{r};{g};{b}m{text}\x1b[0m")
            } else {
                format!("\x1b[{weight}38;5;{}m{text}\x1b[0m", self.ansi256)
            }
        }
    }

    /// Whether the terminal advertises 24-bit color.
    #[must_use]
    pub fn truecolor() -> bool {
        std::env::var("COLORTERM")
            .is_ok_and(|value| matches!(value.as_str(), "truecolor" | "24bit"))
    }
}

fn render(name: &str, percent: f64, elapsed: Duration, tick: usize, style: &Style) -> String {
    use palette::{DARK_GRAY, LIGHT_GRAY, LIGHT_RED, MID_GRAY, RED};
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let percent = percent.clamp(0.0, 1.0);
    let secs = elapsed.as_secs();
    let time = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
    let share = format!("{:.0}%", (percent * 100.0).floor());
    let Some(width) = style.width else {
        return format!("rawr > {name}  {share}  {time}");
    };
    let spin = if style.unicode {
        SPIN[tick % SPIN.len()]
    } else {
        ['|', '/', '-', '\\'][tick % 4]
    };
    let spin = spin.to_string();
    // "rawr S name  " + bar + "  " + "P%  T"
    let used = 5 + 2 + name.chars().count() + 2 + share.chars().count() + 2 + time.chars().count();
    let cells = width.saturating_sub(used + 3).min(24);
    let filled = ((percent * cells as f64) as usize).min(cells);
    let (fill, empty) = if style.unicode {
        ('━', '─')
    } else {
        ('=', '-')
    };
    let bar: String = std::iter::repeat_n(fill, filled).collect();
    let rest: String = std::iter::repeat_n(empty, cells - filled).collect();
    if cells < 8 || !style.color {
        let middle = if cells < 8 {
            String::new()
        } else {
            format!("{bar}{rest}  ")
        };
        let line = format!("rawr {spin} {name}  {middle}{share}  {time}");
        return if line.chars().count() >= width {
            line.chars().take(width.saturating_sub(1)).collect()
        } else {
            line
        };
    }
    let tc = style.truecolor;
    format!(
        "{} {} {}  {}{}  {}  {}",
        LIGHT_RED.paint("rawr", true, tc),
        RED.paint(&spin, false, tc),
        LIGHT_GRAY.paint(name, false, tc),
        RED.paint(&bar, false, tc),
        DARK_GRAY.paint(&rest, false, tc),
        LIGHT_RED.paint(&share, false, tc),
        MID_GRAY.paint(&time, false, tc),
    )
}

/// One check phase's line: its name, and `count`'s finished units of the
/// planned total when the phase counts them.
fn render_phase(phase: &str, count: Option<(usize, Option<usize>)>, style: &Style) -> String {
    use palette::{LIGHT_GRAY, LIGHT_RED, RED};
    let count = match count {
        // Cargo may report more finished units than the plan listed.
        Some((done, Some(total))) => format!("{} of {total}", done.min(total)),
        Some((done, None)) => done.to_string(),
        None => String::new(),
    };
    let line = if count.is_empty() {
        format!("rawr > {phase}")
    } else {
        format!("rawr > {phase}  {count}")
    };
    let width = style.width.unwrap_or(usize::MAX);
    if line.chars().count() >= width {
        return line.chars().take(width.saturating_sub(1)).collect();
    }
    if !style.color {
        return line;
    }
    let tc = style.truecolor;
    let mut painted = format!(
        "{} {} {}",
        LIGHT_RED.paint("rawr", true, tc),
        RED.paint(">", false, tc),
        LIGHT_GRAY.paint(phase, false, tc),
    );
    if !count.is_empty() {
        painted.push_str("  ");
        painted.push_str(&LIGHT_RED.paint(&count, false, tc));
    }
    painted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_phase_line_names_the_phase_and_its_finished_units() {
        assert_eq!(render_phase("resolving", None, &WIDE), "rawr > resolving");
        assert_eq!(
            render_phase(EXTRACTING, Some((12, Some(340))), &WIDE),
            "rawr > extracting  12 of 340"
        );
        assert_eq!(
            render_phase(EXTRACTING, Some((400, Some(340))), &WIDE),
            "rawr > extracting  340 of 340"
        );
        assert_eq!(
            render_phase(EXTRACTING, Some((7, None)), &WIDE),
            "rawr > extracting  7"
        );
        let narrow = Style {
            width: Some(12),
            ..WIDE
        };
        assert_eq!(render_phase("classifying", None, &narrow), "rawr > clas");
    }

    const WIDE: Style = Style {
        width: Some(120),
        unicode: true,
        color: false,
        truecolor: false,
    };

    fn launch(previous: Option<[Record; 4]>) -> (State, Instant) {
        let now = Instant::now();
        let mut state = State::new();
        state.start(now, None, previous);
        (state, now)
    }

    #[test]
    fn renders_spinner_bar_and_plain_lines() {
        assert_eq!(count(b"{\"pkg_id\":1}{\"pkg_id\":2}", b"\"pkg_id\""), 2);
        assert_eq!(count(b"   Compiling a\n   Compiling b\n", b"Compiling "), 2);
        assert_eq!(
            render(STAGES[2], 0.67, Duration::from_secs(108), 2, &WIDE),
            "rawr ⠹ building runner  ━━━━━━━━━━━━━━━━────────  67%  1m48s"
        );
        let colored = Style {
            width: Some(120),
            unicode: true,
            color: true,
            truecolor: true,
        };
        assert_eq!(
            render(STAGES[2], 0.5, Duration::from_secs(9), 3, &colored),
            "\x1b[1;38;2;248;113;113mrawr\x1b[0m \x1b[38;2;228;0;43m⠸\x1b[0m \
             \x1b[38;2;200;200;200mbuilding runner\x1b[0m  \
             \x1b[38;2;228;0;43m━━━━━━━━━━━━\x1b[0m\x1b[38;2;74;74;74m────────────\x1b[0m  \
             \x1b[38;2;248;113;113m50%\x1b[0m  \x1b[38;2;138;138;138m9s\x1b[0m"
        );
        let colored = Style {
            truecolor: false,
            ..colored
        };
        assert!(render(STAGES[2], 0.5, Duration::ZERO, 0, &colored).contains("\x1b[38;5;160m━"));
        let narrow = Style {
            width: Some(42),
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(STAGES[1], 0.52, Duration::from_secs(72), 1, &narrow),
            "rawr / building API  ====----  52%  1m12s"
        );
        let tiny = Style {
            width: Some(20),
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(STAGES[2], 0.1, Duration::ZERO, 0, &tiny)
                .chars()
                .count(),
            19
        );
        let plain = Style {
            width: None,
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(STAGES[0], 0.04, Duration::from_secs(4), 0, &plain),
            "rawr > resolving dependencies  4%  4s"
        );
    }

    #[test]
    fn a_launch_line_names_its_last_step_until_its_stage_moves() {
        let (mut state, now) = launch(None);
        assert_eq!(state.label(), "resolving dependencies");
        state.step = Some("reading API");
        assert_eq!(state.label(), "reading API");
        state.advance(3, now);
        assert_eq!(state.label(), "building runner");
        state.advance(4, now);
        assert_eq!(state.label(), "starting");
    }

    #[test]
    fn overall_percent_sums_finished_stages_and_the_current_fraction() {
        assert!((overall(1, 0.5, &DEFAULT_WEIGHTS) - 0.05).abs() < 1e-9);
        assert!((overall(2, 0.5, &DEFAULT_WEIGHTS) - 0.325).abs() < 1e-9);
        assert!((overall(3, 0.0, &DEFAULT_WEIGHTS) - 0.55).abs() < 1e-9);
        assert!((overall(4, 1.0, &DEFAULT_WEIGHTS) - 1.0).abs() < 1e-9);
        assert!((overall(2, 7.0, &DEFAULT_WEIGHTS) - 0.55).abs() < 1e-9);

        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.units = Some(200);
        state.lines = 100;
        assert!((state.percent(now) - (0.10 + 0.45 * 0.45)).abs() < 1e-9);
        // More finished units than the plan listed caps the crate share.
        state.lines = 284;
        assert!((state.percent(now) - (0.10 + 0.45 * 0.9)).abs() < 1e-9);
        let line = render(STAGES[1], state.percent(now), Duration::ZERO, 0, &WIDE);
        assert!(line.contains('━') && line.contains("  50%"), "{line}");
    }

    #[test]
    fn unknown_totals_use_the_previous_count_else_ease_short_of_the_end() {
        let mut previous = [RECORD; 4];
        previous[2] = Record {
            crates: 100,
            millis: 100_000,
        };
        let (mut state, now) = launch(Some(previous));
        state.advance(3, now);
        state.compiling = 50;
        assert!((state.fraction(now) - 0.45).abs() < 1e-9);
        // Time over the previous duration closes the gap to the cap.
        let expected = 0.45 + (TIME_CAP - 0.45) * 0.6;
        assert!((state.fraction(now + Duration::from_secs(60)) - expected).abs() < 1e-9);
        assert_eq!(state.fraction(now + Duration::from_secs(600)), TIME_CAP);

        let (mut state, now) = launch(None);
        state.advance(3, now);
        let late = state.fraction(now + Duration::from_secs(6_000));
        assert_eq!(late, TIME_CAP);
        assert!(state.fraction(now + Duration::from_secs(30)) < late);
    }

    #[test]
    fn many_fast_crates_then_a_slow_tail_keeps_advancing() {
        let api = Record {
            crates: 283,
            millis: 264_000,
        };
        for previous in [None, Some([RECORD, api, RECORD, RECORD])] {
            let (mut state, now) = launch(previous);
            state.advance(2, now);
            state.units = Some(283);
            // Nearly every crate lands in the first 25 s...
            state.lines = 280;
            let mut last = state.percent(now + Duration::from_secs(25));
            // ...then a few large crates compile for minutes.
            for secs in (60..=240).step_by(30) {
                let shown = state.percent(now + Duration::from_secs(secs));
                assert!(
                    shown > last,
                    "stalled at {shown} after {secs}s ({previous:?})"
                );
                last = shown;
            }
            let end = overall(3, 0.0, &weights(previous));
            assert!(last < end, "time alone never reaches the stage end");
        }
    }

    #[test]
    fn shown_percent_never_goes_backwards() {
        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.units = Some(100);
        state.lines = 80;
        let high = state.percent(now);
        // A larger plan arriving later would lower the raw fraction.
        state.units = Some(400);
        assert_eq!(state.percent(now), high);
        state.lines = 0;
        state.advance(1, now);
        assert_eq!(state.percent(now), high);
    }

    #[test]
    fn stage_transition_keeps_the_bar_and_records_the_stage() {
        let (mut state, now) = launch(None);
        state.drawn = true;
        state.advance(2, now);
        state.units = Some(10);
        state.compiling = 10;
        let end = state.percent(now);
        let later = now + Duration::from_secs(5);
        state.advance(3, later);
        assert!(state.drawn, "a stage change never clears the line");
        let next = state.percent(later);
        assert!(next >= end);
        assert!(render(STAGES[2], next, Duration::ZERO, 0, &WIDE).contains('━'));
        assert_eq!(
            state.current[1],
            Record {
                crates: 10,
                millis: 5_000
            }
        );
        assert!(state.compiled);
    }

    #[test]
    fn learned_weights_round_trip() {
        let records = [
            Record {
                crates: 0,
                millis: 2_000,
            },
            Record {
                crates: 284,
                millis: 10_000,
            },
            Record {
                crates: 120,
                millis: 7_000,
            },
            Record {
                crates: 0,
                millis: 1_000,
            },
        ];
        assert_eq!(parse(&format(&records)), Some(records));
        assert_eq!(parse("1 2\n3 4\n"), None);
        assert_eq!(weights(None), DEFAULT_WEIGHTS);
        assert_eq!(weights(Some([RECORD; 4])), DEFAULT_WEIGHTS);
        assert_eq!(weights(Some(records)), [0.1, 0.5, 0.35, 0.05]);

        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.compiling = 3;
        state.advance(3, now + Duration::from_millis(1_500));
        state.close(now + Duration::from_millis(2_000));
        let read = parse(&format(&state.current)).unwrap();
        assert_eq!(
            read[1],
            Record {
                crates: 3,
                millis: 1_500
            }
        );
        assert_eq!(read[2].millis, 500);
        let next = weights(Some(read));
        assert!((next[1] - 0.75).abs() < 1e-9);
    }
}
