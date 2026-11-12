//! What extraction builds leave in rawr home's nightly build directory.
//!
//! Every project's extraction builds there (D700), and only rawr builds
//! there, so every unit in it is rawr's. Each build records the units it
//! named under its key: the project, the target, the profile and which of
//! extraction's builds it is. After each build, a unit no live record names
//! goes. A record lives while its project's manifest is on disk and its
//! format is this one; the next build of its key replaces it. Nothing goes
//! by age.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;

use super::named::Named;

/// One extraction build's key: what it builds, for which project.
pub(crate) struct Key<'build> {
    pub(crate) project: &'build Path,
    pub(crate) build: Build,
    pub(crate) target: &'build str,
    pub(crate) profile: &'build str,
}

/// Which of a project's builds in the nightly directory a build is. Each
/// one's manifest lives in a bundle of its own, so its path names no build.
#[derive(Clone, Copy)]
pub(crate) enum Build {
    /// The resolution's harness.
    Resolution,
    /// The extraction plan's build.
    Extraction,
}

/// What the last build of one key named.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    /// The project's manifest.
    manifest: PathBuf,
    named: BTreeSet<String>,
}

/// The records' format. A record of another is read as gone.
const VERSION: u32 = 1;

/// Where the records live in the nightly directory; Cargo writes no
/// dot-named entry there.
const RECORDS: &str = ".rawr-records";

