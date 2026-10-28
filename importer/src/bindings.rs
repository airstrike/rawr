//! Generated bindings shared per machine (D608.4, D609).
//!
//! A selected crate whose closure comes only from registries and git commits
//! keeps its extraction table in rawr's home, under a key set by the crate
//! alone (`extract::shared`). A run that finds every selected crate's table
//! there replays them and runs no nightly build (D609.4). A run that misses
//! extracts on the nightly, and publishes each shared table once the
//! project's own build has run its build scripts and given the same results
//! the nightly's gave: a crate whose build scripts answer differently on the
//! two toolchains keeps its table in the project and says so. A table used
//! from the home whose build scripts answer differently stops the run,
//! naming the crate, and is removed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::extract::engine::home;

/// The format of an entry's `meta.json`.
pub(crate) const META_SCHEMA: u32 = 1;
const META: &str = "meta.json";
/// Where the runner's build records its build scripts' results, in the
/// runner's work directory.
pub(crate) const RUNNER_SCRIPTS: &str = "build-scripts.json";
/// An entry unused this long goes when another is published.
const IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// `rawr cache clean` removes entries unused this long.
const CLEAN_IDLE: Duration = Duration::from_secs(60 * 60);

/// One build script's results, as either toolchain's build gives them: its
/// cfgs, its environment with its own `OUT_DIR` written `$OUT_DIR`, and a
/// digest of the files it wrote there.
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
    ) -> Self {
        let directory = out_dir.to_string_lossy();
        let mut cfgs = cfgs.into_iter().collect::<Vec<_>>();
        cfgs.sort_unstable();
        let mut env = env
            .into_iter()
            .map(|(name, value)| {
                let value = value.replace(directory.as_ref(), "$OUT_DIR");
                (name, out_directories(&value))
            })
            .collect::<Vec<_>>();
        env.sort_unstable();
        Self {
            cfgs,
            env,
            out_dir: tree_digest(out_dir),
        }
    }
}

/// `value` with every Cargo `OUT_DIR` spelled `$OUT_DIR`: a path whose last
/// components are `build/<package>-<16 hex digits>/out`. A build directory
/// moved after the build names the old path, so the given `OUT_DIR` alone
/// misses it.
fn out_directories(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut token = String::new();
    let flush = |token: &mut String, normalized: &mut String| {
        normalized.push_str(&out_directory(token).unwrap_or_else(|| token.clone()));
        token.clear();
    };
    for character in value.chars() {
        if matches!(character, ' ' | ';' | ':' | '=' | '"' | ',') {
            flush(&mut token, &mut normalized);
            normalized.push(character);
        } else {
            token.push(character);
        }
    }
    flush(&mut token, &mut normalized);
    normalized
}

fn out_directory(token: &str) -> Option<String> {
    let index = token.find("/out")?;
    let (prefix, rest) = token.split_at(index);
    if !(rest.len() == 4 || rest[4..].starts_with('/')) {
        return None;
    }
    let (build, unit) = prefix.rsplit_once('/')?;
    let (_, hash) = unit.rsplit_once('-')?;
    (build.ends_with("/build")
        && hash.len() == 16
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| format!("$OUT_DIR{}", &rest[4..]))
}

/// A digest of every file under `root`, by its path there and its bytes.
fn tree_digest(root: &Path) -> String {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(root, &path, files);
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push((relative.to_string_lossy().into_owned(), path));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort_unstable();
    let mut digest = Sha256::new();
    for (relative, path) in files {
        let bytes = fs::read(&path).unwrap_or_default();
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
    }
    crate::extract::engine::hex(&digest.finalize())
}

