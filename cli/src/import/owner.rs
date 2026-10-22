//! Production CLI ownership for packages whose application declares `[rust]`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rawr::diagnostic;

const DEBOUNCE: Duration = Duration::from_millis(30);
const IDLE_POLL: Duration = Duration::from_millis(25);
const REFRESH_POLL: Duration = Duration::from_millis(250);

type Lane = crate::watch::native::Lane<rawr::package::import::Brand>;
type Built =
    rawr_import::runner::Manifest<rawr_import::runner::Built, rawr::package::import::Brand>;

pub(super) mod resident;

struct Initial {
    built: Built,
    snapshot: rawr_import::supervisor::snapshot::Bytes,
    inputs: rawr::reload::source::Inputs,
    manifests: resident::Manifests,
}

/// Check one imported application without opening a native window.
pub(crate) fn check(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<PathBuf, diagnostic::Report> {
    let source = target.source().path().to_path_buf();
    let fallback = fallback(target)?;
    let (mut lane, _) = initial(
        target,
        graph,
        rawr_import::process::Lifecycle::Check,
        false,
        #[cfg(feature = "import-baseline")]
        profile,
    )?;
    lane.shutdown()
        .map_err(|error| framework(error, fallback))?;
    Ok(source)
}

/// Run one imported application or preview under the generated child owner.
/// `graph` is the target's load when the caller already has it.
pub(crate) fn run(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<(), diagnostic::Report> {
    let source = fallback(target)?;
    let target_path = project(target)
        .ok_or_else(|| framework("a `[rust]` owner requires a Rawr project", source.clone()))?
        .to_path_buf();
    rawr_import::progress::begin(&target_path);
    let initial = initial_watched(
        target,
        graph,
        &target_path,
        lifecycle,
        #[cfg(feature = "import-baseline")]
        profile,
    );
    rawr_import::progress::finish(initial.is_ok());
    let (mut lane, adapter, mut manifests, mut native_baseline) = initial?;

    let mut refresh_at = Instant::now();
    let mut refresh_required = false;
    let mut native_inputs = None;
    loop {
        if lane
            .reap_if_exited()
            .map_err(|error| framework(error, source.clone()))?
        {
            return Ok(());
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
            let prepared = match super::prepare_in(
                &next_target,
                None,
                revision,
                #[cfg(feature = "import-baseline")]
                profile,
                &scope,
            ) {
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
            let preparation =
                match lane.prepare_started_compile(None, request, snapshot, &observation) {
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
                    .map(|_| crate::watch::native::Watched::Installed)
                    .map_err(|error| framework(error, source.clone())),
                rawr_import::process::hot::Preparation::Miss(miss) => {
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
                Ok(crate::watch::native::Watched::Installed) => {
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
                Ok(crate::watch::native::Watched::Obsolete) => drop(staged),
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
            Ok(crate::watch::Notice::Changed(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(crate::watch::Notice::Backend(error)) => {
                retain_report(
                    &mut lane,
                    framework(
                        format!("Rawr imported-owner watcher stopped: {error}"),
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source);
            }
            Ok(crate::watch::Notice::Schedule(error)) => {
                retain_report(
                    &mut lane,
                    framework(
                        format!("Rawr imported-owner scheduler stopped: {error}"),
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                retain_report(
                    &mut lane,
                    framework(
                        "Rawr imported-owner watcher event channel disconnected",
                        source.clone(),
                    ),
                )?;
                return wait_for_window(lane, source);
            }
        }
    }
}

/// Execute one imported snapshot owner and decode its bounded authenticated
/// outcome into the command's ordinary public result.
pub(crate) fn snapshot(
    target: &rawr::project::Target,
    mode: crate::command::Mode,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<rawr_iced::snapshot::Outcome, diagnostic::Report> {
    let lifecycle = match mode {
        crate::command::Mode::Check => rawr_import::process::Lifecycle::SnapshotCheck,
        crate::command::Mode::Update => rawr_import::process::Lifecycle::SnapshotUpdate,
    };
    let source = fallback(target)?;
    let (mut lane, _) = initial(
        target,
        None,
        lifecycle,
        false,
        #[cfg(feature = "import-baseline")]
        profile,
    )?;
    let outcome = lane
        .snapshot()
        .map_err(|error| framework(error, source.clone()));
    let shutdown = lane
        .shutdown()
        .map_err(|error| framework(error, source.clone()));
    let bytes = outcome?;
    shutdown?;
    rawr_iced::snapshot::wire::decode(&bytes).map_err(|error| framework(error, source))
}

/// Consume one request at the same typed owner edge used by ordinary CLI
/// commands, then retire the installed generation before returning its proof.
#[cfg(feature = "import-baseline")]
pub(crate) fn cell(
    target: &rawr::project::Target,
    command: rawr_import::metrics::report::Command,
    profile: Option<rawr_iced::profile::Fixture>,
) -> Result<[u8; 32], diagnostic::Report> {
    let (lifecycle, started) = match command {
        rawr_import::metrics::report::Command::Check => {
            (rawr_import::process::Lifecycle::Check, false)
        }
        rawr_import::metrics::report::Command::Run
        | rawr_import::metrics::report::Command::Watch => {
            (rawr_import::process::Lifecycle::Run, true)
        }
        rawr_import::metrics::report::Command::Preview => {
            (rawr_import::process::Lifecycle::Preview, true)
        }
        rawr_import::metrics::report::Command::Snapshot => {
            (rawr_import::process::Lifecycle::SnapshotUpdate, false)
        }
    };
    let source = fallback(target)?;
    let (mut lane, _watched) = if started {
        let target_path = project(target)
            .ok_or_else(|| framework("a `[rust]` owner requires a Rawr project", source.clone()))?;
        let (lane, adapter, manifests, native_baseline) =
            initial_watched(target, None, target_path, lifecycle, profile)?;
        // Keep the same registered observer and native baseline as the ordinary
        // owner until the installed generation has actually retired.
        (lane, Some((adapter, manifests, native_baseline)))
    } else {
        let (lane, _) = initial(target, None, lifecycle, false, profile)?;
        (lane, None)
    };
    let observed = match command {
        rawr_import::metrics::report::Command::Snapshot => lane
            .snapshot()
            .map_err(|error| framework(error, source.clone()))
            .and_then(|bytes| {
                rawr_iced::snapshot::wire::decode(&bytes)
                    .map(|_| ())
                    .map_err(|error| framework(error, source.clone()))
            }),
        rawr_import::metrics::report::Command::Check
        | rawr_import::metrics::report::Command::Run
        | rawr_import::metrics::report::Command::Watch
        | rawr_import::metrics::report::Command::Preview => Ok(()),
    };
    let generation = lane
        .current_key()
        .map(|key| *key.as_bytes())
        .ok_or_else(|| framework("the imported owner installed no generation", source.clone()));
    let shutdown = lane
        .shutdown()
        .map_err(|error| framework(error, source.clone()));
    observed?;
    let generation = generation?;
    shutdown?;
    Ok(generation)
}

pub(super) fn initial(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    started: bool,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<(Lane, rawr::reload::source::Inputs), diagnostic::Report> {
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, &source)?;
    let Initial {
        built,
        snapshot,
        inputs,
        manifests: _,
    } = prepare_on_lane(
        &mut lane,
        target,
        graph,
        #[cfg(feature = "import-baseline")]
        profile,
        &source,
    )?;
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

fn initial_watched(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    target_path: &Path,
    lifecycle: rawr_import::process::Lifecycle,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<
    (
        Lane,
        crate::watch::Adapter,
        resident::Manifests,
        rawr_watch::native::Baseline,
    ),
    diagnostic::Report,
> {
    let source = fallback(target)?;
    let mut lane = open_lane(target, lifecycle, &source)?;
    let mut initial = prepare_on_lane(
        &mut lane,
        target,
        graph,
        #[cfg(feature = "import-baseline")]
        profile,
        &source,
    )?;
    let adapter = match crate::watch::Adapter::open_inputs(initial.inputs.clone(), DEBOUNCE) {
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
                initial = refresh_initial(
                    &mut lane,
                    target_path,
                    #[cfg(feature = "import-baseline")]
                    profile,
                    &source,
                )?;
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
            Ok(crate::watch::native::Watched::Installed) => {
                for error in adapter
                    .commit_inputs(staged)
                    .map_err(|error| framework(error, source.clone()))?
                {
                    eprintln!("Rawr imported-owner watcher warning: {error}");
                }
                return Ok((lane, adapter, initial.manifests, native_baseline));
            }
            Ok(crate::watch::native::Watched::Obsolete) => {
                drop(staged);
                initial = refresh_initial(
                    &mut lane,
                    target_path,
                    #[cfg(feature = "import-baseline")]
                    profile,
                    &source,
                )?;
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
            "a `[rust]` owner requires a discovered Rawr project",
            source.clone(),
        )
    })?;
    Lane::for_lifecycle(project, lifecycle).map_err(|error| framework(error, source.clone()))
}

fn prepare_on_lane(
    lane: &mut Lane,
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
    source: &diagnostic::Source,
) -> Result<Initial, diagnostic::Report> {
    let revision = lane
        .revision()
        .map_err(|error| framework(error, source.clone()))?;
    let prepared = super::prepare_loaded(
        target,
        graph,
        revision,
        #[cfg(feature = "import-baseline")]
        profile,
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
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
    source: &diagnostic::Source,
) -> Result<Initial, diagnostic::Report> {
    let target = rawr::project::Target::discover(target_path)
        .map_err(|error| rawr::diagnostic::Report::from_project(&error))?;
    prepare_on_lane(
        lane,
        &target,
        None,
        #[cfg(feature = "import-baseline")]
        profile,
        source,
    )
}

/// Latest wins: once a newer change is waiting, an older attempt's failure is
/// superseded instead of reported. The newer attempt runs next and reports
/// for itself if the failure persists.
fn settle_failure(
    lane: &mut Lane,
    adapter: &crate::watch::Adapter,
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

fn project(target: &rawr::project::Target) -> Option<&Path> {
    target.project().map(|project| project.root().as_path())
}

fn fallback(target: &rawr::project::Target) -> Result<diagnostic::Source, diagnostic::Report> {
    diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| framework(error, None))
}

fn import(error: super::Error, source: diagnostic::Source) -> diagnostic::Report {
    match error {
        super::Error::Rawr(error) => crate::watch::package_report(&error, source),
        error => framework(error, source),
    }
}

fn framework(
    error: impl std::fmt::Display,
    source: impl Into<Option<diagnostic::Source>>,
) -> diagnostic::Report {
    crate::watch::report(diagnostic::code::FRAMEWORK_NATIVE, error, source.into())
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(feature = "import-baseline")]
    use std::path::{Path, PathBuf};
    #[cfg(feature = "import-baseline")]
    use std::process::Command;
    #[cfg(feature = "import-baseline")]
    use std::time::Instant;

    use rawr_import::process::hot::Outcome;

    #[cfg(feature = "import-baseline")]
    const LIFECYCLE_WORKER_MODE: &str = "RAWR_REAL_ICED_LIFECYCLE_WORKER";
    #[cfg(feature = "import-baseline")]
    const LIFECYCLE_WORKER_PROJECT: &str = "RAWR_REAL_ICED_LIFECYCLE_PROJECT";
    #[cfg(feature = "import-baseline")]
    const LIFECYCLE_WORKER_REPORT: &str = "RAWR_REAL_ICED_LIFECYCLE_REPORT";

    #[cfg(feature = "import-baseline")]
    #[derive(serde::Deserialize, serde::Serialize)]
    struct LifecycleReport {
        elapsed_ms: u64,
        counters: rawr_import::metrics::recorder::Snapshot,
    }

    #[test]
    #[ignore = "production cutover gate starts one real run owner and admits watched recovery"]
    fn production_run_and_watch_preserve_one_real_generated_last_good_owner() {
        let project = crate::import::tests::real_iced_fixture();
        let root = project.path().to_path_buf();
        let source = root.join("src/main.rawr");
        let original = fs::read_to_string(&source).expect("fixture source reads");
        let target = rawr::project::Target::discover(&root).expect("run target discovers");
        let (mut lane, _) = super::initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Run,
            true,
            #[cfg(feature = "import-baseline")]
            None,
        )
        .unwrap_or_else(|error| panic!("production imported run rejects: {error}"));
        let initial = lane.current_key().expect("run owner is current");

        fs::write(
            &source,
            original.replace("text(\"generated\")", "text(\"reloaded\")"),
        )
        .expect("valid watched source writes");
        let reloaded = hot_reload(&mut lane, &root).expect("valid watched source admits");
        assert_ne!(reloaded, initial);
        assert_eq!(lane.current_key(), Some(reloaded));

        fs::write(&source, "pub fn main( {").expect("invalid watched source writes");
        let revision = lane.revision().expect("invalid revision allocates");
        let invalid = rawr::project::Target::discover(&root).expect("invalid target discovers");
        assert!(crate::import::prepare(
            &invalid,
            revision,
            #[cfg(feature = "import-baseline")]
            None,
        )
        .is_err());
        assert_eq!(lane.current_key(), Some(reloaded));
        lane.report("injected invalid watched generation")
            .expect("last-good owner accepts the bounded failure");

        fs::write(
            &source,
            original.replace("text(\"generated\")", "text(\"recovered\")"),
        )
        .expect("recovered watched source writes");
        let recovered = hot_reload(&mut lane, &root).expect("recovered watched source admits");
        assert_ne!(recovered, reloaded);
        assert_eq!(lane.current_key(), Some(recovered));
        lane.shutdown().expect("run owner exits and reaps");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "production cutover gate starts and retires one real preview owner"]
    fn production_preview_starts_and_retires_one_real_generated_owner() {
        let project = crate::import::tests::real_iced_fixture();
        let root = project.path().to_path_buf();
        let target = rawr::project::Target::discover(&root).expect("preview target discovers");
        let (mut preview, _) = super::initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Preview,
            true,
            #[cfg(feature = "import-baseline")]
            None,
        )
        .unwrap_or_else(|error| panic!("production imported preview rejects: {error}"));
        assert!(preview.current_key().is_some());
        preview.shutdown().expect("preview owner exits and reaps");
        fs::remove_dir_all(root).ok();
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn real_iced_lifecycle_worker() {
        if std::env::var_os(LIFECYCLE_WORKER_MODE).is_none() {
            return;
        }
        let project = PathBuf::from(
            std::env::var_os(LIFECYCLE_WORKER_PROJECT)
                .expect("the real iced lifecycle project is configured"),
        );
        let report_path = PathBuf::from(
            std::env::var_os(LIFECYCLE_WORKER_REPORT)
                .expect("the real iced lifecycle report is configured"),
        );
        let recorder = rawr_import::metrics::recorder::Recorder::transform(64)
            .expect("the real iced lifecycle recorder opens");
        let started = Instant::now();
        {
            let _scope = recorder
                .scope()
                .expect("the real iced lifecycle evidence scope opens");
            let target = rawr::project::Target::discover(&project)
                .expect("the real iced lifecycle target discovers");
            let (mut lane, _) = super::initial(
                &target,
                None,
                rawr_import::process::Lifecycle::Run,
                true,
                None,
            )
            .unwrap_or_else(|error| panic!("the real iced lifecycle rejects: {error}"));
            assert!(lane.current_key().is_some());
            lane.shutdown()
                .expect("the real iced lifecycle owner exits and reaps");
        }
        let report = LifecycleReport {
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            counters: recorder
                .snapshot()
                .expect("the real iced lifecycle evidence seals"),
        };
        fs::write(
            &report_path,
            serde_json::to_vec(&report).expect("the real iced lifecycle report encodes"),
        )
        .expect("the real iced lifecycle report writes");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "single frozen milestone proves fresh-process real iced Started reuse"]
    fn identical_fresh_real_iced_lifecycle_hits_the_dag_and_starts_without_cargo() {
        let project = crate::import::tests::real_iced_fixture();
        let root = project.path().to_path_buf();
        let first_path = root.with_extension("real-iced-first.json");
        let second_path = root.with_extension("real-iced-second.json");
        run_lifecycle_worker(&root, &first_path);
        let first = read_lifecycle_report(&first_path);
        assert_eq!(
            milestone(&first, rawr_import::metrics::Milestone::Started),
            1
        );

        for path in [
            root.join(".rawr/cache/runners"),
            root.join(".rawr/cache/resolution/authoritative-target"),
        ] {
            if path.exists() {
                fs::remove_dir_all(&path)
                    .unwrap_or_else(|error| panic!("retire {}: {error}", path.display()));
            }
        }

        run_lifecycle_worker(&root, &second_path);
        let second = read_lifecycle_report(&second_path);
        for kind in [
            rawr_import::metrics::NodeKind::Toolchain,
            rawr_import::metrics::NodeKind::Resolution,
            rawr_import::metrics::NodeKind::Api,
            rawr_import::metrics::NodeKind::Generic,
            rawr_import::metrics::NodeKind::Profile,
            rawr_import::metrics::NodeKind::Facade,
            rawr_import::metrics::NodeKind::Checked,
            rawr_import::metrics::NodeKind::ProfileProof,
            rawr_import::metrics::NodeKind::NativeProduct,
            rawr_import::metrics::NodeKind::Overlay,
            rawr_import::metrics::NodeKind::Runner,
        ] {
            assert!(
                node(&second, kind, rawr_import::metrics::NodeEvent::ValidatedHit,) > 0,
                "the warm real iced `{kind:?}` node must validate"
            );
            assert_eq!(
                node(&second, kind, rawr_import::metrics::NodeEvent::ExecutedMiss,),
                0,
                "the warm real iced `{kind:?}` transform must not execute"
            );
        }
        for kind in [
            rawr_import::metrics::ProcessKind::RustupWhich,
            rawr_import::metrics::ProcessKind::RustcVersion,
            rawr_import::metrics::ProcessKind::RustdocVersion,
            rawr_import::metrics::ProcessKind::CargoVersion,
            rawr_import::metrics::ProcessKind::DriverVersion,
            rawr_import::metrics::ProcessKind::ComponentList,
            rawr_import::metrics::ProcessKind::TargetList,
            rawr_import::metrics::ProcessKind::CargoResolution,
            rawr_import::metrics::ProcessKind::PackageList,
            rawr_import::metrics::ProcessKind::MetadataProbe,
            rawr_import::metrics::ProcessKind::CargoRunner,
        ] {
            assert_eq!(
                process(&second, kind),
                0,
                "the warm real iced launch must not spawn `{kind:?}`"
            );
        }
        assert_eq!(
            process(&second, rawr_import::metrics::ProcessKind::Runner),
            1
        );
        assert_eq!(
            milestone(&second, rawr_import::metrics::Milestone::Started),
            1
        );
        assert!(second.counters.runner.is_some());

        let cold_started_ms =
            milestone_elapsed_ms(&first, rawr_import::metrics::Milestone::Started);
        let warm_started_ms =
            milestone_elapsed_ms(&second, rawr_import::metrics::Milestone::Started);
        assert!(warm_started_ms < cold_started_ms);
        eprintln!(
            "REAL_ICED_WARM_LIFECYCLE cold_started_ms={cold_started_ms} warm_started_ms={warm_started_ms} saved_ms={} cold_total_ms={} warm_total_ms={} warm_transforms=0 warm_cargo_children=0 warm_runner_processes=1 warm_started=1",
            cold_started_ms.saturating_sub(warm_started_ms),
            first.elapsed_ms,
            second.elapsed_ms,
        );
        fs::remove_file(first_path).expect("the cold lifecycle report retires");
        fs::remove_file(second_path).expect("the warm lifecycle report retires");
        fs::remove_dir_all(root).ok();
    }

    #[cfg(feature = "import-baseline")]
    fn run_lifecycle_worker(project: &Path, report: &Path) {
        let status = Command::new(
            std::env::current_exe().expect("the real iced lifecycle test executable resolves"),
        )
        .arg("--exact")
        .arg("import::owner::tests::real_iced_lifecycle_worker")
        .arg("--nocapture")
        .env(LIFECYCLE_WORKER_MODE, "1")
        .env(LIFECYCLE_WORKER_PROJECT, project)
        .env(LIFECYCLE_WORKER_REPORT, report)
        .status()
        .expect("the real iced lifecycle worker launches");
        assert!(status.success(), "the real iced lifecycle worker must pass");
    }

    #[cfg(feature = "import-baseline")]
    fn read_lifecycle_report(path: &Path) -> LifecycleReport {
        serde_json::from_slice(&fs::read(path).expect("the lifecycle report reads"))
            .expect("the lifecycle report decodes")
    }

    #[cfg(feature = "import-baseline")]
    fn node(
        report: &LifecycleReport,
        kind: rawr_import::metrics::NodeKind,
        event: rawr_import::metrics::NodeEvent,
    ) -> u64 {
        report
            .counters
            .nodes
            .iter()
            .find(|row| row.kind == kind && row.event == event)
            .map_or(0, |row| row.count)
    }

    #[cfg(feature = "import-baseline")]
    fn process(report: &LifecycleReport, kind: rawr_import::metrics::ProcessKind) -> u64 {
        report
            .counters
            .processes
            .iter()
            .find(|row| row.kind == kind)
            .map_or(0, |row| row.count)
    }

    #[cfg(feature = "import-baseline")]
    fn milestone(report: &LifecycleReport, milestone: rawr_import::metrics::Milestone) -> u64 {
        report
            .counters
            .milestones
            .iter()
            .find(|row| row.milestone == milestone)
            .map_or(0, |row| row.count)
    }

    #[cfg(feature = "import-baseline")]
    fn milestone_elapsed_ms(
        report: &LifecycleReport,
        milestone: rawr_import::metrics::Milestone,
    ) -> u64 {
        report
            .counters
            .milestones
            .iter()
            .find(|row| row.milestone == milestone)
            .map_or(0, |row| row.last_elapsed_ns / 1_000_000)
    }

    fn hot_reload(
        lane: &mut super::Lane,
        root: &std::path::Path,
    ) -> Result<rawr_import::supervisor::GenerationKey, Box<dyn std::error::Error>> {
        let revision = lane.revision()?;
        let target = rawr::project::Target::discover(root)?;
        let prepared = crate::import::prepare(
            &target,
            revision,
            #[cfg(feature = "import-baseline")]
            None,
        )?;
        let (request, snapshot, _, _) = prepared.into_parts();
        match lane.compile(request, snapshot)? {
            Outcome::Hit(key) => Ok(key),
            Outcome::Miss(_) => {
                Err("same imported surface unexpectedly required a cold owner".into())
            }
        }
    }
}
