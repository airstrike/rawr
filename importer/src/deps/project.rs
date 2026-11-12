//! What a runner build leaves in the project's build directory.
//!
//! The directory is the one `cargo build` uses, so only rawr's units are
//! rawr's to remove: the units of the packages rawr generates (the runner,
//! its members and what lives under rawr's own directory), the units its
//! build compiled that weren't there before it, and the copies of rawr's own
//! crates. After each runner build, those that no current build of any
//! workspace there names go. A unit of rawr's that one of Cargo's own units
//! still depends on stays: what both build, such as a path package, is one
//! unit (#1777).
//! Nothing goes by age.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;

use super::named::Named;
use crate::bridge::generate;
use crate::bridge::generate::member;
use crate::runner::mirror;

/// The build directory as a runner build found it.
pub(crate) struct Before {
    started: SystemTime,
    /// The units whose fingerprints were there.
    units: BTreeSet<String>,
}

impl Before {
    /// Take `shared` as it is, just before a build there.
    pub(crate) fn take(shared: &Path) -> Self {
        let mut units = BTreeSet::new();
        for profile in super::profiles(shared) {
            let entries = fs::read_dir(profile.join(".fingerprint"))
                .into_iter()
                .flatten()
                .flatten();
            for entry in entries {
                if let Some((_, hash)) = entry.file_name().to_str().and_then(super::unit_key) {
                    units.insert(hash.to_owned());
                }
            }
        }
        Self {
            started: SystemTime::now(),
            units,
        }
    }

    /// Whether the file at `path` was last written before the build began,
    /// so no build that is still running wrote it.
    fn predates(&self, path: &Path) -> bool {
        fs::symlink_metadata(path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|time| time < self.started)
    }
}

/// What one workspace's last runner build in the directory named, and the
/// units rawr's builds made there that are still on disk.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    workspace: PathBuf,
    named: BTreeSet<String>,
    made: BTreeSet<String>,
}

/// The records' format. A record of another is read as gone.
const VERSION: u32 = 1;

/// Where the records live in the build directory.
const RECORDS: &str = "rawr-units";

/// A unit on disk: its crate and its entries in `deps`, `.fingerprint` and
/// `build`.
struct Present {
    krate: String,
    entries: Vec<PathBuf>,
}

