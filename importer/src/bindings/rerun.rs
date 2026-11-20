//! A build script whose output differs between rawr's nightly build and the
//! project's own (D742).
//!
//! Cargo runs a build script again only when an input it declares changes
//! (`rerun-if-changed`, `rerun-if-env-changed`). In the shared nightly
//! directory (D700) a script that reads an environment variable it does not
//! declare keeps the output of whichever project last ran it, as in any
//! shared `CARGO_TARGET_DIR`. rawr treats a difference as a miss once: the
//! next nightly build cleans the package (`cargo clean -p`), so Cargo runs
//! its script again, and extraction runs again. A difference that survives
//! that run is the toolchains' own (D608.3). The nightly directory keeps it
//! with both outputs, for every project that builds there, and the project
//! keeps it too. A later run cleans again only for a pair of outputs no run
//! again has confirmed there.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Script;

/// Where the project's record lives, in its Rawr directory.
pub(super) const RECORD: &str = "build-scripts-differ.json";
/// Where a shared nightly directory keeps the packages its next build
/// cleans first.
const STALE: &str = ".rawr-stale";
/// Where a shared nightly directory keeps the differences a run again
/// confirmed there.
const CONFIRMED: &str = ".rawr-build-scripts-differ.json";

/// The packages whose runs in one shared nightly directory are stale for a
/// project: one empty file per package name. The next build there cleans
/// each of them, under the directory's lock.
pub(crate) struct Stale(PathBuf);

impl Stale {
    /// The stale packages of the nightly build directory `nightly`.
    pub(crate) fn of(nightly: &Path) -> Self {
        Self(nightly.join(STALE))
    }

    /// Have the next build clean `package`, a Cargo package id. Best effort:
    /// a mark not written leaves the difference to be taken as drift after
    /// the run again finds it.
    pub(crate) fn mark(&self, package: &str) {
        let name = super::package_name(package);
        if fs::create_dir_all(&self.0).is_ok() {
            let _ = fs::write(self.0.join(name), []);
        }
    }

    /// The package names a build cleans before it runs.
    pub(crate) fn pending(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.0) else {
            return Vec::new();
        };
        let mut names = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// Whether a package the project in `rawr` sent to run again is still
    /// to be cleaned here: what the project resolved before holds its stale
    /// run.
    pub(crate) fn awaits(&self, rawr: &Path) -> bool {
        let record = Record::read(rawr, None);
        let mut reruns = record.reruns().peekable();
        if reruns.peek().is_none() {
            return false;
        }
        let pending = self.pending();
        reruns.any(|package| {
            pending
                .iter()
                .any(|name| name == super::package_name(package))
        })
    }

    /// `name` is clean: Cargo runs its build script at the next build.
    pub(crate) fn cleaned(&self, name: &str) {
        let _ = fs::remove_file(self.0.join(name));
    }
}

/// What a project knows of the packages whose build-script outputs
/// differed between the nightly build and its own, with the differences
/// its nightly build directory confirmed for every project.
pub(crate) struct Record {
    path: PathBuf,
    packages: BTreeMap<String, Seen>,
    changed: bool,
    confirmed: Option<Confirmed>,
}

/// The two outputs of one package's build script: the nightly build's and
/// the project's.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    nightly: Vec<Script>,
    project: Vec<Script>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
enum Seen {
    /// The nightly's run of the package was cleaned to run again.
    Rerun,
    /// The outputs still differed after that run: these two.
    Drift(Pair),
}

/// What one difference calls for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    /// The nightly may hold a stale run: clean it and extract again.
    Rerun,
    /// A run again already gave these outputs: the toolchains differ.
    Drift,
}

impl Record {
    /// The record of the project whose Rawr directory is `rawr`, with the
    /// differences the nightly build directory `nightly` confirmed. One
    /// that is absent or unreadable records nothing.
    pub(crate) fn read(rawr: &Path, nightly: Option<&Path>) -> Self {
        let path = rawr.join(RECORD);
        Self {
            packages: read_json(&path),
            path,
            changed: false,
            confirmed: nightly.map(Confirmed::of),
        }
    }

