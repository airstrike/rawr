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
//!
//! One printer thread owns the line, the lines printed above it and the
//! lines held while a program has the terminal raw, with each round's
//! counts. Every other thread sends it events. A line, a launch's end, a
//! restore and a [`flush`] wait for its answer, so what they print is on
//! stderr when they return and keeps its order against the caller's own
//! writes. A count or a move of the line is sent without waiting.

use std::io::{IsTerminal as _, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, OnceLock};
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
    /// What the observer heard last of this launch: the stage and the 5%
    /// step.
    observed: Option<(usize, u8)>,
}

/// The phase whose line counts Cargo's finished units.
const EXTRACTING: &str = "extracting";

const RECORD: Record = Record {
    crates: 0,
    millis: 0,
};

/// The printer thread's inbox ([`printer`]).
static PRINTER: OnceLock<mpsc::Sender<Event>> = OnceLock::new();
/// How many lines [`note`] and [`say`] have printed in this process.
static SAID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Whether this thread is the printer, which hears no event it sends.
    static PRINTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Who hears each stage and each 5% step of a launch beside the status
/// line, such as a supervising `rawr mcp` (set once by the command).
static OBSERVER: OnceLock<Observer> = OnceLock::new();

/// Hears a stage's name and the launch's percent.
type Observer = fn(&str, u8);

/// What the printer thread hears: a change to the status line, a line to
/// print above it, a count toward a round, or a question with the sender
/// that takes its answer. An event that carries `done` is answered once
/// its output is written.
enum Event {
    Begin {
        at: Instant,
        file: PathBuf,
        previous: Option<[Record; 4]>,
    },
    Finish {
        success: bool,
        done: mpsc::Sender<()>,
    },
    BeginPhases,
    /// `mark` is when the phase began, when it counts toward a round.
    Phase {
        phase: &'static str,
        mark: Option<Instant>,
    },
    Stage {
        stage: usize,
        at: Instant,
        mark: Option<Instant>,
    },
    Plan(usize),
    /// Cargo's finished units on stdout and `Compiling` lines on stderr.
    Observe {
        lines: usize,
        compiling: usize,
    },
    Line {
        line: String,
        done: mpsc::Sender<()>,
    },
    Note {
        key: String,
        line: String,
        done: mpsc::Sender<()>,
    },
    /// A runner that had the terminal ended and its modes are back.
    Restored(mpsc::Sender<()>),
    Flush(mpsc::Sender<()>),
    BeginRound,
    Count {
        stage: fn(&mut Round) -> &mut usize,
        amount: usize,
    },
    Rounds(mpsc::Sender<Vec<Round>>),
    Phases {
        round: usize,
        answer: mpsc::Sender<Phases>,
    },
}

/// What the printer thread alone holds: the status line, the lines held
/// while a program has the terminal raw, and each round with the phases
/// it began.
struct Printer {
    line: State,
    held: Vec<String>,
    rounds: Vec<Round>,
    marks: Vec<(usize, &'static str, Instant)>,
    /// When a launch's line is drawn again, so its spinner and clock move.
    next_draw: Instant,
    out: Box<dyn Write + Send>,
    /// Whether `out` is a terminal, where the line is drawn in place.
    terminal: bool,
}

/// Tell `observer` each launch's stage and percent as they move. The first
/// observer a process names is the one it keeps.
pub fn listen(observer: fn(&str, u8)) {
    let _ = OBSERVER.set(observer);
}

fn report(state: &mut State) {
    let Some(observer) = OBSERVER.get() else {
        return;
    };
    let percent = (state.percent(Instant::now()) * 100.0).floor() as u8;
    let step = (state.stage, percent / 5);
    if state.observed != Some(step) {
        state.observed = Some(step);
        observer(STAGES[state.stage.clamp(1, STAGES.len()) - 1], percent);
    }
}

/// The printer thread's inbox, spawning the thread on first use. A thread
/// that cannot spawn drops the inbox's receiver, so every send fails and
/// each caller writes for itself.
fn printer() -> &'static mpsc::Sender<Event> {
    PRINTER.get_or_init(|| {
        let (sender, events) = mpsc::channel();
        let printer = Printer::new(Box::new(std::io::stderr()), std::io::stderr().is_terminal());
        let _ = std::thread::Builder::new()
            .name("rawr-printer".into())
            .spawn(move || printer.run(&events));
        sender
    })
}

