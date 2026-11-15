//! What one run of the suite holds under [`super::scratch`] for its tests,
//! and when each part goes. Every part is removed by the code that made it:
//!
//! - A test's own directory, a [`super::Scratch`], goes when the test ends,
//!   whether it passed or failed.
//! - What the run's tests share goes when the run's process exits: each
//!   fixture's lanes ([`super::fixture`]), each started fixture's slots
//!   ([`super::started`]), and the shared `RAWR_HOME` with its warm-up
//!   project ([`super::runtime`]). The code that makes one of them names it
//!   here first, so a run removes only what it made. A run that is killed
//!   leaves them at their paths, and the next run that makes one there
//!   removes it first: no part comes from an earlier run.
//!
//! A completed run leaves nothing in the scratch directory. With
//! `RAWR_KEEP_TEST_SCRATCH=1`, a test that fails keeps what it held, to be
//! looked at: its own directories, its lane or slot, and the run's shared
//! home. Every other part goes as it would without the variable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

/// The variable that keeps a failing test's parts.
const KEEP: &str = "RAWR_KEEP_TEST_SCRATCH";

/// The parts this run removes when it exits, and those a failure keeps.
struct Held {
    /// Each part's path, removed with all under it but what is kept.
    parts: Vec<PathBuf>,
    /// The paths a failing test kept, within a part or one of their own.
    kept: Vec<PathBuf>,
    /// The parts that serve every test, which a kept failure keeps too.
    shared: Vec<PathBuf>,
}

static HELD: Mutex<Held> = Mutex::new(Held {
    parts: Vec::new(),
    kept: Vec::new(),
    shared: Vec::new(),
});

/// Remove `path` when this run's process exits: a part that several of
/// the run's tests use in turn.
pub fn until_the_run_ends(path: PathBuf) {
    removed_at_exit();
    held().parts.push(path);
}

/// Remove `path` when this run's process exits, unless a failure was kept:
/// a part every test may use, whose state a failure's may depend on.
pub fn shared_until_the_run_ends(path: PathBuf) {
    removed_at_exit();
    held().shared.push(path);
}

/// Whether the current test is failing and its parts are to stay. The test
/// names each part it keeps with [`keep`].
pub fn failure_kept() -> bool {
    std::thread::panicking() && std::env::var_os(KEEP).is_some_and(|keep| keep == "1")
}

/// Keep `path` and all under it when the run ends: a part a failing test
/// held, to be looked at, which its own code then leaves in place. The
/// run's shared parts stay with it.
pub fn keep(path: PathBuf) {
    held().kept.push(path);
}

/// Remove `path` and all under it, if it is there: what an earlier run
/// left where a part is made.
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

fn held() -> MutexGuard<'static, Held> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Remove every part this process named, once it exits: libtest exits the
/// process after its last test, passed or failed, and runs no destructor of
/// a static, so the parts go through the C library's exit handlers. Off
/// Unix, a run leaves them.
fn removed_at_exit() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        #[cfg(unix)]
        {
            extern "C" fn end() {
                remove();
            }
            // SAFETY: `end` is a function of the program, which lives
            // until the process is gone, takes no argument, and does not
            // unwind: `remove` ignores every error a removal returns, takes
            // the lock whether or not a panic poisoned it, and reads no
            // thread-local, which may be gone by the time the process exits.
            let _ = unsafe { nix::libc::atexit(end) };
        }
    });
}

/// Remove the parts, all but what is kept.
fn remove() {
    let held = held();
    let shared = if held.kept.is_empty() {
        held.shared.as_slice()
    } else {
        &[]
    };
    let scratch = super::scratch_directory();
    for part in held.parts.iter().chain(shared) {
        remove_but(part, &held.kept);
        // The directory parts of one kind share goes with the last of them,
        // but the scratch directory stays: it may be the caller's.
        if let Some(parent) = part.parent().filter(|parent| *parent != scratch) {
            let _ = fs::remove_dir(parent);
        }
    }
}

/// Remove `path` and all under it but the paths in `kept`.
fn remove_but(path: &Path, kept: &[PathBuf]) {
    if !kept.iter().any(|keep| keep.starts_with(path)) {
        let _ = fs::remove_dir_all(path).or_else(|_| fs::remove_file(path));
        return;
    }
    if kept.iter().any(|keep| keep == path) {
        return;
    }
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        remove_but(&entry.path(), kept);
    }
}
