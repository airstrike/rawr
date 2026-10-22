#[test]
fn native_lane_rejects_a_missing_project_without_partial_state() {
    let missing =
        std::env::temp_dir().join(format!("rawr-cli-missing-native-{}", std::process::id()));
    assert!(rawr_iced_cli::watch::native::Lane::<()>::new(&missing).is_err());
}
