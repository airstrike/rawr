use crate::p3;

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn changed_surface_returns_one_complete_cold_continuation() {
    let _turn = rawr_import::store::test_turn();
    let root = project("surface-miss");
    let generated = p3::generated_at(&root);
    let mut lane = rawr_iced_cli::watch::native::Lane::new(&root).expect("the native lane binds");
    let initial = lane
        .replace(generated, p3::snapshot_transport())
        .expect("the initial runner admits");
    let (changed, snapshot) = p3::request_at(&root, b"new native demand", p3::Surface::First);
    let rawr_import::process::hot::Outcome::Miss(miss) = lane
        .compile(changed, snapshot)
        .expect("surface comparison is side-effect free")
    else {
        panic!("the changed adapter table must close the hot gate")
    };
    assert_eq!(lane.current_key(), Some(initial));
    let (request, snapshot) = miss.into_parts();
    let generated = request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the sole cold continuation generates");
    let replacement = lane
        .replace(generated, snapshot)
        .expect("the coalesced miss admits through a fresh process");
    assert_ne!(initial, replacement);
    lane.shutdown().expect("the replacement retires");
    let _ = fs::remove_dir_all(root);
}

fn project(label: &str) -> std::path::PathBuf {
    let root = std::path::Path::new("/tmp").join(format!(
        "rawr-cli-{label}-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the native test root exists");
    fs::canonicalize(root).expect("the native test root is canonical")
}
