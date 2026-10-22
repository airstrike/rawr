//! One item the importer cannot represent never fails the import: its
//! definition is retained as rejected, and every other item still imports.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::{ledger, model};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Project {
    root: PathBuf,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-tolerance-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the unique tolerance project can be created");
        let root = fs::canonicalize(root).expect("the tolerance project has a canonical root");
        Self { root }
    }
}

#[test]
fn unrepresentable_anonymous_constants_reject_only_their_own_paths() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let resolution = request(&project, &limits)
        .resolve(&native, &limits)
        .expect("one unrepresentable item never fails the extraction");
    let graph = rawr_import::extract::from_resolution(resolution, &native, &limits)
        .expect("the fixture compiler evidence canonicalizes");

    for name in ["lanes", "switch"] {
        match definition(&graph, name).kind() {
            model::Kind::Rejected {
                reason: ledger::Reason::UnknownCompilerShape { compiler_kind, .. },
                ..
            } => assert!(
                compiler_kind.starts_with("anonymous-constant"),
                "{name}: {compiler_kind}"
            ),
            other => panic!("{name} is rejected as an unknown compiler shape: {other:?}"),
        }
    }
    for name in ["plain", "Lanes", "Switch"] {
        assert!(
            !matches!(
                definition(&graph, name).kind(),
                model::Kind::Rejected { .. }
            ),
            "{name} still imports"
        );
    }
}

fn definition<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    name: &str,
) -> &'a model::Definition {
    let export = graph
        .exports()
        .find_map(|outcome| match outcome {
            ledger::Path::Imported(export)
                if export.key().root_alias() == "tolerant" && export.key().path().eq([name]) =>
            {
                Some(export)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing imported path tolerant::{name}"));
    graph
        .definitions()
        .find(|definition| definition.id() == export.target())
        .unwrap_or_else(|| panic!("tolerant::{name} keeps its definition"))
}

fn request(project: &Project, limits: &Limits) -> Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    Request::builder(&project.root, &project.root.join(".rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("tolerant", &fixture("tolerant-api"), limits)
                .and_then(|dependency| dependency.package("tolerant-api"))
                .expect("the tolerant fixture selection is exact"),
        )
        .expect("the tolerant dependency fits the request")
        .build()
        .expect("the tolerance request seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the immutable tolerance fixture exists")
}
