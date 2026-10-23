use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};

const WORKER_MODE: &str = "RAWR_RUNNER_DAG_WORKER_MODE";
const WORKER_PROJECT: &str = "RAWR_RUNNER_DAG_WORKER_PROJECT";
const WORKER_REPORT: &str = "RAWR_RUNNER_DAG_WORKER_REPORT";
const WORKER_SNAPSHOT: &str = "RAWR_RUNNER_DAG_WORKER_SNAPSHOT";
const WORKER_START: &str = "RAWR_RUNNER_DAG_WORKER_START";

#[derive(Clone, Copy)]
struct LifecycleBrand;

impl rawr_import::surface::Runner for LifecycleBrand {
    fn compile_snapshot(&self) -> bool {
        true
    }
}

#[derive(Default)]
struct Counts {
    overlay_hits: AtomicUsize,
    overlay_misses: AtomicUsize,
    runner_hits: AtomicUsize,
    runner_misses: AtomicUsize,
    runner_published: AtomicUsize,
    cargo_runner: AtomicUsize,
    runner_process: AtomicUsize,
    admitted: AtomicUsize,
    started: AtomicUsize,
}

#[derive(Debug, Eq, PartialEq)]
struct StoreEntry {
    path: PathBuf,
    kind: u8,
    bytes: u64,
    mode: u32,
    modified_ns: u128,
    digest: [u8; 32],
}

fn store_snapshot(root: &std::path::Path) -> Vec<StoreEntry> {
    let mut pending = vec![root.to_path_buf()];
    let mut observed = Vec::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).expect("the shared-store member inspects");
        assert!(
            !metadata.file_type().is_symlink(),
            "the shared store contains no symlink"
        );
        let relative = path
            .strip_prefix(root)
            .expect("the shared-store member stays beneath its root")
            .to_path_buf();
        let modified_ns = metadata
            .modified()
            .expect("the shared-store member modification time reads")
            .duration_since(UNIX_EPOCH)
            .expect("the shared-store member modification time follows the epoch")
            .as_nanos();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::MetadataExt as _;
            metadata.mode()
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        let (kind, digest) = if metadata.is_dir() {
            let mut children = fs::read_dir(&path)
                .expect("the shared-store directory reads")
                .map(|entry| {
                    entry
                        .expect("the shared-store directory entry reads")
                        .path()
                })
                .collect::<Vec<_>>();
            children.sort_unstable();
            children.reverse();
            pending.extend(children);
            (1, [0; 32])
        } else {
            assert!(metadata.is_file(), "the shared store contains only files");
            let bytes = fs::read(&path).expect("the shared-store file reads");
            (2, Sha256::digest(bytes).into())
        };
        observed.push(StoreEntry {
            path: relative,
            kind,
            bytes: metadata.len(),
            mode,
            modified_ns,
            digest,
        });
    }
    observed.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    observed
}

