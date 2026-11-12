//! The owner of a program's runner: it prepares the program through a
//! [`crate::Profile`], builds and starts the runner, and on each save
//! prepares again and swaps the runner's generation, keeping the last good
//! one when a save fails.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rawr::diagnostic;

const DEBOUNCE: Duration = Duration::from_millis(30);
const IDLE_POLL: Duration = Duration::from_millis(25);
const REFRESH_POLL: Duration = Duration::from_millis(250);

/// The runner lane of a program.
pub type Lane = crate::native::Lane<rawr::package::import::Brand>;
type Built =
    rawr_import::runner::Manifest<rawr_import::runner::Built, rawr::package::import::Brand>;

pub mod resident;

struct Initial {
    built: Built,
    snapshot: rawr_import::supervisor::snapshot::Bytes,
    inputs: rawr::reload::source::Inputs,
    manifests: resident::Manifests,
}

/// Check one imported application without opening a native window: its
/// Rust imports are resolved and extracted and the Rawr source is checked
/// against them. No runner is built; `rawr run` builds it. The shared
/// bindings the check used or extracted come back with the source, to
/// settle against the project's `cargo check` ([`settle_checked`]).
pub fn check(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
) -> Result<(PathBuf, rawr_import::bindings::Settlement), Unchecked> {
    same_build(target)?;
    let source = target.source().path().to_path_buf();
    let fallback = fallback(target)?;
    let revision = rawr_import::supervisor::Owner::new()
        .revision()
        .map_err(|error| framework(error, fallback.clone()))?;
    let mut prepared = profile
        .prepare(
            target,
            graph,
            revision,
            &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
            None,
        )
        .map_err(|error| unchecked(error, fallback))?;
    Ok((source, prepared.take_settlement()))
}

/// Why a check failed: the program's report, or a failed build of rawr's
/// copy of the program, whose report waits for `cargo check` to say whose
/// error it is.
pub enum Unchecked {
    /// What the program got wrong, or rawr did.
    Report(diagnostic::Report),
    /// Cargo failed to build rawr's copy of the program.
    Copy {
        /// What Cargo said.
        failure: rawr_import::location::copy::Failure,
        /// The program's entry source, the report's source.
        source: diagnostic::Source,
    },
}

impl Unchecked {
    /// The report, once `cargo` says how the program checks, or `None`
    /// when `cargo check` fails too: the program's error is cargo's to
    /// show, at the program's own paths, and neither rawr's check nor the
    /// copy's build shows its own. A failure cargo doesn't share is rawr's
    /// to report, the copy's without its paths or generated names.
    #[must_use]
    pub fn report(self, cargo: Option<&rawr_cli::cargo::Status>) -> Option<diagnostic::Report> {
        if failing(cargo).is_some() {
            return None;
        }
        Some(self.into_report())
    }

    /// The report with no word from `cargo check`: a failed build of the
    /// copy is rawr's.
    #[must_use]
    pub fn into_report(self) -> diagnostic::Report {
        match self {
            Self::Report(report) => report,
            Self::Copy { failure, source } => copied(failure, source),
        }
    }

    /// Whether this waits for `cargo check` to say whose error it is.
    const fn is_copy(&self) -> bool {
        matches!(self, Self::Copy { .. })
    }
}

/// What a `cargo check` that failed said.
fn failing(cargo: Option<&rawr_cli::cargo::Status>) -> Option<&rawr_cli::cargo::Outcome> {
    match cargo {
        Some(rawr_cli::cargo::Status::Finished(outcome)) if !outcome.is_clean() => Some(outcome),
        _ => None,
    }
}

/// A failed preparation: a failed build of rawr's copy of the program
/// waits for `cargo check`.
fn unchecked(error: crate::Error, source: diagnostic::Source) -> Unchecked {
    match error {
        crate::Error::Copy(failure) => Unchecked::Copy { failure, source },
        error => Unchecked::Report(import(error, source)),
    }
}

/// `cargo check` of the program at `location` as it is on disk now, run
/// when rawr's copy of it failed to build. `None` when it is no Cargo
/// crate or doesn't load.
fn checked_now(location: &Path) -> Option<rawr_cli::cargo::Status> {
    let target = rawr::project::Target::discover(location).ok()?;
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target).ok()?;
    rawr_cli::cargo::begin(&graph).map(rawr_cli::cargo::Pending::finish)
}

impl From<diagnostic::Report> for Unchecked {
    fn from(report: diagnostic::Report) -> Self {
        Self::Report(report)
    }
}

/// The project's `cargo check`, beside `rawr check`.
pub struct Checked {
    /// What cargo said.
    pub status: rawr_cli::cargo::Status,
    /// Its build scripts' outputs, read as it ended; none when cargo
    /// itself failed.
    observed: Option<rawr_import::bindings::Observed>,
}

/// A `cargo check` that runs while rawr checks, read as soon as it ends:
/// a build script's files stay in its `OUT_DIR` only until another build
/// of the same target directory, perhaps on another toolchain, runs it
/// again.
pub struct Checking(std::thread::JoinHandle<Checked>);

impl Checking {
    /// Wait for `pending` on a thread of its own.
    #[must_use]
    pub fn start(pending: rawr_cli::cargo::Pending) -> Self {
        Self(std::thread::spawn(move || {
            let status = pending.finish();
            let observed = match &status {
                rawr_cli::cargo::Status::Finished(outcome) if outcome.failure.is_none() => {
                    Some(rawr_import::bindings::Observed::read(
                        outcome.build_scripts.join("\n").as_bytes(),
                    ))
                }
                _ => None,
            };
            Checked { status, observed }
        }))
    }

    /// What the check came to.
    #[must_use]
    pub fn finish(self) -> Checked {
        self.0
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }
}

/// Publish, or refuse, the shared bindings a check used or extracted, once
/// the project's own `cargo check` has run their build scripts, as a run
/// does after its runner build (D609.4, #661). A `cargo check` that failed
/// on its own, or didn't run, gives no evidence, and the tables stay in the
/// project. A table whose build scripts gave the nightly other outputs is
/// checked again, once its stale run is cleaned (D742).
///
/// A failed check's report is [`Unchecked::report`]'s, `None` when cargo's
/// report is the whole of it.
pub fn settle_checked(
    mut checked: Result<(PathBuf, rawr_import::bindings::Settlement), Unchecked>,
    cargo: Option<&Checked>,
    target: &rawr::project::Target,
    profile: &dyn crate::Profile,
) -> Result<PathBuf, Option<diagnostic::Report>> {
    let status = cargo.map(|cargo| &cargo.status);
    let reported = |unchecked: Unchecked| unchecked.report(status);
    let Some(Checked {
        status: rawr_cli::cargo::Status::Finished(outcome),
        observed: Some(observed),
    }) = cargo
    else {
        return checked.map(|(source, _)| source).map_err(reported);
    };
    if outcome.failure.is_some() {
        return checked.map(|(source, _)| source).map_err(reported);
    }
    let mut restarts = crate::Restarts::default();
    loop {
        let (source, settlement) = checked.map_err(reported)?;
        match settlement.settle_checked(observed) {
            Ok(()) => return Ok(source),
            Err(error) if restarts.again(&error) => checked = check(target, None, profile),
            Err(error) => return Err(Some(framework(error, fallback(target).ok()))),
        }
    }
}

