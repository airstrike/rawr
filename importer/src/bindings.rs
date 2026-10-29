//! Generated bindings shared per machine (D608.4, D609).
//!
//! A selected crate whose closure comes only from registries and git commits
//! keeps its extraction table in rawr's home, under a key set by the crate
//! alone (`extract::shared`). A run that finds every selected crate's table
//! there replays them and runs no nightly build (D609.4). A run that misses
//! extracts on the nightly, and publishes each shared table once the
//! project's own build has run its build scripts and agrees with the nightly
//! extraction's results. Drift or missing evidence leaves the table local and
//! says why. A table used from the home whose build scripts give this project's
//! build other results stops the run, naming the crate, and is evicted when
//! no reader holds its lease.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::extract::engine::home;

/// The format of an entry's `meta.json`.
pub(crate) const META_SCHEMA: u32 = 4;
const META: &str = "meta.json";
/// Where the runner's build records its build scripts' results, in the
/// runner's work directory.
pub(crate) const RUNNER_SCRIPTS: &str = "build-scripts.json";
/// An entry unused this long goes when another is published.
const IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// `rawr cache clean` removes entries unused this long.
const CLEAN_IDLE: Duration = Duration::from_secs(60 * 60);

/// One build script's complete cfg/environment results and generated files.
/// Paths to generated files compare relative to `OUT_DIR`, since the two
/// toolchains build in different directories.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Script {
    pub(crate) cfgs: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) out_dir: String,
}

impl Script {
    pub(crate) fn new(
        cfgs: impl IntoIterator<Item = String>,
        env: impl IntoIterator<Item = (String, String)>,
        out_dir: &Path,
    ) -> std::io::Result<Self> {
        let mut cfgs = cfgs.into_iter().collect::<Vec<_>>();
        cfgs.sort_unstable();
        let mut env = env
            .into_iter()
            .map(|(name, value)| (name, out_directory(&value, out_dir)))
            .collect::<Vec<_>>();
        env.sort_unstable();
        Ok(Self {
            cfgs,
            env,
            out_dir: tree_digest(out_dir)?,
        })
    }
}

/// Normalize a complete path beneath the OUT_DIR Cargo actually observed.
/// Other values stay literal: a Cargo-shaped path or an old relocated path
/// is not evidence that the value belongs to this script's generated tree.
fn out_directory(value: &str, directory: &Path) -> String {
    let Ok(relative) = Path::new(value).strip_prefix(directory) else {
        return value.to_owned();
    };
    if relative
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return value.to_owned();
    }
    if relative.as_os_str().is_empty() {
        "$OUT_DIR".to_owned()
    } else {
        format!("$OUT_DIR/{}", relative.display())
    }
}

/// A digest of every file under `root`, by its path there and its bytes.
fn tree_digest(root: &Path) -> std::io::Result<String> {
    fn walk(
        root: &Path,
        directory: &Path,
        files: &mut Vec<(String, PathBuf)>,
    ) -> std::io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, files)?;
            } else {
                let relative = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::to_str)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "generated path is not relative UTF-8",
                        )
                    })?;
                files.push((relative.to_owned(), path));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_unstable();
    let mut digest = Sha256::new();
    for (relative, path) in files {
        let bytes = fs::read(&path)?;
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
    }
    Ok(crate::extract::engine::hex(&digest.finalize()))
}

/// Build-script results by package id, from cargo's JSON messages.
pub(crate) fn scripts_from_messages(messages: &[u8]) -> Option<BTreeMap<String, Vec<Script>>> {
    #[derive(Deserialize)]
    struct Message {
        package_id: String,
        out_dir: PathBuf,
        cfgs: Vec<String>,
        env: Vec<(String, String)>,
    }
    let mut scripts = BTreeMap::<String, Vec<Script>>::new();
    for line in messages.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let message = serde_json::from_slice::<serde_json::Value>(line).ok()?;
        if message.get("reason").and_then(serde_json::Value::as_str)
            != Some("build-script-executed")
        {
            continue;
        }
        let message: Message = serde_json::from_value(message).ok()?;
        let script = Script::new(message.cfgs, message.env, &message.out_dir).ok()?;
        let entry = scripts.entry(message.package_id).or_default();
        if !entry.contains(&script) {
            entry.push(script);
            entry.sort_unstable();
        }
    }
    Some(scripts)
}