/// Build-script results by package id, from cargo's JSON messages.
pub(crate) fn scripts_from_messages(messages: &[u8]) -> BTreeMap<String, Vec<Script>> {
    let mut scripts = BTreeMap::<String, Vec<Script>>::new();
    for line in messages.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str)
            != Some("build-script-executed")
        {
            continue;
        }
        let (Some(package), Some(out_dir)) = (
            message
                .get("package_id")
                .and_then(serde_json::Value::as_str),
            message.get("out_dir").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let cfgs = message
            .get("cfgs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned);
        let env = message
            .get("env")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|pair| {
                let pair = pair.as_array()?;
                Some((
                    pair.first()?.as_str()?.to_owned(),
                    pair.get(1)?.as_str()?.to_owned(),
                ))
            });
        let script = Script::new(cfgs, env, Path::new(out_dir));
        let entry = scripts.entry(package.to_owned()).or_default();
        if !entry.contains(&script) {
            entry.push(script);
            entry.sort_unstable();
        }
    }
    scripts
}

/// Keep the build scripts' results of the runner's build, where the next
/// settlement reads them. Best effort: none recorded checks nothing.
pub(crate) fn record_runner_scripts(work: &Path, messages: &[u8]) {
    let scripts = scripts_from_messages(messages);
    if let Ok(bytes) = serde_json::to_vec(&scripts) {
        let temporary = work.join(format!(".{RUNNER_SCRIPTS}.{}", std::process::id()));
        if fs::write(&temporary, bytes).is_ok() {
            let _ = fs::rename(&temporary, work.join(RUNNER_SCRIPTS));
        }
    }
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

/// What a shared entry keeps beside its table.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Meta {
    pub(crate) schema: u32,
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) crate_hash: String,
    /// The nightly build's results for each build script in the closure.
    pub(crate) scripts: BTreeMap<String, Vec<Script>>,
    pub(crate) externals: Vec<External>,
}

/// A shared entry, as a run reads it from the home.
pub(crate) struct Entry {
    #[allow(dead_code, reason = "read by the hit path, which replays an entry")]
    pub(crate) meta: Meta,
    pub(crate) table: Vec<u8>,
    _lease: home::Lease,
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
pub(crate) struct Settled {
    pub(crate) key: String,
    pub(crate) crate_name: String,
    pub(crate) scripts: BTreeMap<String, Vec<Script>>,
    /// A table extracted on this run, published once its scripts compare.
    pub(crate) publish: Option<Publish>,
}

pub(crate) struct Publish {
    pub(crate) bindings: PathBuf,
    pub(crate) table: PathBuf,
    #[allow(dead_code, reason = "read by the hit path, which replays an entry")]
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

    /// Compare each table's build scripts with what the runner's build in
    /// the project's Rawr directory gave: publish the tables this
    /// run extracted whose scripts agree, and refuse a table from the home
    /// whose scripts do not, naming its crate.
    ///
    /// # Errors
    ///
    /// A table read from the home whose build scripts give other results on
    /// this project's toolchain. The table is removed first.
    pub fn settle(self) -> Result<(), String> {
        let Some(rawr) = &self.rawr else {
            return Ok(());
        };
        if self.entries.is_empty() {
            return Ok(());
        }
        // The runner builds in `<rawr>/runner` (`runner::build::cached`).
        let runner = rawr.join("runner").join(RUNNER_SCRIPTS);
        let stable = fs::read(&runner)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Vec<Script>>>(&bytes).ok())
            .unwrap_or_default();
        let mut refused = None;
        for entry in self.entries {
            let differs = entry.scripts.iter().find(|(package, nightly)| {
                stable
                    .get(*package)
                    .is_some_and(|stable| nightly.iter().any(|script| !stable.contains(script)))
            });
            match (entry.publish, differs) {
                (Some(publish), None) => publish_entry(&entry.key, &publish),
                (Some(_), Some((package, _))) => crate::process::say(format_args!(
                    "rawr: `{}` keeps its bindings in this project: the build script of `{}` \
                     gives other results on rawr's nightly than on this project's toolchain",
                    entry.crate_name,
                    package_name(package),
                )),
                (None, Some((package, _))) => {
                    if let Some(bindings) = &self.bindings {
                        home::evict(bindings, &entry.key);
                    }
                    refused.get_or_insert_with(|| {
                        format!(
                            "the shared bindings of `{}` were made where the build script of \
                             `{}` gave other results than on this project's toolchain. rawr \
                             removed them: run again to make them for this toolchain",
                            entry.crate_name,
                            package_name(package),
                        )
                    });
                }
                (None, None) => {}
            }
        }
        if let Some(bindings) = &self.bindings {
            let bindings = bindings.clone();
            std::thread::spawn(move || home::prune(&bindings, IDLE));
        }
        refused.map_or(Ok(()), Err)
    }
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