/// After a successful runner build in `shared` that found it as `before`,
/// remove what rawr made there that no current build names, from Cargo's
/// messages (JSON). `work` is the runner's workspace, in rawr's directory,
/// where `incremental-directories` lists rawr's incremental directories.
/// Best effort; the caller holds rawr's build lock, and a build of Cargo's
/// own that holds the directory leaves it for the next runner build.
pub(crate) fn prune(shared: &Path, messages: &[u8], before: &Before, work: &Path) {
    let profiles = super::profiles(shared);
    let rawr = work.parent();
    let named = Named::read(messages, &profiles, rawr);
    if named.is_empty() {
        return;
    }
    let Some(_held) = cargo_locks(&profiles) else {
        return;
    };
    let records = shared.join(RECORDS);
    let own = records.join(format!("{}.json", digest(work)));
    let mut made = BTreeSet::new();
    let mut protected = BTreeSet::new();
    for entry in fs::read_dir(&records).into_iter().flatten().flatten() {
        let path = entry.path();
        let record = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
            .filter(|record| record.version == VERSION);
        let Some(record) = record else {
            let _ = fs::remove_file(&path);
            continue;
        };
        made.extend(record.made);
        // A workspace that is gone names nothing; what it made is pruned
        // here like the rest.
        if path != own && record.workspace.join("Cargo.toml").is_file() {
            protected.extend(record.named);
        } else if path != own {
            let _ = fs::remove_file(&path);
        }
    }
    for (hash, unit) in named.iter() {
        if unit.generated() || (unit.rebuilt() && !before.units.contains(hash)) {
            made.insert(hash.to_owned());
        }
    }
    protected.extend(named.hashes().map(str::to_owned));

    let present = present(&profiles);
    // rawr's crates this build used; another install's units of them are
    // superseded (the runner's and its members' are judged by their sources).
    let used: BTreeSet<&str> = named.iter().map(|(_, unit)| unit.krate()).collect();
    let rawr_crate = |krate: &str| {
        used.contains(krate)
            && (krate == "rawr" || krate.starts_with("rawr_"))
            && !krate.starts_with(RUNNER_CRATE)
    };
    let candidates: BTreeSet<&str> = present
        .iter()
        .filter(|(hash, unit)| {
            !protected.contains(*hash)
                && (made.contains(*hash)
                    || rawr_crate(&unit.krate)
                    || ((used.contains(unit.krate.as_str())
                        || unit.krate.starts_with(RUNNER_CRATE))
                        && rawr.is_some_and(|rawr| generated_sources(unit, rawr))))
        })
        .map(|(hash, _)| hash.as_str())
        .collect();
    let depended = depended_on(&present, &candidates);
    for hash in candidates.iter().filter(|hash| !depended.contains(**hash)) {
        for path in &present[*hash].entries {
            if before.predates(path) {
                remove(path);
            }
        }
    }
    let mut current = BTreeMap::<&str, usize>::new();
    for (hash, unit) in named.iter() {
        if made.contains(hash) && unit.crate_target() {
            *current.entry(unit.krate()).or_default() += 1;
        }
    }
    let rebuilt: BTreeSet<&str> = named
        .iter()
        .filter(|(hash, unit)| made.contains(*hash) && unit.crate_target() && unit.rebuilt())
        .map(|(_, unit)| unit.krate())
        .collect();
    for (hash, unit) in named.iter() {
        if made.contains(hash) && unit.rebuilt() {
            prune_objects(&profiles, unit, hash);
        }
    }
    super::prune_incremental(
        &profiles,
        &current,
        &rebuilt,
        &work.join("incremental-directories"),
        &|path| before.predates(path),
    );

    let record = Record {
        version: VERSION,
        workspace: work.to_path_buf(),
        named: named.hashes().map(str::to_owned).collect(),
        made: made
            .into_iter()
            .filter(|hash| {
                present
                    .get(hash)
                    .is_some_and(|unit| unit.entries.iter().any(|path| path.exists()))
            })
            .collect(),
    };
    if let Ok(bytes) = serde_json::to_vec(&record) {
        let staged = own.with_extension(format!("json.{}", std::process::id()));
        let _ = fs::create_dir_all(&records)
            .and_then(|()| fs::write(&staged, bytes))
            .and_then(|()| fs::rename(&staged, &own));
    }
}

/// The runner's crate, as Cargo names its units; its members' begin with it.
const RUNNER_CRATE: &str = "rawr_generated_runner";

/// The units on disk in `profiles`, by hash.
fn present(profiles: &[PathBuf]) -> BTreeMap<String, Present> {
    let mut present = BTreeMap::<String, Present>::new();
    for profile in profiles {
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
                present
                    .entry(hash.to_owned())
                    .or_insert_with(|| Present {
                        krate,
                        entries: Vec::new(),
                    })
                    .entries
                    .push(entry.path());
            }
        }
    }
    present
}