/// After a successful build of `key` in `shared`, the nightly directory,
/// record what Cargo's `messages` (JSON) name and remove every unit no live
/// record names, and every incremental session but each directory's last.
/// The caller holds the directory's lock, so no other build runs there.
pub(crate) fn prune(shared: &Path, messages: &[u8], key: &Key<'_>) {
    let profiles = super::profiles(shared);
    let named = Named::read(messages, &profiles, None);
    if named.is_empty() {
        return;
    }
    let records = shared.join(RECORDS);
    let record = Record {
        version: VERSION,
        manifest: key.project.join("Cargo.toml"),
        named: named.hashes().map(str::to_owned).collect(),
    };
    let own = records.join(format!("{}.json", key.digest()));
    let written = serde_json::to_vec(&record).is_ok_and(|bytes| {
        let staged = own.with_extension(format!("json.{}", std::process::id()));
        fs::create_dir_all(&records)
            .and_then(|()| fs::write(&staged, bytes))
            .and_then(|()| fs::rename(&staged, &own))
            .is_ok()
    });
    // Without its record this build's units would be another's to remove.
    if !written {
        return;
    }
    let mut live = BTreeSet::new();
    for entry in fs::read_dir(&records).into_iter().flatten().flatten() {
        let path = entry.path();
        let record = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
            .filter(|record| record.version == VERSION)
            .filter(|record| record.manifest.is_file());
        match record {
            Some(record) => live.extend(record.named),
            None => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    for profile in &profiles {
        // Each live unit of a crate may have an incremental directory, which
        // no message names: a crate keeps that many, its newest.
        let mut crates = BTreeMap::<String, usize>::new();
        for directory in ["deps", ".fingerprint", "build"] {
            for entry in fs::read_dir(profile.join(directory))
                .into_iter()
                .flatten()
                .flatten()
            {
                let file = entry.file_name();
                let Some((krate, hash)) = file.to_str().and_then(super::unit_key) else {
                    continue;
                };
                if !live.contains(hash) {
                    remove(&entry.path());
                } else if directory == ".fingerprint" {
                    *crates.entry(krate).or_default() += 1;
                }
            }
        }
        let mut directories = BTreeMap::<String, Vec<(std::time::SystemTime, PathBuf)>>::new();
        for entry in fs::read_dir(profile.join("incremental"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let name = entry.file_name();
            let Some((krate, _)) = name.to_str().and_then(|name| name.rsplit_once('-')) else {
                continue;
            };
            let time = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            directories
                .entry(krate.to_owned())
                .or_default()
                .push((time, path));
        }
        for (krate, mut found) in directories {
            found.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
            let kept = crates.get(&krate).copied().unwrap_or(0);
            for (_, path) in found.drain(kept.min(found.len())..) {
                remove(&path);
            }
            for (_, path) in &found {
                super::prune_sessions(path);
            }
        }
    }
}

impl Key<'_> {
    /// The record name of this key.
    fn digest(&self) -> String {
        use sha2::Digest as _;
        let mut digest = sha2::Sha256::new();
        for part in [
            self.project.as_os_str().as_encoded_bytes(),
            match self.build {
                Build::Resolution => b"resolution".as_slice(),
                Build::Extraction => b"extraction".as_slice(),
            },
            self.target.as_bytes(),
            self.profile.as_bytes(),
        ] {
            digest.update((part.len() as u64).to_le_bytes());
            digest.update(part);
        }
        digest.finalize()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

fn remove(path: &Path) {
    let _ = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A checked unit of `name`, its rmeta and fingerprint.
    fn unit(profile: &Path, name: &str, digit: char) -> [PathBuf; 2] {
        let hash: String = std::iter::repeat_n(digit, 16).collect();
        let rmeta = profile.join(format!("deps/lib{name}-{hash}.rmeta"));
        fs::write(&rmeta, b"").unwrap();
        let fingerprint = profile.join(format!(".fingerprint/{name}-{hash}"));
        fs::create_dir_all(&fingerprint).unwrap();
        [rmeta, fingerprint]
    }

    fn messages(units: &[&[PathBuf; 2]]) -> String {
        let lines: Vec<String> = units
            .iter()
            .map(|unit| {
                format!(
                    r#"{{"reason":"compiler-artifact","fresh":false,"filenames":["{}"]}}"#,
                    unit[0].display()
                )
            })
            .collect();
        lines.join("\n")
    }

    /// Two projects' extractions share the nightly directory. A unit goes
    /// once no live project's last build names it, and a project whose
    /// manifest is gone names nothing.
    #[test]
    fn the_nightly_directory_keeps_what_live_projects_last_named() {
        let root = crate::store::TestProject::new("rawr-deps-nightly");
        let shared = root.path().join("nightly");
        let profile = shared.join("aarch64-apple-darwin/debug");
        for directory in ["deps", ".fingerprint", "build", "incremental"] {
            fs::create_dir_all(profile.join(directory)).unwrap();
        }
        let (first, second) = (root.path().join("a"), root.path().join("b"));
        for project in [&first, &second] {
            fs::create_dir_all(project.join("target")).unwrap();
            fs::write(project.join("Cargo.toml"), b"").unwrap();
        }
        let prune = |project: &Path, named: &[&[PathBuf; 2]]| {
            let key = super::Key {
                project,
                build: super::Build::Resolution,
                target: "aarch64-apple-darwin",
                profile: "dev",
            };
            super::prune(&shared, messages(named).as_bytes(), &key);
        };
        let syn = unit(&profile, "syn", 'a');
        let serde = unit(&profile, "serde", 'b');
        let unrecorded = unit(&profile, "serde", 'c');
        let harness = unit(&profile, "harness", 'd');
        let incremental = |name: &str| {
            let path = profile.join(format!("incremental/harness-{name}"));
            fs::create_dir_all(&path).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            path
        };
        let (old, new) = (incremental("old"), incremental("new"));
        let session = |name: &str| {
            let path = new.join(name);
            fs::create_dir_all(&path).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            path
        };
        let (earlier, last) = (session("s-a-1-x"), session("s-b-2-y"));
        let exists = |unit: &[PathBuf; 2]| unit.iter().all(|path| path.exists());

        prune(&first, &[&syn, &serde, &harness]);
        assert!(exists(&syn) && exists(&serde) && exists(&harness));
        assert!(!exists(&unrecorded), "no record names it");
        assert!(
            new.exists() && !old.exists(),
            "one harness unit, its newest directory"
        );
        assert!(
            last.exists() && !earlier.exists(),
            "the directory's last session"
        );

        // The second project names serde alone; syn stays the first's.
        prune(&second, &[&serde]);
        assert!(exists(&syn) && exists(&serde));
        // The first project's next build supersedes syn.
        let syn_next = unit(&profile, "syn", 'e');
        prune(&first, &[&syn_next, &serde, &harness]);
        assert!(!exists(&syn) && exists(&syn_next));
        // Once the first project is gone, only the second's units stay.
        fs::remove_file(first.join("Cargo.toml")).unwrap();
        prune(&second, &[&serde]);
        assert!(!exists(&syn_next) && !exists(&harness) && exists(&serde));
        assert!(!new.exists(), "no harness unit is left");
        assert_eq!(
            fs::read_dir(shared.join(super::RECORDS)).unwrap().count(),
            1
        );
    }
}
