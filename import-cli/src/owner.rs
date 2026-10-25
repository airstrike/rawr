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
/// against them. No runner is built; `rawr run` builds it.
pub fn check(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
) -> Result<PathBuf, diagnostic::Report> {
    same_build(target)?;
    let source = target.source().path().to_path_buf();
    let fallback = fallback(target)?;
    let revision = rawr_import::supervisor::Owner::new()
        .revision()
        .map_err(|error| framework(error, fallback.clone()))?;
    profile
        .prepare(
            target,
            graph,
            revision,
            &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
        )
        .map_err(|error| import(error, fallback))?;
    Ok(source)
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
    let ran = session(target, graph, lifecycle, profile, None);
    events(&ran);
    ran.map(|ended| matches!(ended, Ended::Exit(0) | Ended::Restart))
}

/// Tell a supervising `rawr mcp` how a session ended (D537.4).
fn events(ran: &Result<Ended, diagnostic::Report>) {
    match ran {
        Ok(Ended::Exit(code)) => rawr_cli::events::ended(*code),
        Ok(Ended::Restart) => {}
        Err(report) => rawr_cli::events::failed(report),
    }
}

/// `rawr run` of a program (D500.1): it runs with `arguments` after its
/// name, as `cargo run` gives them, and with this process's stdin. A save
/// swaps or restarts it as [`run`] does. Once it ends, a save runs it again
/// when stdin is a terminal; otherwise, or when Ctrl-C ended it, this
/// returns its exit status.
///
/// `cargo`'s errors are printed when the program first ends.
pub fn run_program(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
    arguments: &[std::ffi::OsString],
    mut cargo: Option<rawr_cli::cargo::Pending>,
) -> Result<i32, diagnostic::Report> {
    // Ctrl-C at the terminal ends the program with SIGINT, as a shell
    // reports it.
    const INTERRUPTED: i32 = 128 + 2;
    let program = rawr_import::process::Arguments {
        name: rawr_cli::supervise::program_name(target),
        arguments: arguments.to_vec(),
        prepare: Some(crate::signals::unblocked),
    };
    let lifecycle = rawr_import::process::Lifecycle::Run;
    let interactive = rawr_cli::supervise::interactive();
    let mut watcher = graph.as_ref().and_then(rawr_cli::supervise::Watcher::open);
    let mut ran = session(target, graph, lifecycle, profile, Some(&program));
    loop {
        let restart = matches!(ran, Ok(Ended::Restart));
        events(&ran);
        if !restart {
            rawr_cli::cargo::after_run(cargo.take());
            match ran {
                Ok(Ended::Exit(code)) if !interactive || code == INTERRUPTED => return Ok(code),
                Ok(_) => eprintln!("rawr: the program ended; a save runs it again"),
                Err(report) if !interactive || watcher.is_none() => return Err(report),
                Err(report) => rawr_import::process::say(diagnostic::render(&report)),
            }
            let Some(waiting) = watcher.as_ref() else {
                return Ok(0);
            };
            // A save that changed the program's files; a build touching
            // them without changing them is no save.
            let ended = rawr::project::Target::discover(location(target))
                .ok()
                .and_then(|target| contents(&target));
            loop {
                waiting.wait();
                let now = rawr::project::Target::discover(location(target))
                    .ok()
                    .and_then(|target| contents(&target));
                if now.is_none() || now != ended {
                    break;
                }
            }
        }
        let next = match rawr::project::Target::discover(location(target)) {
            Ok(next) => next,
            Err(error) => {
                ran = Err(diagnostic::Report::from_project(&error));
                continue;
            }
        };
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), &next).ok();
        if let Some(next) = graph.as_ref().and_then(rawr_cli::supervise::Watcher::open) {
            watcher = Some(next);
        }
        ran = session(&next, graph, lifecycle, profile, Some(&program));
    }
}

/// The contents of the program's files: its sources, its native modules
/// and its manifests. `None` when it doesn't load.
fn contents(target: &rawr::project::Target) -> Option<Vec<(PathBuf, Option<Vec<u8>>)>> {
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), target).ok()?;
    Some(rawr_cli::supervise::contents(&graph))
}

/// Where the program is: the crate's root, or its single file.
fn location(target: &rawr::project::Target) -> PathBuf {
    project(target).map_or_else(|| target.source().path().to_path_buf(), Path::to_path_buf)
}

/// How a [`session`] ended.
enum Ended {
    /// The program exited with this status.
    Exit(i32),
    /// A save changed a program that restarts on each save; its runner is
    /// gone.
    Restart,
}

