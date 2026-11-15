//! What the suite keeps under [`super::scratch`] between its runs, and what
//! goes. Builds that later runs can use stay; only stale ones go, each
//! replaced by the code that makes it when it no longer matches:
//!
//! - A test's own directory, a [`super::Scratch`], goes when the test ends,
//!   whether it passed or failed: nothing reuses it.
//! - Each fixture's lanes ([`super::fixture`]) stay. A lane's files are
//!   settled to the fixture's on every lease, and Cargo and rawr keep its
//!   builds current, so a lane an earlier run left is warm, never stale.
//! - Each started fixture's slots ([`super::started`]) stay while the
//!   stamp they were made with matches: the fixture's sources and the
//!   `rawr-import` that ran them. A slot whose stamp doesn't match is made
//!   again in its place.
//! - The shared `RAWR_HOME` and its warm-up project ([`super::runtime`])
//!   stay; rawr replaces a runtime the moment it builds the one that
//!   supersedes it.
//!
//! With `RAWR_KEEP_TEST_SCRATCH=1`, a test that fails leaves what it held
//! in place, to be looked at, instead of removing its own directory or
//! giving its slot up.

use std::fs;
use std::path::Path;

/// The variable that keeps a failing test's parts.
const KEEP: &str = "RAWR_KEEP_TEST_SCRATCH";

/// Whether the current test is failing and its parts are to stay.
pub fn failure_kept() -> bool {
    std::thread::panicking() && std::env::var_os(KEEP).is_some_and(|keep| keep == "1")
}

/// Remove `path` and all under it, if it is there: a stale part, where a
/// current one is made.
///
/// # Panics
///
/// If `path` is there and does not go.
pub fn clear(path: &Path) {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("{} did not go: {error}", path.display())
        }
        _ => {}
    }
}
