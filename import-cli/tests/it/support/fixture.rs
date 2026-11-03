//! A fixture project that several tests run programs in, prepared in a few
//! lanes that the tests borrow one at a time. A lane is a copy of the
//! fixture that keeps its `target/` between tests and between suite runs,
//! as a user's project does, so the fixture's crates, its bindings and its
//! runner's dependencies build once per lane, and a test builds only what
//! its own program changes. Lanes are made as tests ask for them, up to one
//! per test thread, so tests of one fixture run in parallel.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// A fixture under `tests/fixtures` and its lanes.
pub struct Fixture {
    name: &'static str,
    /// Files every lane holds beside the fixture's own: each path in the
    /// lane and the fixture file it holds.
    extra: &'static [(&'static str, &'static str)],
    lanes: Mutex<Lanes>,
    returned: Condvar,
}

/// The lanes made so far and those no test holds.
struct Lanes {
    made: usize,
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
                made: 0,
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
        let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
            "rawr-import-driver{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            driver.is_file(),
            "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
            driver.display()
        );
        let root = {
            let mut lanes = self.lanes();
            loop {
                if let Some(root) = lanes.free.pop() {
                    break root;
                }
                if lanes.made < limit() {
                    lanes.made += 1;
                    break crate::support::scratch()
                        .join("lanes")
                        .join(self.name)
                        .join(lanes.made.to_string());
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
        self.fixture.lanes().free.push(self.root.clone());
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

/// Every file under `directory`, by its path under `base`, with its bytes.
fn gather(directory: &Path, base: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = base.join(entry.file_name());
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