/// Send `event` to the printer thread. The event comes back when no
/// printer hears it: on the printer thread itself, or once it is gone.
fn send(event: Event) -> Result<(), Event> {
    if PRINTING.get() {
        return Err(event);
    }
    printer().send(event).map_err(|error| error.0)
}

/// Send the event `asking` makes and wait for its answer, or `None` when
/// no printer heard it or the printer ended before answering.
fn ask<T>(asking: impl FnOnce(mpsc::Sender<T>) -> Event) -> Option<T> {
    let (answer, answered) = mpsc::channel();
    send(asking(answer)).ok()?;
    answered.recv().ok()
}

/// Write `line` to stderr from the calling thread, which no printer hears.
fn write_here(line: &str) {
    eprint!("{line}");
    if !line.ends_with('\n') {
        eprintln!();
    }
}

/// Start showing progress for one launch of the project at `project`, to
/// its [`finish`].
pub fn begin(project: &Path) {
    let file = crate::location::rawr_directory_here(project).join("cache/progress");
    let previous = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| parse(&text));
    let _ = send(Event::Begin {
        at: Instant::now(),
        file,
        previous,
    });
}

/// Clear the status line: the window is up (`success`), or the launch failed.
/// A successful launch that compiled anything records its stages for the next.
/// It returns once the line is cleared.
pub fn finish(success: bool) {
    let _ = ask(|done| Event::Finish { success, done });
}

/// Show a check's phases on the status line, to its [`finish`].
pub fn begin_phases() {
    let _ = send(Event::BeginPhases);
}

/// Enter `phase` of a check. During a launch, the phase is the step of
/// its stage the line names; outside a check or a launch, nothing is
/// shown.
pub fn phase(phase: &'static str) {
    let _ = send(Event::Phase {
        phase,
        mark: mark_here(),
    });
}

/// When a phase entered now begins for its round: never on a thread
/// [`uncounted`].
fn mark_here() -> Option<Instant> {
    (!UNCOUNTED.get()).then(Instant::now)
}

/// Print `line` once per process, above the status line. It returns once
/// the line is written or held.
pub fn note(key: &str, line: &str) {
    let heard = ask(|done| Event::Note {
        key: key.into(),
        line: line.into(),
        done,
    });
    if heard.is_none() {
        SAID.fetch_add(1, Ordering::Relaxed);
        write_here(line);
    }
}

/// Print `line` above the status line, every time.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn say(line: &str) {
    SAID.fetch_add(1, Ordering::Relaxed);
    print(line.to_owned());
}

/// Print `line` above the status line, or hold it while a program has the
/// terminal raw. It returns once the line is written or held, so it keeps
/// its order against what the caller writes to stderr next.
pub(crate) fn print(line: String) {
    let heard = ask(|done| Event::Line {
        line: line.clone(),
        done,
    });
    if heard.is_none() {
        write_here(&line);
    }
}

/// Print the lines held while a runner had the terminal raw, now that its
/// modes are back, and clear a finished launch's line its screen kept.
/// It returns once they are written.
pub(crate) fn restored() {
    let _ = ask(Event::Restored);
}

