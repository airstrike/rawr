use std::fs;

#[test]
fn empty_native_lane_shutdown_preserves_an_empty_route() {
    let root = project("restart");
    let mut lane = rawr_iced_cli::watch::native::Lane::<()>::new(&root)
        .expect("the optional native lane binds");
    lane.shutdown().expect("the first empty shutdown succeeds");
    lane.shutdown().expect("the second empty shutdown succeeds");
    assert_eq!(lane.current_key(), None);
    let _ = fs::remove_dir_all(root);
}

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("rawr-cli-{name}-{}", std::process::id()));
    fs::create_dir(&root).expect("the CLI native test project exists");
    fs::canonicalize(root).expect("the CLI native test project is canonical")
}
