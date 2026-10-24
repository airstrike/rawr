//! rawr-iced's programs under the runner owner rawr-import provides: the
//! owner prepares each through the iced [`Iced`] profile. Snapshots and the
//! private evidence cells are rawr-iced's own.

use rawr::diagnostic;
#[cfg(feature = "import-baseline")]
use rawr_import_cli::owner::initial_watched;
use rawr_import_cli::owner::{fallback, framework, initial, same_build};

use super::Iced;

#[cfg(test)]
type Lane = rawr_import_cli::owner::Lane;

/// Check one imported iced program without opening a window: its Rust
/// imports are resolved and extracted and the Rawr source is checked
/// against them. No runner is built; `rawr run` builds it.
pub(crate) fn check(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<std::path::PathBuf, diagnostic::Report> {
    rawr_import_cli::owner::check(
        target,
        graph,
        &Iced::new(
            #[cfg(feature = "import-baseline")]
            profile,
        ),
    )
}

/// Run one imported iced application or preview under the runner owner.
/// `Ok(false)` is a program that ended with a failing exit: it printed its
/// own error as it ran.
pub(crate) fn run(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<bool, diagnostic::Report> {
    rawr_import_cli::owner::run(
        target,
        graph,
        lifecycle,
        &Iced::new(
            #[cfg(feature = "import-baseline")]
            profile,
        ),
    )
}

/// Execute one imported snapshot owner and decode its bounded authenticated
/// outcome into the command's ordinary public result.
pub(crate) fn snapshot(
    target: &rawr::project::Target,
    mode: crate::command::Mode,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<rawr_iced::snapshot::Outcome, diagnostic::Report> {
    same_build(target)?;
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
        &Iced::new(
            #[cfg(feature = "import-baseline")]
            profile,
        ),
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
        let target_path = target
            .project()
            .map(|project| project.root().as_path())
            .ok_or_else(|| framework("a native owner requires a Rawr crate", source.clone()))?;
        let (lane, adapter, manifests, native_baseline) =
            initial_watched(target, None, target_path, lifecycle, &Iced::new(profile))?;
        // Keep the same registered observer and native baseline as the ordinary
        // owner until the installed generation has actually retired.
        (lane, Some((adapter, manifests, native_baseline)))
    } else {
        let (lane, _) = initial(target, None, lifecycle, false, &Iced::new(profile))?;
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

#[cfg(test)]
mod tests {
    use super::{initial, Iced};
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
        let source = root.join("src/main.rs");
        let original = fs::read_to_string(&source).expect("fixture source reads");
        let target = rawr::project::Target::discover(&root).expect("run target discovers");
        let (mut lane, _) = initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Run,
            true,
            &Iced::new(
                #[cfg(feature = "import-baseline")]
                None,
            ),
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
        let (mut preview, _) = initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Preview,
            true,
            &Iced::new(
                #[cfg(feature = "import-baseline")]
                None,
            ),
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
            let (mut lane, _) = initial(
                &target,
                None,
                rawr_import::process::Lifecycle::Run,
                true,
                &Iced::new(None),
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
            root.join("target/rawr/cache/runners"),
            root.join("target/rawr/cache/resolution/authoritative-target"),
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