/// Keep the build scripts' results of the runner's build, where the next
/// settlement reads them. A failed capture removes older evidence so it cannot
/// authorize a run built without the required observations.
pub(crate) fn record_runner_scripts(work: &Path, messages: &[u8]) {
    let recorded = work.join(RUNNER_SCRIPTS);
    if let Some(scripts) = scripts_from_messages(messages)
        && let Ok(bytes) = serde_json::to_vec(&scripts)
    {
        let temporary = work.join(format!(".{RUNNER_SCRIPTS}.{}", std::process::id()));
        if fs::write(&temporary, bytes).is_ok() && fs::rename(&temporary, &recorded).is_ok() {
            return;
        }
        let _ = fs::remove_file(temporary);
    }
    let _ = fs::remove_file(recorded);
}

/// An external crate a shared table names, by the unit that compiled it;
/// none for a crate whose items Rawr cannot call, as a proc-macro crate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct External {
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) unit_key: Option<String>,
}

/// What a shared entry keeps beside its table: `scripts` holds the results
/// that the extraction nightly and publishing project's builds agreed on for
/// every build script in the crate's closure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Meta {
    pub(crate) schema: u32,
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) crate_hash: String,
    pub(crate) scripts: BTreeMap<String, Vec<Script>>,
    pub(crate) inputs: Option<crate::input::bindings::Inputs>,
    pub(crate) externals: Vec<External>,
}

impl Meta {
    /// Every compared output contributes even when rustc did not read an env
    /// value or generated file and therefore left the table bytes unchanged.
    pub(crate) fn scripts_identity(&self) -> Option<[u8; 32]> {
        let mut digest = Sha256::new();
        digest.update(b"rawr.bindings.script-outputs/v1");
        digest.update(serde_json::to_vec(&self.scripts).ok()?);
        Some(digest.finalize().into())
    }
}

/// A shared entry, as a run reads it from the home.
pub(crate) struct Entry {
    pub(crate) meta: Meta,
    pub(crate) table: Vec<u8>,
    _lease: home::Lease,
}

impl Entry {
    pub(crate) fn generation(&self) -> &str {
        self._lease.generation()
    }
}

/// Read `key`'s entry from the home, leased while this run holds it.
pub(crate) fn read(bindings: &Path, key: &str) -> Option<Entry> {
    let (lease, table) = home::read(bindings, key)?;
    let meta = fs::read(bindings.join(key).join(META)).ok()?;
    let meta = serde_json::from_slice::<Meta>(&meta).ok()?;
    (meta.schema == META_SCHEMA).then_some(Entry {
        meta,
        table,
        _lease: lease,
    })
}

/// The bindings directory under the home that `cargo_home` gives.
pub(crate) fn directory(cargo_home: &Path) -> PathBuf {
    home::directory(&crate::store::home(cargo_home))
}

/// One shared table a run used or extracted.
pub(crate) enum Settled {
    /// A table this run extracted, published with the results the
    /// project's build gives for the build scripts of `packages`.
    Extracted {
        key: String,
        packages: Vec<String>,
        inputs_packages: Vec<String>,
        /// None means at least one required output was absent or unreadable.
        nightly: Option<BTreeMap<String, Vec<Script>>>,
        publish: Box<Publish>,
    },
    /// A table read from the home, with the results its publisher's build
    /// gave.
    Read {
        key: String,
        crate_name: String,
        scripts: BTreeMap<String, Vec<Script>>,
        inputs: crate::input::bindings::Inputs,
    },
}

impl Settled {
    fn key(&self) -> &str {
        match self {
            Self::Extracted { key, .. } | Self::Read { key, .. } => key,
        }
    }
}

pub(crate) struct Publish {
    pub(crate) bindings: PathBuf,
    pub(crate) table: PathBuf,
    pub(crate) meta: Meta,
}