/// Whether `unit`'s dep-info names a first source rawr generated: one
/// under `rawr`, the directory rawr generates its packages in, or, as Cargo
/// writes a workspace member's, one relative to the runner's root, which
/// for any crate but the runner's own lies in its members' or copies'
/// directories. The runner's binary of the script crate's name, whose first
/// source is a `src/main.rs` as the project's own is, names the program's
/// directory among its sources (#1793).
fn generated_sources(unit: &Present, rawr: &Path) -> bool {
    let generated = |source: &Path| {
        if source.is_absolute() {
            return source.starts_with(rawr);
        }
        unit.krate.starts_with(RUNNER_CRATE)
            || source.components().next().is_some_and(|first| {
                first.as_os_str() == member::DIRECTORY || first.as_os_str() == mirror::COPIES
            })
    };
    unit.entries
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "d"))
        .filter(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "deps")
        })
        .any(|path| {
            fs::read_to_string(path).is_ok_and(|text| {
                text.lines()
                    .next()
                    .and_then(|line| line.split_once(": "))
                    .is_some_and(|(_, sources)| {
                        let mut sources = sources.split(' ').map(Path::new);
                        sources.next().is_some_and(generated)
                            || sources.any(|source| source.starts_with(generate::PROGRAM_DIRECTORY))
                    })
            })
        })
}

/// The `candidates` some other unit on disk depends on, directly or through
/// another such candidate, by the dependencies Cargo's fingerprints record:
/// Cargo's own build of them would otherwise compile them again.
fn depended_on(
    present: &BTreeMap<String, Present>,
    candidates: &BTreeSet<&str>,
) -> BTreeSet<String> {
    let mut depended = BTreeSet::new();
    if candidates.is_empty() {
        return depended;
    }
    // Each fingerprint file holds its unit's fingerprint as the hex of its
    // little-endian bytes; a dependent's JSON names it as a number.
    let mut fingerprints = BTreeMap::new();
    for hash in candidates {
        for directory in fingerprint_directories(&present[*hash]) {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let text = text.trim();
                if text.len() == 16 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    fingerprints.insert(text.to_owned(), (*hash).to_owned());
                }
            }
        }
    }
    let mut pending: Vec<&str> = present
        .keys()
        .map(String::as_str)
        .filter(|hash| !candidates.contains(hash))
        .collect();
    while let Some(hash) = pending.pop() {
        for directory in fingerprint_directories(&present[hash]) {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                for dependency in fingerprint_dependencies(&path) {
                    let Some(found) = fingerprints.get(&dependency) else {
                        continue;
                    };
                    if depended.insert(found.clone())
                        && let Some((key, _)) = present.get_key_value(found)
                    {
                        pending.push(key);
                    }
                }
            }
        }
    }
    depended
}

fn fingerprint_directories(unit: &Present) -> impl Iterator<Item = &PathBuf> {
    unit.entries.iter().filter(|path| {
        path.parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".fingerprint")
    })
}

/// The fingerprints, as their files spell them, that the fingerprint JSON at
/// `path` records as its unit's dependencies: each `[package, name, public,
/// fingerprint]`.
fn fingerprint_dependencies(path: &Path) -> Vec<String> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    json.get("deps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|dependency| dependency.get(3).and_then(serde_json::Value::as_u64))
        .map(|fingerprint| format!("{:016x}", fingerprint.swap_bytes()))
        .collect()
}

