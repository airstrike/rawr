use crate::p3;

#[test]
fn generated_manifest_is_ordered_and_matches_exact_files() {
    let manifest = p3::generated();
    let paths = manifest.files().map(|file| file.path()).collect::<Vec<_>>();
    // The runner's crate, its base and one crate of adapters per imported
    // root, one module per identity bucket (#1670), in path order.
    assert!(paths.is_sorted(), "{paths:?}");
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "build.rs",
        "rawr-crates/adapters-facade_a/Cargo.toml",
        "rawr-crates/base/Cargo.toml",
        "rawr-crates/base/build.rs",
        "rawr-crates/base/src/lib.rs",
        "src/main.rs",
        "src/rawr_surface.rs",
    ] {
        assert!(paths.contains(&path), "{path} in {paths:?}");
    }
    assert!(paths
        .iter()
        .any(|path| path.starts_with("rawr-crates/adapters-facade_a/src/rawr_adapters_facade_a_")));
    assert!(paths.iter().all(
        |path| ["Cargo.lock", "Cargo.toml", "build.rs"].contains(path)
            || path.starts_with("src/")
            || path.starts_with("rawr-crates/")
    ));
    assert!(manifest.generated_manifest().entries().len() > 8);
    assert!(!manifest.generated_manifest().source_map().is_empty());
}

#[test]
fn carrier_is_purely_derived_and_the_build_script_has_no_hash_cycle() {
    let project = rawr_import::store::TestProject::new("rawr-p3-manifest");
    let root = project.path();
    let first = p3::generated_at(root);
    let second = p3::generated_at(root);
    let build_key = hex(first.generated_manifest().build_request_key().as_bytes());
    assert_eq!(
        first
            .generated_manifest()
            .carrier()
            .expect("the first carrier derives"),
        second
            .generated_manifest()
            .carrier()
            .expect("the second carrier derives")
    );

    let build_script = first
        .files()
        .find(|file| file.path() == "build.rs")
        .expect("the static build script exists");
    assert!(!build_script
        .bytes()
        .windows(64)
        .any(|window| window == build_key.as_bytes()));
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
