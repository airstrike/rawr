//! Opt-in emission for a direct rustc check against the real compiler/UI APIs.
//!
//! The metadata is a deterministic fixture: this neither admits a Cargo
//! universe nor creates a runnable/qualified native generation. The consumer
//! typechecks `main.rs` with the matching real Rawr, rawr-iced and iced rlibs.

use std::fs;
use std::path::{Path, PathBuf};

use rawr_import::{profile, surface};

struct Compiler(Box<[u8]>);

impl surface::Runner for Compiler {
    fn compile_snapshot(&self) -> bool {
        true
    }

    fn facade_plan(&self) -> Option<&[u8]> {
        Some(&self.0)
    }
}

#[test]
#[ignore = "explicit compile-only emission; requires RAWR_GENERATED_RUNNER_OUTPUT"]
fn emit_live_runner_for_real_api_typecheck() {
    let root = PathBuf::from(
        std::env::var_os("RAWR_GENERATED_RUNNER_OUTPUT")
            .expect("set RAWR_GENERATED_RUNNER_OUTPUT to a new owned temporary directory"),
    );
    assert!(root.is_absolute(), "the output directory must be absolute");
    let parent = root.parent().unwrap().canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let shared_temporary = Path::new("/tmp").canonicalize().unwrap();
    assert!(
        parent.starts_with(&temporary) || parent.starts_with(&shared_temporary),
        "the output parent must be an existing temporary directory"
    );
    // Exclusive creation prevents the fixture from replacing existing files.
    fs::create_dir(&root).expect("the requested temporary directory must not exist");

    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile();
    let definition = crate::profile::Definition::new(&limits).unwrap();
    let profile_contract = profile::classify(&definition, &graph, &limits).unwrap();
    let checked = crate::profile::Checked::from_fingerprint([0x65; 32]);
    let profile_demand =
        profile::demand(&definition, &profile_contract, &checked, &limits).unwrap();
    let generated_profile =
        profile::Proof::generate(&definition, &profile_demand, &limits).unwrap();
    assert!(generated_profile.owns_product_lifecycle());

    let contract = graph
        .classify(&rawr_import::classify::Policy::standard(), &limits)
        .unwrap();
    let facade = surface::facade::Plan::encode(&contract, &limits).unwrap();
    let mut snapshot = surface::Snapshot::builder(&limits);
    snapshot.entry("/generated-fixture/main.rs").unwrap();
    snapshot
        .file(
            "/generated-fixture/main.rs",
            b"#[preview] pub fn sample() -> iced::Element<()> { throw \"compile-only fixture\" }",
        )
        .unwrap();
    let demand = contract
        .compiler(Compiler(facade), snapshot.seal().unwrap(), &limits)
        .unwrap()
        .seal()
        .unwrap();

    let harness = rawr_import::bridge::fixture_runner_harness(&root).unwrap();
    fs::write(
        harness.with_file_name("Cargo.lock"),
        b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let generated = rawr_import::bridge::fixture_generation_from_demand(
        demand,
        generated_profile,
        &root,
        supervisor.revision().unwrap(),
    )
    .unwrap()
    .generate(&limits)
    .unwrap();
    let source = generated
        .files()
        .find(|file| file.path() == "src/main.rs")
        .unwrap();
    let text = std::str::from_utf8(source.bytes()).unwrap();
    assert!(text.contains("fn prepare_resident("));
    assert!(text.contains("compile_package_snapshot_resident("));
    assert!(text.contains("fn live_protocol("));
    fs::write(root.join("main.rs"), source.bytes()).unwrap();
    fs::write(
        root.join("rawr-generated-manifest.bin"),
        generated.generated_manifest().carrier().unwrap(),
    )
    .unwrap();
    eprintln!("compile-only live runner emitted to {}", root.display());
}