/// What a run did with shared tables, settled after the runner builds.
#[derive(Default)]
pub struct Settlement {
    pub(crate) entries: Vec<Settled>,
    pub(crate) bindings: Option<PathBuf>,
    /// The project's Rawr directory, where the runner builds.
    pub(crate) rawr: Option<PathBuf>,
}

impl Settlement {
    /// Explain a failed adapter build when its script outputs prove that the
    /// project's toolchain saw a different API from the extraction nightly.
    pub fn drift_diagnostic(&self) -> Option<String> {
        let stable = runner_scripts(self.rawr.as_ref()?)?;
        for entry in &self.entries {
            let (crate_name, scripts) = match entry {
                Settled::Extracted {
                    nightly: Some(scripts),
                    publish,
                    ..
                } => (&publish.meta.crate_name, scripts),
                Settled::Read {
                    crate_name,
                    scripts,
                    ..
                } => (crate_name, scripts),
                Settled::Extracted { nightly: None, .. } => continue,
            };
            if let Some((package, _)) = scripts.iter().find(|(package, scripts)| {
                stable
                    .get(*package)
                    .is_some_and(|current| current != *scripts)
            }) {
                return Some(format!("the API of `{crate_name}` differs between the extraction nightly and the project's toolchain: build script `{}` produced different outputs", package_name(package)));
            }
        }
        None
    }

    /// Take what `other` left too, as the preparation loop prepares again.
    pub fn absorb(&mut self, other: Self) {
        self.entries.extend(other.entries);
        if self.bindings.is_none() {
            self.bindings = other.bindings;
        }
        if self.rawr.is_none() {
            self.rawr = other.rawr;
        }
    }

    /// Publish newly extracted tables only when the nightly and project's
    /// build-script outputs agree. Drift leaves them local and reports the
    /// affected crate. A shared table must still agree with this project.
    ///
    /// # Errors
    ///
    /// A table read from the home whose build scripts give other results on
    /// this project's toolchain. Evict the table when its lease is free.
    pub fn settle(self) -> Result<(), String> {
        let Some(rawr) = &self.rawr else {
            return Ok(());
        };
        if self.entries.is_empty() {
            return Ok(());
        }
        let stable = runner_scripts(rawr);
        let mut refused = None;
        // The preparation loop settles a table once for each demand it
        // extracts, and each probe names the external crates its demand
        // reached: the entry keeps them all, as its table keeps every
        // demand's units.
        let mut entries = Vec::<Settled>::with_capacity(self.entries.len());
        for entry in self.entries {
            let Some(kept) = entries.iter_mut().find(|kept| kept.key() == entry.key()) else {
                entries.push(entry);
                continue;
            };
            if let (
                Settled::Extracted {
                    publish: kept,
                    nightly: kept_nightly,
                    ..
                },
                Settled::Extracted {
                    publish, nightly, ..
                },
            ) = (kept, entry)
            {
                if *kept_nightly != nightly {
                    *kept_nightly = None;
                }
                for external in publish.meta.externals {
                    if !kept.meta.externals.contains(&external) {
                        kept.meta.externals.push(external);
                    }
                }
            }
        }
        for entry in entries {
            match entry {
                Settled::Extracted {
                    key,
                    packages,
                    nightly,
                    mut publish,
                    ..
                } => {
                    if publish.meta.inputs.is_none() {
                        eprintln!("rawr: bindings of `{}` stay in this project: crate-specific input evidence is unavailable", publish.meta.crate_name);
                        continue;
                    }
                    let Some(nightly) = nightly else {
                        eprintln!("rawr: bindings of `{}` stay in this project: the extraction nightly's build-script outputs are missing or unreadable", publish.meta.crate_name);
                        continue;
                    };
                    let differs = packages.iter().find(|package| {
                        let recorded = nightly.get(*package);
                        recorded.is_none()
                            || recorded != stable.as_ref().and_then(|stable| stable.get(*package))
                    });
                    if let Some(package) = differs {
                        eprintln!("rawr: bindings of `{}` stay in this project: build-script outputs for `{}` differ or are unavailable between the extraction nightly and the project's toolchain", publish.meta.crate_name, package_name(package));
                        continue;
                    }
                    publish.meta.scripts = nightly;
                    publish_entry(&key, &publish);
                }
                Settled::Read {
                    key,
                    crate_name,
                    scripts,
                    ..
                } => {
                    let differs = scripts.iter().find(|(package, recorded)| {
                        stable.as_ref().and_then(|stable| stable.get(*package)) != Some(*recorded)
                    });
                    let Some((package, _)) = differs else {
                        continue;
                    };
                    if let Some(bindings) = &self.bindings {
                        home::evict(bindings, &key);
                    }
                    refused.get_or_insert_with(|| {
                        format!(
                            "the shared bindings of `{crate_name}` were extracted with different \
                             or unavailable build-script outputs for `{}` on this project's \
                             toolchain. rawr refused them: run again to make them here",
                            package_name(package),
                        )
                    });
                }
            }
        }
        if let Some(bindings) = &self.bindings {
            let bindings = bindings.clone();
            std::thread::spawn(move || home::prune(&bindings, IDLE));
        }
        refused.map_or(Ok(()), Err)
    }
}