/// Run one imported application or preview under the generated child owner.
/// `graph` is the target's load when the caller already has it. `Ok(false)`
/// is a program that ended with a failing exit: it printed its own error as
/// it ran.
pub fn run(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
) -> Result<bool, diagnostic::Report> {
    let ran = session(
        target,
        Origin::graph(graph),
        lifecycle,
        profile,
        None,
        None,
        &cells(target),
        &mut rawr_import::api::Kept::default(),
    )
    .map_err(Unchecked::into_report);
    events(ran.as_ref().map_err(Some), None);
    ran.map(|ended| matches!(ended, Ended::Exit(0) | Ended::Restart(..)))
}

/// Tell a supervising `rawr mcp` how a session ended (D537.4). A failure
/// without a report is `cargo`'s.
fn events(
    ran: Result<&Ended, Option<&diagnostic::Report>>,
    cargo: Option<&rawr_cli::cargo::Status>,
) {
    match ran {
        Ok(Ended::Exit(code)) => rawr_cli::events::ended(*code),
        Ok(Ended::Restart(..)) => {}
        Err(Some(report)) => rawr_cli::events::failed(report),
        Err(None) => {
            if let Some(outcome) = failing(cargo) {
                rawr_cli::events::failed_cargo(outcome);
            }
        }
    }
}

/// `rawr run` of a program (D500.1): it runs with `arguments` after its
/// name, as `cargo run` gives them, and with this process's stdin. A save
/// swaps or restarts it as [`run`] does. A program that panics or exits
/// with an error runs again on a save when stdin is a terminal or a `rawr
/// mcp` supervises the run; otherwise, or when it exits with status 0 or
/// Ctrl-C ended it, this returns its exit status (#1800).
///
/// `cargo`'s errors are printed when the program first ends. A program
/// whose copy fails to build as `cargo check` fails shows cargo's errors
/// alone, and fails with no report of its own (`Err(None)`).
pub fn run_program(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
    arguments: &[std::ffi::OsString],
    mut cargo: Option<rawr_cli::cargo::Pending>,
) -> Result<i32, Option<diagnostic::Report>> {
    // Ctrl-C at the terminal ends the program with SIGINT, as a shell
    // reports it.
    const INTERRUPTED: i32 = 128 + 2;
    let program = rawr_import::process::Arguments {
        name: rawr_cli::supervise::program_name(target),
        arguments: arguments.to_vec(),
        prepare: Some(prepare),
    };
    // The program hears its reload outcomes on this stream (#1166). A
    // program runs without it when it can't be made.
    let reload = rawr_cli::events::reload::Stream::open()
        .inspect_err(|error| {
            eprintln!("rawr: the program won't hear its reload outcomes: {error}");
        })
        .ok();
    // A program whose profile has `panic = "abort"` ends a panic, which the
    // runner reports as Rust's status 101, as SIGABRT does in a shell.
    const ABORTED: i32 = 128 + 6;
    let aborts = |graph: Option<&rawr::package::Graph>| {
        graph.is_some_and(|graph| {
            graph.entry_package().panic() == rawr::project::cargo::Panic::Abort
        })
    };
    let mut abort = aborts(graph.as_ref());
    let lifecycle = rawr_import::process::Lifecycle::Run;
    let interactive = rawr_cli::supervise::interactive();
    let mut watcher = graph.as_ref().and_then(rawr_cli::supervise::Watcher::open);
    let cells = cells(target);
    // The API graph each session's saves share, kept across restarts.
    let mut kept = rawr_import::api::Kept::default();
    let mut ran = session(
        target,
        Origin::graph(graph),
        lifecycle,
        profile,
        Some(&program),
        reload.as_ref(),
        &cells,
        &mut kept,
    );
    loop {
        let restarted = matches!(ran, Ok(Ended::Restart(..)));
        // A session that ended hears from `cargo check`: the one started
        // with the run, or, for a later copy that failed to build, one of
        // the program as it is now.
        let checked = if restarted {
            None
        } else {
            let pending = cargo.take().map(rawr_cli::cargo::Pending::finish);
            match &ran {
                Err(unchecked) if unchecked.is_copy() && pending.is_none() => {
                    checked_now(&location(target))
                }
                _ => pending,
            }
        };
        let mut settled = ran.map_err(|unchecked| unchecked.report(checked.as_ref()));
        events(settled.as_ref().map_err(Option::as_ref), checked.as_ref());
        if let Some(reload) = &reload {
            reload.starting(match &settled {
                Ok(Ended::Restart(reason, _)) => Some(reason.as_str()),
                _ => None,
            });
        }
        // The preparation a restart already made, which the next session
        // starts from while it holds (#1761).
        let carried = match &mut settled {
            Ok(Ended::Restart(_, prepared)) => prepared.take(),
            _ => None,
        };
        if !restarted {
            rawr_cli::cargo::after_run(checked);
            match settled {
                Ok(Ended::Exit(101)) if abort && !interactive => return Ok(ABORTED),
                Ok(Ended::Exit(code)) if !interactive || code == 0 || code == INTERRUPTED => {
                    return Ok(code);
                }
                Ok(_) => eprintln!("rawr: the program ended; a save runs it again"),
                Err(report) if !interactive || watcher.is_none() => return Err(report),
                Err(Some(report)) => rawr_import::process::say(diagnostic::render(&report)),
                Err(None) => {}
            }
            let Some(waiting) = watcher.as_ref() else {
                return Ok(0);
            };
            // A save that changed the program's files; a build touching
            // them without changing them is no save.
            let ended = rawr::project::Target::discover(location(target))
                .ok()
                .and_then(|target| contents(&target, &cells));
            loop {
                waiting.wait();
                let now = rawr::project::Target::discover(location(target))
                    .ok()
                    .and_then(|target| contents(&target, &cells));
                if now.is_none() || now != ended {
                    break;
                }
            }
        }
        let next = match rawr::project::Target::discover(location(target)) {
            Ok(next) => next,
            Err(error) => {
                ran = Err(diagnostic::Report::from_project(&error).into());
                continue;
            }
        };
        let parsed = cells.counts().parsed;
        let graph = load(&next, &cells).ok();
        if rawr_cli::cells::verbose() {
            eprintln!(
                "rawr-import: a new session's load parsed {} sources",
                cells.counts().parsed - parsed
            );
        }
        if let Some(next) = graph.as_ref().and_then(rawr_cli::supervise::Watcher::open) {
            watcher = Some(next);
        }
        abort = aborts(graph.as_ref());
        ran = session(
            &next,
            Origin {
                graph,
                prepared: carried,
            },
            lifecycle,
            profile,
            Some(&program),
            reload.as_ref(),
            &cells,
            &mut kept,
        );
    }
}

