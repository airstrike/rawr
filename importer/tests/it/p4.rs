#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;
use rawr_import::surface::Snapshot;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The generic fixture's contract, extracted and classified once per
/// process, for tests that only read it.
pub fn shared() -> &'static rawr_import::Contract<rawr_import::Classified> {
    static CONTRACT: OnceLock<rawr_import::Contract<rawr_import::Classified>> = OnceLock::new();
    CONTRACT.get_or_init(|| contract().1)
}

/// A contract of its own, for a test that consumes it into a native proof.
pub fn contract() -> (
    Project,
    rawr_import::Contract<rawr_import::Classified>,
    Limits,
) {
    let root = std::env::temp_dir().join(format!(
        "rawr-p4-surface-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique P4 project can be created");
    let project = Project(fs::canonicalize(root).expect("the P4 project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let lane = crate::shared::lane();
    let graph = request(&project.0, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
        .expect("real generic API evidence canonicalizes");
    drop(lane);
    let contract = graph
        .classify(&rawr_import::classify::Policy::standard(), &limits)
        .expect("real generic API evidence classifies");
    (project, contract, limits)
}

pub fn snapshot(limits: &Limits) -> Snapshot {
    let mut snapshot = Snapshot::builder(limits);
    snapshot
        .entry("application.rs")
        .expect("the entry path is bounded");
    snapshot
        .file("application.rs", b"generic::identity(1)")
        .expect("the source is bounded");
    snapshot.seal().expect("the P4 snapshot seals")
}

fn request(root: &Path, limits: &Limits) -> Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the P4 environment is sealable");
    let fixture = fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generic-api"),
    )
    .expect("the immutable generic fixture exists");
    Request::builder(root, &root.join("target/rawr/cache"), limits)
        .expect("the P4 request builder seals")
        .environment(environment)
        .policy(Policy::manifest().seal())
        .dependency(
            Dependency::path("generic", &fixture)
                .and_then(|dependency| dependency.package("generic-api"))
                .expect("the generic facade alias is exact"),
        )
        .build()
        .expect("the P4 request seals")
}