/// One run of the program's runners, until the program ends or a save
/// restarts it.
fn session(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
    program: Option<&rawr_import::process::Arguments>,
) -> Result<Ended, diagnostic::Report> {
    same_build(target)?;
    // What the program starts from, read before it is prepared: a save
    // restarts it only when this changed.
    let mut started_from = program.and_then(|_| contents(target));
    let source = fallback(target)?;
    let target_path = project(target)
        .ok_or_else(|| framework("a native owner requires a Rawr crate", source.clone()))?
        .to_path_buf();
    // A host's window may listen on a socket under `target/rawr`; a killed
    // window cannot remove it, so its owner does, and `cargo clean` never
    // meets a stale one.
    let _socket = profile
        .socket(&target_path)
        .map(rawr_import::process::remove_on_exit);
    // A `rawr mcp` that started this run hears what happens, and its death
    // ends the run (D537.3–4).
    rawr_cli::events::watch_parent(|| drop(rawr_import::process::retire_owned()));
    if rawr_cli::events::supervised() {
        rawr_import::progress::listen(rawr_cli::events::building);
    }
    rawr_import::progress::begin(&target_path);
    let initial = initial_watched_program(target, graph, &target_path, lifecycle, profile, program);
    rawr_import::progress::finish(initial.is_ok());
    let (mut lane, adapter, mut manifests, mut native_baseline) = initial?;
    rawr_cli::events::running();

    // A marked `main` keeps running across a save: the runner compiles the
    // new generation beside it and admits it at its next call from native
    // code into script code (D553). An async `main` still restarts.
    let marked = program.is_some() && !manifests.has_islands() && !manifests.restarts_on_save();
    // Whether a save swapped into the running marked `main`.
    let mut swapped_marked = false;
    let mut refresh_at = Instant::now();
    let mut refresh_required = false;
    // The preparation a restart was last announced for.
    let mut announced = None;
    let mut native_inputs = None;
    loop {
        match lane.reap_if_exited() {
            Ok(true) => return Ok(Ended::Exit(0)),
            Ok(false) => {}
            // A panic after a swap, such as a framework's own check that
            // its hooks keep their order, restarts the program with the
            // new code instead of ending the run (D553.6): the framework
            // may be half-changed, so going back is not safe.
            Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(
                101,
            )))) if swapped_marked && lifecycle == rawr_import::process::Lifecycle::Run => {
                let reason = "the program panicked after a save";
                rawr_import::process::say(format_args!("rawr: {reason}; restarting the program"));
                rawr_cli::events::restarting(reason);
                return Ok(Ended::Restart);
            }
            Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(
                code,
            )))) if lifecycle == rawr_import::process::Lifecycle::Run => {
                return Ok(Ended::Exit(code));
            }
            Err(error) => return Err(framework(error, source.clone())),
        }

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
                let changed = next.as_ref().ok().and_then(contents);
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
                        return Ok(Ended::Restart);
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
                &mut native_inputs,
                &scope,
                &source,
            ) {
                Ok(resident::Attempt::Settled) => continue,
                Ok(resident::Attempt::Prepare(staged)) => staged,
                Err(report) => {
                    settle_failure(&mut lane, &adapter, report)?;
                    continue;
                }
            };
            let prepared = match profile.prepare(&next_target, None, revision, &scope) {
                Ok(prepared) => prepared,
                // A newer save; its own attempt follows.
                Err(crate::Error::Rawr(error)) if error.source_changed() => continue,
                Err(error) => {
                    settle_failure(&mut lane, &adapter, import(error, source.clone()))?;
                    continue;
                }
            };
            let next_manifests = prepared.manifests.clone();
            let limits = prepared.limits();
            let (request, snapshot, _, inputs) = prepared.into_parts();
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
                    // The runner refused a generation the running marked
                    // `main` can't take, and said why (D553.6).
                    Err(crate::native::Error::Process(
                        rawr_import::process::RunnerError::ProfileRejected(reason),
                    )) if marked => {
                        rawr_import::process::say(format_args!(
                            "rawr: {reason}; restarting the program"
                        ));
                        rawr_cli::events::restarting(&reason);
                        return Ok(Ended::Restart);
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
                                    format!("watcher registration failed: {error}; hot abort also failed: {cleanup}"),
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
                // A new native build restarts a marked `main` as a
                // new session, which builds it.
                rawr_import::process::hot::Preparation::Miss(_) if marked => {
                    drop(staged);
                    let reason = manifests.restart_line(&next_manifests);
                    rawr_import::process::say(format_args!(
                        "rawr: {reason}; restarting the program"
                    ));
                    rawr_cli::events::restarting(&reason);
                    return Ok(Ended::Restart);
                }
                rawr_import::process::hot::Preparation::Miss(miss) => {
                    // D500.4: a new native build restarts the program, and
                    // says why in one line, once for each new build.
                    if (announced.as_ref() != Some(&next_manifests) || manifests.restarts_on_save())
                        && let Some(reason) = manifests.restart_reason(&next_manifests)
                    {
                        rawr_import::process::say(format_args!(
                            "rawr: {reason}; restarting the program"
                        ));
                        rawr_cli::events::restarting(&reason);
                        announced = Some(next_manifests.clone());
                    }
                    let (request, snapshot) = miss.into_parts();
                    request
                        .generate(&limits)
                        .map_err(|error| framework(error, source.clone()))
                        .and_then(|generated| {
                            lane.build(generated)
                                .map_err(|error| framework(error, source.clone()))
                        })
                        .and_then(|mut built| {
                            built
                                .retain_startup(&snapshot)
                                .map_err(|error| framework(error, source.clone()))?;
                            built
                                .revalidate_prepared()
                                .map_err(|error| framework(error, source.clone()))?;
                            lane.replace_built_started_request(
                                built,
                                snapshot,
                                &adapter,
                                &observation,
                                watch_request,
                            )
                            .map_err(|error| framework(error, source.clone()))
                        })
                }
            };
            match admitted {
                Ok(crate::native::Watched::Installed) => {
                    // A marked `main`'s runner admits the generation at
                    // its next call from native code, and says `swapped`
                    // then (D553.3).
                    if swapped && marked {
                        swapped_marked = true;
                        rawr_cli::events::queued();
                    } else if swapped {
                        rawr_cli::events::swapped();
                    } else {
                        rawr_cli::events::running();
                    }
                    manifests = next_manifests;
                    native_baseline = next_native_baseline;
                    native_inputs = None;
                    refresh_required = false;
                    for error in adapter
                        .commit_inputs(staged)
                        .map_err(|error| framework(error, source.clone()))?
                    {
                        rawr_import::process::say(format_args!(
                            "Rawr imported-owner watcher warning: {error}"
                        ));
                    }
                }
                Ok(crate::native::Watched::Obsolete) => drop(staged),
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
                        format!("Rawr imported-owner watcher stopped: {error}"),
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source).map(|()| Ended::Exit(0));
            }
            Ok(rawr_watch::Notice::Schedule(error)) => {
                retain_report(
                    &mut lane,
                    framework(
                        format!("Rawr imported-owner scheduler stopped: {error}"),
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source).map(|()| Ended::Exit(0));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                retain_report(
                    &mut lane,
                    framework(
                        "Rawr imported-owner watcher event channel disconnected",
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source).map(|()| Ended::Exit(0));
            }
        }
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
    } = prepare_on_lane(&mut lane, target, graph, profile, &source)?;
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
    initial_watched_program(target, graph, target_path, lifecycle, profile, None)
}