/// `rawr test` of a program (#1653): its runner runs the tests built as
/// `cargo test` builds them, with libtest's `arguments`, and reports as
/// libtest does. It runs them once, as `cargo test` does: a save that
/// restarts the runner runs them again from the program as it is then.
/// The status is libtest's.
///
/// `cargo`'s errors are printed after the report. Tests whose copy fails
/// to build as `cargo check` fails show cargo's errors alone, and fail
/// with no report of their own (`Err(None)`).
pub fn run_tests(
    target: &rawr::project::Target,
    graph: rawr::package::Graph,
    profile: &dyn crate::Profile,
    arguments: &[std::ffi::OsString],
    cargo: Option<rawr_cli::cargo::Pending>,
) -> Result<i32, Option<diagnostic::Report>> {
    let program = rawr_import::process::Arguments {
        name: rawr_cli::supervise::program_name(target),
        arguments: arguments.to_vec(),
        prepare: Some(crate::signals::unblocked),
    };
    let lifecycle = rawr_import::process::Lifecycle::Run;
    let mut origin = Origin::graph(Some(graph));
    let cells = cells(target);
    let mut kept = rawr_import::api::Kept::default();
    let ran = loop {
        match session(
            target,
            std::mem::take(&mut origin),
            lifecycle,
            profile,
            Some(&program),
            None,
            &cells,
            &mut kept,
        ) {
            Ok(Ended::Restart(_, prepared)) => {
                origin = Origin {
                    graph: None,
                    prepared,
                };
            }
            Ok(Ended::Exit(code)) => break Ok(code),
            Err(unchecked) => break Err(unchecked),
        }
    };
    let checked = cargo.map(rawr_cli::cargo::Pending::finish);
    let checked = match (&ran, checked) {
        (Err(unchecked), None) if unchecked.is_copy() => checked_now(&location(target)),
        (_, checked) => checked,
    };
    let settled = ran.map_err(|unchecked| unchecked.report(checked.as_ref()));
    rawr_cli::cargo::after_run(checked);
    settled
}

/// What each runner's command gets before it spawns: no blocked signals,
/// and the program's reload stream.
fn prepare(command: &mut std::process::Command) {
    crate::signals::unblocked(command);
    if let Some(path) = rawr_cli::events::reload::path() {
        command.env(rawr_cli::events::reload::VARIABLE, path);
    }
}

/// The contents of the program's files: its sources, its native modules
/// and its manifests, loaded through `cells`. `None` when it doesn't load.
fn contents(
    target: &rawr::project::Target,
    cells: &rawr::package::Cells,
) -> Option<Vec<(PathBuf, Option<Vec<u8>>)>> {
    let graph = load(target, cells).ok()?;
    Some(rawr_cli::supervise::contents(&graph))
}

/// The program `target` selects, each source read from its parse in
/// `cells` while its text is the same, and parsed otherwise (#1762).
fn load(
    target: &rawr::project::Target,
    cells: &rawr::package::Cells,
) -> Result<rawr::package::Graph, rawr::package::Error> {
    rawr::package::Graph::load_with_cells(&rawr::Engine::new(), target, cells)
}

/// The parse cells of the program `target` selects, remembered across its
/// loads: each save and each session of a run reads a source again only
/// when its text changed (#1604, #1762).
fn cells(target: &rawr::project::Target) -> rawr::package::Cells {
    rawr::package::Cells::new(rawr_cli::cells::directory(target)).remember()
}

/// Where the program is: the crate's root, or its single file.
fn location(target: &rawr::project::Target) -> PathBuf {
    target.project().map_or_else(
        || target.source().path().to_path_buf(),
        |project| project.location().to_path_buf(),
    )
}

/// How a [`session`] ended.
enum Ended {
    /// The program exited with this status.
    Exit(i32),
    /// A save changed the program in a way its running process can't
    /// take, for this reason; its runner is gone. The preparation the save
    /// made comes along, for the next session to start from (#1761).
    Restart(String, Option<Box<crate::Prepared>>),
}

/// What a [`session`] starts its program from: the caller's load of it,
/// and the preparation a restart already made, which the session's start
/// uses instead of preparing again while the files it read hold (#1761).
#[derive(Default)]
struct Origin {
    graph: Option<rawr::package::Graph>,
    prepared: Option<Box<crate::Prepared>>,
}

impl Origin {
    /// A start from the caller's load, with no preparation yet.
    const fn graph(graph: Option<rawr::package::Graph>) -> Self {
        Self {
            graph,
            prepared: None,
        }
    }
}

