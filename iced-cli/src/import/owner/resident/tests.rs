use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rawr::reload::{scheduler, source};
use rawr_import::input::proof::Watch;

use super::{Attempt, Manifests};

#[test]
fn resident_manifests_ignore_source_body_edits() {
    let fixture = Fixture::new();
    let before = Manifests::from_graph(&fixture.graph());
    fixture.write("src/main.rs", "pub fn main() -> i64 { 2 }\n");
    assert!(before == Manifests::from_graph(&fixture.graph()));
}

#[test]
fn resident_manifests_detect_exact_cargo_declarations() {
    let fixture = Fixture::new();
    let manifest = concat!(
        "[package]\nname = \"resident-fixture\"\n",
        "[dependencies]\nnative = { version = \"1\" }\n",
    );
    fixture.write("Cargo.toml", manifest);
    let before = Manifests::from_graph(&fixture.graph());
    fixture.write(
        "Cargo.toml",
        &manifest.replace("version = \"1\"", "version = \"2\""),
    );
    assert!(before != Manifests::from_graph(&fixture.graph()));
    fixture.write(
        "Cargo.toml",
        &format!("{manifest}# declaration bytes changed\n"),
    );
    assert!(before != Manifests::from_graph(&fixture.graph()));
    fixture.write("Cargo.toml", manifest);
    assert!(before == Manifests::from_graph(&fixture.graph()));
}

#[test]
fn resident_manifests_bind_canonical_paths() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    assert!(Manifests::from_graph(&fixture.graph()) != Manifests::from_graph(&other.graph()));
}

#[test]
fn resident_prepare_continuation_retains_watch_coverage_through_fallthrough() {
    let current = Fixture::new();
    let incoming = Fixture::new();
    let adapter = rawr_watch::Adapter::manual_inputs(current.inputs(), Duration::ZERO).unwrap();
    let incoming_source = incoming.root.join("src/main.rs");
    adapter.inject_modify(&incoming_source);
    assert!(adapter.start(Instant::now()).is_none());

    let original = adapter
        .gate()
        .observe_and_start(Instant::now())
        .unwrap()
        .unwrap()
        .complete(());
    let continuation = Attempt::Prepare(Some(adapter.prepare_inputs(incoming.inputs()).unwrap()));
    adapter.inject_modify(&incoming_source);
    assert!(matches!(
        adapter.admit_current(original, |_| panic!(
            "continuation must retain candidate inputs"
        )),
        scheduler::Admission::Obsolete(())
    ));

    let fallback_inputs = match continuation {
        Attempt::Prepare(staged) => staged,
        Attempt::Settled => panic!("native miss must continue full preparation"),
    };
    let preparing = adapter.start(Instant::now()).unwrap().complete(());
    adapter.inject_modify(&incoming_source);
    assert!(matches!(
        adapter.admit_current(preparing, |_| panic!(
            "full preparation must remain observed"
        )),
        scheduler::Admission::Obsolete(())
    ));

    // The full path registers its replacement before retiring the continuation.
    let replacement = adapter.prepare_inputs(incoming.inputs()).unwrap();
    drop(fallback_inputs);
    let installing = adapter.start(Instant::now()).unwrap().complete(());
    adapter.inject_modify(&incoming_source);
    assert!(matches!(
        adapter.admit_current(installing, |_| panic!(
            "overlapping replacement must remain observed"
        )),
        scheduler::Admission::Obsolete(())
    ));
    drop(adapter.start(Instant::now()).unwrap());
    assert!(adapter.commit_inputs(replacement).unwrap().is_empty());
    adapter.inject_modify(&incoming_source);
    assert!(adapter.start(Instant::now()).is_some());
}

#[test]
fn resident_dropped_prepare_continuation_retires_candidate_inputs() {
    let current = Fixture::new();
    let incoming = Fixture::new();
    let adapter = rawr_watch::Adapter::manual_inputs(current.inputs(), Duration::ZERO).unwrap();
    let continuation = Attempt::Prepare(Some(adapter.prepare_inputs(incoming.inputs()).unwrap()));
    drop(continuation);
    adapter.inject_modify(&incoming.root.join("src/main.rs"));
    assert!(adapter.start(Instant::now()).is_none());
    adapter.inject_modify(&current.root.join("src/main.rs"));
    assert!(adapter.start(Instant::now()).is_some());
}