/// Write everything sent to the printer before this call, clear the status
/// line and print the held lines unless a program still has the terminal
/// raw. It returns once they are written. A process calls it before it
/// exits and before a panic's message.
pub fn flush() {
    let _ = ask(Event::Flush);
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

/// With `RAWR_VERBOSE`, say how long one step of a round took, `took`:
/// the steps of a native save's preparation besides its nightly check and
/// its probe, and the check of the program (#1794).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn timed(step: &str, took: Duration) {
    if verbose() && !UNCOUNTED.get() {
        say(&format!(
            "rawr-import: {step} in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

/// With `RAWR_VERBOSE`, say how long each step of the program's check
/// took, in order: loading it, installing the crates' facades, resolving
/// it, lowering it for its uses, and requiring them of the facade.
pub fn checked_steps(steps: &[(&str, Duration)]) {
    if !verbose() || UNCOUNTED.get() {
        return;
    }
    let steps = steps
        .iter()
        .map(|(step, took)| format!("{step} {:.1} s", took.as_secs_f64()))
        .collect::<Vec<_>>();
    say(&format!(
        "rawr-import: the program's check took: {}",
        steps.join(", ")
    ));
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
    count_round(|round| &mut round.rebuilt, 1);
    if verbose() && !UNCOUNTED.get() {
        say(&format!(
            "rawr-import: the round built the API graph from the store in {:.1} s",
            took.as_secs_f64()
        ));
    }
}

/// Count one API graph a round framed from a kept graph of the same
/// content under another build, in place of building it from the store
/// (#1786).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn framed_graph() {
    count_round(|round| &mut round.framed, 1);
}

/// Count one stored classification a round decoded, as the one an owner
/// kept was of another API or another classification (#1786).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn replayed_classification() {
    count_round(|round| &mut round.replayed, 1);
}

/// Count one API graph a round built from the store because the kept graph
/// of the same content could not be framed by its build; with
/// `RAWR_VERBOSE`, say so.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn unframed_graph() {
    count_round(|round| &mut round.rebuilt, 1);
    if verbose() && !UNCOUNTED.get() {
        say("rawr-import: the round could not frame the kept API graph, so it built it from the store");
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

/// What this process's typed store did so far, one counter per cost that
/// grows with the whole store (#1843, #1844).
static STORE: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];

/// One cost of the typed store ([`StoreWork`]).
#[derive(Clone, Copy)]
pub(crate) enum Store {
    /// It decoded the installation ledger from its file.
    LedgerDecode,
    /// It read one candidate index row while sizing the index.
    IndexRow,
    /// It flushed a file or directory to the drive, and the drive's cache
    /// to lasting storage.
    FullSync,
    /// It pushed a file or directory's writes to the drive, to last with
    /// the next flush.
    Push,
    /// It published one node.
    Publication,
    /// It checked one read-set entry against its storage.
    EntryWalked,
}

/// Count `amount` of one store cost.
pub(crate) fn store(cost: Store, amount: usize) {
    STORE[cost as usize].fetch_add(amount, Ordering::Relaxed);
}

/// What this process's typed store did so far ([`store_work`]).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoreWork([usize; 6]);

impl StoreWork {
    /// What the store did since `earlier`.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self(std::array::from_fn(|at| self.0[at] - earlier.0[at]))
    }
}

impl std::fmt::Display for StoreWork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [decodes, rows, flushes, pushes, publications, entries] = self.0;
        write!(
            formatter,
            "ledger decodes {decodes}, index rows read {rows}, flushes {flushes}, \
             pushes {pushes}, node publications {publications}, \
             read-set entries walked {entries}"
        )
    }
}

/// What this process's typed store did so far.
#[must_use]
pub fn store_work() -> StoreWork {
    StoreWork(std::array::from_fn(|at| STORE[at].load(Ordering::Relaxed)))
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
/// bindings for, how many of the program's sources it parsed, and how many
/// its check lowered rather than taking an earlier check's. A stage a cache
/// answered counts nothing. Last, what its replays of a stored Cargo view
/// did: how many parsed the view's metadata, how many read and hashed its
/// recorded inputs again, and how many took what this process kept (#1759).
/// And how many API graphs it built from the store, and how many it framed
/// from a kept graph of the same content under another build, and how many
/// stored classifications it decoded (#1786). And how many units of the
/// facades' headers (D604) and how many paths' sites its plans wrote,
/// rather than taking an earlier plan's (#2222).
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
    lowered: usize,
    views: Views,
    rebuilt: usize,
    framed: usize,
    replayed: usize,
    headers: usize,
    sites: usize,
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
             canonicalized {}, classified {}, generated {}, parsed {}, lowered {}, \
             Cargo views parsed {}, compared {}, kept {}, \
             API graphs rebuilt {}, framed {}, classifications replayed {}, \
             headers {}, sites {}",
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
            self.lowered,
            self.views.parsed,
            self.views.compared,
            self.views.kept,
            self.rebuilt,
            self.framed,
            self.replayed,
            self.headers,
            self.sites,
        )
    }
}

/// Begin one round of a check: the stages that follow count toward it.
pub fn begin_round() {
    let _ = send(Event::BeginRound);
}

/// How long each phase of the round at `index` took, in order: the last
/// one to now (#1815).
#[must_use]
pub fn phases(index: usize) -> Phases {
    ask(|answer| Event::Phases {
        round: index,
        answer,
    })
    .unwrap_or_default()
}