/// One run of the program's runners, until the program ends or a save
/// restarts it with a new start. A restart that needs no native build
/// starts a new process of the same runner here instead (D553.6), and
/// `reload`'s connections hear it restarted. Each save reads the program's
/// unchanged sources from their parses in `cells`, and parses only those
/// whose text changed (#1604). Each save shares the API graph `kept`, which
/// outlasts the session (#1822).
#[allow(clippy::too_many_arguments)]
fn session(
    target: &rawr::project::Target,
    origin: Origin,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
    program: Option<&rawr_import::process::Arguments>,
    reload: Option<&rawr_cli::events::reload::Stream>,
    cells: &rawr::package::Cells,
    kept: &mut rawr_import::api::Kept,
) -> Result<Ended, Unchecked> {
    same_build(target)?;
    // Each save that changes Rawr source reads the same API fragments.
    rawr_import::extract::keep_fragment_reads();
    // What the program starts from, read before it is prepared: a save
    // restarts it only when this changed. The caller's load is it.
    let mut started_from = program.and_then(|_| match &origin.graph {
        Some(graph) => Some(rawr_cli::supervise::contents(graph)),
        None => contents(target, cells),
    });
    let source = fallback(target)?;
    let root = project(target).ok_or_else(|| {
        framework(
            "cannot find a Cargo project for this program",
            source.clone(),
        )
    })?;
    // Where a save finds the program again: the crate's root, or a
    // script's own file.
    let target_path = location(target);
    // A host's window may listen on a socket under `target/rawr`; a killed
    // window cannot remove it, so its owner does, and `cargo clean` never
    // meets a stale one.
    let _socket = profile
        .socket(&root)
        .map(rawr_import::process::remove_on_exit);
    // A `rawr mcp` that started this run hears what happens (D537.3–4),
    // and the death of whatever started it, a server, a shell or a test,
    // ends the run and its program.
    rawr_cli::events::watch_parent(|| drop(rawr_import::process::retire_owned()));
    if rawr_cli::events::supervised() {
        rawr_import::progress::listen(rawr_cli::events::building);
    }
    rawr_import::progress::begin(&root);
    let start = resident::Start::now();
    let initial = initial_watched_program(
        target,
        origin,
        &target_path,
        lifecycle,
        profile,
        program,
        Some(&mut *kept),
    );
    rawr_import::progress::finish(initial.is_ok());
    start.tell();
    let (mut lane, adapter, mut manifests, mut native_baseline) = initial?;
    rawr_cli::events::running();

    let mut marked = marked(program.is_some(), &manifests);
    let mut admits = admits(program.is_some(), &manifests);
    // Whether a save swapped into the running `main`.
    let mut swapped_marked = false;
    let mut refresh_at = Instant::now();
    let mut refresh_required = false;
    // The preparation a restart was last announced for.
    let mut announced = None;
    let mut watched = resident::Watched::default();
    // The last save the runner compiled in place, which a restart as the
    // program refuses it, or panics after it, starts from.
    let mut swap = None;
    // A save whose runner build found a cache entry stale is attempted
    // again, whose preparation rebuilds it (#613).
    let mut restarts = crate::Restarts::default();
    // The program's run, as a block whose failures are checked against
    // the program's own end below.
    let ran = (|| -> Result<Ended, diagnostic::Report> {
        loop {
            match reaped(&mut lane, swapped_marked, lifecycle, &source)? {
                // The running build still stands when no native input
                // changed since it was checked.
                Some(Ended::Restart(reason, _))
                    if !refresh_required && adapter.resident_eligible(&native_baseline) =>
                {
                    if let Some(save) = swap.take()
                        && respawned(&mut lane, save, &reason, reload)
                    {
                        swapped_marked = false;
                        continue;
                    }
                    return Ok(Ended::Restart(reason, None));
                }
                Some(ended) => return Ok(ended),
                None => {}
            }

            adapter.poll();
            let now = Instant::now();
            if now >= refresh_at {
                refresh_at = now + REFRESH_POLL;
                if lane
                    .take_refresh()
                    .map_err(|error| framework(error, source.clone()))?
                {
                    refresh_required = true;
                    adapter
                        .gate()
                        .observe(now)
                        .map_err(|error| framework(error, source.clone()))?;
                }
            }
            if let Some(watch_request) = adapter.start(now) {
                let revision = match lane.revision() {
                    Ok(revision) => revision,
                    Err(error) => {
                        settle_failure(&mut lane, &adapter, framework(error, source.clone()))?;
                        continue;
                    }
                };
                // An async `main` runs on its runtime's thread, which a new
                // generation can't reach: each save that prepares restarts the
                // program (D500.1), and one that fails leaves it running.
                if program.is_some() && !manifests.has_islands() && !marked {
                    let next = rawr::project::Target::discover(&target_path);
                    let changed = next.as_ref().ok().and_then(|next| contents(next, cells));
                    if changed.is_some() && changed == started_from {
                        continue;
                    }
                    let prepared = next
                        .map_err(|error| rawr::diagnostic::Report::from_project(&error))
                        .and_then(|next| {
                            profile
                                .prepare(
                                    &next,
                                    None,
                                    revision,
                                    &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
                                    Some(&mut *kept),
                                )
                                .map_err(|error| import(error, source.clone()))
                        });
                    match prepared {
                        Ok(prepared) => {
                            let reason = manifests.restart_line(&prepared.manifests);
                            rawr_import::process::say(format_args!(
                                "rawr: {reason}; restarting the program"
                            ));
                            rawr_cli::events::restarting(&reason);
                            return Ok(Ended::Restart(reason, Some(Box::new(prepared))));
                        }
                        // Latest wins: a newer save reports for itself.
                        Err(_) if adapter.deadline().is_some() => {}
                        Err(report) => {
                            rawr_import::process::say(diagnostic::render(&report));
                            rawr_cli::events::failed(&report);
                        }
                    }
                    // A failed save is reported once.
                    started_from = changed;
                    continue;
                }
                let started = lane
                    .begin_compile()
                    .map_err(|error| framework(error, source.clone()))?;
                let scope = rawr_import::supervisor::snapshot::attempt::Scope::begin();
                let next_target = match rawr::project::Target::discover(&target_path) {
                    Ok(target) => target,
                    Err(error) => {
                        lane.cancel_compile(started)
                            .map_err(|error| framework(error, source.clone()))?;
                        settle_failure(
                            &mut lane,
                            &adapter,
                            rawr::diagnostic::Report::from_project(&error),
                        )?;
                        continue;
                    }
                };
                let work = resident::Work::now(cells);
                let _fallback_inputs = match resident::attempt(
                    &mut lane,
                    &adapter,
                    &watch_request,
                    started,
                    revision,
                    &next_target,
                    &manifests,
                    &native_baseline,
                    refresh_required,
                    &mut watched,
                    &scope,
                    cells,
                    &source,
                ) {
                    Ok(resident::Attempt::Settled) => continue,
                    // The runner admits the generation at its
                    // next call from native code, and says `swapped` then
                    // (D553.3).
                    Ok(resident::Attempt::Swapped(save)) if admits => {
                        swapped_marked = true;
                        swap = Some(save);
                        rawr_cli::events::queued();
                        work.tell(cells);
                        continue;
                    }
                    Ok(resident::Attempt::Swapped(save)) => {
                        swap = Some(save);
                        rawr_cli::events::swapped();
                        work.tell(cells);
                        continue;
                    }
                    // The running `main` can't take the save, and said
                    // why: it restarts now, with no native build prepared
                    // for a swap that couldn't happen (D553.6). The runner
                    // compiled the save against its own build, so a new
                    // process of it runs the save.
                    Ok(resident::Attempt::Restart(reason, save, staged)) => {
                        rawr_import::process::say(format_args!(
                            "rawr: {reason}; restarting the program"
                        ));
                        rawr_cli::events::restarting(&reason);
                        // A native input that changed since the compile
                        // takes a new native build, which prepares and
                        // builds below while the program keeps running.
                        if !adapter.resident_eligible(&native_baseline) {
                            Some(staged)
                        } else if !respawned(&mut lane, save, &reason, reload) {
                            return Ok(Ended::Restart(reason.into(), None));
                        } else {
                            swapped_marked = false;
                            swap = None;
                            for error in adapter
                                .commit_inputs(staged)
                                .map_err(|error| framework(error, source.clone()))?
                            {
                                rawr_import::process::say(format_args!(
                                    "warning: file watcher: {error}"
                                ));
                            }
                            continue;
                        }
                    }
                    Ok(resident::Attempt::Prepare(staged, reason)) => {
                        rawr_import::process::say(format_args!("rawr: preparing again: {reason}"));
                        rawr_cli::events::preparing(&reason.to_string());
                        staged
                    }
                    Err(report) => {
                        settle_failure(&mut lane, &adapter, report)?;
                        continue;
                    }
                };
                let preparation = resident::Start::now();
                let prepared =
                    profile.prepare(&next_target, None, revision, &scope, Some(&mut *kept));
                preparation.tell_preparation();
                let prepared = match prepared {
                    Ok(prepared) => prepared,
                    // A newer save; its own attempt follows.
                    Err(crate::Error::Rawr(error)) if error.source_changed() => continue,
                    Err(error) => {
                        settle_failure(&mut lane, &adapter, import(error, source.clone()))?;
                        continue;
                    }
                };
                let next_manifests = prepared.manifests.clone();
                let demand = prepared.demand().to_vec();
                let (request, snapshot, project, inputs) = prepared.into_parts();
                // What a new process of the running runner starts from when
                // the running `main` refuses this save, as it compiles it or
                // as it admits it (#1761).
                let respawn_from = admits.then(|| snapshot.clone());
                let observation = match inputs.encode_observation() {
                    Ok(observation) => observation,
                    Err(error) => {
                        settle_failure(&mut lane, &adapter, framework(error, source.clone()))?;
                        continue;
                    }
                };
                // A program with native islands (D501) keeps its runner while
                // the native build stays the same: the new generation swaps in
                // at its next call through a trampoline. Any other program's
                // swap went by the resident route above.
                let started = if next_manifests.has_islands() || marked {
                    match lane.begin_compile() {
                        Ok(started) => started,
                        Err(error) => {
                            settle_failure(&mut lane, &adapter, framework(error, source.clone()))?;
                            continue;
                        }
                    }
                } else {
                    None
                };
                let preparation =
                    match lane.prepare_started_compile(started, request, snapshot, &observation) {
                        Ok(preparation) => preparation,
                        // The runner refused a generation the running
                        // `main` can't take, and said why (D553.6).
                        Err(crate::native::Error::Process(
                            rawr_import::process::RunnerError::ProfileRejected(reason),
                        )) if admits => {
                            rawr_import::process::say(format_args!(
                                "rawr: {reason}; restarting the program"
                            ));
                            rawr_cli::events::restarting(&reason);
                            // The runner compiled the save against its own
                            // build, whose request key the preparation kept:
                            // a new process of it runs the save (D553.6).
                            let next_native_baseline = adapter.native_baseline();
                            let save = respawn_from.map(|snapshot| {
                                resident::Respawn::new(snapshot, observation.into())
                            });
                            let staged = match save {
                                Some(save) => adapter
                                    .prepare_inputs(inputs)
                                    .ok()
                                    .filter(|_| respawned(&mut lane, save, &reason, reload)),
                                None => None,
                            };
                            let Some(staged) = staged else {
                                return Ok(Ended::Restart(reason.into(), None));
                            };
                            manifests = next_manifests;
                            native_baseline = next_native_baseline;
                            watched = resident::Watched::default();
                            swapped_marked = false;
                            swap = None;
                            refresh_required = false;
                            restarts = crate::Restarts::default();
                            for error in adapter
                                .commit_inputs(staged)
                                .map_err(|error| framework(error, source.clone()))?
                            {
                                rawr_import::process::say(format_args!(
                                    "warning: file watcher: {error}"
                                ));
                            }
                            continue;
                        }
                        Err(error) => {
                            settle_failure(&mut lane, &adapter, framework(error, source.clone()))?;
                            continue;
                        }
                    };
                let staged = match adapter.prepare_inputs(inputs) {
                    Ok(staged) => staged,
                    Err(error) => {
                        let report = match preparation {
                        rawr_import::process::hot::Preparation::Prepared(prepared) => {
                            match lane.abort_compile(prepared) {
                                Ok(()) => framework(error, source.clone()),
                                Err(cleanup) => framework(
                                    format!("cannot watch the program's files: {error}; cancelling the reload also failed: {cleanup}"),
                                    source.clone(),
                                ),
                            }
                        }
                        rawr_import::process::hot::Preparation::Miss(_) => {
                            framework(error, source.clone())
                        }
                    };
                        settle_failure(&mut lane, &adapter, report)?;
                        continue;
                    }
                };
                // Capture before the full path's final current validation, never
                // after adoption where an intervening native event could disappear.
                let next_native_baseline = adapter.native_baseline();
                // A hot commit swaps; a new runner restarts the program.
                let swapped = matches!(
                    preparation,
                    rawr_import::process::hot::Preparation::Prepared(_)
                );
                let mut stale = false;
                let admitted = match preparation {
                    rawr_import::process::hot::Preparation::Prepared(prepared) => lane
                        .revalidate_compile(prepared)
                        // Child callbacks revoke the retained request on the exact
                        // UI gate. Parent notices only schedule work; never hold
                        // the parent gate through validation, pinning or hot IPC.
                        .and_then(|prepared| lane.commit_compile(prepared))
                        .map(|_| crate::native::Watched::Installed)
                        .or_else(|error| {
                            // A newer save supersedes it, and its attempt follows.
                            if error.source_changed() {
                                Ok(crate::native::Watched::Obsolete)
                            } else {
                                Err(framework(error, source.clone()))
                            }
                        }),
                    // D500.4: a new native build restarts the program, and
                    // says why in one line, once for each new build. It
                    // builds from this preparation while the program keeps
                    // running (#1761, #1810); a build that fails leaves it
                    // running.
                    rawr_import::process::hot::Preparation::Miss(miss) => {
                        let reason = if marked {
                            Some(manifests.restart_line(&next_manifests))
                        } else if announced.as_ref() != Some(&next_manifests)
                            || manifests.restarts_on_save()
                        {
                            manifests.restart_reason(&next_manifests, || lane.native_change(&miss))
                        } else {
                            None
                        };
                        if let Some(reason) = &reason {
                            rawr_import::process::say(format_args!(
                                "rawr: {reason}; restarting the program"
                            ));
                            rawr_cli::events::restarting(reason);
                            announced = Some(next_manifests.clone());
                        }
                        let (request, snapshot) = miss.into_parts();
                        let kept = request.kept().ok();
                        let mut report = |error: &dyn std::fmt::Display, again: bool| {
                            stale |= again;
                            framework(error, source.clone())
                        };
                        let walks = rawr_import::progress::read_set_walks();
                        let built = request
                            .generate()
                            .map_err(|error| report(&error, restarts.again(&error)))
                            .and_then(|generated| {
                                lane.build(generated)
                                    .map_err(|error| report(&error, restarts.again(&error)))
                            })
                            .inspect(|_| {
                                if let Some(kept) = &kept {
                                    crate::runner_demand::store(&project, kept, &demand);
                                }
                            })
                            .and_then(|mut built| {
                                built
                                    .retain_startup(&snapshot)
                                    .map_err(|error| report(&error, restarts.again(&error)))?;
                                Ok(built)
                            });
                        // A program that exited with status 0 as its next
                        // runner built ended the run (D951). One that failed
                        // runs again from the save.
                        if built.is_ok() && matches!(lane.reap_if_exited(), Ok(true)) {
                            return Ok(Ended::Exit(0));
                        }
                        // A marked `main` ends before the new one runs, as a
                        // new session's start would end it (D943).
                        let handover = if marked {
                            if let (Some(reload), Some(reason)) = (reload, &reason) {
                                reload.starting(Some(reason.as_str()));
                            }
                            crate::native::Handover::Retire
                        } else {
                            crate::native::Handover::Overlap
                        };
                        built
                            .and_then(|built| {
                                // Its start walks its read set once, as the
                                // new runner observes its inputs (#1760).
                                lane.replace_built_started_request(
                                    built,
                                    snapshot,
                                    &adapter,
                                    &observation,
                                    watch_request,
                                    handover,
                                )
                                .map_err(|error| report(&error, restarts.again(&error)))
                            })
                            .inspect(|_| {
                                if rawr_cli::cells::verbose() {
                                    eprintln!(
                                        "rawr-import: a new runner started, read-set walks {}",
                                        rawr_import::progress::read_set_walks() - walks
                                    );
                                }
                            })
                    }
                };
                match admitted {
                    Ok(crate::native::Watched::Installed) => {
                        // The runner admits the generation at
                        // its next call from native code, and says `swapped`
                        // then (D553.3).
                        if swapped && admits {
                            swapped_marked = true;
                            rawr_cli::events::queued();
                        } else if swapped {
                            rawr_cli::events::swapped();
                        } else {
                            // A new runner: its program has taken no swap,
                            // and its kind is its new manifests'.
                            swapped_marked = false;
                            marked = self::marked(program.is_some(), &next_manifests);
                            admits = self::admits(program.is_some(), &next_manifests);
                            rawr_cli::events::running();
                        }
                        manifests = next_manifests;
                        native_baseline = next_native_baseline;
                        watched = resident::Watched::default();
                        // A swap the program refuses as it admits it, or
                        // panics after, restarts from it in a new process
                        // of this runner (#1761).
                        swap = respawn_from
                            .filter(|_| swapped)
                            .map(|snapshot| resident::Respawn::new(snapshot, observation.into()));
                        refresh_required = false;
                        restarts = crate::Restarts::default();
                        for error in adapter
                            .commit_inputs(staged)
                            .map_err(|error| framework(error, source.clone()))?
                        {
                            rawr_import::process::say(format_args!(
                                "warning: file watcher: {error}"
                            ));
                        }
                    }
                    Ok(crate::native::Watched::Obsolete) => drop(staged),
                    // The old program ended for the new one, which didn't
                    // start: no program runs, and it takes the full start.
                    Err(report) if lane.current_key().is_none() => {
                        drop(staged);
                        rawr_import::process::say(diagnostic::render(&report));
                        return Ok(Ended::Restart(
                            manifests.restart_line(&next_manifests),
                            None,
                        ));
                    }
                    // The save is attempted again, and its preparation rebuilds
                    // the entry that went stale.
                    Err(_) if stale => {
                        drop(staged);
                        adapter
                            .gate()
                            .observe(Instant::now())
                            .map_err(|error| framework(error, source.clone()))?;
                    }
                    Err(report) => {
                        drop(staged);
                        settle_failure(&mut lane, &adapter, report)?;
                    }
                }
            }

            let now = Instant::now();
            let timeout = adapter
                .deadline()
                .map_or(IDLE_POLL, |deadline| {
                    deadline.saturating_duration_since(now)
                })
                .min(IDLE_POLL);
            match adapter.notices().recv_timeout(timeout) {
                Ok(rawr_watch::Notice::Changed(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(rawr_watch::Notice::Backend(error)) => {
                    retain_report(
                    &mut lane,
                    framework(
                        format!("the file watcher stopped, so saves no longer reload the program: {error}"),
                        source.clone(),
                    ),
                )?;
                    return wait_for_window(&mut lane, &source).map(|()| Ended::Exit(0));
                }
                Ok(rawr_watch::Notice::Schedule(error)) => {
                    retain_report(
                    &mut lane,
                    framework(
                        format!("the reload scheduler stopped, so saves no longer reload the program: {error}"),
                        source.clone(),
                    ),
                )?;
                    return wait_for_window(&mut lane, &source).map(|()| Ended::Exit(0));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    retain_report(
                        &mut lane,
                        framework(
                            "the file watcher stopped, so saves no longer reload the program",
                            source.clone(),
                        ),
                    )?;
                    return wait_for_window(&mut lane, &source).map(|()| Ended::Exit(0));
                }
            }
        }
    })();
    match ran {
        // A program that ended as its owner asked its runner something
        // ended the run: its status stands, not the ask's failure.
        Err(report) if lane.exited() => {
            Ok(reaped(&mut lane, swapped_marked, lifecycle, &source)?.ok_or(report)?)
        }
        ran => Ok(ran?),
    }
}

/// Whether the program is a marked `main`, which keeps running across a
/// save: the runner compiles the new generation beside it and admits it at
/// its next call from native code into script code (D553). An async `main`
/// restarts.
fn marked(program: bool, manifests: &resident::Manifests) -> bool {
    program && !manifests.has_islands() && !manifests.restarts_on_save()
}

/// Whether the running `main` admits a save: a marked one, or a native one
/// (D501), which runs on the thread that owns the program too (#1119) and
/// admits a save at its next call into marked code, and refuses one it
/// can't take, as a marked `main`'s does.
fn admits(program: bool, manifests: &resident::Manifests) -> bool {
    marked(program, manifests) || (program && manifests.has_islands())
}

/// Restart the program for `reason` in a new process of its runner, from
/// `save`, with no check and no native build (D553.6). `false` when it
/// didn't start, and no runner runs: the program takes the full start.
fn respawned(
    lane: &mut Lane,
    save: resident::Respawn,
    reason: &str,
    reload: Option<&rawr_cli::events::reload::Stream>,
) -> bool {
    if let Some(reload) = reload {
        reload.starting(Some(reason));
    }
    let walks = rawr_import::progress::read_set_walks();
    match save.start(lane) {
        Ok(_) => {
            if rawr_cli::cells::verbose() {
                eprintln!(
                    "rawr-import: a restart respawned its runner with no check, read-set walks {}",
                    rawr_import::progress::read_set_walks() - walks
                );
            }
            rawr_cli::events::running();
            true
        }
        Err(error) => {
            // Each cause down to the runner's own reason, as when the new
            // process refuses the save at its first compile (#1761).
            if rawr_cli::cells::verbose() {
                let mut reason = error.to_string();
                let mut cause = std::error::Error::source(&error);
                while let Some(next) = cause {
                    // A wrapper that shows its cause already says it.
                    let said = next.to_string();
                    if !reason.ends_with(&said) {
                        reason = format!("{reason}: {said}");
                    }
                    cause = next.source();
                }
                eprintln!("rawr-import: a restart couldn't respawn its runner: {reason}");
            }
            false
        }
    }
}

/// How the session ends when its program ended: `None` while it runs.
fn reaped(
    lane: &mut Lane,
    swapped_marked: bool,
    lifecycle: rawr_import::process::Lifecycle,
    source: &diagnostic::Source,
) -> Result<Option<Ended>, diagnostic::Report> {
    match lane.reap_if_exited() {
        Ok(true) => Ok(Some(Ended::Exit(0))),
        Ok(false) => Ok(None),
        // A panic after a swap, such as a framework's own check that its
        // hooks keep their order, restarts the program with the new code
        // instead of ending the run (D553.6): the framework may be
        // half-changed, so going back is not safe.
        Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(101))))
            if swapped_marked && lifecycle == rawr_import::process::Lifecycle::Run =>
        {
            let reason = "the program panicked after a save";
            rawr_import::process::say(format_args!("rawr: {reason}; restarting the program"));
            rawr_cli::events::restarting(reason);
            Ok(Some(Ended::Restart(reason.to_owned(), None)))
        }
        // The runner refused, as it admitted it, a save a value or a
        // callable made since its compile depends on the old types of, and
        // said why: the program starts again with the new code (#1746).
        Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(
            rawr::island::REFUSED,
        )))) if swapped_marked && lifecycle == rawr_import::process::Lifecycle::Run => {
            let reason = "a value made before a save took effect holds its old types";
            rawr_cli::events::restarting(reason);
            Ok(Some(Ended::Restart(reason.to_owned(), None)))
        }
        Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(code))))
            if lifecycle == rawr_import::process::Lifecycle::Run =>
        {
            Ok(Some(Ended::Exit(code)))
        }
        Err(error) => Err(framework(error, source.clone())),
    }
}

