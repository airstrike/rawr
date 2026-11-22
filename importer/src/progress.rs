//! One status line on stderr while a cold launch builds its Rust dependencies.
//!
//! The line says what the launch does, as Cargo reports it: the stage, or
//! the step of it the launch named last, such as the check's "reading API"
//! (#1763); the units Cargo has finished of the total; the crate that has
//! compiled longest of those still compiling; and the time since the launch
//! began. The total is the unit graph's count, else the previous cold
//! launch's count for that stage (kept in `<project>/target/rawr/cache/progress`),
//! said as "about". Counts come from Cargo's own output, read line by line:
//! one stdout JSON line per finished unit (`--message-format json`), and one
//! `Compiling` stderr line per package it starts. While nothing moves, only
//! the clock does.
//!
//! A Cargo child that waits for a lock another Cargo holds says so on its
//! stderr. The line then says that it waits, for which directory, and for
//! how long, until Cargo says anything else. The wait's own line is also
//! printed above the status line, marked as Cargo's.
//!
//! A launch whose first stage settles within [`QUIET`] prints nothing, so
//! warm launches stay silent; once shown, the line stays up until the
//! window appears or the launch fails. A stage begins where its work does:
//! a Cargo child of its kind, the runner's generation (building runner) and
//! the runner's build, a hit included (starting).
//!
//! A check shows its phase on the same line instead: the phase's name, and
//! for extraction the units Cargo has finished of the planned total. It has
//! no clock, and is redrawn only when the phase or its count moves.
//!
//! One printer thread owns the line, the lines printed above it and the
//! lines held while a program has the terminal raw, with each round's
//! counts. Every other thread sends it events. A line, a launch's end, a
//! restore and a [`flush`] wait for its answer, so what they print is on
//! stderr when they return and keeps its order against the caller's own
//! writes. A count or a move of the line is sent without waiting. The
//! observer a command names, such as a supervising `rawr mcp`, hears the
//! same text whenever it changes, with the clock in whole minutes.

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
const QUIET: Duration = Duration::from_secs(1);
const TICK: Duration = Duration::from_millis(250);

/// One package Cargo builds, as its `Compiling` line and its artifacts name
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Package {
    name: String,
    version: String,
}

/// A wait for a lock another process holds: what the line says it waits
/// for, and since when the wait was heard.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wait {
    what: String,
    since: Instant,
}

/// One line a Cargo child wrote, or a wait of rawr's own, that moves the
/// status line.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    /// A unit finished: an artifact, fresh or built, or a build script's
    /// run. It names the package when its library or binary is done.
    Finished(Option<Package>),
    /// Cargo started compiling a package.
    Compiling(Package),
    /// The launch waits for `what` another process holds, such as
    /// "another cargo to release target-textfix".
    Waits { what: String },
    /// Any other line on Cargo's stderr, or a wait of rawr's own that
    /// ended: the launch is past any wait.
    Moved,
    /// The child ended, so nothing it started is still compiling.
    Ended,
}

struct State {
    launch: Option<Instant>,
    stage: usize,
    since: Option<Instant>,
    /// Units Cargo finished in this stage.
    finished: usize,
    /// Packages Cargo started compiling in this stage.
    compiling: usize,
    /// The packages still compiling, oldest first.
    building: Vec<Package>,
    waiting: Option<Wait>,
    units: Option<usize>,
    tick: usize,
    drawn: bool,
    /// What a plain line last said: the stage, and whether Cargo waited.
    announced: Option<(usize, bool)>,
    notes: Vec<String>,
    file: Option<PathBuf>,
    /// The previous cold launch's finished units per stage.
    previous: Option<[usize; 4]>,
    current: [usize; 4],
    compiled: bool,
    /// Whether this process shows a check's phases.
    phases: bool,
    phase: Option<&'static str>,
    /// The step of its stage a launch named last, which its line says in
    /// place of the stage's name (#1763).
    step: Option<&'static str>,
    /// What the observer heard last of this launch.
    observed: Option<String>,
}

