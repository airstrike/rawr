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
//! that run is the toolchains' own (D608.3). It is remembered with both
//! outputs, so a later run cleans again only when one of them changes.

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
    /// a mark not written leaves the difference to be reported as drift
    /// after the run again finds it.
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

    /// `name` is clean: Cargo runs its build script at the next build.
    pub(crate) fn cleaned(&self, name: &str) {
        let _ = fs::remove_file(self.0.join(name));
    }
}

/// What a project knows of the packages whose build-script outputs
/// differed between the nightly build and its own.
pub(crate) struct Record {
    path: PathBuf,
    packages: BTreeMap<String, Seen>,
    changed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
enum Seen {
    /// The nightly's run of the package was cleaned to run again.
    Rerun,
    /// The outputs still differed after that run: these two.
    Drift {
        nightly: Vec<Script>,
        project: Vec<Script>,
    },
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
    /// The record of the project whose Rawr directory is `rawr`. One that
    /// is absent or unreadable records nothing.
    pub(crate) fn read(rawr: &Path) -> Self {
        let path = rawr.join(RECORD);
        let packages = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            packages,
            changed: false,
        }
    }

    /// Judge `package`, whose nightly and project outputs differ. Call once
    /// per package per settlement: a package already cleaned to run again
    /// that still differs is drift.
    pub(crate) fn judge(
        &mut self,
        package: &str,
        nightly: &[Script],
        project: &[Script],
    ) -> Verdict {
        let drift = Seen::Drift {
            nightly: nightly.to_vec(),
            project: project.to_vec(),
        };
        let (seen, verdict) = match self.packages.get(package) {
            Some(seen) if *seen == drift => return Verdict::Drift,
            Some(Seen::Rerun) => (drift, Verdict::Drift),
            Some(Seen::Drift { .. }) | None => (Seen::Rerun, Verdict::Rerun),
        };
        self.packages.insert(package.to_owned(), seen);
        self.changed = true;
        verdict
    }

    /// `package` gave the nightly and the project the same outputs.
    pub(crate) fn agreed(&mut self, package: &str) {
        self.changed |= self.packages.remove(package).is_some();
    }

    /// Keep what changed. Best effort: a record not kept cleans once more.
    pub(crate) fn write(self) {
        if !self.changed {
            return;
        }
        if self.packages.is_empty() {
            let _ = fs::remove_file(&self.path);
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&self.packages) else {
            return;
        };
        let temporary = self
            .path
            .with_extension(format!("json.{}", std::process::id()));
        if fs::write(&temporary, bytes).is_err() || fs::rename(&temporary, &self.path).is_err() {
            let _ = fs::remove_file(temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

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
            out_dir: "digest".to_owned(),
        }]
    }

    #[test]
    fn a_difference_reruns_once_and_drift_is_remembered_until_an_output_changes() {
        let root = scratch("record");
        let package = "registry+https://index#demo@1.0.0";
        let judge = |nightly: &str, project: &str| {
            let mut record = Record::read(&root);
            let verdict = record.judge(package, &script(nightly), &script(project));
            record.write();
            verdict
        };
        assert_eq!(judge("one", "two"), Verdict::Rerun);
        assert_eq!(judge("one", "two"), Verdict::Drift);
        // A later run with the same two outputs cleans nothing again.
        assert_eq!(judge("one", "two"), Verdict::Drift);
        // A changed output may be a stale run again.
        assert_eq!(judge("one", "three"), Verdict::Rerun);
        let mut record = Record::read(&root);
        record.agreed(package);
        record.write();
        assert!(!root.join(super::RECORD).exists());
        assert_eq!(judge("one", "three"), Verdict::Rerun);
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
        fs::remove_dir_all(root).unwrap();
    }
}
