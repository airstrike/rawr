use crate::p3;

#[test]
fn generated_manifest_is_ordered_and_matches_exact_files() {
    let _turn = rawr_import::store::test_turn();
    let manifest = p3::generated();
    let paths = manifest.files().map(|file| file.path()).collect::<Vec<_>>();
    assert_eq!(
        paths,
        ["Cargo.lock", "Cargo.toml", "build.rs", "src/main.rs"]
    );
    assert!(manifest.generated_manifest().entries().len() > 8);
    assert!(!manifest.generated_manifest().source_map().is_empty());
    assert_eq!(
        manifest.generated_manifest().build_request_key(),
        manifest.generated_manifest().build_request_key()
    );
}

#[test]
fn carrier_is_purely_derived_and_the_build_script_has_no_hash_cycle() {
    let _turn = rawr_import::store::test_turn();
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