#[allow(clippy::type_complexity)]
fn initial_watched_program(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    target_path: &Path,
    lifecycle: rawr_import::process::Lifecycle,
    profile: &dyn crate::Profile,
    program: Option<&rawr_import::process::Arguments>,
) -> Result<
    (
        Lane,
        rawr_watch::Adapter,
        resident::Manifests,
        rawr_watch::native::Baseline,
    ),
    diagnostic::Report,
> {
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, program, &source)?;
    let mut initial = prepare_on_lane(&mut lane, target, graph, profile, &source)?;
    let adapter = match rawr_watch::Adapter::open_inputs(initial.inputs.clone(), DEBOUNCE) {
        Ok(adapter) => adapter,
        Err(error) => {
            drop(lane.shutdown());
            return Err(framework(error, source));
        }
    };

    loop {
        let staged = match adapter.prepare_inputs(initial.inputs.clone()) {
            Ok(staged) => staged,
            Err(error) => {
                drop(lane.shutdown());
                return Err(framework(error, source));
            }
        };
        let checkpoint = match adapter.checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                drop(lane.shutdown());
                return Err(framework(error, source));
            }
        };
        let native_baseline = adapter.native_baseline();
        match initial.built.revalidate_prepared() {
            Ok(()) => {}
            Err(
                rawr_import::node::Error::StaleReadSet { .. }
                | rawr_import::node::Error::SourceChanged(_),
            ) => {
                drop(staged);
                initial = refresh_initial(&mut lane, target_path, profile, &source)?;
                continue;
            }
            Err(error) => {
                drop(staged);
                drop(lane.shutdown());
                return Err(framework(error, source));
            }
        }
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
                    rawr_import::process::say(format_args!(
                        "Rawr imported-owner watcher warning: {error}"
                    ));
                }
                return Ok((lane, adapter, initial.manifests, native_baseline));
            }
            Ok(crate::native::Watched::Obsolete) => {
                drop(staged);
                initial = refresh_initial(&mut lane, target_path, profile, &source)?;
            }
            // A save as the program started (D531.9): it starts again from
            // the saved files.
            Err(error) if error.source_changed() => {
                drop(staged);
                rawr_import::process::say(
                    "rawr: the source changed as the program started; starting it again",
                );
                initial = refresh_initial(&mut lane, target_path, profile, &source)?;
            }
            Err(error) => {
                drop(staged);
                drop(lane.shutdown());
                return Err(framework(error, source));
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
            "a native owner requires a discovered Rawr crate",
            source.clone(),
        )
    })?;
    Lane::for_program(project, lifecycle, program.cloned())
        .map_err(|error| framework(error, source.clone()))
}