/// Prepare, build and install the program's first generation on a new
/// lane, started when `started` (a run or a preview) and not otherwise.
pub fn initial(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    started: bool,
    profile: &dyn crate::Profile,
) -> Result<(Lane, rawr::reload::source::Inputs), diagnostic::Report> {
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, None, &source)?;
    let Initial {
        built,
        snapshot,
        inputs,
        manifests: _,
    } = prepare_on_lane(
        &mut lane,
        target,
        Origin::graph(graph),
        profile,
        &source,
        None,
    )
    .map_err(Unchecked::into_report)?;
    let installed = if started {
        lane.replace_built_started(built, snapshot)
    } else {
        lane.replace_built(built, snapshot)
    };
    if let Err(error) = installed {
        drop(lane.shutdown());
        return Err(framework(error, source));
    }
    Ok((lane, inputs))
}

/// [`initial`], started and registered with a watcher over its inputs.
#[allow(clippy::type_complexity)]
pub fn initial_watched(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    target_path: &Path,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
) -> Result<
    (
        Lane,
        rawr_watch::Adapter,
        resident::Manifests,
        rawr_watch::native::Baseline,
    ),
    diagnostic::Report,
> {
    initial_watched_program(
        target,
        Origin::graph(graph),
        target_path,
        lifecycle,
        profile,
        None,
        None,
    )
    .map_err(Unchecked::into_report)
}

