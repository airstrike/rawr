use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr::package::Graph;
use rawr::{Engine, project};
use rawr_import::api::ledger;
use rawr_import::cargo::Limits;
use rawr_import::cargo::config::{Environment, Role};
use rawr_import::process::Native;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Project {
    workspace: PathBuf,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let workspace = std::env::temp_dir().join(format!("rawr-rust-import-{}-{}", std::process::id(), TEST_ID.fetch_add(1, Ordering::Relaxed)));
        let root = workspace.join("tests/fixtures/rust-import-zero");
        let catalog = workspace.join("importer/tests/fixtures/catalog-api");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        for (source, destination) in [
            (repository.join("tests/fixtures/rust-import-zero/rawr.toml"), root.join("rawr.toml")),
            (repository.join("tests/fixtures/rust-import-zero/src/main.rawr"), root.join("src/main.rawr")),
            (repository.join("importer/tests/fixtures/catalog-api/Cargo.toml"), catalog.join("Cargo.toml")),
            (repository.join("importer/tests/fixtures/catalog-api/src/lib.rs"), catalog.join("src/lib.rs")),
        ] {
            fs::create_dir_all(destination.parent().expect("fixture files have parents")).expect("the isolated fixture directory is created");
            fs::copy(source, destination).expect("the immutable fixture is copied byte-for-byte");
        }
        Self { workspace, root }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.workspace);
    }
}

#[test]
fn unchanged_rust_library_flows_from_manifest_to_extracted_graph() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let target = project::Target::discover(&project.root).expect("the zero-Rawr project is discoverable");
    let graph = Graph::load(&Engine::new(), &target).expect("the Rawr package graph validates");
    let limits = Limits::standard();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment seals");
    let request = graph
        .rust_import(environment, &limits)
        .expect("the validated manifest transforms into an importer request")
        .expect("the entry manifest declares Rust imports");
    let native = Native::pinned().expect("the crate-owned nightly is installed");
    let resolution = request.resolve(&native, &limits).expect("the manifest-derived Cargo universe resolves");

    let catalog = resolution
        .selections()
        .find(|selection| selection.alias() == "catalog")
        .expect("the manifest library selection is present");
    assert!(catalog.roles().contains(Role::Library));
    assert!(!catalog.roles().contains(Role::Runtime));
    assert!(!catalog.roles().contains(Role::Compiler));
    let rawr = resolution
        .selections()
        .find(|selection| selection.alias() == "__rawr_internal")
        .expect("the crate-owned Rawr selection is injected");
    assert!(!rawr.roles().contains(Role::Library));
    assert!(rawr.roles().contains(Role::Runtime));
    assert!(rawr.roles().contains(Role::Compiler));
    let harness = fs::read_to_string(resolution.harness_root().join("Cargo.toml")).expect("the final-form harness manifest is readable");
    let internal = harness
        .lines()
        .find(|line| line.starts_with("__rawr_internal = "))
        .expect("the generated manifest contains the internal Rawr dependency");
    assert!(internal.contains("default-features = false"));
    // Generated runtime assembly binds imported types through Rawr's
    // `rust-import`-gated runtime API, so the internal dependency enables it.
    assert!(internal.contains("features = [\"rust-import\", \"std\"]"), "{internal}");

    let extracted = rawr_import::extract::from_resolution(resolution, &native, &limits).expect("the unchanged Rust library canonicalizes");
    let excluded = extracted
        .exports()
        .find(|outcome| outcome.key().root_alias() == "catalog" && outcome.key().path().eq(["Product"].into_iter()))
        .expect("the manifest exclusion has one selected-path outcome");
    match excluded {
        ledger::Path::Excluded { reason, policy_span, .. } => {
            assert_eq!(reason.as_ref(), "the application does not expose product storage");
            assert!(policy_span.file().ends_with("rawr.toml"));
        }
        ledger::Path::Imported(_) | ledger::Path::Rejected { .. } => {
            panic!("the exact manifest path must remain excluded")
        }
    }
    assert!(
        extracted
            .exports()
            .any(|outcome| { outcome.key().root_alias() == "catalog" && outcome.key().path().eq(["Availability"].into_iter()) && matches!(outcome, ledger::Path::Imported(_)) })
    );
}