/// The phase whose line counts Cargo's finished units.
const EXTRACTING: &str = "extracting";

/// The printer thread's inbox ([`printer`]).
static PRINTER: OnceLock<mpsc::Sender<Event>> = OnceLock::new();
/// How many lines [`note`] and [`say`] have printed in this process.
static SAID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Whether this thread is the printer, which hears no event it sends.
    static PRINTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Who hears each change to a launch's line beside the status line, such
/// as a supervising `rawr mcp` (set once by the command).
static OBSERVER: OnceLock<Observer> = OnceLock::new();

/// Hears a stage's name and what the launch's line says.
type Observer = fn(&str, &str);

/// What the printer thread hears: a change to the status line, a line to
/// print above it, a count toward a round, or a question with the sender
/// that takes its answer. An event that carries `done` is answered once
/// its output is written.
enum Event {
    Begin {
        at: Instant,
        file: PathBuf,
        previous: Option<[usize; 4]>,
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
    Again(Instant),
    Plan(usize),
    /// A line of a Cargo child's, or a wait of rawr's own, heard `at`.
    Said {
        said: Said,
        at: Instant,
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

/// Tell `observer` each launch's stage and what its line says, whenever
/// that changes. The first observer a process names is the one it keeps.
pub fn listen(observer: fn(&str, &str)) {
    let _ = OBSERVER.set(observer);
}

fn report(state: &mut State) {
    let Some(observer) = OBSERVER.get() else {
        return;
    };
    let text = state.status(Instant::now(), Clock::Minutes).plain(" · ");
    if state.observed.as_ref() != Some(&text) {
        observer(STAGES[state.stage.clamp(1, STAGES.len()) - 1], &text);
        state.observed = Some(text);
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

/// The launch prepares the program again, from its first stage: a save
/// landed as it started (D531.9). Its later stages come again.
pub fn again() {
    let _ = send(Event::Again(Instant::now()));
}

/// Record the planned unit count from a `--unit-graph` listing.
pub(crate) fn plan(unit_graph: &[u8]) {
    let units = count(unit_graph, b"\"pkg_id\"");
    if units > 0 {
        let _ = send(Event::Plan(units));
    }
}

/// Where a Cargo child keeps what its locks cover: its build directory
/// and its artifact directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Directories {
    build: PathBuf,
    artifacts: PathBuf,
}

impl Directories {
    pub(crate) const fn new(build: PathBuf, artifacts: PathBuf) -> Self {
        Self { build, artifacts }
    }

    /// What a wait for the lock on `what`, as Cargo names it, waits for:
    /// the directory's own name, or Cargo's name for anything else.
    fn place(&self, what: &str) -> String {
        let directory = match what {
            "build directory" => &self.build,
            "artifact directory" => &self.artifacts,
            "package cache" => return "Cargo's package cache".to_owned(),
            _ => return what.to_owned(),
        };
        directory.file_name().map_or_else(
            || directory.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// Reads one stream of a building Cargo child, line by line, and tells the
/// status line what each says. When the stderr watch goes, the child has
/// ended.
pub(crate) struct Watch {
    stream: Stream,
    /// The start of a line whose end has not come yet.
    partial: Vec<u8>,
}

/// Which of a Cargo child's streams a [`Watch`] reads.
enum Stream {
    /// Cargo's JSON messages.
    Stdout,
    /// Cargo's status lines and rustc's rendered diagnostics, from a child
    /// that keeps its locks in these directories.
    Stderr(Directories),
}

impl Watch {
    pub(crate) const fn stdout() -> Self {
        Self {
            stream: Stream::Stdout,
            partial: Vec::new(),
        }
    }

    pub(crate) const fn stderr(directories: Directories) -> Self {
        Self {
            stream: Stream::Stderr(directories),
            partial: Vec::new(),
        }
    }

    /// Hear the next `chunk` of the stream: each line it ends.
    pub(crate) fn feed(&mut self, chunk: &[u8]) {
        self.partial.extend_from_slice(chunk);
        let Some(end) = self.partial.iter().rposition(|byte| *byte == b'\n') else {
            return;
        };
        let lines = self.partial.drain(..=end).collect::<Vec<_>>();
        for line in lines.split(|byte| *byte == b'\n') {
            self.hear(line);
        }
    }

    fn hear(&self, line: &[u8]) {
        let said = match &self.stream {
            Stream::Stdout => stdout_said(line),
            Stream::Stderr(directories) => {
                let text = String::from_utf8_lossy(line);
                let said = stderr_said(&text, directories);
                // rawr keeps Cargo's stderr, so its wait is said above
                // the line too, where a log keeps it.
                if matches!(said, Some(Said::Waits { .. })) {
                    say(&cargos_own(&text));
                }
                said
            }
        };
        if let Some(said) = said {
            let _ = send(Event::Said {
                said,
                at: Instant::now(),
            });
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let rest = std::mem::take(&mut self.partial);
        if !rest.is_empty() {
            self.hear(&rest);
        }
        if matches!(self.stream, Stream::Stderr(_)) {
            let _ = send(Event::Said {
                said: Said::Ended,
                at: Instant::now(),
            });
        }
    }
}

/// What one of Cargo's JSON messages on stdout says of its progress.
fn stdout_said(line: &[u8]) -> Option<Said> {
    if contains(line, br#""reason":"build-script-executed""#) {
        return Some(Said::Finished(None));
    }
    if !contains(line, br#""reason":"compiler-artifact""#) {
        return None;
    }
    let message = serde_json::from_slice::<serde_json::Value>(line).ok();
    let package = message.as_ref().and_then(|message| {
        let kinds = message["target"]["kind"].as_array()?;
        // A build script's compile leaves its package compiling.
        if kinds.iter().any(|kind| kind == "custom-build") {
            return None;
        }
        package(message["package_id"].as_str()?)
    });
    Some(Said::Finished(package))
}

/// The package a Cargo package id names: `kind+url#name@version`, or
/// `kind+url#version` when the name is the url's last segment, or the older
/// `name version (source)`.
fn package(id: &str) -> Option<Package> {
    let (name, version) = match id.split_once('#') {
        Some((source, fragment)) => match fragment.split_once('@') {
            Some((name, version)) => (name, version),
            None => {
                let path = source.split('?').next()?.trim_end_matches('/');
                (path.rsplit('/').next()?, fragment)
            }
        },
        None => {
            let mut fields = id.split_whitespace();
            (fields.next()?, fields.next()?)
        }
    };
    Some(Package {
        name: name.to_owned(),
        version: version.to_owned(),
    })
}

/// What one line of a Cargo child's stderr says of its progress: a wait
/// for a lock, in `directories`, a package it starts, or that it moved.
fn stderr_said(line: &str, directories: &Directories) -> Option<Said> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    if let Some(what) = line.strip_prefix(CARGO_WAIT) {
        let what = what.trim_start().strip_prefix("on ").unwrap_or(what).trim();
        return Some(Said::Waits {
            what: format!("another cargo to release {}", directories.place(what)),
        });
    }
    if let Some(rest) = line.strip_prefix("Compiling ") {
        let mut fields = rest.split_whitespace();
        if let (Some(name), Some(version)) = (fields.next(), fields.next()) {
            return Some(Said::Compiling(Package {
                name: name.to_owned(),
                version: version.trim_start_matches('v').to_owned(),
            }));
        }
    }
    Some(Said::Moved)
}

/// Say on the status line that the launch waits for `what`, a lock of
/// rawr's own another process holds, such as "rawr's runtime, used by
/// process 4242", to its [`waited`].
pub(crate) fn waiting(what: String) {
    let _ = send(Event::Said {
        said: Said::Waits { what },
        at: Instant::now(),
    });
}

/// The wait [`waiting`] said is over.
pub(crate) fn waited() {
    let _ = send(Event::Said {
        said: Said::Moved,
        at: Instant::now(),
    });
}

/// Cargo's `line` that it waits for a lock, marked as Cargo's own.
fn cargos_own(line: &str) -> String {
    format!("{} (Cargo's own lock)", line.trim_end())
}

/// What Cargo says when it finds a lock it needs held.
const CARGO_WAIT: &str = "Blocking waiting for file lock";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
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
            finished: 0,
            compiling: 0,
            building: Vec::new(),
            waiting: None,
            units: None,
            tick: 0,
            drawn: false,
            announced: None,
            notes: Vec::new(),
            file: None,
            previous: None,
            current: [0; 4],
            compiled: false,
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

    fn start(&mut self, now: Instant, file: Option<PathBuf>, previous: Option<[usize; 4]>) {
        self.launch = Some(now);
        self.stage = 1;
        self.since = Some(now);
        self.file = file;
        self.previous = previous;
    }

    /// Close the current stage's record. A stage a launch enters again
    /// keeps the most it did in any one pass.
    fn close(&mut self) {
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        self.current[index] = self.current[index].max(self.finished.max(self.compiling));
        self.compiled |= self.compiling > 0;
    }

    fn advance(&mut self, stage: usize, now: Instant) {
        if self.launch.is_some() && stage > self.stage && stage <= STAGES.len() {
            self.enter(stage, now);
        }
    }

    /// Go back to the first stage, as a launch that prepares again does.
    fn again(&mut self, now: Instant) {
        if self.launch.is_some() {
            self.enter(1, now);
        }
    }

    fn enter(&mut self, stage: usize, now: Instant) {
        self.close();
        self.stage = stage;
        self.since = Some(now);
        self.finished = 0;
        self.compiling = 0;
        self.building.clear();
        self.waiting = None;
        self.step = None;
    }

    /// Take what a Cargo child said `at`.
    fn hear(&mut self, said: Said, at: Instant) {
        match said {
            Said::Finished(package) => {
                self.finished += 1;
                self.waiting = None;
                if let Some(package) = package {
                    self.building.retain(|building| *building != package);
                }
            }
            Said::Compiling(package) => {
                self.compiling += 1;
                self.waiting = None;
                self.building.push(package);
            }
            Said::Waits { what } => {
                if self.waiting.as_ref().is_none_or(|wait| wait.what != what) {
                    self.waiting = Some(Wait { what, since: at });
                }
            }
            Said::Moved => self.waiting = None,
            Said::Ended => {
                self.building.clear();
                self.waiting = None;
            }
        }
    }

    /// What the line says at `now`, its clock read as `clock` reads it.
    fn status(&self, now: Instant, clock: Clock) -> Status {
        let mut parts = vec![(self.label().to_owned(), Tone::Label)];
        let since = match &self.waiting {
            Some(wait) => {
                parts.push((format!("waiting for {}", wait.what), Tone::Label));
                Some(wait.since)
            }
            None => {
                if self.finished > 0 || !self.building.is_empty() {
                    parts.push((self.crates(), Tone::Count));
                }
                if let Some(package) = self.building.first() {
                    parts.push((package.name.clone(), Tone::Label));
                }
                self.launch
            }
        };
        let elapsed = since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        if let Some(time) = clock.read(elapsed) {
            parts.push((time, Tone::Quiet));
        }
        Status(parts)
    }

    /// The units finished of the stage's total: the unit graph's count,
    /// else about the previous launch's.
    fn crates(&self) -> String {
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        let planned = (self.stage == 2).then_some(self.units).flatten();
        let previous = self.previous.map(|crates| crates[index]);
        let finished = self.finished;
        match (planned, previous) {
            // Cargo may report more finished units than the plan listed.
            (Some(total), _) if total > 0 => format!("{} of {total} crates", finished.min(total)),
            (_, Some(total)) if total >= finished && total > 0 => {
                format!("{finished} of about {total} crates")
            }
            _ => format!("{finished} crates"),
        }
    }
}

/// How a line reads its clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clock {
    /// To the second, for the line a person watches.
    Seconds,
    /// In whole minutes and silent for the first, for an observer, which
    /// hears every change.
    Minutes,
}

impl Clock {
    fn read(self, elapsed: Duration) -> Option<String> {
        let secs = elapsed.as_secs();
        match self {
            Self::Seconds if secs >= 60 => Some(format!("{}m{:02}s", secs / 60, secs % 60)),
            Self::Seconds => Some(format!("{secs}s")),
            Self::Minutes if secs >= 60 => Some(format!("{}m", secs / 60)),
            Self::Minutes => None,
        }
    }
}

/// How one part of a launch's line is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Label,
    Count,
    Quiet,
}

/// What a launch's line says, part by part.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Status(Vec<(String, Tone)>);

impl Status {
    /// The parts joined by `separator`, unpainted.
    fn plain(&self, separator: &str) -> String {
        self.0
            .iter()
            .map(|(part, _)| part.as_str())
            .collect::<Vec<_>>()
            .join(separator)
    }
}

fn format(records: &[usize; 4]) -> String {
    records.iter().map(|crates| format!("{crates}\n")).collect()
}

/// The previous launch's finished units per stage: the first field of
/// each of four lines.
fn parse(text: &str) -> Option<[usize; 4]> {
    let mut records = [0; 4];
    let mut lines = text.lines();
    for record in &mut records {
        *record = lines.next()?.split_whitespace().next()?.parse().ok()?;
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
            Event::Again(at) => self.line.again(at),
            Event::Plan(units) => self.line.units = Some(units),
            Event::Said { said, at } => {
                let finished = matches!(said, Said::Finished(_));
                self.line.hear(said, at);
                if finished && self.line.phase == Some(EXTRACTING) {
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
            state.close();
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
            self.line.finished = 0;
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
        let (Some(_), Some(since)) = (self.line.launch, self.line.since) else {
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
        let status = self.line.status(Instant::now(), Clock::Seconds);
        if self.terminal {
            let style = Style::terminal();
            self.line.tick = self.line.tick.wrapping_add(1);
            let line = render(&status, self.line.tick, &style);
            self.write(&format!("\r\x1b[2K{line}"));
            self.line.drawn = true;
        } else {
            let announced = Some((self.line.stage, self.line.waiting.is_some()));
            if self.line.announced != announced {
                let line = render(&status, 0, &Style::PLAIN);
                self.write(&format!("{line}\n"));
                self.line.announced = announced;
            }
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
        let count = (phase == EXTRACTING).then_some((self.line.finished, self.line.units));
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
    /// A plain line, for a log.
    const PLAIN: Self = Self {
        width: None,
        unicode: false,
        color: false,
        truecolor: false,
    };

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

    /// #E4002B: the one colored swatch (the spinner).
    pub const RED: Color = Color {
        rgb: (228, 0, 43),
        ansi256: 160,
    };
    /// #F87171: the `rawr` mark and counts.
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
    /// #4A4A4A: separators and empty space.
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

/// A launch's line: the `rawr` mark, a spinner at `tick`, and what the
/// line says, cut to the terminal's width. A line with no width is a plain
/// line for a log.
fn render(status: &Status, tick: usize, style: &Style) -> String {
    use palette::{DARK_GRAY, LIGHT_GRAY, LIGHT_RED, MID_GRAY, RED};
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let separator = if style.unicode { " · " } else { " - " };
    let Some(width) = style.width else {
        return format!("rawr > {}", status.plain(separator));
    };
    let spin = if style.unicode {
        SPIN[tick % SPIN.len()]
    } else {
        ['|', '/', '-', '\\'][tick % 4]
    }
    .to_string();
    let line = format!("rawr {spin} {}", status.plain(separator));
    if line.chars().count() >= width {
        return line.chars().take(width.saturating_sub(1)).collect();
    }
    if !style.color {
        return line;
    }
    let tc = style.truecolor;
    let parts = status
        .0
        .iter()
        .map(|(part, tone)| {
            let color = match tone {
                Tone::Label => LIGHT_GRAY,
                Tone::Count => LIGHT_RED,
                Tone::Quiet => MID_GRAY,
            };
            color.paint(part, false, tc)
        })
        .collect::<Vec<_>>();
    format!(
        "{} {} {}",
        LIGHT_RED.paint("rawr", true, tc),
        RED.paint(&spin, false, tc),
        parts.join(&DARK_GRAY.paint(separator, false, tc)),
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

    // Cargo's own lines, as `cargo build --message-format=json-render-diagnostics`
    // wrote them, with their paths shortened.
    const BUILD_SCRIPT: &[u8] = br#"{"reason":"compiler-artifact","package_id":"path+file:///w/dep-one#0.2.0","manifest_path":"/w/dep-one/Cargo.toml","target":{"kind":["custom-build"],"crate_types":["bin"],"name":"build-script-build","src_path":"/w/dep-one/build.rs","edition":"2021","doc":false,"doctest":false,"test":false},"profile":{"opt_level":"0","debuginfo":0,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/t/debug/build/dep-one-2bd1384c031aee2f/build-script-build"],"executable":null,"fresh":false}"#;
    const BUILD_SCRIPT_RAN: &[u8] = br#"{"reason":"build-script-executed","package_id":"path+file:///w/dep-one#0.2.0","linked_libs":[],"linked_paths":[],"cfgs":[],"env":[],"out_dir":"/t/debug/build/dep-one-1283f485e2d1bc32/out"}"#;
    const DEP_ONE: &[u8] = br#"{"reason":"compiler-artifact","package_id":"path+file:///w/dep-one#0.2.0","manifest_path":"/w/dep-one/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"dep_one","src_path":"/w/dep-one/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/t/debug/deps/libdep_one-7cdabcd64dd5618a.rlib","/t/debug/deps/libdep_one-7cdabcd64dd5618a.rmeta"],"executable":null,"fresh":false}"#;
    const SERDE: &[u8] = br#"{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.219","manifest_path":"/c/serde-1.0.219/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"serde","src_path":"/c/serde-1.0.219/src/lib.rs","edition":"2018","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":["default","std"],"filenames":["/t/debug/deps/libserde-0c1f.rlib"],"executable":null,"fresh":true}"#;
    const FINISHED_BUILD: &[u8] = br#"{"reason":"build-finished","success":true}"#;
    const COMPILING_DEP_ONE: &str = "   Compiling dep-one v0.2.0 (/w/dep-one)";
    const COMPILING_SERDE: &str = "   Compiling serde v1.0.219";
    const COMPILING_WGPU: &str = "   Compiling iced_wgpu v0.13.5";
    const BLOCKING: &str = "    Blocking waiting for file lock on build directory";
    const FINISHED: &str =
        "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.45s";

    fn directories() -> Directories {
        Directories::new(
            PathBuf::from("/Users/a/target-textfix"),
            PathBuf::from("/p/target/rawr/runner/target"),
        )
    }

    fn package(name: &str, version: &str) -> Package {
        Package {
            name: name.into(),
            version: version.into(),
        }
    }

    fn launch(previous: Option<[usize; 4]>) -> (State, Instant) {
        let now = Instant::now();
        let mut state = State::new();
        state.start(now, None, previous);
        (state, now)
    }

    /// A state that hears Cargo's `stderr` and `stdout` lines at `at`.
    fn hear(state: &mut State, stderr: &[&str], stdout: &[&[u8]], at: Instant) {
        for line in stderr {
            if let Some(said) = stderr_said(line, &directories()) {
                state.hear(said, at);
            }
        }
        for line in stdout {
            if let Some(said) = stdout_said(line) {
                state.hear(said, at);
            }
        }
    }

    #[test]
    fn cargo_lines_say_what_moves_the_line() {
        assert_eq!(stdout_said(BUILD_SCRIPT), Some(Said::Finished(None)));
        assert_eq!(stdout_said(BUILD_SCRIPT_RAN), Some(Said::Finished(None)));
        assert_eq!(
            stdout_said(DEP_ONE),
            Some(Said::Finished(Some(package("dep-one", "0.2.0"))))
        );
        assert_eq!(
            stdout_said(SERDE),
            Some(Said::Finished(Some(package("serde", "1.0.219"))))
        );
        assert_eq!(stdout_said(FINISHED_BUILD), None);
        assert_eq!(
            super::package("serde 1.0.219 (registry+https://github.com/rust-lang/crates.io-index)"),
            Some(package("serde", "1.0.219"))
        );
        let directories = directories();
        assert_eq!(
            stderr_said(COMPILING_DEP_ONE, &directories),
            Some(Said::Compiling(package("dep-one", "0.2.0")))
        );
        assert_eq!(
            stderr_said(BLOCKING, &directories),
            Some(Said::Waits {
                what: "another cargo to release target-textfix".into()
            })
        );
        assert_eq!(
            stderr_said(
                "    Blocking waiting for file lock on package cache",
                &directories
            ),
            Some(Said::Waits {
                what: "another cargo to release Cargo's package cache".into()
            })
        );
        assert_eq!(stderr_said(FINISHED, &directories), Some(Said::Moved));
        assert_eq!(stderr_said("", &directories), None);
        assert_eq!(
            cargos_own(BLOCKING),
            "    Blocking waiting for file lock on build directory (Cargo's own lock)"
        );
    }

    #[test]
    fn a_moving_build_says_its_count_the_crate_compiling_longest_and_the_clock() {
        let (mut state, now) = launch(Some([0, 0, 340, 0]));
        state.advance(3, now);
        let at = now + Duration::from_secs(190);
        hear(
            &mut state,
            &[COMPILING_SERDE, COMPILING_WGPU, COMPILING_DEP_ONE],
            &[SERDE, BUILD_SCRIPT, BUILD_SCRIPT_RAN],
            at,
        );
        let line = |state: &State| {
            render(
                &state.status(now + Duration::from_secs(192), Clock::Seconds),
                2,
                &WIDE,
            )
        };
        // serde is done and dep-one's build script ran, so iced_wgpu has
        // compiled longest of the rest.
        assert_eq!(
            line(&state),
            "rawr ⠹ building runner · 3 of about 340 crates · iced_wgpu · 3m12s"
        );
        hear(&mut state, &[], &[DEP_ONE], at);
        assert_eq!(
            line(&state),
            "rawr ⠹ building runner · 4 of about 340 crates · iced_wgpu · 3m12s"
        );

        // The unit graph's count is the total, said as it is.
        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.units = Some(340);
        state.finished = 211;
        hear(&mut state, &[COMPILING_WGPU], &[SERDE], now);
        assert_eq!(
            state
                .status(now + Duration::from_secs(9), Clock::Seconds)
                .plain(" · "),
            "building API · 212 of 340 crates · iced_wgpu · 9s"
        );
        let plain = render(&state.status(now, Clock::Seconds), 0, &Style::PLAIN);
        assert_eq!(
            plain,
            "rawr > building API - 212 of 340 crates - iced_wgpu - 0s"
        );
    }

    #[test]
    fn a_stalled_build_moves_only_its_clock() {
        let (mut state, now) = launch(Some([0, 0, 340, 0]));
        state.advance(3, now);
        hear(
            &mut state,
            &[COMPILING_SERDE, COMPILING_WGPU],
            &[SERDE],
            now + Duration::from_secs(60),
        );
        let at = |minutes: u64, clock| {
            state
                .status(now + Duration::from_secs(minutes * 60 + 30), clock)
                .plain(" · ")
        };
        assert_eq!(
            at(2, Clock::Seconds),
            "building runner · 1 of about 340 crates · iced_wgpu · 2m30s"
        );
        assert_eq!(
            at(22, Clock::Seconds),
            "building runner · 1 of about 340 crates · iced_wgpu · 22m30s"
        );
        // An observer hears the same text, once a minute while it stalls.
        assert_eq!(
            at(22, Clock::Minutes),
            "building runner · 1 of about 340 crates · iced_wgpu · 22m"
        );
        assert_eq!(
            state
                .status(now + Duration::from_secs(22 * 60 + 59), Clock::Minutes)
                .plain(" · "),
            at(22, Clock::Minutes)
        );
        assert_eq!(
            state
                .status(now + Duration::from_secs(20), Clock::Minutes)
                .plain(" · "),
            "building runner · 1 of about 340 crates · iced_wgpu"
        );
    }

    #[test]
    fn a_build_blocked_on_a_lock_says_it_waits_on_which_directory_and_since_when() {
        let (mut state, now) = launch(Some([0, 0, 340, 0]));
        state.advance(3, now);
        let blocked = now + Duration::from_secs(30);
        hear(&mut state, &[BLOCKING], &[], blocked);
        // Cargo says it again for the same lock: the wait began when first
        // heard.
        hear(
            &mut state,
            &[BLOCKING],
            &[],
            blocked + Duration::from_secs(5),
        );
        let later = blocked + Duration::from_secs(240);
        assert_eq!(
            render(&state.status(later, Clock::Seconds), 0, &WIDE),
            "rawr ⠋ building runner · waiting for another cargo to release target-textfix · 4m00s"
        );
        assert_eq!(
            state.status(later, Clock::Minutes).plain(" · "),
            "building runner · waiting for another cargo to release target-textfix · 4m"
        );
        // Once Cargo has the lock, it compiles, and the line says so.
        hear(&mut state, &[COMPILING_WGPU], &[], later);
        assert_eq!(
            state.status(later, Clock::Seconds).plain(" · "),
            "building runner · 0 of about 340 crates · iced_wgpu · 4m30s"
        );
        // A wait that ends with the child, every unit fresh, is over too.
        hear(&mut state, &[BLOCKING], &[], later);
        state.hear(Said::Ended, later);
        assert_eq!(
            state.status(later, Clock::Seconds).plain(" · "),
            "building runner · 4m30s"
        );
    }

    #[test]
    fn a_launch_line_is_painted_and_cut_to_the_terminal() {
        let status = Status(vec![
            ("building runner".into(), Tone::Label),
            ("3 crates".into(), Tone::Count),
            ("9s".into(), Tone::Quiet),
        ]);
        let colored = Style {
            width: Some(120),
            unicode: true,
            color: true,
            truecolor: true,
        };
        assert_eq!(
            render(&status, 3, &colored),
            "\x1b[1;38;2;248;113;113mrawr\x1b[0m \x1b[38;2;228;0;43m⠸\x1b[0m \
             \x1b[38;2;200;200;200mbuilding runner\x1b[0m\x1b[38;2;74;74;74m · \x1b[0m\
             \x1b[38;2;248;113;113m3 crates\x1b[0m\x1b[38;2;74;74;74m · \x1b[0m\
             \x1b[38;2;138;138;138m9s\x1b[0m"
        );
        let ascii = Style {
            width: Some(120),
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(&status, 1, &ascii),
            "rawr / building runner - 3 crates - 9s"
        );
        // A line wider than the terminal is cut, unpainted, short of its
        // last column.
        let narrow = Style {
            width: Some(20),
            ..colored
        };
        assert_eq!(render(&status, 0, &narrow), "rawr ⠋ building run");
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
    fn each_stage_records_its_count_for_the_next_launch() {
        assert_eq!(count(b"{\"pkg_id\":1}{\"pkg_id\":2}", b"\"pkg_id\""), 2);
        let (mut state, now) = launch(None);
        state.advance(2, now);
        hear(&mut state, &[COMPILING_SERDE], &[SERDE, DEP_ONE], now);
        state.advance(3, now);
        assert!(state.compiled);
        assert_eq!(state.current, [0, 2, 0, 0]);
        let records = [0, 284, 120, 0];
        assert_eq!(parse(&format(&records)), Some(records));
        // A record written with each stage's time still reads.
        assert_eq!(
            parse("0 2000\n284 10000\n120 7000\n0 1000\n"),
            Some(records)
        );
        assert_eq!(parse("1\n3\n"), None);
    }
}