impl rawr_import::metrics::Sink for Counts {
    fn node(&self, kind: rawr_import::metrics::NodeKind, event: rawr_import::metrics::NodeEvent) {
        match (kind, event) {
            (
                rawr_import::metrics::NodeKind::Overlay,
                rawr_import::metrics::NodeEvent::ValidatedHit,
            ) => &self.overlay_hits,
            (
                rawr_import::metrics::NodeKind::Overlay,
                rawr_import::metrics::NodeEvent::ExecutedMiss,
            ) => &self.overlay_misses,
            (
                rawr_import::metrics::NodeKind::Runner,
                rawr_import::metrics::NodeEvent::ValidatedHit,
            ) => &self.runner_hits,
            (
                rawr_import::metrics::NodeKind::Runner,
                rawr_import::metrics::NodeEvent::ExecutedMiss,
            ) => &self.runner_misses,
            (
                rawr_import::metrics::NodeKind::Runner,
                rawr_import::metrics::NodeEvent::Published,
            ) => &self.runner_published,
            _ => return,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    fn process(&self, kind: rawr_import::metrics::ProcessKind) {
        if kind == rawr_import::metrics::ProcessKind::CargoRunner {
            self.cargo_runner.fetch_add(1, Ordering::Relaxed);
        }
        if kind == rawr_import::metrics::ProcessKind::Runner {
            self.runner_process.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn milestone(&self, milestone: rawr_import::metrics::Milestone, _observed_ns: Option<u64>) {
        if milestone == rawr_import::metrics::Milestone::Admitted {
            self.admitted.fetch_add(1, Ordering::Relaxed);
        }
        if milestone == rawr_import::metrics::Milestone::Started {
            self.started.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn runner_owned(&self, _owned: &rawr_import::metrics::runner::Owned) {}

    #[cfg(feature = "import-baseline")]
    fn profile(&self, _evidence: &rawr_import::metrics::profile::Evidence) {}

    #[cfg(feature = "import-baseline")]
    fn pipeline(&self, _evidence: &rawr_import::metrics::pipeline::Evidence) {}

    fn scope_opened(&self) {}

    fn scope_joined(&self) {}
}

fn context(project: &std::path::Path) -> rawr_import::context::Context {
    let cargo_home = project.join("cargo-home");
    fs::create_dir_all(&cargo_home).expect("the runner DAG Cargo home creates");
    seed_offline_cargo_sources(&cargo_home);
    let mut environment = rawr_import::cargo::config::Environment::capture()
        .expect("the runner DAG environment captures");
    for control in [
        WORKER_MODE,
        WORKER_PROJECT,
        WORKER_REPORT,
        WORKER_SNAPSHOT,
        WORKER_START,
    ] {
        environment = environment
            .removing(control)
            .expect("the runner DAG control is excluded from product authority");
    }
    let environment = environment
        .setting(
            "CARGO_HOME",
            cargo_home
                .to_str()
                .expect("the runner DAG Cargo home is Unicode"),
        )
        .expect("the runner DAG Cargo home seals");
    rawr_import::context::Context::capture(
        project,
        environment,
        "aarch64-apple-darwin",
        &rawr_import::cargo::Limits::standard(),
    )
    .expect("the runner DAG context seals")
}

#[cfg(unix)]
fn seed_offline_cargo_sources(cargo_home: &std::path::Path) {
    use std::os::unix::fs::symlink;

    let source_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .expect("the runner DAG source Cargo home resolves");
    for name in ["git", "registry"] {
        let source = source_home.join(name);
        if !source.is_dir() {
            continue;
        }
        let target = cargo_home.join(name);
        match symlink(&source, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("the runner DAG offline {name} source links: {error}"),
        }
    }
}

#[cfg(not(unix))]
fn seed_offline_cargo_sources(_cargo_home: &std::path::Path) {}

fn retire_shared_store(project: &std::path::Path) {
    context(project)
        .open_store()
        .expect("the shared runner store opens for fixture retirement")
        .retire_fixture()
        .expect("the immutable shared runner store retires as one fixture");
}

fn prepare_runner_harness(project: &std::path::Path) {
    let manifest = rawr_import::bridge::fixture_runner_harness(project)
        .expect("the runner DAG harness fixture materializes");
    let status = Command::new("cargo")
        .arg("+nightly-2026-07-11")
        .arg("generate-lockfile")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .status()
        .expect("the runner DAG fixture lock generation launches");
    assert!(status.success(), "the runner DAG fixture lock must seal");
}

fn execute_runner(project: &std::path::Path, snapshot: u8, start: bool) -> (u128, Counts) {
    let chain = rawr_import::node::Chain::fixture(context(project))
        .expect("the runner DAG fixture parent validates");
    let cache = rawr_import::cache::Writer::new(project).expect("the runner cache binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let counts = Arc::new(Counts::default());
    let scope = rawr_import::metrics::Scope::transform_at(
        Arc::clone(&counts) as Arc<dyn rawr_import::metrics::Sink>,
        1,
    )
    .expect("the runner DAG evidence scope opens");
    let started = Instant::now();
    if start {
        let (snapshot, transport) = lifecycle_snapshot(project);
        let request = rawr_import::bridge::fixture_lifecycle_generation(
            chain,
            project,
            LifecycleBrand,
            snapshot,
            supervisor.revision().expect("the runner revision seals"),
            &rawr_import::cargo::Limits::standard(),
        )
        .expect("the lifecycle-owner fixture request seals");
        execute_request(request, transport, project, &cache, &mut supervisor, true);
    } else {
        let (snapshot, transport) = runner_snapshot(project, snapshot);
        let request = rawr_import::bridge::fixture_generation(
            chain,
            project,
            snapshot,
            supervisor.revision().expect("the runner revision seals"),
            &rawr_import::cargo::Limits::standard(),
        )
        .expect("the generated-owner fixture request seals");
        execute_request(request, transport, project, &cache, &mut supervisor, false);
    }
    let elapsed = started.elapsed().as_millis();
    drop(scope);
    let counts = Arc::into_inner(counts).expect("the runner count scope retires");
    (elapsed, counts)
}

fn execute_request<B: rawr_import::surface::Runner>(
    request: rawr_import::bridge::GenerationRequestKey<B>,
    transport: rawr_import::supervisor::snapshot::Bytes,
    project: &std::path::Path,
    cache: &rawr_import::cache::Writer,
    supervisor: &mut rawr_import::supervisor::Owner,
    start: bool,
) {
    let manifest = request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the generated-owner fixture materializes");
    let built = manifest
        .build_on_demand(cache)
        .expect("the runner artifact seals or restores");
    let process = rawr_import::process::Owner::for_lifecycle(
        project,
        if start {
            rawr_import::process::Lifecycle::Run
        } else {
            rawr_import::process::Lifecycle::Check
        },
    )
    .expect("the generated-owner process socket seals");
    let mut admitted = supervisor
        .bootstrap(built, transport, &process)
        .expect("the generated owner reaches admitted state");
    if start {
        admitted
            .start()
            .expect("the synthetic lifecycle reaches started state");
    }
    admitted
        .shutdown()
        .expect("the generated owner exits and reaps");
}

fn write_worker_report(path: &std::path::Path, elapsed_ms: u128, counts: &Counts) {
    let report = format!(
        "elapsed_ms={elapsed_ms}\noverlay_hits={}\noverlay_misses={}\nhits={}\nmisses={}\npublished={}\ncargo={}\nrunner_process={}\nadmitted={}\nstarted={}\n",
        counts.overlay_hits.load(Ordering::Relaxed),
        counts.overlay_misses.load(Ordering::Relaxed),
        counts.runner_hits.load(Ordering::Relaxed),
        counts.runner_misses.load(Ordering::Relaxed),
        counts.runner_published.load(Ordering::Relaxed),
        counts.cargo_runner.load(Ordering::Relaxed),
        counts.runner_process.load(Ordering::Relaxed),
        counts.admitted.load(Ordering::Relaxed),
        counts.started.load(Ordering::Relaxed),
    );
    fs::write(path, report).expect("the runner worker report writes");
}

fn runner_snapshot(
    project: &std::path::Path,
    value: u8,
) -> (
    rawr_import::surface::Snapshot,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let source = [value];
    let entry = snapshot_file(project, &source);
    let limits = rawr_import::cargo::Limits::standard();
    let mut snapshot = rawr_import::surface::Snapshot::builder(&limits);
    snapshot
        .entry(&entry)
        .expect("the runner snapshot entry is bounded");
    snapshot
        .file(&entry, &source)
        .expect("the runner snapshot source is bounded");
    let snapshot = snapshot.seal().expect("the runner snapshot seals");

    let mut transport = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    transport
        .entry(&entry)
        .expect("the runner transport entry is bounded");
    transport
        .file(&entry, &source)
        .expect("the runner transport source is bounded");
    let transport = transport
        .bind(snapshot, &limits)
        .expect("the runner transport matches its sealed snapshot");
    (snapshot, transport)
}

fn lifecycle_snapshot(
    project: &std::path::Path,
) -> (
    rawr_import::surface::Snapshot,
    rawr_import::supervisor::snapshot::Bytes,
) {
    const SOURCE: &[u8] = b"40 + 2";
    let entry = snapshot_file(project, SOURCE);
    let limits = rawr_import::cargo::Limits::standard();
    let mut snapshot = rawr_import::surface::Snapshot::builder(&limits);
    snapshot
        .entry(&entry)
        .expect("the lifecycle snapshot entry is bounded");
    snapshot
        .file(&entry, SOURCE)
        .expect("the lifecycle source is bounded");
    let snapshot = snapshot.seal().expect("the lifecycle snapshot seals");
    let mut transport = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    transport
        .entry(&entry)
        .expect("the lifecycle transport entry is bounded");
    transport
        .file(&entry, SOURCE)
        .expect("the lifecycle transport source is bounded");
    let transport = transport
        .bind(snapshot, &limits)
        .expect("the lifecycle transport matches its snapshot");
    (snapshot, transport)
}

/// The supervisor revalidates snapshot sources on disk, as the CLI's are.
fn snapshot_file(project: &std::path::Path, source: &[u8]) -> String {
    let path = project.join("application.rs");
    fs::write(&path, source).expect("the snapshot source writes");
    path.to_str()
        .expect("the snapshot path is Unicode")
        .to_owned()
}

fn read_worker_report(path: &std::path::Path) -> std::collections::BTreeMap<String, u128> {
    let bytes = fs::read_to_string(path).expect("the runner worker report reads");
    bytes
        .lines()
        .map(|line| {
            let (key, value) = line
                .split_once('=')
                .expect("the runner worker report line has a separator");
            let value = value
                .parse::<u128>()
                .expect("the runner worker report value is numeric");
            (key.to_owned(), value)
        })
        .collect()
}

fn worker_command(project: &std::path::Path, report: &std::path::Path, snapshot: u8) -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("the runner test executable resolves"));
    command
        .arg("--exact")
        .arg("runner_dag::runner_dag_worker")
        .arg("--nocapture")
        .env(WORKER_MODE, "1")
        .env(WORKER_PROJECT, project)
        .env(WORKER_REPORT, report)
        .env(WORKER_SNAPSHOT, snapshot.to_string());
    command
}

fn run_worker(project: &std::path::Path, report: &std::path::Path, snapshot: u8) {
    let status = worker_command(project, report, snapshot)
        .status()
        .expect("the runner DAG worker launches");
    assert!(status.success(), "the runner DAG worker must succeed");
}

fn run_started_worker(project: &std::path::Path, report: &std::path::Path) {
    let status =
        Command::new(std::env::current_exe().expect("the runner test executable resolves"))
            .arg("--exact")
            .arg("runner_dag::runner_dag_worker")
            .arg("--nocapture")
            .env(WORKER_MODE, "1")
            .env(WORKER_START, "1")
            .env(WORKER_PROJECT, project)
            .env(WORKER_REPORT, report)
            .env(WORKER_SNAPSHOT, "0")
            .status()
            .expect("the started runner DAG worker launches");
    assert!(
        status.success(),
        "the started runner DAG worker must succeed"
    );
}

#[test]
fn runner_dag_worker() {
    let _turn = rawr_import::store::test_turn();
    if std::env::var_os(WORKER_MODE).is_none() {
        return;
    }
    let project =
        PathBuf::from(std::env::var_os(WORKER_PROJECT).expect("the worker project is configured"));
    let report =
        PathBuf::from(std::env::var_os(WORKER_REPORT).expect("the worker report is configured"));
    let snapshot = std::env::var(WORKER_SNAPSHOT)
        .expect("the worker snapshot is configured")
        .parse::<u8>()
        .expect("the worker snapshot is numeric");
    let (elapsed_ms, counts) =
        execute_runner(&project, snapshot, std::env::var_os(WORKER_START).is_some());
    write_worker_report(&report, elapsed_ms, &counts);
}

#[test]
fn same_key_fresh_processes_allocate_compile_and_publish_one_shared_bundle() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-race-dag");
    let project = guard.path().to_path_buf();
    prepare_runner_harness(&project);
    let first_report = project.join("first-race.report");
    let second_report = project.join("second-race.report");
    let mut first = worker_command(&project, &first_report, 1)
        .spawn()
        .expect("the first runner contender launches");
    let mut second = worker_command(&project, &second_report, 1)
        .spawn()
        .expect("the second runner contender launches");
    assert!(
        first
            .wait()
            .expect("the first runner contender reaps")
            .success(),
        "the first runner contender succeeds",
    );
    assert!(
        second
            .wait()
            .expect("the second runner contender reaps")
            .success(),
        "the second runner contender succeeds",
    );

    let reports = [
        read_worker_report(&first_report),
        read_worker_report(&second_report),
    ];
    let aggregate = |field: &str| {
        reports
            .iter()
            .map(|report| report.get(field).copied().unwrap_or_default())
            .sum::<u128>()
    };
    assert_eq!(aggregate("overlay_misses"), 1);
    assert_eq!(aggregate("overlay_hits"), 1);
    assert_eq!(aggregate("misses"), 1);
    assert_eq!(aggregate("hits"), 1);
    assert_eq!(aggregate("published"), 1);
    assert_eq!(aggregate("cargo"), 1);
    assert_eq!(aggregate("runner_process"), 2);
    assert_eq!(aggregate("admitted"), 2);

    let store = context(&project)
        .open_store()
        .expect("the runner race store opens");
    let sealed = store_snapshot(store.root());
    assert_eq!(
        sealed
            .iter()
            .filter(
                |entry| entry.path.ends_with("allocation") && entry.path.starts_with("executions")
            )
            .count(),
        1,
        "the two contenders publish one immutable Cargo bundle",
    );
    store
        .retire_fixture()
        .expect("the runner race store retires");
}

#[test]
fn identical_fresh_lifecycle_reuses_and_starts_without_transform_or_cargo_children() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-started-dag");
    let project = guard.path().to_path_buf();
    prepare_runner_harness(&project);
    let first_report = project.join("first-started.report");
    let second_report = project.join("second-started.report");

    run_started_worker(&project, &first_report);
    let first = read_worker_report(&first_report);
    assert_eq!(first.get("overlay_misses"), Some(&1));
    assert_eq!(first.get("misses"), Some(&1));
    assert_eq!(first.get("cargo"), Some(&1));
    assert_eq!(first.get("runner_process"), Some(&1));
    assert_eq!(first.get("admitted"), Some(&1));
    assert_eq!(first.get("started"), Some(&1));

    let cache = rawr_import::cache::Writer::new(&project).expect("the runner cache rebinds");
    assert!(
        cache
            .inspect()
            .expect("the legacy started-runner cache inspects")
            .is_empty(),
        "the shared cold build must not publish project-local authority",
    );

    run_started_worker(&project, &second_report);
    let second = read_worker_report(&second_report);
    assert_eq!(second.get("overlay_hits"), Some(&1));
    assert_eq!(second.get("overlay_misses"), Some(&0));
    assert_eq!(second.get("hits"), Some(&1));
    assert_eq!(second.get("misses"), Some(&0));
    assert_eq!(second.get("published"), Some(&0));
    assert_eq!(second.get("cargo"), Some(&0));
    assert_eq!(second.get("runner_process"), Some(&1));
    assert_eq!(second.get("admitted"), Some(&1));
    assert_eq!(second.get("started"), Some(&1));

    let cold_ms = *first
        .get("elapsed_ms")
        .expect("the cold Started time is reported");
    let warm_ms = *second
        .get("elapsed_ms")
        .expect("the warm Started time is reported");
    let saved_ms = cold_ms.saturating_sub(warm_ms);
    let reduction = if cold_ms == 0 {
        0.0
    } else {
        saved_ms as f64 * 100.0 / cold_ms as f64
    };
    eprintln!(
        "FRESH_LIFECYCLE_DAG_STARTED cold_ms={cold_ms} warm_ms={warm_ms} saved_ms={saved_ms} reduction_percent={reduction:.1} warm_overlay_hits=1 warm_runner_hits=1 warm_overlay_transforms=0 warm_runner_transforms=0 warm_cargo=0 warm_owner_processes=1 warm_admitted=1 warm_started=1",
    );
    assert!(warm_ms < cold_ms, "the warm Started path must be faster");

    retire_shared_store(&project);
}

#[test]
fn identical_fresh_runner_process_reuses_without_transform_or_cargo_children() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-fresh-dag");
    let project = guard.path().to_path_buf();
    prepare_runner_harness(&project);
    let source_tree = store_snapshot(&project.join("runner-harness"));
    let first_report = project.join("first.report");
    let second_report = project.join("second.report");

    run_worker(&project, &first_report, 1);
    let first = read_worker_report(&first_report);
    assert_eq!(first.get("overlay_hits"), Some(&0));
    assert_eq!(first.get("overlay_misses"), Some(&1));
    assert_eq!(first.get("hits"), Some(&0));
    assert_eq!(first.get("misses"), Some(&1));
    assert_eq!(first.get("published"), Some(&1));
    assert_eq!(first.get("cargo"), Some(&1));
    assert_eq!(first.get("runner_process"), Some(&1));
    assert_eq!(first.get("admitted"), Some(&1));

    let cache = rawr_import::cache::Writer::new(&project).expect("the runner cache rebinds");
    assert!(
        cache
            .inspect()
            .expect("the legacy fresh-runner cache inspects")
            .is_empty(),
        "the shared cold build must not publish project-local authority",
    );

    run_worker(&project, &second_report, 1);
    let second = read_worker_report(&second_report);
    assert_eq!(second.get("overlay_hits"), Some(&1));
    assert_eq!(second.get("overlay_misses"), Some(&0));
    assert_eq!(second.get("hits"), Some(&1));
    assert_eq!(second.get("misses"), Some(&0));
    assert_eq!(second.get("published"), Some(&0));
    assert_eq!(second.get("cargo"), Some(&0));
    assert_eq!(second.get("runner_process"), Some(&1));
    assert_eq!(second.get("admitted"), Some(&1));

    let first_ms = *first
        .get("elapsed_ms")
        .expect("the cold elapsed time is reported");
    let second_ms = *second
        .get("elapsed_ms")
        .expect("the warm elapsed time is reported");
    let saved_ms = first_ms.saturating_sub(second_ms);
    let reduction = if first_ms == 0 {
        0.0
    } else {
        saved_ms as f64 * 100.0 / first_ms as f64
    };
    eprintln!(
        "FRESH_RUNNER_DAG_READY cold_ms={first_ms} warm_ms={second_ms} saved_ms={saved_ms} reduction_percent={reduction:.1} cold_overlay=1 warm_overlay=0 cold_runner_transform=1 warm_runner_transform=0 cold_cargo=1 warm_cargo=0 cold_owner_process=1 warm_owner_process=1 cold_admitted=1 warm_admitted=1",
    );
    assert!(
        saved_ms > 0,
        "the fresh stored hit must reduce elapsed time"
    );
    assert_eq!(
        store_snapshot(&project.join("runner-harness")),
        source_tree,
        "runner bundle compilation must not mutate its imported source workspace",
    );

    retire_shared_store(&project);
}

#[test]
fn changed_rawr_snapshot_reuses_the_exact_runner_without_cargo() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-edit-dag");
    let project = guard.path().to_path_buf();
    prepare_runner_harness(&project);
    let first_report = project.join("first-edit.report");
    let second_report = project.join("second-edit.report");

    run_worker(&project, &first_report, 1);
    let first = read_worker_report(&first_report);
    assert_eq!(first.get("overlay_misses"), Some(&1));
    assert_eq!(first.get("misses"), Some(&1));
    assert_eq!(first.get("cargo"), Some(&1));
    assert_eq!(first.get("runner_process"), Some(&1));
    assert_eq!(first.get("admitted"), Some(&1));

    let cache = rawr_import::cache::Writer::new(&project).expect("the edit runner cache binds");
    assert!(
        cache
            .inspect()
            .expect("the legacy edit-runner cache inspects")
            .is_empty(),
        "the shared cold build must not publish project-local authority",
    );

    run_worker(&project, &second_report, 2);
    let second = read_worker_report(&second_report);
    // The generated source never reads the snapshot: a Rawr edit reuses it.
    assert_eq!(second.get("overlay_hits"), Some(&1));
    assert_eq!(second.get("overlay_misses"), Some(&0));
    assert_eq!(second.get("hits"), Some(&1));
    assert_eq!(second.get("misses"), Some(&0));
    assert_eq!(second.get("cargo"), Some(&0));
    assert_eq!(second.get("runner_process"), Some(&1));
    assert_eq!(second.get("admitted"), Some(&1));

    let cold_ms = *first
        .get("elapsed_ms")
        .expect("the cold edit time is reported");
    let warm_ms = *second
        .get("elapsed_ms")
        .expect("the warm edit time is reported");
    eprintln!(
        "RAWR_EDIT_RUNNER_DAG_READY cold_ms={cold_ms} warm_ms={warm_ms} saved_ms={} cold_overlay=1 warm_overlay=0 cold_cargo=1 warm_cargo=0 cold_runner_transform=1 warm_runner_transform=0 cold_owner_process=1 warm_owner_process=1 cold_admitted=1 warm_admitted=1",
        cold_ms.saturating_sub(warm_ms),
    );
    assert!(
        warm_ms < cold_ms,
        "the Rawr edit must reduce elapsed owner preparation"
    );

    retire_shared_store(&project);
}

#[test]
fn rawr_only_hot_edit_reuses_one_admitted_owner_without_build_work() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-hot-edit-dag");
    let project = guard.path().to_path_buf();
    prepare_runner_harness(&project);

