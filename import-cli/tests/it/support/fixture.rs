//! A fixture project that several tests run programs in, prepared in a few
//! lanes that the tests borrow one at a time. A lane is a copy of the
//! fixture that keeps Cargo's build units between the run's tests, so the
//! fixture's crates and its runner's dependencies compile once per lane in
//! a run, and a test compiles only what its own program changes. Rawr's
//! own products for the project, `target/rawr`, stay too, as they do for a
//! user who replaces one program with another in a project: each test's
//! program runs as it would fresh whichever programs ran in the lane before
//! (#893). Lanes are made as tests ask for them, up to one per test thread,
//! so tests of one fixture run in parallel. Lanes stay between runs, and a
//! run takes again the lanes the last one left ([`super::held`]); a lane a
//! failing test kept is not lent again, and another is made in its place.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, Once, PoisonError};

use crate::support::held;

/// A fixture under `tests/fixtures` and its lanes.
pub struct Fixture {
    name: &'static str,
    /// Files every lane holds beside the fixture's own: each path in the
    /// lane and the fixture file it holds.
    extra: &'static [(&'static str, &'static str)],
    lanes: Mutex<Lanes>,
    returned: Condvar,
}

/// The lanes made so far in this run and those no test holds.
struct Lanes {
    /// The numbers of the lanes that serve tests and of the lanes failures
    /// kept.
    taken: BTreeSet<usize>,
    claims: held::Claims,
    /// The lanes that serve tests: those no test holds, and those held by
    /// a test that has not kept its failure.
    live: usize,
    free: Vec<PathBuf>,
}

/// A lane one test holds: the fixture's files, with the test's own written
/// over them. The test may change the lane as it likes; the next lease
/// puts the fixture's files back.
pub struct Lease {
    fixture: &'static Fixture,
    root: PathBuf,
}

impl Fixture {
    /// The fixture `tests/fixtures/{name}`.
    pub const fn new(name: &'static str) -> Self {
        Self::with(name, &[])
    }

    /// The fixture `tests/fixtures/{name}`, whose lanes also hold each of
    /// `extra`'s fixture files at its path.
    pub const fn with(name: &'static str, extra: &'static [(&'static str, &'static str)]) -> Self {
        Self {
            name,
            extra,
            lanes: Mutex::new(Lanes {
                taken: BTreeSet::new(),
                claims: held::Claims::new(),
                live: 0,
                free: Vec::new(),
            }),
            returned: Condvar::new(),
        }
    }

    /// The fixture's own directory.
    pub fn source(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(self.name)
    }

    /// The text of the fixture's file at `path`.
    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.source().join(path)).unwrap()
    }

    /// A lane holding the fixture's files with `files`, each a path in the
    /// lane and its text, written over them. It waits for a lane when every
    /// one is held and no more can be made.
    pub fn lease(&'static self, files: &[(&str, &str)]) -> Lease {
        crate::support::driver();
        // Before the lanes' lock: the test may wait here for its turn.
        let scratch = crate::support::scratch();
        static PRUNED: Once = Once::new();
        PRUNED.call_once(|| held::prune_tree(&scratch.join("lanes"), limit()));
        let root = {
            let mut lanes = self.lanes();
            loop {
                if let Some(root) = lanes.free.pop() {
                    break root;
                }
                if lanes.live < limit() {
                    // A lane an earlier run left stays warm: its files are
                    // settled below, and Cargo and rawr keep its builds.
                    let lanes_of_fixture = scratch.join("lanes").join(self.name);
                    let Lanes { taken, claims, .. } = &mut *lanes;
                    let number = claims.number(&lanes_of_fixture, taken);
                    taken.insert(number);
                    lanes.live += 1;
                    break lanes_of_fixture.join(number.to_string());
                }
                lanes = self
                    .returned
                    .wait(lanes)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let lease = Lease {
            fixture: self,
            root,
        };
        let mut wanted = BTreeMap::new();
        gather(&self.source(), Path::new(""), &mut wanted);
        for (path, from) in self.extra {
            wanted.insert(
                PathBuf::from(path),
                fs::read(self.source().join(from)).unwrap(),
            );
        }
        for (path, text) in files {
            wanted.insert(PathBuf::from(path), text.as_bytes().to_vec());
        }
        settle(&lease.root, &wanted);
        lease
    }

    fn lanes(&self) -> MutexGuard<'_, Lanes> {
        self.lanes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Deref for Lease {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<Path> for Lease {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut lanes = self.fixture.lanes();
        if held::failure_kept() {
            // Left as the failure left it, out of the pool.
            lanes.live -= 1;
        } else {
            lanes.free.push(std::mem::take(&mut self.root));
        }
        drop(lanes);
        self.fixture.returned.notify_one();
    }
}

/// One lane per test thread, as many as Cargo runs tests on by default.
fn limit() -> usize {
    std::env::var("RUST_TEST_THREADS")
        .ok()
        .and_then(|threads| threads.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
        .max(1)
}

/// Every file under `directory`, by its path under `base`, with its bytes,
/// but what Cargo makes at the fixture's root, `target/` and `Cargo.lock`:
/// a build of the fixture where it sits is no part of it, and its stale
/// units would pass for fresh ones in a lane.
fn gather(directory: &Path, base: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = base.join(entry.file_name());
        if path == Path::new("target") || path == Path::new("Cargo.lock") {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            gather(&entry.path(), &path, files);
        } else {
            files.insert(path, fs::read(entry.path()).unwrap());
        }
    }
}

/// Make `root` hold `wanted` and nothing else but what Cargo and rawr make
/// there, `target/` and `Cargo.lock`. A file that already holds its bytes
/// is left alone, so its modification time tells Cargo it hasn't changed.
fn settle(root: &Path, wanted: &BTreeMap<PathBuf, Vec<u8>>) {
    let directories = wanted
        .keys()
        .flat_map(|path| path.ancestors().skip(1))
        .map(Path::to_path_buf)
        .collect::<BTreeSet<_>>();
    let mut pending = vec![PathBuf::new()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(root.join(&directory)).into_iter().flatten() {
            let entry = entry.unwrap();
            let path = directory.join(entry.file_name());
            let built = directory.as_os_str().is_empty()
                && (path == Path::new("target") || path == Path::new("Cargo.lock"));
            if built {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                if directories.contains(&path) {
                    pending.push(path);
                } else {
                    fs::remove_dir_all(entry.path()).unwrap();
                }
            } else if !wanted.contains_key(&path) {
                fs::remove_file(entry.path()).unwrap();
            }
        }
    }
    for (path, bytes) in wanted {
        let target = root.join(path);
        if fs::read(&target).is_ok_and(|held| held == *bytes) {
            continue;
        }
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, bytes).unwrap();
    }
}