#[allow(clippy::type_complexity)]
fn initial_watched_program(
    target: &rawr::project::Target,
    origin: Origin,
    target_path: &Path,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
    program: Option<&rawr_import::process::Arguments>,
    mut kept: Option<&mut rawr_import::api::Kept>,
) -> Result<
    (
        Lane,
        rawr_watch::Adapter,
        resident::Manifests,
        rawr_watch::native::Baseline,
    ),
    Unchecked,
> {
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, program, &source)?;
    let mut initial = prepare_on_lane(
        &mut lane,
        target,
        origin,
        profile,
        &source,
        kept.as_deref_mut(),
    )?;
    let adapter = match rawr_watch::Adapter::open_inputs(initial.inputs.clone(), DEBOUNCE) {
        Ok(adapter) => adapter,
        Err(error) => {
            drop(lane.shutdown());
            return Err(framework(error, source).into());
        }
    };

    loop {
        let staged = match adapter.prepare_inputs(initial.inputs.clone()) {
            Ok(staged) => staged,
            Err(error) => {
                drop(lane.shutdown());
                return Err(framework(error, source).into());
            }
        };
        let checkpoint = match adapter.checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                drop(lane.shutdown());
                return Err(framework(error, source).into());
            }
        };
        let native_baseline = adapter.native_baseline();
        // The runner walks its read set once, after this checkpoint, as it
        // observes its inputs (#1760).
        let observation = initial
            .inputs
            .encode_observation()
            .map_err(|error| framework(error, source.clone()))?;
        match lane.replace_built_started_watched(
            initial.built,
            initial.snapshot,
            &adapter,
            &observation,
            checkpoint,
        ) {
            Ok(crate::native::Watched::Installed) => {
                for error in adapter
                    .commit_inputs(staged)
                    .map_err(|error| framework(error, source.clone()))?
                {
                    rawr_import::process::say(format_args!("warning: file watcher: {error}"));
                }
                return Ok((lane, adapter, initial.manifests, native_baseline));
            }
            Ok(crate::native::Watched::Obsolete) => {
                drop(staged);
                initial = refresh_initial(
                    &mut lane,
                    target_path,
                    profile,
                    &source,
                    kept.as_deref_mut(),
                )?;
            }
            // An input of the build changed since it was prepared: it is
            // prepared again, whose lookups rebuild what went stale.
            Err(error) if error.read_set_stale() => {
                drop(staged);
                initial = refresh_initial(
                    &mut lane,
                    target_path,
                    profile,
                    &source,
                    kept.as_deref_mut(),
                )?;
            }
            // A save as the program started (D531.9): it starts again from
            // the saved files.
            Err(error) if error.source_changed() => {
                drop(staged);
                rawr_import::process::say(
                    "rawr: the source changed as the program started; starting it again",
                );
                initial = refresh_initial(
                    &mut lane,
                    target_path,
                    profile,
                    &source,
                    kept.as_deref_mut(),
                )?;
            }
            Err(error) => {
                drop(staged);
                drop(lane.shutdown());
                return Err(framework(error, source).into());
            }
        }
    }
}