    let context = context(&project);
    let store = context
        .open_store()
        .expect("the hot-edit shared store opens for fixture retirement");
    let chain =
        rawr_import::node::Chain::fixture(context).expect("the hot-edit fixture parent validates");
    let cache = rawr_import::cache::Writer::new(&project).expect("the runner cache binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let counts = Arc::new(Counts::default());
    let scope = rawr_import::metrics::Scope::transform_at(
        Arc::clone(&counts) as Arc<dyn rawr_import::metrics::Sink>,
        1,
    )
    .expect("the hot-edit evidence scope opens");

    let cold_started = Instant::now();
    let (cold_snapshot, cold_transport) = runner_snapshot(&project, 1);
    let cold_request = rawr_import::bridge::fixture_generation(
        chain.clone(),
        &project,
        cold_snapshot,
        supervisor.revision().expect("the cold revision seals"),
        &rawr_import::cargo::Limits::standard(),
    )
    .expect("the cold generated-owner request seals");
    let build = cold_request.build_request_key();
    let generated = cold_request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the cold generated-owner fixture materializes");
    let built = generated
        .build_on_demand(&cache)
        .expect("the cold runner artifact seals");
    let process = rawr_import::process::Owner::new(&project)
        .expect("the generated-owner process socket seals");
    let mut admitted = supervisor
        .bootstrap(built, cold_transport, &process)
        .expect("the cold generated owner reaches admitted state");
    let cold_key = admitted.key();
    let cold_elapsed = cold_started.elapsed();

    assert_eq!(counts.overlay_hits.load(Ordering::Relaxed), 0);
    assert_eq!(counts.overlay_misses.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_hits.load(Ordering::Relaxed), 0);
    assert_eq!(counts.runner_misses.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.cargo_runner.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_process.load(Ordering::Relaxed), 1);
    assert_eq!(counts.admitted.load(Ordering::Relaxed), 1);

    let (aborted_snapshot, aborted_transport) = runner_snapshot(&project, 2);
    let aborted_request = rawr_import::bridge::fixture_generation(
        chain.clone(),
        &project,
        aborted_snapshot,
        supervisor
            .revision()
            .expect("the revoked hot revision seals"),
        &rawr_import::cargo::Limits::standard(),
    )
    .expect("the revoked Rawr-only request seals");
    assert_eq!(aborted_request.build_request_key(), build);
    let aborted = match admitted
        .prepare_compile(aborted_request, aborted_transport)
        .expect("the owner prepares the revocable Rawr-only edit")
    {
        rawr_import::process::hot::Preparation::Prepared(prepared) => prepared,
        rawr_import::process::hot::Preparation::Miss(_) => {
            panic!("a revocable Rawr-only edit unexpectedly requested build work")
        }
    };
    assert_eq!(admitted.key(), cold_key);
    admitted
        .abort_compile(aborted)
        .expect("the obsolete Rawr-only preparation aborts");
    assert_eq!(admitted.key(), cold_key);

    let hot_started = Instant::now();
    let (hot_snapshot, hot_transport) = runner_snapshot(&project, 3);
    let hot_request = rawr_import::bridge::fixture_generation(
        chain,
        &project,
        hot_snapshot,
        supervisor
            .revision()
            .expect("the admitted hot revision seals"),
        &rawr_import::cargo::Limits::standard(),
    )
    .expect("the admitted Rawr-only request seals");
    assert_eq!(hot_request.build_request_key(), build);
    let prepared = match admitted
        .prepare_compile(hot_request, hot_transport)
        .expect("the owner prepares the admitted Rawr-only edit")
    {
        rawr_import::process::hot::Preparation::Prepared(prepared) => prepared,
        rawr_import::process::hot::Preparation::Miss(_) => {
            panic!("a Rawr-only edit unexpectedly requested build work")
        }
    };
    assert_eq!(admitted.key(), cold_key);
    let hot_key = admitted
        .commit_compile(prepared)
        .expect("the current Rawr-only preparation commits");
    let hot_elapsed = hot_started.elapsed();
    assert_ne!(hot_key, cold_key);
    assert_eq!(admitted.key(), hot_key);

    // Source and runner are produced once; the aborted and the committed
    // Rawr-only preparations each reuse both.
    assert_eq!(counts.overlay_hits.load(Ordering::Relaxed), 2);
    assert_eq!(counts.overlay_misses.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_hits.load(Ordering::Relaxed), 2);
    assert_eq!(counts.runner_misses.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.cargo_runner.load(Ordering::Relaxed), 1);
    assert_eq!(counts.runner_process.load(Ordering::Relaxed), 1);
    assert_eq!(counts.admitted.load(Ordering::Relaxed), 1);

    admitted
        .shutdown()
        .expect("the hot-edited generated owner exits and reaps");
    drop(scope);
    let counts = Arc::into_inner(counts).expect("the hot-edit count scope retires");
    eprintln!(
        "RAWR_HOT_EDIT_OWNER_REUSE cold_ms={} hot_ms={} saved_ms={} aborted_preparations=1 total_overlay_transforms=1 hot_overlay_transforms=0 total_runner_transforms=1 hot_runner_transforms=0 total_cargo_children=1 hot_cargo_children=0 total_owner_processes=1 hot_owner_processes=0 admitted_generations=1",
        cold_elapsed.as_millis(),
        hot_elapsed.as_millis(),
        cold_elapsed.saturating_sub(hot_elapsed).as_millis(),
    );
    assert_eq!(counts.runner_process.load(Ordering::Relaxed), 1);
    assert!(
        hot_elapsed < cold_elapsed,
        "the Rawr-only hot edit must be faster than the cold owner build"
    );

    store
        .retire_fixture()
        .expect("the hot-edit shared store retires as one fixture");
}

#[test]
fn identical_runner_restores_from_shared_node_without_cargo() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-dag");
    let project = guard.path().to_path_buf();
    let context = context(&project);
    let store = context
        .open_store()
        .expect("the shared runner store opens for fixture retirement");
    let chain = rawr_import::node::Chain::fixture(context)
        .expect("the runner DAG fixture parent validates");
    let cache = rawr_import::cache::Writer::new(&project).expect("the runner cache binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();

    let cold_counts = Arc::new(Counts::default());
    let cold_scope = rawr_import::metrics::Scope::transform_at(
        Arc::clone(&cold_counts) as Arc<dyn rawr_import::metrics::Sink>,
        1,
    )
    .expect("the cold evidence scope opens");
    let cold_started = Instant::now();
    let cold = rawr_import::runner::fixture(
        chain.clone(),
        supervisor.revision().expect("the cold revision seals"),
    );
    let key = cold.generated_manifest().build_request_key();
    let cargo = rawr_import::cargo::Owner::acquire().expect("the cold Cargo lane acquires");
    let cold = cold
        .build(&cargo, &cache)
        .expect("the cold runner artifact seals");
    drop(cargo);
    cold.artifact()
        .revalidate()
        .expect("the cold runner artifact revalidates");
    let cold_elapsed = cold_started.elapsed();
    drop(cold);
    drop(cold_scope);
    assert_eq!(cold_counts.runner_hits.load(Ordering::Relaxed), 0);
    assert_eq!(cold_counts.runner_misses.load(Ordering::Relaxed), 1);
    assert_eq!(cold_counts.runner_published.load(Ordering::Relaxed), 1);
    assert_eq!(cold_counts.cargo_runner.load(Ordering::Relaxed), 1);

    assert!(
        cache
            .inspect()
            .expect("the legacy runner cache inspects after the cold build")
            .is_empty(),
        "the shared runner build must not publish project-local authority",
    );
    let cold_store = store_snapshot(store.root());

    let warm_counts = Arc::new(Counts::default());
    let warm_scope = rawr_import::metrics::Scope::transform_at(
        Arc::clone(&warm_counts) as Arc<dyn rawr_import::metrics::Sink>,
        1,
    )
    .expect("the warm evidence scope opens");
    let warm_started = Instant::now();
    let warm = rawr_import::runner::fixture(
        chain,
        supervisor.revision().expect("the warm revision seals"),
    );
    assert_eq!(warm.generated_manifest().build_request_key(), key);
    let cargo = rawr_import::cargo::Owner::acquire()
        .expect("the unrelated Cargo lane remains held across the warm lookup");
    let warm = warm
        .build_on_demand(&cache)
        .expect("the warm runner restores without acquiring the held Cargo lane");
    drop(cargo);
    warm.artifact()
        .revalidate()
        .expect("the restored runner artifact revalidates");
    let warm_elapsed = warm_started.elapsed();
    drop(warm);
    drop(warm_scope);
    assert_eq!(warm_counts.runner_hits.load(Ordering::Relaxed), 1);
    assert_eq!(warm_counts.runner_misses.load(Ordering::Relaxed), 0);
    assert_eq!(warm_counts.runner_published.load(Ordering::Relaxed), 0);
    assert_eq!(warm_counts.cargo_runner.load(Ordering::Relaxed), 0);
    assert!(
        cache
            .inspect()
            .expect("the legacy runner cache inspects after the warm hit")
            .is_empty(),
        "the shared runner hit must not materialize a project-local entry",
    );
    assert_eq!(
        store_snapshot(store.root()),
        cold_store,
        "the healthy shared runner hit must not durably mutate the store",
    );
    eprintln!(
        "RUNNER_DAG cold_ms={} warm_ms={} saved_ms={} cold_cargo_runner=1 warm_cargo_runner=0",
        cold_elapsed.as_millis(),
        warm_elapsed.as_millis(),
        cold_elapsed.saturating_sub(warm_elapsed).as_millis(),
    );
    store
        .retire_fixture()
        .expect("the immutable shared runner store retires as one fixture");
}

#[test]
fn cold_on_demand_busy_owner_leaves_no_runner_materialization() {
    let _turn = rawr_import::store::test_turn();
    let guard = rawr_import::store::TestProject::new("rawr-runner-busy-owner-dag");
    let project = guard.path().to_path_buf();
    let context = context(&project);
    let store = context
        .open_store()
        .expect("the busy-owner shared store opens for fixture retirement");
    let chain = rawr_import::node::Chain::fixture(context)
        .expect("the busy-owner fixture parent validates");
    let cache = rawr_import::cache::Writer::new(&project).expect("the runner cache binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let generated = rawr_import::runner::fixture(
        chain.clone(),
        supervisor.revision().expect("the busy revision seals"),
    );
    let key = generated.generated_manifest().build_request_key();
    let held = rawr_import::cargo::Owner::acquire().expect("the unrelated Cargo lane acquires");
    assert!(
        generated.build_on_demand(&cache).is_err(),
        "a true cold miss cannot build while the sole Cargo lane is held"
    );
    assert!(
        cache
            .inspect()
            .expect("the failed cold cache inspects")
            .is_empty(),
        "the failed owner acquisition must not prepare a runner entry"
    );
    drop(held);

    let generated = rawr_import::runner::fixture(
        chain,
        supervisor.revision().expect("the recovered revision seals"),
    );
    assert_eq!(generated.generated_manifest().build_request_key(), key);
    let built = generated
        .build_on_demand(&cache)
        .expect("the cold runner builds after the Cargo lane is released");
    built
        .artifact()
        .revalidate()
        .expect("the recovered runner artifact revalidates");
    drop(built);
    store
        .retire_fixture()
        .expect("the busy-owner shared store retires as one fixture");
}
