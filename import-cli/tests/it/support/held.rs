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
//! Lanes and slots are numbered parts of their fixture, and a run holds
//! each one it uses by a [`Claim`]. A run takes the lowest numbers no live
//! run holds, so it takes again the parts the last run left. Before its
//! first part of a kind, a run removes every part of that kind that no
//! live run holds and no run takes again ([`prune_tree`]): those numbered
//! above what one run takes, which a killed run, a concurrent one or kept
//! failures made, and every part of a fixture the suite no longer has.
//!
//! With `RAWR_KEEP_TEST_SCRATCH=1`, a test that fails leaves what it held
//! in place, to be looked at, instead of removing its own directory or
//! giving its slot up. A kept part stays to the next run's start, which
//! takes it again or removes it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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

/// A run's hold on one numbered part under a fixture's directory: the
/// lock on the part's claim file, `{number}.claim` beside it. The lock
/// belongs to the open file, so the system lets it go when the run ends,
/// however it ends, and a part whose claim no process locks belongs to no
/// live run.
pub struct Claim {
    _file: fs::File,
}

impl Claim {
    /// Hold part `number` under `parts`, unless another holder has it: a
    /// live run, or this run through another claim.
    ///
    /// # Panics
    ///
    /// If the claim file can't be opened, or can't be locked for a reason
    /// other than another holder's lock.
    pub fn take(parts: &Path, number: usize) -> Option<Self> {
        fs::create_dir_all(parts).unwrap();
        let path = claim_file(parts, number);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("{} did not open: {error}", path.display()));
        match file.try_lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => None,
            Err(fs::TryLockError::Error(error)) => {
                panic!("{} did not lock: {error}", path.display())
            }
        }
    }
}

/// The claims one run holds on one fixture's parts, by number.
pub struct Claims {
    held: BTreeMap<usize, Claim>,
}

impl Claims {
    /// No claims yet.
    pub const fn new() -> Self {
        Self {
            held: BTreeMap::new(),
        }
    }

    /// The lowest number under `parts` outside `taken` that this run holds
    /// or can claim, held from now to the run's end.
    pub fn number(&mut self, parts: &Path, taken: &BTreeSet<usize>) -> usize {
        (1..)
            .filter(|number| !taken.contains(number))
            .find(|&number| {
                self.held.contains_key(&number)
                    || Claim::take(parts, number)
                        .map(|claim| self.held.insert(number, claim))
                        .is_some()
            })
            .expect("numbers outnumber the parts any run holds")
    }
}

/// Prune each fixture's parts under `tree`: to the first `kept`, or to
/// none for a fixture `tests/fixtures` no longer holds.
pub fn prune_tree(tree: &Path, kept: usize) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for entry in fs::read_dir(tree).into_iter().flatten() {
        let entry = entry.unwrap();
        let kept = if fixtures.join(entry.file_name()).is_dir() {
            kept
        } else {
            0
        };
        prune(&entry.path(), kept);
    }
}

/// Remove each part under `parts` numbered above `kept` that no live run
/// holds, with its claim file. Its directory goes before its claim file,
/// so a run that claims the number afresh finds no part there.
pub fn prune(parts: &Path, kept: usize) {
    let numbers = fs::read_dir(parts)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_str()?;
            name.strip_suffix(".claim").unwrap_or(name).parse().ok()
        })
        .filter(|&number: &usize| number > kept)
        .collect::<BTreeSet<_>>();
    for number in numbers {
        let Some(claim) = Claim::take(parts, number) else {
            continue;
        };
        clear(&parts.join(number.to_string()));
        let _ = fs::remove_file(claim_file(parts, number));
        drop(claim);
    }
}

/// The claim file of part `number` under `parts`.
fn claim_file(parts: &Path, number: usize) -> PathBuf {
    parts.join(format!("{number}.claim"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_removes_only_parts_no_live_run_holds() {
        let parts = Path::new(env!("CARGO_TARGET_TMPDIR")).join("held-prune");
        clear(&parts);
        // Lanes a dead run left: their claim files stay, and no process
        // locks them.
        for number in [1, 2] {
            fs::create_dir_all(parts.join(number.to_string())).unwrap();
            drop(Claim::take(&parts, number).expect("no run holds a new claim"));
        }
        // A live run's lane, held by this process.
        let live = Claim::take(&parts, 3).expect("no run holds a new claim");
        fs::create_dir_all(parts.join("3")).unwrap();

        prune(&parts, 1);

        assert!(parts.join("1").is_dir(), "the next run takes lane 1 again");
        assert!(!parts.join("2").exists(), "no live run holds lane 2");
        assert!(
            !claim_file(&parts, 2).exists(),
            "lane 2's claim goes with it"
        );
        assert!(parts.join("3").is_dir(), "a live run holds lane 3");
        drop(live);
        clear(&parts);
    }
}