#[test]
fn resident_cold_retry_observes_fresh_sources_without_a_hot_begin() {
    let fixture = Fixture::new();
    let baseline = Manifests::from_graph(&fixture.graph());
    let adapter = rawr_watch::Adapter::manual_inputs(fixture.inputs(), Duration::ZERO).unwrap();
    let native = adapter.native_baseline();
    let request = adapter
        .gate()
        .observe_and_start(Instant::now())
        .unwrap()
        .unwrap();
    let mut lane = super::super::Lane::new(&fixture.root).unwrap();
    let revision = lane.revision().unwrap();
    let target = rawr::project::Target::discover(&fixture.root).unwrap();
    fixture.write("src/main.rs", "pub fn main( {");
    let source = rawr::diagnostic::Source::new("main.rs", "pub fn main( {").unwrap();
    let result = super::attempt(
        &mut lane,
        &adapter,
        &request,
        None,
        revision,
        &target,
        &baseline,
        &native,
        false,
        &mut None,
        &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
        &source,
    );
    let Err(report) = result else {
        panic!("a cold retry must capture current source before continuing native preparation");
    };
    assert!(
        !report.message().contains("no admitted runner"),
        "source capture must fail first"
    );
}

#[test]
fn resident_parent_only_native_event_invalidates_parent_baseline() {
    let fixture = Fixture::new();
    let native = fixture.root.join("native.rs");
    fs::write(&native, "pub fn value() -> i64 { 1 }\n").unwrap();
    let inputs = fixture
        .inputs()
        .with_imported([Watch::Exact(native.clone())]);
    let parent = rawr_watch::Adapter::manual_inputs(inputs.clone(), Duration::ZERO).unwrap();
    let child = rawr_watch::Adapter::manual_inputs(inputs.clone(), Duration::ZERO).unwrap();
    let parent_baseline = parent.native_baseline();
    let child_baseline = child.native_baseline();

    parent.inject_modify(&native);
    let _parent_request = parent.start(Instant::now()).unwrap();
    let _child_request = child
        .gate()
        .observe_and_start(Instant::now())
        .unwrap()
        .unwrap();
    assert!(!parent.resident_eligible(&parent_baseline));
    assert!(child.resident_eligible(&child_baseline));
    assert!(!parent.resident_eligible(&child_baseline));

    let before_validation = parent.native_baseline();
    let staged = parent.prepare_inputs(inputs).unwrap();
    assert!(parent.resident_eligible(&before_validation));
    parent.inject_modify(&native);
    assert!(parent.commit_inputs(staged).unwrap().is_empty());
    assert!(!parent.resident_eligible(&before_validation));
    assert!(child.resident_eligible(&child_baseline));
}

#[test]
fn resident_obsolete_completed_failures_do_not_publish_fresh_diagnostics() {
    use crate::watch::native::Error;
    use rawr_import::process::RunnerError;
    let fixture = Fixture::new();
    let adapter = rawr_watch::Adapter::manual_inputs(fixture.inputs(), Duration::ZERO).unwrap();
    let request = adapter
        .gate()
        .observe_and_start(Instant::now())
        .unwrap()
        .unwrap();
    let source = rawr::diagnostic::Source::new("main.rs", "pub fn main() {}").unwrap();
    let drift = || {
        Error::Process(RunnerError::Snapshot(
            rawr_import::supervisor::snapshot::Error::Mismatch,
        ))
    };
    assert!(super::failed(drift(), &adapter, &request, &source).is_err());
    adapter.inject_modify(&fixture.root.join("src/main.rs"));
    for error in [
        drift(),
        Error::Process(RunnerError::HotConsumedRejected("obsolete".into())),
    ] {
        assert!(matches!(
            super::failed(error, &adapter, &request, &source),
            Ok(Attempt::Settled)
        ));
    }
    for error in [
        RunnerError::OwnerLost {
            reason: "lost owner".into(),
        },
        RunnerError::HotCleanup {
            failure: Box::new(RunnerError::Snapshot(
                rawr_import::supervisor::snapshot::Error::Mismatch,
            )),
            cleanup: Box::new(RunnerError::Stale),
        },
        RunnerError::Stale,
    ] {
        assert!(super::failed(Error::Process(error), &adapter, &request, &source).is_err());
    }
}

#[test]
fn resident_runner_late_event_cannot_drop_the_newest_write() {
    let fixture = Fixture::new();
    let main = fixture.root.join("src/main.rs");
    let adapter = rawr_watch::Adapter::manual_inputs(fixture.inputs(), Duration::ZERO).unwrap();
    let runner = scheduler::Gate::new(Duration::ZERO);
    let source = rawr::diagnostic::Source::new("main.rs", "pub fn main() {}").unwrap();
    // The CLI watcher sees the final write first; its request captures those
    // bytes and HotBegin opens the runner's request on the runner's own gate.
    fixture.write("src/main.rs", "pub fn main() -> i64 { 2 }\n");
    adapter.inject_modify(&main);
    let request = adapter.start(Instant::now()).unwrap();
    let begun = runner.observe_and_start(Instant::now()).unwrap().unwrap();
    // The runner's watcher reports that same write late and revokes its request:
    // the child answers ResidentObsolete while the CLI request is still newest.
    runner.observe(Instant::now()).unwrap();
    assert!(!runner.request_is_current(&begun));
    assert!(adapter.gate().request_is_current(&request));
    assert!(matches!(
        super::obsolete(&adapter, &request, &source),
        Ok(Attempt::Settled)
    ));
    // No later event exists for these bytes, so only a retry can show them.
    let retry = adapter
        .start(Instant::now())
        .expect("the newest write must be compiled again, not dropped");

    // Latest-wins: a newer CLI-observed write supersedes without an extra attempt.
    adapter.inject_modify(&main);
    assert!(matches!(
        super::obsolete(&adapter, &retry, &source),
        Ok(Attempt::Settled)
    ));
    assert!(adapter.start(Instant::now()).is_some());
    assert!(adapter.start(Instant::now()).is_none());
}