fn prepare_on_lane(
    lane: &mut Lane,
    target: &rawr::project::Target,
    mut graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
    source: &diagnostic::Source,
) -> Result<Initial, diagnostic::Report> {
    // A save while the runner builds (D531.9): the build is kept, and the
    // program is prepared again from the saved files.
    let mut saved = None;
    loop {
        let target = saved.as_ref().unwrap_or(target);
        let revision = lane
            .revision()
            .map_err(|error| framework(error, source.clone()))?;
        let prepared = match profile.prepare(
            target,
            graph.take(),
            revision,
            &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
        ) {
            Ok(prepared) => prepared,
            Err(crate::Error::Rawr(error)) if error.source_changed() => {
                saved = Some(discover(target)?);
                continue;
            }
            Err(error) => return Err(import(error, source.clone())),
        };
        let manifests = prepared.manifests.clone();
        let limits = prepared.limits();
        let (request, snapshot, _, inputs) = prepared.into_parts();
        let generated = request
            .generate(&limits)
            .map_err(|error| framework(error, source.clone()))?;
        // The API phase has ended; hand its freed allocator pages back before the build.
        crate::release::freed_memory();
        let mut built = lane
            .build(generated)
            .map_err(|error| framework(error, source.clone()))?;
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
            Err(error) => return Err(framework(error, source.clone())),
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
) -> Result<Initial, diagnostic::Report> {
    let target = rawr::project::Target::discover(target_path)
        .map_err(|error| rawr::diagnostic::Report::from_project(&error))?;
    prepare_on_lane(lane, &target, None, profile, source)
}

/// Latest wins: once a newer change is waiting, an older attempt's failure is
/// superseded instead of reported. The newer attempt runs next and reports
/// for itself if the failure persists.
fn settle_failure(
    lane: &mut Lane,
    adapter: &rawr_watch::Adapter,
    report: diagnostic::Report,
) -> Result<(), diagnostic::Report> {
    if adapter.deadline().is_some() {
        return Ok(());
    }
    retain_report(lane, report)
}

fn retain_report(lane: &mut Lane, report: diagnostic::Report) -> Result<(), diagnostic::Report> {
    rawr_cli::events::failed(&report);
    let rendered = diagnostic::render(&report);
    rawr_import::process::say(format_args!(
        "Rawr imported owner kept the last good generation:\n{rendered}"
    ));
    lane.report(&rendered)
        .map_err(|error| framework(error, None))
}

fn wait_for_window(mut lane: Lane, source: diagnostic::Source) -> Result<(), diagnostic::Report> {
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

/// A project that builds as Rawr builds it, or the error naming the files
/// it needs.
pub fn same_build(target: &rawr::project::Target) -> Result<(), diagnostic::Report> {
    let Some(root) = project(target) else {
        return Ok(());
    };
    crate::project::same_build(root).map_err(|message| {
        diagnostic::Report::error(diagnostic::code::ENVIRONMENT_PROJECT_BUILD, message).finish()
    })
}

fn project(target: &rawr::project::Target) -> Option<&Path> {
    target.project().map(|project| project.root().as_path())
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

/// A failure of the runner's machinery, reported against `source`.
pub fn framework(
    error: impl std::fmt::Display,
    source: impl Into<Option<diagnostic::Source>>,
) -> diagnostic::Report {
    crate::report::report(diagnostic::code::FRAMEWORK_NATIVE, error, source.into())
}
