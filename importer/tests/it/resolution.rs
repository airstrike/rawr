use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::cargo::config::{Dependency, Environment, Role};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;
use rawr_import::Error;

const HOST: &str = "aarch64-apple-darwin";
const GIT_REV: &str = "be88873d38544455e26c7a104eb757da2cbbac6b";
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
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-resolution-{name}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("unique test project can be created");
        let root = fs::canonicalize(root).expect("test project has a canonical root");
        Self { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("target/rawr/cache")
    }
}

#[test]
fn real_pinned_cargo_seals_final_form_resolution_without_mutating_import() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("native");
    let fixture = fixture();
    let before = fixture_bytes(&fixture);
    let limits = Limits::standard();
    let request = native_request(&project, &fixture, &limits);
    let native = Native::pinned().expect("the crate-owned nightly is installed");

    let resolution = request
        .resolve(&native, &limits)
        .expect("the real pinned Cargo Build and direct probe preserve one universe");

    assert_eq!(
        resolution.harness_root(),
        project.root.join("target/rawr/cache/resolution")
    );
    assert_eq!(resolution.target(), HOST);
    assert_eq!(resolution.profile(), "dev");
    assert_eq!(resolution.toolchain().channel(), "nightly-2026-07-11");
    assert_eq!(
        resolution.toolchain().rustdoc_commit(),
        resolution.toolchain().rustc_commit()
    );
    assert_eq!(resolution.toolchain().target(), HOST);
    assert_eq!(resolution.selections().len(), 2);
    let selected = resolution
        .selections()
        .find(|selection| selection.alias() == "catalog")
        .expect("the selected library is proven");
    assert_eq!(selected.alias(), "catalog");
    assert!(selected.roles().contains(Role::Library));
    assert!(!selected.roles().contains(Role::Runtime));
    assert!(!selected.roles().contains(Role::Compiler));
    let internal = resolution
        .selections()
        .find(|selection| selection.alias() == "__rawr_internal")
        .expect("the crate-owned Rawr package is proven");
    assert!(!internal.roles().contains(Role::Library));
    assert!(internal.roles().contains(Role::Runtime));
    assert!(internal.roles().contains(Role::Compiler));
    assert_ne!(resolution.build_unit_digest().as_bytes(), &[0; 32]);
    let manifest = fs::read_to_string(resolution.harness_root().join("Cargo.toml"))
        .expect("the generated final-form manifest is readable");
    assert!(manifest.contains("[workspace]"));
    assert!(manifest.contains("resolver = \"3\""));
    assert!(manifest.contains("catalog = { package = \"catalog-api\""));
    assert!(!manifest.contains("members ="));
    assert!(!manifest.contains("rawr-import"));
    let lock = fs::read_to_string(resolution.harness_root().join("Cargo.lock"))
        .expect("the Rawr-owned final lock exists");
    assert!(lock.contains("name = \"catalog-api\""));
    assert_eq!(fixture_bytes(&fixture), before);
}

fn native_request(project: &Project, fixture: &Path, limits: &Limits) -> Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the current environment is sealable");
    Request::builder(&project.root, &project.cache(), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("catalog", fixture, limits)
                .and_then(|dependency| dependency.package("catalog-api"))
                .expect("the unchanged fixture specification is valid"),
        )
        .expect("the catalog dependency fits the request")
        .build()
        .expect("the request seals before native execution")
}

#[test]
fn unsupported_target_and_nonexact_git_revision_reject_while_roles_stay_canonical() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("validation");
    let limits = Limits::standard();
    let error = match Dependency::git("catalog", "https://example.invalid/catalog", "abc", &limits)
    {
        Ok(_) => panic!("a short Git revision is never exact"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        Error::Request {
            field: "dependency git revision",
            ..
        }
    ));
    Dependency::git(
        "catalog",
        "https://example.invalid/catalog",
        GIT_REV,
        &limits,
    )
    .expect("a full Git revision seals");
    Dependency::registry("catalog", "1", &limits)
        .expect("Cargo's shorthand registry requirement seals semantically");

    let error = Request::builder(&project.root, &project.cache(), &limits)
        .expect("the limits-bound request builder seals")
        .environment(Environment::empty())
        .policy(Policy::manifest(&limits).seal())
        .expect("the manifest policy fits the request")
        .target("x86_64-unknown-linux-gnu")
        .expect("the triple is syntactically valid")
        .dependency(
            Dependency::path("catalog", &fixture(), &limits)
                .and_then(|dependency| dependency.package("catalog-api"))
                .expect("the fixture specification is valid"),
        )
        .expect("the catalog dependency fits the request")
        .build()
        .expect_err("unsupported targets reject before Cargo graph authority");
    assert!(matches!(
        error,
        Error::Request {
            field: "Cargo target",
            ..
        }
    ));
}

fn fixture() -> PathBuf {
    fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api"))
        .expect("the unchanged catalog fixture exists")
}

fn fixture_bytes(root: &Path) -> Vec<u8> {
    let mut bytes = fs::read(root.join("Cargo.toml")).expect("fixture manifest is readable");
    bytes.extend(fs::read(root.join("src/lib.rs")).expect("fixture source is readable"));
    bytes
}
