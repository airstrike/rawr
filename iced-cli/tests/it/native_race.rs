use crate::p3;

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn latest_miss_is_the_only_cold_candidate_allowed_to_admit() {
    let _turn = rawr_import::store::test_turn();
    let root = project("native-race");
    let generated = p3::generated_at(&root);
    let mut lane = rawr_iced_cli::watch::native::Lane::new(&root).expect("the native lane binds");
    let initial = lane
        .replace(generated, p3::snapshot_transport())
        .expect("the initial runner admits");
    let mut revisions = rawr_import::supervisor::Owner::new();
    let first_revision = revisions
        .revision()
        .expect("the first miss revision exists");
    let second_revision = revisions
        .revision()
        .expect("the newest miss revision exists");
    let (first, first_snapshot) = p3::request_at_revision(
        &root,
        b"first changed surface",
        p3::Surface::First,
        first_revision,
    );
    let (second, second_snapshot) = p3::request_at_revision(
        &root,
        b"newest changed surface",
        p3::Surface::First,
        second_revision,
    );
    let rawr_import::process::hot::Outcome::Miss(first) = lane
        .compile(first, first_snapshot)
        .expect("the first miss closes the gate")
    else {
        panic!("the first changed surface must miss")
    };
    let rawr_import::process::hot::Outcome::Miss(second) = lane
        .compile(second, second_snapshot)
        .expect("the newest miss remains cold while the gate is closed")
    else {
        panic!("the closed gate cannot hot-admit a newer request")
    };
    let mut latest = rawr_import::process::hot::Latest::new();
    assert!(latest.coalesce(first).is_none());
    let obsolete = latest
        .coalesce(second)
        .expect("the newer request returns the obsolete continuation");
    let (obsolete, obsolete_snapshot) = obsolete.into_parts();
    let obsolete = obsolete
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the already-running obsolete request can finish generation");
    assert!(matches!(
        lane.replace(obsolete, obsolete_snapshot),
        Err(rawr_iced_cli::watch::native::Error::Obsolete)
    ));
    assert_eq!(lane.current_key(), Some(initial));

    let (newest, newest_snapshot) = latest
        .take()
        .expect("the newest complete request remains owned")
        .into_parts();
    let newest = newest
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the newest request generates once");
    let admitted = lane
        .replace(newest, newest_snapshot)
        .expect("only the newest cold candidate admits");
    assert_ne!(admitted, initial);
    lane.shutdown().expect("the newest runner retires");
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