#[test]
fn resident_runner_late_event_at_install_retries_but_real_failures_report() {
    use crate::watch::native::Error;
    use rawr_import::process::RunnerError;
    let fixture = Fixture::new();
    let main = fixture.root.join("src/main.rs");
    let adapter = rawr_watch::Adapter::manual_inputs(fixture.inputs(), Duration::ZERO).unwrap();
    let runner = scheduler::Gate::new(Duration::ZERO);
    let source = rawr::diagnostic::Source::new("main.rs", "pub fn main() {}").unwrap();
    fixture.write("src/main.rs", "pub fn main() -> i64 { 2 }\n");
    adapter.inject_modify(&main);
    let request = adapter.start(Instant::now()).unwrap();
    let begun = runner.observe_and_start(Instant::now()).unwrap().unwrap();
    // The child compiled while current; the runner's late event for the same
    // write lands before install, so the commit comes back superseded.
    assert!(runner.request_is_current(&begun));
    runner.observe(Instant::now()).unwrap();
    assert!(!runner.request_is_current(&begun));
    assert!(adapter.gate().request_is_current(&request));
    let superseded = || Error::Process(RunnerError::HotSuperseded);
    assert!(matches!(
        super::failed(superseded(), &adapter, &request, &source),
        Ok(Attempt::Settled)
    ));
    let retry = adapter
        .start(Instant::now())
        .expect("the newest write must be compiled again, not dropped");

    // A real install failure on the newest request reports and never re-arms.
    let rejected = Error::Process(RunnerError::HotConsumedRejected("widget failed".into()));
    assert!(super::failed(rejected, &adapter, &retry, &source).is_err());
    assert!(adapter.start(Instant::now()).is_none());

    // Latest-wins: once a newer CLI request exists, superseded adds no attempt.
    adapter.inject_modify(&main);
    assert!(matches!(
        super::failed(superseded(), &adapter, &retry, &source),
        Ok(Attempt::Settled)
    ));
    assert!(adapter.start(Instant::now()).is_some());
    assert!(adapter.start(Instant::now()).is_none());
}

struct Fixture {
    root: PathBuf,
}

#[test]
fn superseded_attempt_failure_is_not_reported() {
    let fixture = Fixture::new();
    let adapter = rawr_watch::Adapter::manual_inputs(fixture.inputs(), Duration::ZERO).unwrap();
    let mut lane = super::super::Lane::new(&fixture.root).unwrap();
    let failure =
        || super::super::framework("prepared runner read set rejected before admission", None);
    // No live runner: reporting a current attempt's failure reaches the lane.
    assert!(super::super::settle_failure(&mut lane, &adapter, failure()).is_err());
    // A newer edit arrived mid-preparation: the stale failure stays silent.
    adapter.inject_modify(&fixture.root.join("src/main.rs"));
    assert!(super::super::settle_failure(&mut lane, &adapter, failure()).is_ok());
    assert!(adapter.start(Instant::now()).is_some());
}

impl Fixture {
    const MANIFEST: &str = "[package]\nname = \"resident-fixture\"\n";

    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-cli-resident-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            root: fs::canonicalize(root).unwrap(),
        };
        fixture.write("Cargo.toml", Self::MANIFEST);
        fixture.write("src/main.rs", "pub fn main() -> i64 { 1 }\n");
        fixture
    }

    /// Writes `relative`; a `.rs` file is marked `// rawr`.
    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if path.extension().is_some_and(|extension| extension == "rs") {
            fs::write(path, format!("// rawr\n{text}")).unwrap();
        } else {
            fs::write(path, text).unwrap();
        }
    }

    fn graph(&self) -> rawr::package::Graph {
        let target = rawr::project::Target::discover(&self.root).unwrap();
        rawr::package::Graph::load(&rawr::Engine::new(), &target).unwrap()
    }

    fn inputs(&self) -> source::Inputs {
        source::Inputs::from_graph(&source::Graph::from_package(&self.graph()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
