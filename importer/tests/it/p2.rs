#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The symbolic fixture's contract, extracted and classified once per
/// process; its raw graph is [`rawr_import::Contract::raw_graph`].
pub fn contract() -> &'static rawr_import::Contract<rawr_import::Classified> {
    static CONTRACT: OnceLock<rawr_import::Contract<rawr_import::Classified>> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        graphs(1)
            .pop()
            .expect("one requested real P2 graph is returned")
            .classify(
                &rawr_import::classify::Policy::standard(),
                &Limits::standard(),
            )
            .expect("the real P2 graph classifies")
    })
}

pub fn graphs(count: usize) -> Vec<rawr_import::api::Graph<rawr_import::api::Extracted>> {
    let root = std::env::temp_dir().join(format!(
        "rawr-p2-contract-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique P2 project can be created");
    let project = Project(fs::canonicalize(root).expect("the P2 project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    (0..count)
        .map(|_| {
            let _lane = crate::shared::lane();
            request(&project.0, &limits)
                .resolve(&native, &limits)
                .and_then(|resolution| {
                    rawr_import::extract::from_resolution(resolution, &native, &limits)
                })
                .expect("the real P2 facade evidence canonicalizes")
        })
        .collect()
}

pub fn outcome<'a>(
    contract: &'a rawr_import::Contract<rawr_import::Classified>,
    root: &str,
    path: &[&str],
) -> rawr_import::classify::Outcome<'a> {
    contract
        .outcomes()
        .find(|outcome| {
            outcome.export().root_alias() == root
                && outcome.export().path().eq(path.iter().copied())
        })
        .unwrap_or_else(|| panic!("the real contract retains {root}::{path:?}"))
}

fn request(root: &Path, limits: &Limits) -> Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the P2 environment is sealable");
    let fixture = fixture("symbolic-api");
    Request::builder(root, &root.join("target/rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest().seal())
        .dependency(
            Dependency::path("facade_a", &fixture)
                .and_then(|dependency| dependency.package("symbolic-api"))
                .expect("the first facade alias is exact"),
        )
        .dependency(
            Dependency::path("facade_b", &fixture)
                .and_then(|dependency| dependency.package("symbolic-api"))
                .expect("the second facade alias is exact"),
        )
        .build()
        .expect("the P2 request seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the immutable P2 fixture exists")
}