    /// Judge `package`, whose nightly and project outputs differ. Call once
    /// per package per settlement: a package already cleaned to run again
    /// that still differs is drift, and so is a pair of outputs a run again
    /// confirmed in the nightly directory, for any project.
    pub(crate) fn judge(
        &mut self,
        package: &str,
        nightly: &[Script],
        project: &[Script],
    ) -> Verdict {
        let pair = Pair {
            nightly: nightly.to_vec(),
            project: project.to_vec(),
        };
        let seen = self.packages.get(package);
        let confirmed = match seen {
            Some(Seen::Rerun) => true,
            Some(Seen::Drift(kept)) => *kept == pair,
            None => false,
        } || self
            .confirmed
            .as_ref()
            .is_some_and(|confirmed| confirmed.holds(package, &pair));
        if !confirmed {
            self.packages.insert(package.to_owned(), Seen::Rerun);
            self.changed = true;
            return Verdict::Rerun;
        }
        if let Some(shared) = &mut self.confirmed {
            shared.add(package, &pair);
        }
        let drift = Seen::Drift(pair);
        if seen != Some(&drift) {
            self.packages.insert(package.to_owned(), drift);
            self.changed = true;
        }
        Verdict::Drift
    }

    /// `package` gave the nightly and the project the same outputs.
    pub(crate) fn agreed(&mut self, package: &str) {
        self.changed |= self.packages.remove(package).is_some();
    }

    /// Keep what changed. Best effort: a record not kept cleans once more.
    pub(crate) fn write(self) {
        if let Some(confirmed) = self.confirmed {
            confirmed.write();
        }
        if !self.changed {
            return;
        }
        if self.packages.is_empty() {
            let _ = fs::remove_file(&self.path);
            return;
        }
        write_json(&self.path, &self.packages);
    }

    /// The packages this project sent to run again.
    fn reruns(&self) -> impl Iterator<Item = &str> {
        self.packages
            .iter()
            .filter(|(_, seen)| **seen == Seen::Rerun)
            .map(|(package, _)| package.as_str())
    }
}

/// The pairs of outputs a run again confirmed in one shared nightly build
/// directory, by package. The directory belongs to one nightly toolchain,
/// and the project's toolchain gives the project side of a pair, so a pair
/// kept here holds for every project whose build gives that output.
struct Confirmed {
    path: PathBuf,
    packages: BTreeMap<String, Vec<Pair>>,
    added: Vec<(String, Pair)>,
}

impl Confirmed {
    fn of(nightly: &Path) -> Self {
        let path = nightly.join(CONFIRMED);
        Self {
            packages: read_json(&path),
            path,
            added: Vec::new(),
        }
    }

    fn holds(&self, package: &str, pair: &Pair) -> bool {
        self.packages
            .get(package)
            .is_some_and(|pairs| pairs.contains(pair))
    }

    fn add(&mut self, package: &str, pair: &Pair) {
        if self.holds(package, pair) {
            return;
        }
        self.packages
            .entry(package.to_owned())
            .or_default()
            .push(pair.clone());
        self.added.push((package.to_owned(), pair.clone()));
    }

    /// Add what this run confirmed to what the directory holds now, which
    /// another project may have added to since it was read. Best effort: a
    /// pair not kept costs one more run again.
    fn write(self) {
        if self.added.is_empty() {
            return;
        }
        let mut packages = read_json::<BTreeMap<String, Vec<Pair>>>(&self.path);
        for (package, pair) in self.added {
            let pairs = packages.entry(package).or_default();
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
        write_json(&self.path, &packages);
    }
}

/// What the JSON file at `path` holds: the default when it is absent or
/// unreadable.
fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Replace the file at `path` with `value` as JSON, whole. Best effort.
fn write_json(path: &Path, value: &impl Serialize) {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    let temporary = path.with_extension(format!("json.{}", std::process::id()));
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&temporary, bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(temporary);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Record, Stale, Verdict};
    use crate::bindings::Script;

    fn scratch(name: &str) -> PathBuf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-rerun-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn script(value: &str) -> Vec<Script> {
        vec![Script {
            cfgs: Vec::new(),
            env: vec![("VALUE".to_owned(), value.to_owned())],
            files: BTreeMap::from([("generated.rs".to_owned(), "digest".to_owned())]),
        }]
    }

    /// Judge one difference as a settlement does: read the records, judge,
    /// keep them.
    fn judge(rawr: &Path, nightly: Option<&Path>, outputs: (&str, &str)) -> Verdict {
        let package = "registry+https://index#demo@1.0.0";
        let mut record = Record::read(rawr, nightly);
        let verdict = record.judge(package, &script(outputs.0), &script(outputs.1));
        record.write();
        verdict
    }