    use super::{home, Meta, Publish, Script, Settled, Settlement, META_SCHEMA};

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
    fn an_entry_appears_whole_and_a_leased_one_outlives_its_removal() {
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
        // Replaced under a lease: the reader keeps what it read, and the
        // retired entry waits for the lease.
        home::publish(
            &bindings,
            "key",
            &[("table.json", b"second"), ("meta.json", b"{}")],
        )
        .unwrap();
        assert_eq!(home::read(&bindings, "key").unwrap().1, b"second");
        let retired = || {
            fs::read_dir(&bindings)
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".retired-"))
                .count()
        };
        assert_eq!(retired(), 1, "a leased entry is never removed");
        drop(lease);
        home::prune(&bindings, Duration::from_secs(3600));
        assert_eq!(retired(), 0);
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
        let leased = fs::File::open(bindings.join("leased/.lock")).unwrap();
        leased.lock_shared().unwrap();
        home::prune(&bindings, Duration::from_secs(30 * 24 * 60 * 60));
        let mut left = fs::read_dir(&bindings)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
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
        // The nightly's messages were moved after its build: its environment
        // names the directory it ran in, not the one it moved to.
        let moved = root.join("bundle/build/demo-0123456789abcdef/out");
        let left = Script::new(
            ["b".to_owned(), "a".to_owned()],
            [(
                "GENERATED".to_owned(),
                format!("{}/generated.rs", moved.display()),
            )],
            &nightly,
        );
        let right = Script::new(
            ["a".to_owned(), "b".to_owned()],
            [(
                "GENERATED".to_owned(),
                format!("{}/generated.rs", stable.display()),
            )],
            &stable,
        );
        assert_eq!(left, right);
        assert_eq!(left.env[0].1, "$OUT_DIR/generated.rs");
        fs::write(stable.join("generated.rs"), "pub const X: u8 = 2;").unwrap();
        let changed = Script::new(["a".to_owned(), "b".to_owned()], [], &stable);
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
    fn a_table_publishes_when_its_scripts_agree_and_a_shared_one_that_differs_stops_the_run() {
        let root = scratch("settle");
        let bindings = root.join("bindings");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let nightly = BTreeMap::from([(package.clone(), vec![script("nightly")])]);
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let extracted = |key: &str, scripts: &BTreeMap<String, Vec<Script>>| Settled {
            key: key.to_owned(),
            crate_name: "demo".to_owned(),
            scripts: scripts.clone(),
            publish: Some(Publish {
                bindings: bindings.clone(),
                table: table.clone(),
                meta: meta(scripts.clone()),
            }),
        };
        // The project's build agrees with one table and not with the other.
        let rawr = runner(
            &root,
            &BTreeMap::from([(package.clone(), vec![script("stable")])]),
        );
        let agreed = BTreeMap::from([(package.clone(), vec![script("stable")])]);
        Settlement {
            entries: vec![extracted("agrees", &agreed), extracted("differs", &nightly)],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr.clone()),
        }
        .settle()
        .unwrap();
        assert_eq!(super::read(&bindings, "agrees").unwrap().table, b"units");
        assert!(
            super::read(&bindings, "differs").is_none(),
            "kept in the project"
        );
        // A shared table whose scripts differ from this project's stops the
        // run, names the crate, and leaves the home.
        let refused = Settlement {
            entries: vec![Settled {
                key: "agrees".to_owned(),
                crate_name: "demo".to_owned(),
                scripts: nightly,
                publish: None,
            }],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr),
        }
        .settle()
        .unwrap_err();
        assert!(refused.contains("`demo`"), "{refused}");
        assert!(super::read(&bindings, "agrees").is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