fn open_lane(
    target: &rawr::project::Target,
    lifecycle: rawr_import::process::Lifecycle,
    program: Option<&rawr_import::process::Arguments>,
    source: &diagnostic::Source,
) -> Result<Lane, diagnostic::Report> {
    let project = project(target).ok_or_else(|| {
        framework(
            "cannot find a Cargo project for this program",
            source.clone(),
        )
    })?;
    Lane::for_program(&project, lifecycle, program.cloned())
        .map_err(|error| framework(error, source.clone()))
}

fn prepare_on_lane(
    lane: &mut Lane,
    target: &rawr::project::Target,
    origin: Origin,
    profile: &dyn crate::Profile,
    source: &diagnostic::Source,
    mut kept: Option<&mut rawr_import::api::Kept>,
) -> Result<Initial, Unchecked> {
    // A save while the runner builds (D531.9): the build is kept, and the
    // program is prepared again from the saved files.
    let mut saved = None;
    // A cache entry that went stale before the runner's build used it
    // prepares again, whose lookups rebuild it (#613).
    let mut restarts = crate::Restarts::default();
    let Origin {
        mut graph,
        mut prepared,
    } = origin;
    loop {
        let target = saved.as_ref().unwrap_or(target);
        let revision = lane
            .revision()
            .map_err(|error| framework(error, source.clone()))?;
        // A restart's own preparation is used once: one that no longer
        // holds, as its runner binds the files it read, prepares again.
        let prepared = match prepared.take() {
            Some(prepared) => {
                if rawr_cli::cells::verbose() {
                    eprintln!("rawr-import: a start used the preparation its restart made");
                }
                Ok(*prepared)
            }
            None => profile.prepare(
                target,
                graph.take(),
                revision,
                &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
                kept.as_deref_mut(),
            ),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(crate::Error::Rawr(error)) if error.source_changed() => {
                saved = Some(discover(target)?);
                continue;
            }
            Err(error) => return Err(unchecked(error, source.clone())),
        };
        let manifests = prepared.manifests.clone();
        let mut prepared = prepared;
        let settlement = prepared.take_settlement();
        let demand = prepared.demand().to_vec();
        let (request, snapshot, project, inputs) = prepared.into_parts();
        let kept = request.kept().ok();
        let generated = match request.generate() {
            Ok(generated) => generated,
            Err(error) if restarts.again(&error) => continue,
            Err(error) => return Err(framework(error, source.clone()).into()),
        };
        // The API phase has ended; hand its freed allocator pages back before the build.
        crate::release::freed_memory();
        let mut built = match lane.build(generated) {
            Ok(built) => {
                if let Some(kept) = &kept {
                    crate::runner_demand::store(&project, kept, &demand);
                }
                built
            }
            Err(error) if restarts.again(&error) => continue,
            Err(error) => {
                return Err(match settlement.drift_diagnostic() {
                    // A stale build-script run in the shared nightly
                    // directory may be why: it is cleaned, and preparing
                    // again extracts afresh (D742).
                    Err(rerun) if restarts.again(&rerun) => continue,
                    Err(rerun) => framework(format!("{rerun}\n\n{error}"), source.clone()),
                    Ok(Some(drift)) => framework(format!("{drift}\n\n{error}"), source.clone()),
                    Ok(None) => framework(error, source.clone()),
                }
                .into());
            }
        };
        // Shared bindings are published, or refused, once the project's
        // own build has run their build scripts (D609.4). A table whose
        // build scripts gave the nightly other outputs prepares again.
        match settlement.settle() {
            Ok(()) => {}
            Err(error) if restarts.again(&error) => continue,
            Err(error) => return Err(framework(error, source.clone()).into()),
        }
        match built.retain_startup(&snapshot) {
            Ok(()) => {
                return Ok(Initial {
                    built,
                    snapshot,
                    inputs,
                    manifests,
                });
            }
            Err(rawr_import::node::Error::SourceChanged(_)) => saved = Some(discover(target)?),
            Err(error) if restarts.again(&error) => {}
            Err(error) => return Err(framework(error, source.clone()).into()),
        }
    }
}

