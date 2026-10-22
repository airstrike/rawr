use crate::p5;

#[test]
fn demanded_profile_projection_is_rooted_in_generated_runner_source() {
    let _turn = rawr_import::store::test_turn();
    let project = p5::Project::new("profile-generation");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the first revision exists");
    let (manifest, _snapshot) = p5::generated(
        &project,
        p5::Mode::Success,
        None,
        b"profile generation",
        revision,
    );
    let source = manifest
        .files()
        .find(|file| file.path() == "src/main.rs")
        .expect("the generated runner source exists");
    let source = std::str::from_utf8(source.bytes()).expect("generated Rust is UTF-8");
    assert!(source.contains("mod __rawr_profile"));
    assert!(source.contains("let _anchor = clock::Clock"));
    assert!(source.contains("profile.message(&payload[72..])"));
}
