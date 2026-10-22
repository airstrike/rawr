use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::ledger;
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;
use sha2::{Digest as _, Sha256};

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
            "rawr-extraction-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the unique extraction project can be created");
        let root = fs::canonicalize(root).expect("the extraction project has a canonical root");
        Self { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join(".rawr/cache")
    }
}

#[test]
fn pinned_normal_build_extracts_once_per_selected_unit_and_repeats_byte_identically() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    assert_fixture_hashes();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");

    let first = request(&project, &limits)
        .resolve(&native, &limits)
        .expect("the first one-pass Build extraction seals");
    let first = rawr_import::extract::from_resolution(first, &native, &limits)
        .expect("the first compiler capture canonicalizes");
    let second = request(&project, &limits)
        .resolve(&native, &limits)
        .expect("the second one-pass Build extraction seals");
    let second = rawr_import::extract::from_resolution(second, &native, &limits)
        .expect("the second compiler capture canonicalizes");

    assert_eq!(first.invocation_count(), 2);
    assert_eq!(second.invocation_count(), 2);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.fingerprint(), second.fingerprint());

    let mut paths = BTreeSet::new();
    let mut imported = 0_usize;
    for outcome in first.exports() {
        let key = outcome.key();
        let path = (
            key.root_alias().to_owned(),
            key.path().map(str::to_owned).collect::<Vec<_>>(),
            key.namespace().as_str(),
        );
        assert!(paths.insert(path), "every public path has one outcome");
        match outcome {
            ledger::Path::Imported(_) => imported += 1,
            ledger::Path::Rejected { reason, span, .. } => {
                let range = span.byte_range();
                assert!(range.start <= range.end, "rejection span remains explicit");
                assert_rejected_reason(reason);
            }
            ledger::Path::Excluded { .. } => {
                panic!("the unchanged fixture policy excludes no compiler path")
            }
        }
    }
    assert!(imported > 0, "representable compiler paths import");
    assert_fixture_hashes();
}

fn assert_rejected_reason(reason: &ledger::Reason) {
    match reason {
        ledger::Reason::ExternalAuthorityUnavailable {
            crate_name,
            stable_crate_id,
        } => {
            assert!(!crate_name.is_empty());
            assert!(!stable_crate_id.is_empty());
        }
        ledger::Reason::UnknownCompilerShape { compiler_kind, .. }
        | ledger::Reason::UnsupportedPublicPath { compiler_kind } => {
            assert!(!compiler_kind.is_empty());
        }
        ledger::Reason::UnnameablePrivate(_)
        | ledger::Reason::UnresolvedDefinition(_)
        | ledger::Reason::InvalidAliasChain
        | ledger::Reason::ConflictingIdentity => {}
    }
}

#[test]
fn selected_rmeta_mutation_rejects_publication() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let resolution = request(&project, &limits)
        .resolve(&native, &limits)
        .expect("the exact artifact capture seals");
    let artifact = find_selected_rmeta(&project.cache())
        .expect("the sealed cache contains selected geometry rmeta");
    let mut bytes = fs::read(&artifact).expect("the selected rmeta is readable");
    let byte = bytes.first_mut().expect("the selected rmeta is nonempty");
    *byte ^= 1;
    fs::write(&artifact, bytes).expect("the isolated selected rmeta can be mutated");

    let error = match rawr_import::extract::from_resolution(resolution, &native, &limits) {
        Ok(_) => panic!("artifact drift cannot construct an extracted graph"),
        Err(error) => error,
    };
    assert!(format!("{error}").contains("selected-artifact-drift"));
}

fn find_selected_rmeta(root: &Path) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "rmeta")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("libgeometry_api-"))
            {
                return Some(path);
            }
        }
    }
    None
}

fn request(project: &Project, limits: &Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    Request::builder(&project.root, &project.cache(), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("geometry", &fixture("geometry-api"), limits)
                .and_then(|dependency| dependency.package("geometry-api"))
                .expect("the geometry fixture selection is exact"),
        )
        .expect("the geometry dependency fits the request")
        .dependency(
            Dependency::path("catalog", &fixture("catalog-api"), limits)
                .and_then(|dependency| dependency.package("catalog-api"))
                .expect("the catalog fixture selection is exact"),
        )
        .expect("the catalog dependency fits the request")
        .build()
        .expect("the extraction request seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the immutable extraction fixture exists")
}

fn assert_fixture_hashes() {
    for (path, expected) in [
        (
            "geometry-api/Cargo.toml",
            "bfec3084aa7257b6b710ff0ad2d378caead455b39c818628db5e8e90e42bcd8b",
        ),
        (
            "geometry-api/src/lib.rs",
            "dcf1547f6dd1c5df62410b77a829348f84a3c28819b0de1bb8248ea3408208f8",
        ),
        (
            "catalog-api/Cargo.toml",
            "26fa69c14bee5d3cf1fb2458b2d6e670b459171d21d5bde82896883a45307ce5",
        ),
        (
            "catalog-api/src/lib.rs",
            "6c9889e9c54ebfbcd147ed6b98daabe01fc4e011e9849bb37ce14d1d3f264e0a",
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(path);
        let bytes = fs::read(&path).expect("fixture authority is readable");
        assert_eq!(hex(Sha256::digest(bytes)), expected, "{}", path.display());
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let mut encoded = String::new();
    for byte in bytes.as_ref() {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").expect("String formatting is infallible");
    }
    encoded
}