/// Remove the codegen objects of the rebuilt unit `krate`-`hash` that its
/// output no longer holds: each incremental compile leaves a new set beside
/// it (macOS keeps debug info in them), and the earlier sets stay. The
/// current set is the one its rlib holds or its binary's debug map names;
/// without either every object stays.
fn prune_objects(profiles: &[PathBuf], unit: &super::named::Unit, hash: &str) {
    let krate = unit.krate();
    for profile in profiles {
        let deps = profile.join("deps");
        let unit = format!("{krate}-{hash}");
        let current = super::archive_members(&deps.join(format!("lib{unit}.rlib"))).or_else(|| {
            super::debug_map_objects(&deps.join(format!("{unit}{}", std::env::consts::EXE_SUFFIX)))
        });
        let Some(current) = current else { continue };
        for entry in fs::read_dir(&deps).into_iter().flatten().flatten() {
            let file = entry.file_name();
            let Some(file) = file.to_str() else { continue };
            if file.starts_with(&format!("{unit}."))
                && file.ends_with(".rcgu.o")
                && !current.contains(file)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Cargo's locks on each profile, held, or none when a build of Cargo's own
/// holds one.
fn cargo_locks(profiles: &[PathBuf]) -> Option<Vec<fs::File>> {
    let mut held = Vec::new();
    for profile in profiles {
        for name in [".cargo-build-lock", ".cargo-lock"] {
            let Ok(file) = fs::File::open(profile.join(name)) else {
                continue;
            };
            match file.try_lock() {
                Ok(()) => held.push(file),
                Err(fs::TryLockError::WouldBlock) => return None,
                Err(fs::TryLockError::Error(_)) => {}
            }
        }
    }
    Some(held)
}

fn remove(path: &Path) {
    let _ = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
}

/// The record name of the workspace `work`.
fn digest(work: &Path) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(work.as_os_str().as_encoded_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn hash(digit: char) -> String {
        std::iter::repeat_n(digit, 16).collect()
    }

    /// A library unit `name`-`hash` whose dep-info names `source`, and whose
    /// fingerprint `fingerprint` lists the fingerprints `dependencies`.
    fn unit(
        profile: &Path,
        name: &str,
        digit: char,
        source: &Path,
        (fingerprint, dependencies): (u64, &[u64]),
    ) -> Vec<PathBuf> {
        let hash = hash(digit);
        let output = profile.join(format!("deps/lib{name}-{hash}.rlib"));
        fs::write(&output, b"").unwrap();
        let info = profile.join(format!("deps/{name}-{hash}.d"));
        fs::write(
            &info,
            format!("{}: {}\n", output.display(), source.display()),
        )
        .unwrap();
        let directory = profile.join(format!(".fingerprint/{}-{hash}", name.replace('_', "-")));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("lib-{name}")),
            format!("{:016x}", fingerprint.swap_bytes()),
        )
        .unwrap();
        let deps: Vec<String> = dependencies
            .iter()
            .map(|dependency| format!(r#"[1,"x",false,{dependency}]"#))
            .collect();
        fs::write(
            directory.join(format!("lib-{name}.json")),
            format!(r#"{{"deps":[{}]}}"#, deps.join(",")),
        )
        .unwrap();
        vec![output, info, directory]
    }

    fn artifact(output: &Path, package: &str, fresh: bool) -> String {
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"{package}","fresh":{fresh},"filenames":["{}"]}}"#,
            output.display()
        )
    }

    fn later() -> super::Before {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let before = super::Before::take(Path::new("/nonexistent"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        before
    }

    /// Runner builds of one project, with Cargo's own units beside them.
    /// Each later build's superseded program and rawr-only units go;
    /// Cargo's stay, and so does a unit rawr made while one of Cargo's own
    /// depends on it.
    #[test]
    fn a_runner_build_removes_what_rawr_made_that_no_build_names() {
        let project = crate::store::TestProject::new("rawr-deps-project");
        let shared = project.path().join("target");
        let profile = shared.join("debug");
        for directory in ["deps", ".fingerprint", "build", "incremental"] {
            fs::create_dir_all(profile.join(directory)).unwrap();
        }
        let work = shared.join("rawr/runner");
        fs::create_dir_all(&work).unwrap();
        // The runner's binary and the project's own, each a workspace
        // member's `src/main.rs` as Cargo writes it; the runner's names the
        // program's directory too.
        let generated = PathBuf::from(format!(
            "src/main.rs {}/rawr_proxies.rs",
            super::generate::PROGRAM_DIRECTORY
        ));
        let user = PathBuf::from("src/main.rs src/home.rs");
        let located = project.path().join("located/src/lib.rs");
        let registry = project.path().join("registry/serde/src/lib.rs");
        let program = "path+file:///runner#rawr-generated-runner@0.0.0";
        let serde_id = "registry+https://x#serde@1.0.0";
        let located_id = "path+file:///p/located#0.1.0";

        // Cargo's own: serde, the project's crate and the path package it
        // depends on. A unit from before any record, of the runner's binary
        // under the project's crate's name, is rawr's.
        let serde = unit(&profile, "serde", 'a', &registry, (10, &[]));
        let cargo_user = unit(&profile, "app", 'b', &user, (11, &[12]));
        let cargo_located = unit(&profile, "located", 'c', &located, (12, &[]));
        let legacy = unit(&profile, "app", '9', &generated, (19, &[]));
        // The first runner build: the runner's binary, of the project's
        // crate's name, and a serde of its own features.
        let before = super::Before::take(&shared);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let old_program = unit(&profile, "app", 'd', &generated, (13, &[]));
        let old_serde = unit(&profile, "serde", 'e', &registry, (14, &[]));
        let first = [
            artifact(&serde[0], serde_id, true),
            artifact(&old_serde[0], serde_id, false),
            artifact(&old_program[0], program, false),
        ]
        .join("\n");
        super::prune(&shared, first.as_bytes(), &before, &work);
        let exists = |paths: &[PathBuf]| paths.iter().all(|path| path.exists());
        let removed = |paths: &[PathBuf]| paths.iter().all(|path| !path.exists());
        assert!(removed(&legacy), "a generated unit no build names");
        assert!(exists(&old_program) && exists(&old_serde));

        // A gone workspace's record of a unit it made.
        let gone = unit(&profile, "rawr_iced", '8', &registry, (18, &[]));
        fs::write(
            shared.join("rawr-units/gone.json"),
            format!(
                r#"{{"version":1,"workspace":"{}","named":["{}"],"made":["{}"]}}"#,
                project.path().join("gone").display(),
                hash('8'),
                hash('8')
            ),
        )
        .unwrap();

        // The second build: a new program unit, and serde under Cargo's
        // features.
        let before = later();
        let new_program = unit(&profile, "app", 'f', &generated, (15, &[]));
        let second = [
            artifact(&serde[0], serde_id, true),
            artifact(&new_program[0], program, false),
            artifact(&cargo_located[0], located_id, true),
        ]
        .join("\n");
        super::prune(&shared, second.as_bytes(), &before, &work);
        assert!(
            exists(&serde) && exists(&cargo_user) && exists(&cargo_located),
            "Cargo's units stay"
        );
        assert!(exists(&new_program), "the current program unit stays");
        assert!(removed(&old_program), "the superseded program unit goes");
        assert!(removed(&old_serde), "the serde only rawr built goes");
        assert!(removed(&gone), "what a gone workspace made goes");
        assert!(!shared.join("rawr-units/gone.json").exists());

        // rawr makes a `located` that a unit of Cargo's then depends on:
        // it stays while that unit is there.
        let before = later();
        let made_located = unit(&profile, "located", '7', &located, (17, &[]));
        let third = [
            artifact(&new_program[0], program, true),
            artifact(&made_located[0], located_id, false),
        ]
        .join("\n");
        super::prune(&shared, third.as_bytes(), &before, &work);
        let dependent = unit(&profile, "app", '6', &user, (16, &[17]));
        let fourth = artifact(&new_program[0], program, true);
        let before = later();
        super::prune(&shared, fourth.as_bytes(), &before, &work);
        assert!(
            exists(&made_located) && exists(&dependent),
            "Cargo's unit depends on it"
        );
        fs::remove_dir_all(&dependent[2]).unwrap();
        super::prune(&shared, fourth.as_bytes(), &before, &work);
        assert!(removed(&made_located), "nothing depends on it any more");
        assert!(exists(&new_program) && exists(&serde) && exists(&cargo_user));

        assert_eq!(
            super::super::unit_key("librawr_iced-0123456789abcdef.rlib"),
            Some(("rawr_iced".to_owned(), "0123456789abcdef"))
        );
        assert_eq!(
            super::super::unit_key("liblibc-0123456789abcdef.rlib").map(|key| key.0),
            Some("libc".to_owned())
        );
        assert_eq!(super::super::unit_key("invoked.timestamp"), None);
    }

    /// A rebuilt runner keeps the codegen objects its binary's debug map
    /// names, and of its incremental directories the one the build touched,
    /// with only its last session.
    #[test]
    fn a_rebuilt_runner_keeps_its_current_objects_and_session() {
        let project = crate::store::TestProject::new("rawr-deps-objects");
        let shared = project.path().join("target");
        let profile = shared.join("debug");
        for directory in ["deps", ".fingerprint", "incremental"] {
            fs::create_dir_all(profile.join(directory)).unwrap();
        }
        let work = shared.join("rawr/runner");
        fs::create_dir_all(&work).unwrap();
        let runner = format!("rawr_generated_runner-{}", hash('1'));
        let old_objects = profile.join(format!("deps/{runner}.cgu.old.rcgu.o"));
        fs::write(&old_objects, b"").unwrap();
        let old_incremental = profile.join("incremental/rawr_generated_runner-old");
        fs::create_dir_all(&old_incremental).unwrap();
        fs::write(
            work.join("incremental-directories"),
            format!("{}\n", old_incremental.display()),
        )
        .unwrap();
        let before = later();
        let new_objects = profile.join(format!("deps/{runner}.cgu.new.rcgu.o"));
        fs::write(&new_objects, b"").unwrap();
        // A linked runner whose debug map (one `N_OSO` stab) names the new set.
        let oso = format!("\0{}\0", new_objects.display()).into_bytes();
        let mut macho = Vec::new();
        for word in [0xfeed_facf_u32, 0x0100_000c, 0, 2, 1, 24, 0, 0] {
            macho.extend_from_slice(&word.to_le_bytes());
        }
        for word in [2_u32, 24, 56, 1, 56 + 16, u32::try_from(oso.len()).unwrap()] {
            macho.extend_from_slice(&word.to_le_bytes());
        }
        macho.extend_from_slice(&1_u32.to_le_bytes());
        macho.extend_from_slice(&[0x66, 0, 0, 0]);
        macho.extend_from_slice(&0_u64.to_le_bytes());
        macho.extend_from_slice(&oso);
        let binary = profile.join(format!("deps/{runner}"));
        fs::write(&binary, &macho).unwrap();
        fs::create_dir_all(
            profile.join(format!(".fingerprint/rawr-generated-runner-{}", hash('1'))),
        )
        .unwrap();
        let copy = work.join("target/debug/rawr-generated-runner");
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        fs::hard_link(&binary, &copy).unwrap();
        let incremental = profile.join("incremental/rawr_generated_runner-new");
        let (earlier, last) = (incremental.join("s-a-1-x"), incremental.join("s-b-2-y"));
        fs::create_dir_all(&earlier).unwrap();
        fs::write(incremental.join("s-a-1.lock"), b"").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::create_dir_all(&last).unwrap();
        fs::write(incremental.join("s-b-2.lock"), b"").unwrap();
        let messages = format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file:///runner#rawr-generated-runner@0.0.0","fresh":false,"filenames":["{}"],"executable":"{}"}}"#,
            copy.display(),
            copy.display()
        );
        super::prune(&shared, messages.as_bytes(), &before, &work);
        assert!(new_objects.exists() && binary.exists());
        assert!(
            !old_objects.exists(),
            "the runner's earlier codegen objects"
        );
        assert!(
            !old_incremental.exists(),
            "incremental state the build did not touch"
        );
        assert!(last.exists() && incremental.join("s-b-2.lock").exists());
        assert!(
            !earlier.exists() && !incremental.join("s-a-1.lock").exists(),
            "an earlier session"
        );
        assert_eq!(
            fs::read_to_string(work.join("incremental-directories")).unwrap(),
            format!("{}\n", incremental.display())
        );
    }
}
