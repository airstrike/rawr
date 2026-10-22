use std::fs;

#[test]
fn optional_native_lane_starts_without_claiming_a_process() {
    let root = project("bootstrap");
    let mut lane = rawr_cli::watch::native::Lane::<()>::new(&root)
        .expect("the optional native lane binds exact local authorities");
    assert_eq!(lane.current_key(), None);
    lane.shutdown().expect("empty shutdown is idempotent");
    let _ = fs::remove_dir_all(root);
}

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("rawr-cli-{name}-{}", std::process::id()));
    fs::create_dir(&root).expect("the CLI native test project exists");
    fs::canonicalize(root).expect("the CLI native test project is canonical")
}