/// The program as it is on disk now.
fn discover(target: &rawr::project::Target) -> Result<rawr::project::Target, diagnostic::Report> {
    rawr::project::Target::discover(location(target))
        .map_err(|error| rawr::diagnostic::Report::from_project(&error))
}

fn refresh_initial(
    lane: &mut Lane,
    target_path: &Path,
    profile: &dyn crate::Profile,
    source: &diagnostic::Source,
    kept: Option<&mut rawr_import::api::Kept>,
) -> Result<Initial, Unchecked> {
    let target = rawr::project::Target::discover(target_path)
        .map_err(|error| rawr::diagnostic::Report::from_project(&error))?;
    prepare_on_lane(lane, &target, Origin::default(), profile, source, kept)
}

/// Latest wins: once a newer change is waiting, an older attempt's failure is
/// superseded instead of reported. The newer attempt runs next and reports
/// for itself if the failure persists.
fn settle_failure(
    lane: &mut Lane,
    adapter: &rawr_watch::Adapter,
    report: diagnostic::Report,
) -> Result<(), diagnostic::Report> {
    // A newer save reports for itself, and a program that ended as the
    // save ran ends the run at the next reap.
    if adapter.deadline().is_some() || lane.exited() {
        return Ok(());
    }
    retain_report(lane, report)
}

fn retain_report(lane: &mut Lane, report: diagnostic::Report) -> Result<(), diagnostic::Report> {
    rawr_cli::events::failed(&report);
    let rendered = diagnostic::render(&report);
    rawr_import::process::say(format_args!(
        "rawr: the save failed; the last good program keeps running\n{rendered}"
    ));
    lane.report(&rendered)
        .map_err(|error| framework(error, None))
}

fn wait_for_window(lane: &mut Lane, source: &diagnostic::Source) -> Result<(), diagnostic::Report> {
    loop {
        if lane
            .reap_if_exited()
            .map_err(|error| framework(error, source.clone()))?
        {
            return Ok(());
        }
        std::thread::sleep(IDLE_POLL);
    }
}

/// A project that builds as Rawr builds it, or the error saying what to
/// change.
pub fn same_build(target: &rawr::project::Target) -> Result<(), diagnostic::Report> {
    let Some(root) = project(target) else {
        return Ok(());
    };
    crate::project::same_build(&root).map_err(|message| {
        diagnostic::Report::error(diagnostic::code::ENVIRONMENT_PROJECT_BUILD, message).finish()
    })?;
    crate::project::toolchain(&root).map_err(|message| {
        diagnostic::Report::error(diagnostic::code::ENVIRONMENT_PROJECT_BUILD, message).finish()
    })
}

/// The program's package root, where Cargo builds its runner: the crate's
/// root, or a script's directory in rawr's area (D608 point 6), created
/// here on a script's first run. `None` for another single file.
fn project(target: &rawr::project::Target) -> Option<PathBuf> {
    if let Some(project) = target.project() {
        return Some(project.root().as_path().to_path_buf());
    }
    if !rawr::project::frontmatter::is_script(target.source().text()) {
        return None;
    }
    let root = rawr::package::Graph::root_of(target);
    // A failure to create it is the lane's to report.
    let _ = std::fs::create_dir_all(&root);
    Some(root)
}

/// The program's entry source, for a diagnostic with nothing more precise.
pub fn fallback(target: &rawr::project::Target) -> Result<diagnostic::Source, diagnostic::Report> {
    diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| framework(error, None))
}

fn import(error: crate::Error, source: diagnostic::Source) -> diagnostic::Report {
    match error {
        crate::Error::Rawr(error) => crate::report::package(&error, source),
        error => framework(error, source),
    }
}

/// A failed build of rawr's copy of the program, reported at the marked
/// function's signature when rustc's error is in the code rawr generated
/// for native code to call it, and else against `source`.
fn copied(
    failure: rawr_import::location::copy::Failure,
    source: diagnostic::Source,
) -> diagnostic::Report {
    let Some(trampoline) = failure.trampoline() else {
        return framework(failure, source);
    };
    let report = (|| {
        let marked = diagnostic::Source::new(
            trampoline.file().to_string_lossy().into_owned(),
            trampoline.text(),
        )
        .ok()?;
        let (line, column) = trampoline.place();
        let line = u16::try_from(line).ok()?;
        let start = u16::try_from(column).ok()?;
        let end = u16::try_from(column + trampoline.width()).ok()?;
        let span = rawr::Span::new(
            rawr::Position::new(line, start),
            rawr::Position::new(line, end),
        );
        diagnostic::Report::error(diagnostic::code::FRAMEWORK_NATIVE, failure.to_string())
            .primary(marked, span, None)
            .ok()
    })();
    match report {
        Some(report) => report.finish(),
        None => framework(failure, source),
    }
}

/// A failure of the runner's machinery, reported against `source`.
pub fn framework(
    error: impl std::fmt::Display,
    source: impl Into<Option<diagnostic::Source>>,
) -> diagnostic::Report {
    crate::report::report(diagnostic::code::FRAMEWORK_NATIVE, error, source.into())
}