    #[test]
    fn a_difference_reruns_once_and_drift_is_remembered_until_an_output_changes() {
        let root = scratch("record");
        let judge = |nightly: &str, project: &str| judge(&root, None, (nightly, project));
        assert_eq!(judge("one", "two"), Verdict::Rerun);
        assert_eq!(judge("one", "two"), Verdict::Drift);
        // A later run with the same two outputs cleans nothing again.
        assert_eq!(judge("one", "two"), Verdict::Drift);
        // A changed output may be a stale run again.
        assert_eq!(judge("one", "three"), Verdict::Rerun);
        let mut record = Record::read(&root, None);
        record.agreed("registry+https://index#demo@1.0.0");
        record.write();
        assert!(!root.join(super::RECORD).exists());
        assert_eq!(judge("one", "three"), Verdict::Rerun);
        fs::remove_dir_all(root).unwrap();
    }

    /// A difference a run again confirmed is kept in the nightly directory
    /// and persists there: another project that builds there, or this one
    /// with its Rawr directory removed, takes the same pair of outputs as
    /// drift at once, while another pair still runs again once. Projects
    /// with other outputs each keep their own pair.
    #[test]
    fn a_confirmed_difference_holds_for_every_project_of_the_nightly_directory() {
        let root = scratch("confirmed");
        let nightly = root.join("nightly");
        let (first, second) = (root.join("first"), root.join("second"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        assert_eq!(
            judge(&first, Some(&nightly), ("one", "two")),
            Verdict::Rerun
        );
        assert_eq!(
            judge(&first, Some(&nightly), ("one", "two")),
            Verdict::Drift
        );
        assert!(nightly.join(super::CONFIRMED).is_file());
        assert_eq!(
            judge(&second, Some(&nightly), ("one", "two")),
            Verdict::Drift
        );
        // The project's record lost: the nightly directory still knows.
        fs::remove_file(first.join(super::RECORD)).unwrap();
        assert_eq!(
            judge(&first, Some(&nightly), ("one", "two")),
            Verdict::Drift
        );
        assert!(
            first.join(super::RECORD).is_file(),
            "the project keeps the pair it was judged by"
        );
        // Another project's other output is its own pair: it runs again
        // once, and the first pair stays confirmed beside it.
        assert_eq!(
            judge(&second, Some(&nightly), ("one", "three")),
            Verdict::Rerun
        );
        assert_eq!(
            judge(&second, Some(&nightly), ("one", "three")),
            Verdict::Drift
        );
        assert_eq!(
            judge(&first, Some(&nightly), ("one", "two")),
            Verdict::Drift
        );
        // Another nightly directory, another toolchain, knows nothing yet.
        let other = root.join("other-nightly");
        fs::remove_file(first.join(super::RECORD)).unwrap();
        assert_eq!(judge(&first, Some(&other), ("one", "two")), Verdict::Rerun);
        fs::remove_dir_all(root).unwrap();
    }

    /// The record of an earlier rawr reads the same: a pair kept as
    /// `{"Drift": {"nightly": …, "project": …}}` is drift without a run
    /// again.
    #[test]
    fn a_kept_drift_record_reads_as_drift() {
        let root = scratch("kept");
        let package = "registry+https://index#demo@1.0.0";
        let kept = serde_json::json!({
            package: {"Drift": {"nightly": script("one"), "project": script("two")}}
        });
        fs::write(root.join(super::RECORD), kept.to_string()).unwrap();
        assert_eq!(judge(&root, None, ("one", "two")), Verdict::Drift);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_stale_package_is_pending_by_name_until_it_is_cleaned() {
        let root = scratch("stale");
        let stale = Stale::of(&root);
        assert!(stale.pending().is_empty());
        stale.mark("git+file:///fixture/dependency?rev=abc#undeclared-env@0.1.0");
        stale.mark("path+file:///fixture/local#0.1.0");
        assert_eq!(stale.pending(), ["local", "undeclared-env"]);
        stale.cleaned("local");
        assert_eq!(stale.pending(), ["undeclared-env"]);
        // A project awaits only the clean of a package it sent to run again.
        let package = "git+file:///fixture/dependency?rev=abc#undeclared-env@0.1.0";
        let rawr = root.join("rawr");
        fs::create_dir_all(&rawr).unwrap();
        assert!(!stale.awaits(&rawr));
        let mut record = Record::read(&rawr, None);
        assert_eq!(
            record.judge(package, &script("one"), &script("two")),
            Verdict::Rerun
        );
        record.write();
        assert!(stale.awaits(&rawr));
        stale.cleaned("undeclared-env");
        assert!(!stale.awaits(&rawr));
        fs::remove_dir_all(root).unwrap();
    }
}
