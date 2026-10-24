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
    same_build(target)?;
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
    rawr_import::progress::begin(&target_path);
    let initial = initial_watched(target, graph, &target_path, lifecycle, profile);
    rawr_import::progress::finish(initial.is_ok());
    let (mut lane, adapter, mut manifests, mut native_baseline) = initial?;

    let mut refresh_at = Instant::now();
    let mut refresh_required = false;
    // The preparation a restart was last announced for.
    let mut announced = None;
    let mut native_inputs = None;
    loop {
        match lane.reap_if_exited() {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(crate::native::Error::Process(rawr_import::process::RunnerError::Exit(Some(
                _,
            )))) if lifecycle == rawr_import::process::Lifecycle::Run => return Ok(false),
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
            let started = if next_manifests.has_islands() {
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
            let admitted = match preparation {
                rawr_import::process::hot::Preparation::Prepared(prepared) => lane
                    .revalidate_compile(prepared)
                    // Child callbacks revoke the retained request on the exact
                    // UI gate. Parent notices only schedule work; never hold
                    // the parent gate through validation, pinning or hot IPC.
                    .and_then(|prepared| lane.commit_compile(prepared))
                    .map(|_| crate::native::Watched::Installed)
                    .map_err(|error| framework(error, source.clone())),
                rawr_import::process::hot::Preparation::Miss(miss) => {
                    // D500.4: a new native build restarts the program, and
                    // says why in one line, once for each new build.
                    if announced.as_ref() != Some(&next_manifests)
                        && let Some(reason) = manifests.restart_reason(&next_manifests)
                    {
                        eprintln!("rawr: {reason}; restarting the program");
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
                    manifests = next_manifests;
                    native_baseline = next_native_baseline;
                    native_inputs = None;
                    refresh_required = false;
                    for error in adapter
                        .commit_inputs(staged)
                        .map_err(|error| framework(error, source.clone()))?
                    {
                        eprintln!("Rawr imported-owner watcher warning: {error}");
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
                return wait_for_window(lane, source).map(|()| true);
            }
            Ok(rawr_watch::Notice::Schedule(error)) => {
                retain_report(
                    &mut lane,
                    framework(
                        format!("Rawr imported-owner scheduler stopped: {error}"),
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source).map(|()| true);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                retain_report(
                    &mut lane,
                    framework(
                        "Rawr imported-owner watcher event channel disconnected",
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source).map(|()| true);
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
    let mut lane = open_lane(target, lifecycle, &source)?;
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
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, &source)?;
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
            Err(rawr_import::node::Error::StaleReadSet { .. }) => {
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
                    eprintln!("Rawr imported-owner watcher warning: {error}");
                }
                return Ok((lane, adapter, initial.manifests, native_baseline));
            }
            Ok(crate::native::Watched::Obsolete) => {
                drop(staged);
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
    source: &diagnostic::Source,
) -> Result<Lane, diagnostic::Report> {
    let project = project(target).ok_or_else(|| {
        framework(
            "a native owner requires a discovered Rawr crate",
            source.clone(),
        )
    })?;
    Lane::for_lifecycle(project, lifecycle).map_err(|error| framework(error, source.clone()))
}

fn prepare_on_lane(
    lane: &mut Lane,
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    profile: &dyn crate::Profile,
    source: &diagnostic::Source,
) -> Result<Initial, diagnostic::Report> {
    let revision = lane
        .revision()
        .map_err(|error| framework(error, source.clone()))?;
    let prepared = profile
        .prepare(
            target,
            graph,
            revision,
            &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
        )
        .map_err(|error| import(error, source.clone()))?;
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
    built
        .retain_startup(&snapshot)
        .map_err(|error| framework(error, source.clone()))?;
    Ok(Initial {
        built,
        snapshot,
        inputs,
        manifests,
    })
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
    let rendered = diagnostic::render(&report);
    eprintln!("Rawr imported owner kept the last good generation:\n{rendered}");
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