fn runner_scripts(rawr: &Path) -> Option<BTreeMap<String, Vec<Script>>> {
    let bytes = fs::read(rawr.join("runner").join(RUNNER_SCRIPTS)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn publish_entry(key: &str, publish: &Publish) {
    let (Ok(table), Ok(meta)) = (fs::read(&publish.table), serde_json::to_vec(&publish.meta))
    else {
        return;
    };
    let _ = home::publish(
        &publish.bindings,
        key,
        &[("table.json", &table), (META, &meta)],
    );
}

/// A package's name, from its cargo package id: `…#name@version`, or
/// `…/name?…#version` where the name is the source's last segment.
fn package_name(package: &str) -> &str {
    let (source, fragment) = package.rsplit_once('#').unwrap_or((package, package));
    match fragment.split_once('@') {
        Some((name, _)) => name,
        None => {
            let source = source.split_once('?').map_or(source, |(source, _)| source);
            source.rsplit('/').next().unwrap_or(source)
        }
    }
}

/// `rawr cache clean`: remove shared bindings unused for an hour.
pub fn clean(cargo_home: &Path) {
    home::prune(&directory(cargo_home), CLEAN_IDLE);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::{home, External, Meta, Publish, Script, Settled, Settlement, META_SCHEMA};

    fn scratch(name: &str) -> PathBuf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-bindings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn meta(scripts: BTreeMap<String, Vec<Script>>) -> Meta {
        Meta {
            schema: META_SCHEMA,
            crate_name: "fixture".to_owned(),
            stable_crate_id: "0123456789abcdef".to_owned(),
            crate_hash: "svh".to_owned(),
            scripts,
            inputs: Some(crate::input::bindings::Inputs::fixture()),
            externals: Vec::new(),
        }
    }

    fn script(cfg: &str) -> Script {
        Script {
            cfgs: vec![cfg.to_owned()],
            env: Vec::new(),
            out_dir: "digest".to_owned(),
        }
    }

    #[test]
    fn binding_identity_includes_every_script_output_even_with_the_same_table() {
        let original = meta(BTreeMap::from([(
            "demo".to_owned(),
            vec![script("original")],
        )]));
        let identity = |meta: &Meta| {
            use sha2::{Digest as _, Sha256};
            let mut digest = Sha256::new();
            digest.update(b"identical table and API body");
            digest.update(meta.scripts_identity().unwrap());
            digest.finalize()
        };
        let expected = identity(&original);
        for change in 0..3 {
            let mut changed = original.clone();
            let script = &mut changed.scripts.get_mut("demo").unwrap()[0];
            match change {
                0 => script.cfgs.push("another_cfg".to_owned()),
                1 => script.env.push(("UNUSED".to_owned(), "changed".to_owned())),
                _ => script.out_dir = "other generated bytes".to_owned(),
            }
            assert_ne!(identity(&changed), expected);
        }
    }

    /// A project whose runner's build gave `scripts`.
    fn runner(root: &Path, scripts: &BTreeMap<String, Vec<Script>>) -> PathBuf {
        let rawr = root.join("rawr");
        fs::create_dir_all(rawr.join("runner")).unwrap();
        fs::write(
            rawr.join("runner").join(super::RUNNER_SCRIPTS),
            serde_json::to_vec(scripts).unwrap(),
        )
        .unwrap();
        rawr
    }

    #[test]
    fn an_entry_appears_whole_and_a_lease_blocks_its_replacement() {
        let root = scratch("lease");
        let bindings = root.join("bindings");
        home::publish(
            &bindings,
            "key",
            &[("table.json", b"first"), ("meta.json", b"{}")],
        )
        .unwrap();
        let (lease, table) = home::read(&bindings, "key").unwrap();
        assert_eq!(table, b"first");
        // Publication must not replace the directory from which this read
        // will still take its metadata.
        let replacement = home::publish(
            &bindings,
            "key",
            &[("table.json", b"second"), ("meta.json", b"{}")],
        );
        assert_eq!(
            replacement.unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(home::read(&bindings, "key").unwrap().1, b"first");
        drop(lease);
        home::publish(
            &bindings,
            "key",
            &[("table.json", b"second"), ("meta.json", b"{}")],
        )
        .unwrap();
        assert_eq!(home::read(&bindings, "key").unwrap().1, b"second");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removal_by_last_use_skips_recent_and_leased_entries() {
        let root = scratch("prune");
        let bindings = root.join("bindings");
        for key in ["idle", "leased", "recent"] {
            home::publish(&bindings, key, &[("table.json", b"t")]).unwrap();
        }
        let old = SystemTime::now() - Duration::from_secs(90 * 24 * 60 * 60);
        for key in ["idle", "leased"] {
            fs::File::open(bindings.join(key))
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        let (leased, _) = home::read(&bindings, "leased").unwrap();
        fs::File::open(bindings.join("leased"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        home::prune(&bindings, Duration::from_secs(30 * 24 * 60 * 60));
        let mut left = fs::read_dir(&bindings)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(left, ["leased", "recent"]);
        drop(leased);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_out_dir_in_a_build_scripts_environment_compares_as_its_name() {
        let root = scratch("out-dir");
        let nightly = root.join("rawr/build/demo-0123456789abcdef/out");
        let stable = root.join("target/debug/build/demo-fedcba9876543210/out");
        for directory in [&nightly, &stable] {
            fs::create_dir_all(directory).unwrap();
            fs::write(directory.join("generated.rs"), "pub const X: u8 = 1;").unwrap();
        }
        let left = Script::new(
            ["b".to_owned(), "a".to_owned()],
            [
                (
                    "GENERATED".to_owned(),
                    format!("{}/generated.rs", nightly.display()),
                ),
                ("FEATURES".to_owned(), "neon".to_owned()),
            ],
            &nightly,
        )
        .unwrap();
        let right = Script::new(
            ["a".to_owned(), "b".to_owned()],
            [
                (
                    "GENERATED".to_owned(),
                    format!("{}/generated.rs", stable.display()),
                ),
                ("FEATURES".to_owned(), "neon".to_owned()),
            ],
            &stable,
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.env.len(), 2);
        assert_eq!(left.env[1].1, "$OUT_DIR/generated.rs");
        // Ordinary rustc-env values can select a public type's const length
        // through env!("WIDTH").len(); they cannot be discarded.
        let mut changed_env = right.clone();
        changed_env.env[0].1 = "neon, lse2".to_owned();
        assert_ne!(left, changed_env);
        fs::write(stable.join("generated.rs"), "pub const X: u8 = 2;").unwrap();
        let changed = Script::new(["a".to_owned(), "b".to_owned()], [], &stable).unwrap();
        assert_ne!(left.out_dir, changed.out_dir);
        assert_eq!(
            super::package_name("registry+https://index#num-traits@0.2.19"),
            "num-traits"
        );
        assert_eq!(
            super::package_name("git+https://github.com/iced-rs/iced?rev=be88873#0.15.0-dev"),
            "iced"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn environment_paths_normalize_only_beneath_the_observed_out_dir() {
        let root = scratch("literal-output-paths");
        let nightly = root.join("nightly/build/demo-0123456789abcdef/out");
        let stable = root.join("stable/build/demo-fedcba9876543210/out");
        for path in [&nightly, &stable] {
            fs::create_dir_all(path).unwrap();
        }
        let observed = |path: &Path, value: String| {
            Script::new([], [("GENERATED".to_owned(), value)], path).unwrap()
        };
        for suffix in ["", "/generated.rs"] {
            assert_eq!(
                observed(&nightly, format!("{}{suffix}", nightly.display())),
                observed(&stable, format!("{}{suffix}", stable.display())),
            );
        }
        for (left, right) in [
            (
                "/arbitrary/build/first-0123456789abcdef/out/value".to_owned(),
                "/arbitrary/build/second-fedcba9876543210/out/value".to_owned(),
            ),
            (
                format!("{}side/value", nightly.display()),
                format!("{}side/value", stable.display()),
            ),
            (
                format!("literal={}/value", nightly.display()),
                format!("literal={}/value", stable.display()),
            ),
            (
                format!("{}/../value", nightly.display()),
                format!("{}/../value", stable.display()),
            ),
        ] {
            let left_output = observed(&nightly, left.clone());
            let right_output = observed(&stable, right.clone());
            assert_eq!(left_output.env[0].1, left);
            assert_eq!(right_output.env[0].1, right);
            assert_ne!(left_output, right_output);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_table_publishes_only_with_matching_nightly_scripts_and_an_incompatible_hit_stops_the_run()
    {
        let root = scratch("settle");
        let bindings = root.join("bindings");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let first = runner(
            &root.join("first"),
            &BTreeMap::from([(package.clone(), vec![script("first")])]),
        );
        // Settled once for each of two demands, whose probes named
        // different external crates: published once, with both.
        let extracted = |external: &str| {
            let mut meta = meta(BTreeMap::new());
            meta.externals.push(External {
                crate_name: external.to_owned(),
                stable_crate_id: "0".to_owned(),
                unit_key: None,
            });
            Settled::Extracted {
                key: "demo".to_owned(),
                packages: vec![package.clone()],
                inputs_packages: vec![package.clone()],
                nightly: Some(BTreeMap::from([(package.clone(), vec![script("first")])])),
                publish: Box::new(Publish {
                    bindings: bindings.clone(),
                    table: table.clone(),
                    meta,
                }),
            }
        };
        Settlement {
            entries: vec![extracted("first"), extracted("second")],
            bindings: Some(bindings.clone()),
            rawr: Some(first.clone()),
        }
        .settle()
        .unwrap();
        let entry = super::read(&bindings, "demo").unwrap();
        assert_eq!(entry.table, b"units");
        let externals = entry
            .meta
            .externals
            .iter()
            .map(|external| external.crate_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(externals, ["first", "second"]);
        let recorded = entry.meta.scripts.clone();
        assert_eq!(recorded[&package], [script("first")]);
        drop(entry);
        let read = || Settled::Read {
            key: "demo".to_owned(),
            crate_name: "demo".to_owned(),
            scripts: recorded.clone(),
            inputs: crate::input::bindings::Inputs::fixture(),
        };
        // The same results in another project use the table.
        Settlement {
            entries: vec![read()],
            bindings: Some(bindings.clone()),
            rawr: Some(first),
        }
        .settle()
        .unwrap();
        assert!(super::read(&bindings, "demo").is_some());
        // A project whose build gives other results stops the run, names
        // the crate, and removes the table.
        let second = runner(
            &root.join("second"),
            &BTreeMap::from([(package.clone(), vec![script("second")])]),
        );
        let refused = Settlement {
            entries: vec![read()],
            bindings: Some(bindings.clone()),
            rawr: Some(second),
        }
        .settle()
        .unwrap_err();
        assert!(refused.contains("`demo`"), "{refused}");
        assert!(super::read(&bindings, "demo").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nightly_drift_or_missing_evidence_never_publishes_a_table() {
        let root = scratch("nightly-drift");
        let bindings = root.join("bindings");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let expected = script("stable");
        let rawr = runner(
            &root,
            &BTreeMap::from([(package.clone(), vec![expected.clone()])]),
        );
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let mut cfg = expected.clone();
        cfg.cfgs = vec!["nightly".to_owned()];
        let mut env = expected.clone();
        env.env.push(("WIDTH".to_owned(), "longer".to_owned()));
        let mut generated = expected.clone();
        generated.out_dir = "different-generated-source".to_owned();
        for (index, nightly) in [Some(cfg), Some(env), Some(generated), None]
            .into_iter()
            .enumerate()
        {
            let key = format!("drift-{index}");
            let known_drift = nightly.is_some();
            let settlement = Settlement {
                entries: vec![Settled::Extracted {
                    key: key.clone(),
                    packages: vec![package.clone()],
                    inputs_packages: vec![package.clone()],
                    nightly: nightly
                        .map(|script| BTreeMap::from([(package.clone(), vec![script])])),
                    publish: Box::new(Publish {
                        bindings: bindings.clone(),
                        table: table.clone(),
                        meta: meta(BTreeMap::new()),
                    }),
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
            };
            let diagnostic = settlement.drift_diagnostic();
            assert_eq!(diagnostic.is_some(), known_drift);
            if let Some(diagnostic) = diagnostic {
                assert!(
                    diagnostic.contains("`fixture`") && diagnostic.contains("`demo`"),
                    "{diagnostic}"
                );
            }
            settlement.settle().unwrap();
            assert!(
                super::read(&bindings, &key).is_none(),
                "mismatch {index} was published"
            );
        }
        // Missing project evidence is not agreement with the nightly either.
        fs::remove_file(rawr.join("runner").join(super::RUNNER_SCRIPTS)).unwrap();
        Settlement {
            entries: vec![Settled::Extracted {
                key: "missing-project".to_owned(),
                packages: vec![package.clone()],
                inputs_packages: vec![package.clone()],
                nightly: Some(BTreeMap::from([(package, vec![expected])])),
                publish: Box::new(Publish {
                    bindings: bindings.clone(),
                    table: table.clone(),
                    meta: meta(BTreeMap::new()),
                }),
            }],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr.clone()),
        }
        .settle()
        .unwrap();
        assert!(super::read(&bindings, "missing-project").is_none());
        // A crate with no scripts has no script evidence to require.
        Settlement {
            entries: vec![Settled::Extracted {
                key: "no-scripts".to_owned(),
                packages: Vec::new(),
                inputs_packages: Vec::new(),
                nightly: Some(BTreeMap::new()),
                publish: Box::new(Publish {
                    bindings: bindings.clone(),
                    table,
                    meta: meta(BTreeMap::new()),
                }),
            }],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr),
        }
        .settle()
        .unwrap();
        assert!(super::read(&bindings, "no-scripts").is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_project_outputs_refuse_a_shared_table_before_execution() {
        let root = scratch("missing-output");
        let bindings = root.join("bindings");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let scripts = BTreeMap::from([(package, vec![script("cfg")])]);
        for available in [false, true] {
            let rawr = if available {
                runner(&root, &BTreeMap::new())
            } else {
                root.join("rawr")
            };
            home::publish(
                &bindings,
                "key",
                &[
                    ("table.json", b"units"),
                    (
                        "meta.json",
                        &serde_json::to_vec(&meta(scripts.clone())).unwrap(),
                    ),
                ],
            )
            .unwrap();
            let error = Settlement {
                entries: vec![Settled::Read {
                    key: "key".to_owned(),
                    crate_name: "fixture".to_owned(),
                    scripts: scripts.clone(),
                    inputs: crate::input::bindings::Inputs::fixture(),
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr),
            }
            .settle()
            .unwrap_err();
            assert!(
                error.contains("`fixture`") && error.contains("`demo`"),
                "{error}"
            );
            assert!(super::read(&bindings, "key").is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_generated_files_invalidate_previous_runner_evidence() {
        let root = scratch("unreadable-output");
        let work = root.join("runner");
        fs::create_dir_all(&work).unwrap();
        let directory = root.join("out");
        fs::create_dir_all(&directory).unwrap();
        let message = serde_json::json!({
            "reason": "build-script-executed", "package_id": "demo", "out_dir": directory,
            "cfgs": [], "env": [["WIDTH", "four"]]
        });
        let bytes = serde_json::to_vec(&message).unwrap();
        super::record_runner_scripts(&work, &bytes);
        let recorded = work.join(super::RUNNER_SCRIPTS);
        assert!(recorded.exists());
        assert_eq!(
            super::scripts_from_messages(&bytes).unwrap()["demo"][0].env,
            [("WIDTH".to_owned(), "four".to_owned())]
        );
        fs::remove_dir(&directory).unwrap();
        assert!(Script::new([], [], &directory).is_err());
        super::record_runner_scripts(&work, &bytes);
        assert!(
            !recorded.exists(),
            "failed capture retained old success evidence"
        );
        let mut malformed = message;
        malformed["env"] = serde_json::json!([["WIDTH"]]);
        assert!(super::scripts_from_messages(&serde_json::to_vec(&malformed).unwrap()).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn actual_cargo_outputs_publish_only_when_both_toolchains_agree() {
        let root = scratch("cargo-outputs");
        let project = root.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\n[package]\nname = \"script-evidence\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            project.join("src/lib.rs"),
            r#"#[cfg(changed_api)]
pub type Number = u64;
#[cfg(not(changed_api))]
pub type Number = u8;
pub type Bytes = [u8; env!("WIDTH").len()];
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
"#,
        )
        .unwrap();
        fs::write(
            project.join("build.rs"),
            r#"fn main() {
    let rustc = std::env::var_os("RUSTC").unwrap();
    let version = std::process::Command::new(rustc).arg("--version").output().unwrap();
    let nightly = String::from_utf8(version.stdout).unwrap().contains("nightly");
    let mode = std::env::var("RAWR_TEST_OUTPUT").unwrap();
    println!("cargo:rerun-if-env-changed=RAWR_TEST_OUTPUT");
    println!("cargo:rustc-check-cfg=cfg(changed_api)");
    if nightly && mode == "cfg" {
        println!("cargo:rustc-cfg=changed_api");
    }
    let width = if nightly && mode == "env" { "longer" } else { "one" };
    println!("cargo:rustc-env=WIDTH={width}");
    let length = if nightly && mode == "generated" { 2 } else { 1 };
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(output.join("generated.rs"), format!("pub type Generated = [u8; {length}];\n")).unwrap();
}
"#,
        )
        .unwrap();
        let bindings = root.join("bindings");
        let rawr = root.join("rawr");
        fs::create_dir_all(rawr.join("runner")).unwrap();
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let build = |toolchain: &str, mode: &str| {
            let output = std::process::Command::new("cargo")
                .arg(format!("+{toolchain}"))
                .args(["build", "--offline", "--message-format=json"])
                .current_dir(&project)
                .env("CARGO_TARGET_DIR", root.join(toolchain))
                .env("RAWR_TEST_OUTPUT", mode)
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        for mode in ["match", "cfg", "env", "generated"] {
            let nightly =
                super::scripts_from_messages(&build(crate::cargo::toolchain::CHANNEL, mode))
                    .unwrap();
            let stable = build("stable", mode);
            super::record_runner_scripts(&rawr.join("runner"), &stable);
            let packages = nightly.keys().cloned().collect::<Vec<_>>();
            assert_eq!(packages.len(), 1);
            let settlement = Settlement {
                entries: vec![Settled::Extracted {
                    key: mode.to_owned(),
                    packages: packages.clone(),
                    inputs_packages: packages,
                    nightly: Some(nightly),
                    publish: Box::new(Publish {
                        bindings: bindings.clone(),
                        table: table.clone(),
                        meta: meta(BTreeMap::new()),
                    }),
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
            };
            assert_eq!(settlement.drift_diagnostic().is_some(), mode != "match");
            settlement.settle().unwrap();
            assert_eq!(
                super::read(&bindings, mode).is_some(),
                mode == "match",
                "{mode}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
