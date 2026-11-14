//! A fixture project that several tests start from where one run of it
//! left it. A slot is a copy of the fixture at a path of its own, which
//! `rawr-import run` built and ran once in this run of the suite, kept
//! with the bytes and modification times of all it wrote. Each test that
//! takes the slot finds it as that run left it, whatever a test before did
//! there: the same Cargo units, runner and `target/rawr` (#1751). A cold
//! start of a fixture many tests run is paid once per slot, not once per
//! test. Rawr keys a project's builds by its path, so a slot's state
//! serves that slot alone.
//!
//! No slot comes from an earlier run of the suite (#1712). A test waits
//! for a slot when every one is held and no more can be made. A test that
//! fails leaves its slot as it left it, to be looked at, and the next test
//! makes another. A test that runs [`super::alone`] does so before it
//! takes a slot: once it holds one, a test waiting for that slot would
//! never end, and neither would its wait to run alone.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// The most slots of one fixture: a test that waits for one holds a test
/// thread, while each more slot costs one more cold start.
const SLOTS: usize = 3;

/// A fixture under `tests/fixtures` and its slots.
pub struct Started {
    name: &'static str,
    /// The variables of the run that makes a slot, each a name and a path
    /// in the slot: what lets the fixture's program end by itself.
    variables: &'static [(&'static str, &'static str)],
    slots: Mutex<Slots>,
    returned: Condvar,
}

/// The slots made so far in this run and those no test holds.
struct Slots {
    /// The number the next slot takes.
    next: usize,
    /// The slots that serve tests: those no test holds, and those held by
    /// a test that has not failed.
    live: usize,
    free: Vec<PathBuf>,
}

/// A slot a test takes: one a run made and no test holds, or a new one.
enum Taken {
    Kept(PathBuf),
    New(PathBuf),
}

/// A slot one test holds, as the slot's run left it. The test may change
/// it as it likes; the next test finds it as the run left it again.
pub struct Project {
    fixture: &'static Started,
    /// The slot's directory, which holds the project and what its run left.
    slot: PathBuf,
    root: PathBuf,
}

impl Started {
    /// The fixture `tests/fixtures/{name}`, whose run to make a slot has
    /// `variables`, each a name and a path in the slot.
    pub const fn new(
        name: &'static str,
        variables: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            name,
            variables,
            slots: Mutex::new(Slots {
                next: 0,
                live: 0,
                free: Vec::new(),
            }),
            returned: Condvar::new(),
        }
    }

    /// A slot as its run left it. It waits for one when every slot is held
    /// and no more can be made.
    pub fn project(&'static self) -> Project {
        // Before the slots' lock: the test may wait here for its turn.
        let fixtures = crate::support::scratch().join("started").join(self.name);
        let taken = {
            let mut slots = self.slots();
            loop {
                if let Some(slot) = slots.free.pop() {
                    break Taken::Kept(slot);
                }
                if slots.live < SLOTS {
                    // Each run of the suite makes its slots anew.
                    if slots.next == 0 {
                        remove(&fixtures);
                    }
                    slots.next += 1;
                    slots.live += 1;
                    break Taken::New(fixtures.join(slots.next.to_string()));
                }
                slots = self
                    .returned
                    .wait(slots)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        // Held before it is made or restored: a test that fails here gives
        // the slot up, and another can be made in its place.
        match taken {
            Taken::Kept(slot) => {
                let mut project = self.held(slot);
                project.again();
                project
            }
            Taken::New(slot) => {
                let project = self.held(slot);
                self.make(&project.root);
                clone(&project.root, &project.slot.join("kept"));
                project
            }
        }
    }

    fn held(&'static self, slot: PathBuf) -> Project {
        Project {
            fixture: self,
            root: slot.join("project"),
            slot,
        }
    }

    /// Copy the fixture to `root` and run it there once.
    fn make(&self, root: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(self.name);
        crate::support::copy(&source, root);
        let mut run = crate::support::rawr_import();
        run.arg("run")
            .arg(root)
            .stdin(Stdio::null())
            .env("RAWR_HOME", crate::support::runtime_home())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE");
        for (name, path) in self.variables {
            run.env(name, root.join(path));
        }
        let output = run.output().unwrap();
        assert!(
            output.status.success(),
            "{} must run before tests start from it:\nstdout:\n{}\nstderr:\n{}",
            self.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn slots(&self) -> MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Project {
    /// Put the slot back as its run left it, at the same path: each file
    /// the test changed has its bytes and time from before.
    pub fn again(&mut self) {
        remove(&self.root);
        clone(&self.slot.join("kept"), &self.root);
    }
}

impl std::ops::Deref for Project {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<Path> for Project {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<std::ffi::OsStr> for Project {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.root.as_os_str()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let mut slots = self.fixture.slots();
        if std::thread::panicking() {
            slots.live -= 1;
        } else {
            slots.free.push(std::mem::take(&mut self.slot));
        }
        drop(slots);
        self.fixture.returned.notify_one();
    }
}

/// Remove `path` and all under it, if it is there.
fn remove(path: &Path) {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("{} did not go: {error}", path.display())
        }
        _ => {}
    }
}

/// Copy the tree `from` to `to`, each file with its modification time, so
/// Cargo and rawr judge the copy as they judged the tree.
fn clone(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            clone(&path, &target);
            continue;
        }
        // The runner's links to the project's directories stay links, to
        // what they name (`runner::mirror`).
        if entry.file_type().unwrap().is_symlink() {
            let _ = fs::remove_file(&target);
            std::os::unix::fs::symlink(fs::read_link(&path).unwrap(), &target).unwrap();
            continue;
        }
        fs::copy(&path, &target).unwrap();
        // A read-only file's time is set through a handle that reads it.
        let modified = entry.metadata().unwrap().modified().unwrap();
        fs::File::open(&target)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }
}