/// The phases of one round and how long each took ([`phases`]).
#[derive(Default)]
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
    ask(Event::Rounds).unwrap_or_default()
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
fn count_round(stage: fn(&mut Round) -> &mut usize, amount: usize) {
    if !UNCOUNTED.get() {
        let _ = send(Event::Count { stage, amount });
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

/// Count the units of the facades' headers a plan wrote in this round.
pub(crate) fn headed(units: usize) {
    count_round(|round| &mut round.headers, units);
}

/// Count the paths whose sites a plan wrote in this round.
pub(crate) fn sited(paths: usize) {
    count_round(|round| &mut round.sites, paths);
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

/// Count `sources` of the program its check lowered in this round, rather
/// than taking what an earlier check of them found.
pub fn lowered(sources: usize) {
    count_round(|round| &mut round.lowered, sources);
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
    let stage: fn(&mut Round) -> &mut usize = match view {
        View::Parsed => |round| &mut round.views.parsed,
        View::Compared => |round| &mut round.views.compared,
        View::Kept => |round| &mut round.views.kept,
    };
    count_round(stage, 1);
}

/// How many lines this process has printed above the status line: a
/// command that did nothing printed none.
pub fn said() -> usize {
    SAID.load(Ordering::Relaxed)
}

/// Enter `stage` (1-based); stages only move forward.
pub(crate) fn stage(stage: usize) {
    let _ = send(Event::Stage {
        stage,
        at: Instant::now(),
        mark: mark_here(),
    });
}

/// Record the planned unit count from a `--unit-graph` listing.
pub(crate) fn plan(unit_graph: &[u8]) {
    let units = count(unit_graph, b"\"pkg_id\"");
    if units > 0 {
        let _ = send(Event::Plan(units));
    }
}

/// Observe one chunk of a building Cargo child's stdout or stderr.
pub(crate) fn observe(stdout: bool, chunk: &[u8]) {
    let (lines, compiling) = if stdout {
        (count(chunk, b"\n"), 0)
    } else {
        (0, count(chunk, b"Compiling "))
    };
    if lines > 0 || compiling > 0 {
        let _ = send(Event::Observe { lines, compiling });
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
            observed: None,
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

impl Printer {
    fn new(out: Box<dyn Write + Send>, terminal: bool) -> Self {
        Self {
            line: State::new(),
            held: Vec::new(),
            rounds: Vec::new(),
            marks: Vec::new(),
            next_draw: Instant::now(),
            out,
            terminal,
        }
    }

    /// Hear each event in the order it was sent, for as long as a sender
    /// lives. During a launch the wait for the next event ends at
    /// `next_draw`, which draws the line again.
    fn run(mut self, events: &mpsc::Receiver<Event>) {
        PRINTING.set(true);
        loop {
            if self.line.launch.is_some() {
                let wait = self.next_draw.saturating_duration_since(Instant::now());
                match events.recv_timeout(wait) {
                    Ok(event) => self.hear(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                let Ok(event) = events.recv() else {
                    return;
                };
                self.hear(event);
            }
            if self.line.launch.is_some() && Instant::now() >= self.next_draw {
                self.next_draw = Instant::now() + TICK;
                self.draw();
                report(&mut self.line);
            }
        }
    }

    fn hear(&mut self, event: Event) {
        match event {
            Event::Begin { at, file, previous } => {
                let notes = std::mem::take(&mut self.line.notes);
                self.line = State::new();
                self.line.notes = notes;
                self.line.start(at, Some(file), previous);
                self.next_draw = at + TICK;
            }
            Event::Finish { success, done } => {
                self.finish(success);
                let _ = done.send(());
            }
            Event::BeginPhases => self.line.phases = true,
            Event::Phase { phase, mark } => self.phase(phase, mark),
            Event::Stage { stage, at, mark } => {
                self.line.advance(stage, at);
                // The build that feeds extraction is a check's extracting
                // phase; a launch's line names it by its stage.
                if stage == 2 && self.line.launch.is_none() {
                    self.phase(EXTRACTING, mark);
                }
            }
            Event::Plan(units) => self.line.units = Some(units),
            Event::Observe { lines, compiling } => {
                self.line.lines += lines;
                self.line.compiling += compiling;
                if lines > 0 && self.line.phase == Some(EXTRACTING) {
                    self.draw();
                }
            }
            Event::Line { line, done } => {
                self.above(line);
                let _ = done.send(());
            }
            Event::Note { key, line, done } => {
                if !self.line.notes.contains(&key) {
                    self.line.notes.push(key);
                    SAID.fetch_add(1, Ordering::Relaxed);
                    self.above(line);
                }
                let _ = done.send(());
            }
            Event::Restored(done) => {
                if self.line.launch.is_none() {
                    self.clear();
                }
                self.release();
                let _ = done.send(());
            }
            Event::Flush(done) => {
                self.clear();
                self.release();
                let _ = done.send(());
            }
            Event::BeginRound => self.rounds.push(Round::default()),
            Event::Count { stage, amount } => {
                if let Some(round) = self.rounds.last_mut() {
                    *stage(round) += amount;
                }
            }
            Event::Rounds(answer) => {
                let _ = answer.send(self.rounds.clone());
            }
            Event::Phases { round, answer } => {
                let _ = answer.send(self.phases(round));
            }
        }
    }

    fn finish(&mut self, success: bool) {
        let state = &mut self.line;
        if state.launch.is_some() {
            state.close(Instant::now());
            if let Some(file) = state.file.as_ref().filter(|_| success && state.compiled) {
                if let Some(parent) = file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(file, format(&state.current));
            }
        }
        self.clear();
        self.line.launch = None;
        self.line.phases = false;
        self.line.phase = None;
        self.line.step = None;
    }

    fn phase(&mut self, phase: &'static str, mark: Option<Instant>) {
        if let Some(at) = mark {
            self.mark(phase, at);
        }
        if self.line.launch.is_some() {
            if self.line.step != Some(phase) {
                self.line.step = Some(phase);
                self.draw();
            }
            return;
        }
        if !self.line.phases || self.line.phase == Some(phase) {
            return;
        }
        self.line.phase = Some(phase);
        if phase == EXTRACTING {
            self.line.lines = 0;
        }
        self.draw();
    }

    /// Mark `phase` as begun `at` in the current round, unless it is the
    /// phase the round is in; outside a round, nothing is marked.
    fn mark(&mut self, phase: &'static str, at: Instant) {
        let Some(round) = self.rounds.len().checked_sub(1) else {
            return;
        };
        let current = self
            .marks
            .last()
            .filter(|(index, _, _)| *index == round)
            .map(|(_, current, _)| *current);
        if current != Some(phase) {
            self.marks.push((round, phase, at));
        }
    }

    fn phases(&self, index: usize) -> Phases {
        let marks = self
            .marks
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

    /// Print `line` above the status line, or hold it while a program has
    /// the terminal raw, so it never lands inside a full-screen program.
    fn above(&mut self, mut line: String) {
        if crate::process::terminal::raw() {
            self.held.push(line);
            return;
        }
        let shown = self.line.drawn;
        self.clear();
        if !line.ends_with('\n') {
            line.push('\n');
        }
        self.write(&line);
        if shown {
            self.line.drawn = true;
            self.draw();
        }
    }

    /// Print the held lines; any still held when a program has the
    /// terminal raw again.
    fn release(&mut self) {
        for line in std::mem::take(&mut self.held) {
            self.above(line);
        }
    }

    fn draw(&mut self) {
        let (Some(launch), Some(since)) = (self.line.launch, self.line.since) else {
            self.draw_phase();
            return;
        };
        if !self.line.drawn && since.elapsed() < QUIET {
            return;
        }
        // A program that has the screen draws it alone.
        if crate::process::terminal::raw() {
            return;
        }
        let percent = self.line.percent(Instant::now());
        if self.terminal {
            let style = Style::terminal();
            self.line.tick = self.line.tick.wrapping_add(1);
            let line = render(
                self.line.label(),
                percent,
                launch.elapsed(),
                self.line.tick,
                &style,
            );
            self.write(&format!("\r\x1b[2K{line}"));
            self.line.drawn = true;
        } else if self.line.announced < self.line.stage {
            let style = Style {
                width: None,
                unicode: false,
                color: false,
                truecolor: false,
            };
            let line = render(self.line.label(), percent, launch.elapsed(), 0, &style);
            self.write(&format!("{line}\n"));
            self.line.announced = self.line.stage;
        }
    }

    /// Draw a check's phase in place. A phase line is for a person at a
    /// terminal; a log or a test reading stderr sees none of it.
    fn draw_phase(&mut self) {
        let Some(phase) = self.line.phase.filter(|_| self.line.phases) else {
            return;
        };
        if !self.terminal || crate::process::terminal::raw() {
            return;
        }
        let count = (phase == EXTRACTING).then_some((self.line.lines, self.line.units));
        let line = render_phase(phase, count, &Style::terminal());
        self.write(&format!("\r\x1b[2K{line}"));
        self.line.drawn = true;
    }

    fn clear(&mut self) {
        // While a program has the screen, the line stays until it is restored.
        if self.line.drawn && !crate::process::terminal::raw() {
            self.line.drawn = false;
            self.write("\r\x1b[2K");
        }
    }

    /// Write `text` from one buffer and flush it.
    fn write(&mut self, text: &str) {
        let _ = self
            .out
            .write_all(text.as_bytes())
            .and_then(|()| self.out.flush());
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
    /// The style of the terminal stderr is.
    fn terminal() -> Self {
        let width = rustix::termios::tcgetwinsize(std::io::stderr()).map_or(80, |size| size.ws_col);
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

    /// A printer's output as a test reads it: each write, sent on.
    struct Kept(mpsc::Sender<Vec<u8>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.send(bytes.to_vec());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn kept() -> (Printer, mpsc::Receiver<Vec<u8>>) {
        let (written, kept) = mpsc::channel();
        (Printer::new(Box::new(Kept(written)), false), kept)
    }

    fn text(kept: &mpsc::Receiver<Vec<u8>>) -> String {
        String::from_utf8(kept.try_iter().flatten().collect()).unwrap()
    }

    #[test]
    fn a_flush_answers_after_every_line_sent_before_it_is_written() {
        let (printer, kept) = kept();
        let (sender, events) = mpsc::channel();
        let printing = std::thread::spawn(move || printer.run(&events));
        let (unheard, _) = mpsc::channel();
        for (key, line) in [
            ("one", "rawr: one\n"),
            ("two", "rawr: two"),
            ("one", "rawr: again"),
        ] {
            let event = Event::Note {
                key: key.into(),
                line: line.into(),
                done: unheard.clone(),
            };
            assert!(sender.send(event).is_ok(), "the printer runs");
        }
        let line = Event::Line {
            line: "rawr: three".into(),
            done: unheard,
        };
        assert!(sender.send(line).is_ok(), "the printer runs");
        let (done, flushed) = mpsc::channel();
        assert!(sender.send(Event::Flush(done)).is_ok(), "the printer runs");
        assert_eq!(flushed.recv(), Ok(()));
        assert_eq!(text(&kept), "rawr: one\nrawr: two\nrawr: three\n");
        drop(sender);
        assert!(
            printing.join().is_ok(),
            "the printer ends with its last sender"
        );
    }

    #[test]
    fn a_flush_prints_the_lines_held_while_a_program_had_the_terminal() {
        let (mut printer, kept) = kept();
        printer.held.push("rawr: the program ended".into());
        let (done, flushed) = mpsc::channel();
        printer.hear(Event::Flush(done));
        assert_eq!(flushed.try_recv(), Ok(()));
        assert!(printer.held.is_empty());
        assert_eq!(text(&kept), "rawr: the program ended\n");
    }

    #[test]
    fn the_printer_counts_toward_the_last_round_and_marks_its_phases() {
        let (mut printer, _kept) = kept();
        let now = Instant::now();
        printer.hear(Event::Count {
            stage: |round| &mut round.parsed,
            amount: 9,
        });
        printer.hear(Event::BeginRound);
        printer.hear(Event::Count {
            stage: |round| &mut round.parsed,
            amount: 2,
        });
        for phase in ["resolving", "resolving", "reading API"] {
            printer.hear(Event::Phase {
                phase,
                mark: Some(now),
            });
        }
        printer.hear(Event::Phase {
            phase: "classifying",
            mark: None,
        });
        let (answer, rounds) = mpsc::channel();
        printer.hear(Event::Rounds(answer));
        let expected = Round {
            parsed: 2,
            ..Round::default()
        };
        assert_eq!(rounds.try_recv(), Ok(vec![expected]));
        let phases = printer.phases(0).0;
        let names = phases.iter().map(|(phase, _)| *phase).collect::<Vec<_>>();
        assert_eq!(names, ["resolving", "reading API"]);
    }

    #[test]
    fn the_printer_thread_never_waits_for_itself() {
        PRINTING.set(true);
        let heard = ask(Event::Rounds);
        PRINTING.set(false);
        assert!(
            heard.is_none(),
            "an event the printer sends itself is never queued"
        );
    }

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
