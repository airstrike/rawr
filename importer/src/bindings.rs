//! Generated bindings shared per machine (D608.4, D609).
//!
//! A selected crate whose closure comes only from registries and git commits
//! keeps its extraction table in rawr's home, under a key set by the crate
//! alone (`extract::shared`). A run that finds every selected crate's table
//! there replays them and runs no nightly build (D609.4). A run that misses
//! extracts on the nightly, and publishes each shared table once the
//! project's own build has run its build scripts and agrees with the nightly
//! extraction's results. A table whose build scripts gave the nightly other
//! results than the project's build, whether this run extracted it, staged it
//! or read it from the home, is a miss once: the nightly's run of the package
//! is cleaned and extraction runs again (`rerun`, D742). A difference that
//! survives that run is drift between the toolchains, which leaves the table
//! local and says why, as missing evidence does.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::extract::engine::home;

pub(crate) mod format;
mod rerun;

pub(crate) use rerun::Stale;

/// The format of an entry's `meta.json`.
pub(crate) const META_SCHEMA: u32 = 5;
const META: &str = "meta.json";
/// Where the runner's build records its build scripts' results, in the
/// runner's work directory.
pub(crate) const RUNNER_SCRIPTS: &str = "build-scripts.json";
/// An entry unused this long goes when another is published.
const IDLE: Duration = format::ENTRY_IDLE;

/// One build script's complete cfg/environment results and generated files.
/// Paths to generated files compare relative to `OUT_DIR`, since the two
/// toolchains build in different directories.
///
/// `files` holds each file under `OUT_DIR` by its path there, with a digest
/// of its bytes. A project's build keeps every file its script left; the
/// nightly's keeps the files its compile of the package read
/// ([`Script::read`]), since a file rustc never read, such as a native
/// build's objects or a CMake cache, cannot change the crate's API.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Script {
    pub(crate) cfgs: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) files: BTreeMap<String, String>,
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
            files: tree_files(out_dir)?,
        })
    }

    /// Keep only the generated files `read` names: a path rustc read, or a
    /// directory it read every file of.
    pub(crate) fn read(&mut self, read: &BTreeSet<String>) {
        self.files.retain(|name, _| {
            read.iter().any(|path| {
                name == path
                    || name
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        });
    }

    /// What tells this recorded output from the `project` build's: equal
    /// cfgs and environment, and each file this one keeps with the same
    /// bytes there. A file only the project's build left is not compared.
    fn difference(&self, project: &Self) -> Option<Difference> {
        if self.cfgs != project.cfgs {
            return Some(Difference::Cfgs);
        }
        if self.env != project.env {
            let name = self
                .env
                .iter()
                .chain(&project.env)
                .find(|pair| !self.env.contains(pair) || !project.env.contains(pair))
                .map_or_else(String::new, |(name, _)| name.clone());
            return Some(Difference::Env(name));
        }
        self.files
            .iter()
            .find(|(name, digest)| project.files.get(*name) != Some(digest))
            .map(|(name, _)| Difference::File(name.clone()))
    }

    /// This project output as `recorded` compares it: only the files the
    /// recorded output keeps.
    fn seen_by(&self, recorded: &[Self]) -> Self {
        let mut seen = self.clone();
        seen.files.retain(|name, _| {
            recorded
                .iter()
                .any(|script| script.files.contains_key(name))
        });
        seen
    }
}

/// What tells a build script's recorded outputs from the project build's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Difference {
    /// The project's build has no output for it.
    Missing,
    /// It ran another number of times.
    Runs,
    Cfgs,
    /// The value of one `rustc-env` variable.
    Env(String),
    /// One generated file the nightly's compile read, by its path under
    /// `OUT_DIR`.
    File(String),
}

impl std::fmt::Display for Difference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("this project's build did not run it"),
            Self::Runs => formatter.write_str("it ran another number of times"),
            Self::Cfgs => formatter.write_str("the cfgs it set differ"),
            Self::Env(name) => write!(formatter, "the value it gave `{name}` differs"),
            Self::File(name) => write!(formatter, "the `{name}` it wrote in OUT_DIR differs"),
        }
    }
}

/// What tells `recorded`, one package's outputs, from what the project's
/// build gave it: each recorded run must match a run of the project's.
pub(crate) fn difference(recorded: &[Script], project: Option<&[Script]>) -> Option<Difference> {
    let Some(project) = project else {
        return Some(Difference::Missing);
    };
    if recorded.len() != project.len() {
        return Some(Difference::Runs);
    }
    recorded.iter().find_map(|script| {
        if project.iter().any(|run| script.difference(run).is_none()) {
            return None;
        }
        project.first().and_then(|run| script.difference(run))
    })
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

/// Every file under `root`, by its path there, with a digest of its bytes.
fn tree_files(root: &Path) -> std::io::Result<BTreeMap<String, String>> {
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
    files
        .into_iter()
        .map(|(relative, path)| {
            let mut digest = Sha256::new();
            std::io::copy(&mut fs::File::open(&path)?, &mut digest)?;
            Ok((relative, crate::extract::engine::hex(&digest.finalize())))
        })
        .collect()
}

/// What a project's own `cargo check` gave its build scripts, read as soon
/// as it ends. The files a script wrote stay in its `OUT_DIR` only until the
/// next build of the same target directory runs it again, which may be a
/// build on another toolchain while rawr still extracts.
pub struct Observed(Option<BTreeMap<String, Vec<Script>>>);

impl Observed {
    /// Read the build scripts' results that cargo's JSON `messages` name.
    #[must_use]
    pub fn read(messages: &[u8]) -> Self {
        Self(scripts_from_messages(messages))
    }
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
    /// Where the table is: the entry's own, or the copy a staged entry
    /// names.
    pub(crate) path: PathBuf,
    _lease: home::Lease,
}

impl Entry {
    pub(crate) fn generation(&self) -> &str {
        self._lease.generation()
    }
}

/// Beside a staged entry's metadata, the one copy of its table: the file
/// the driver wrote, by its length, modification time and digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Copy {
    path: PathBuf,
    length: u64,
    modified: (u64, u32),
    digest: String,
}

impl Copy {
    /// The record of the table at `path`. An unchanged file keeps
    /// `previous`'s digest: the driver replaces a table whole, which
    /// changes its time.
    fn of(path: &Path, previous: Option<&Self>) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        let modified = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        let length = metadata.len();
        let modified = (modified.as_secs(), modified.subsec_nanos());
        let digest = match previous {
            Some(previous)
                if previous.path == path
                    && previous.length == length
                    && previous.modified == modified =>
            {
                previous.digest.clone()
            }
            _ => digest(path)?,
        };
        Some(Self {
            path: path.to_path_buf(),
            length,
            modified,
            digest,
        })
    }
}

/// Beside a staged entry's metadata, what names its table's copy.
const COPY: &str = "copy.json";

/// The digest of the file at `path`, read in pieces.
fn digest(path: &Path) -> Option<String> {
    let mut digest = Sha256::new();
    std::io::copy(&mut fs::File::open(path).ok()?, &mut digest).ok()?;
    Some(crate::extract::engine::hex(&digest.finalize()))
}

/// Where the entry for `key` keeps its own table.
pub(crate) fn table(bindings: &Path, key: &str) -> PathBuf {
    bindings.join(key).join(home::TABLE)
}

fn meta_in(entry: &Path) -> Option<Meta> {
    let meta = serde_json::from_slice::<Meta>(&fs::read(entry.join(META)).ok()?).ok()?;
    (meta.schema == META_SCHEMA).then_some(meta)
}

/// Read `key`'s entry from the home, leased while this run holds it. A
/// staged entry's table is the copy it names, read only while that copy
/// is still the one it named.
pub(crate) fn read(bindings: &Path, key: &str) -> Option<Entry> {
    let lease = home::hold(bindings, key)?;
    let entry = bindings.join(key);
    let meta = meta_in(&entry)?;
    let own = entry.join(home::TABLE);
    let (path, table) = match fs::read(&own) {
        Ok(table) => (own, table),
        Err(_) => {
            let copy = serde_json::from_slice::<Copy>(&fs::read(entry.join(COPY)).ok()?).ok()?;
            let table = fs::read(&copy.path).ok()?;
            let digest = crate::extract::engine::hex(&Sha256::digest(&table));
            (table.len() as u64 == copy.length && digest == copy.digest)
                .then_some((copy.path, table))?
        }
    };
    Some(Entry {
        meta,
        table,
        path,
        _lease: lease,
    })
}

/// `key`'s metadata, read under its lease without its table.
pub(crate) fn meta(bindings: &Path, key: &str) -> Option<Meta> {
    let _lease = home::hold(bindings, key)?;
    meta_in(&bindings.join(key))
}

/// The bindings directory of this rawr's extraction protocol under the home
/// that `cargo_home` gives ([`format`]).
pub(crate) fn directory(cargo_home: &Path) -> PathBuf {
    format::bindings(&crate::store::home(cargo_home))
}

/// Where the project whose Rawr directory is `rawr` keeps the tables it
/// extracted before they settle.
pub(crate) fn staged(rawr: &Path) -> PathBuf {
    rawr.join("staged")
}

/// Keep each table this extraction made, with what the nightly's build
/// scripts gave, where a later demand of the same resolution reads it as it
/// reads a settled one. Settling still decides what is shared (D609.4); a
/// staged table only spares the run its nightly build and probe. The
/// external crates every probe named are kept together, as settling keeps
/// them. `inputs` are what the replay checks first, which may name the
/// project's own sources. A staged entry keeps only that metadata and names
/// the table the driver wrote, which stays the one copy; an entry whose
/// metadata and table are unchanged is not written. `keys` are every key
/// the plan names: after staging, the entries no kept plan names go
/// (`retire`). Best effort: a table not staged is probed again.
pub(crate) fn stage(
    directory: &Path,
    keys: &[&str],
    settled: &[(&Settled, crate::input::bindings::Inputs)],
) {
    let kept = plans(directory, keys);
    for (entry, inputs) in settled {
        let Settled::Extracted {
            key,
            nightly: Some(nightly),
            publish,
            ..
        } = entry
        else {
            continue;
        };
        // A table the driver read where it settled is read there again.
        if publish.table == table(&publish.bindings, key) {
            continue;
        }
        let mut meta = publish.meta.clone();
        meta.scripts = nightly.clone();
        meta.inputs = Some(inputs.clone());
        let (kept_meta, kept_copy) = home::hold(directory, key)
            .map(|_lease| {
                let entry = directory.join(key);
                let copy = fs::read(entry.join(COPY))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Copy>(&bytes).ok());
                (meta_in(&entry), copy)
            })
            .unwrap_or_default();
        if let Some(kept) = &kept_meta
            && kept.crate_hash == meta.crate_hash
        {
            for external in &kept.externals {
                if !meta.externals.contains(external) {
                    meta.externals.push(external.clone());
                }
            }
        }
        let Some(copy) = Copy::of(&publish.table, kept_copy.as_ref()) else {
            continue;
        };
        if kept_meta.as_ref() == Some(&meta) && kept_copy.as_ref() == Some(&copy) {
            continue;
        }
        let (Ok(meta), Ok(copy)) = (serde_json::to_vec(&meta), serde_json::to_vec(&copy)) else {
            continue;
        };
        let _ = home::publish(directory, key, &[(META, &meta), (COPY, &copy)]);
    }
    retire(directory, &kept);
}

/// Where staged/ records the keys of each plan it staged for.
const PLANS: &str = ".plans";
/// The plans whose keys staged/ keeps, as the project keeps the Cargo
/// outputs of its four latest requests.
const KEPT_PLANS: usize = 4;

/// Record `keys` as the newest plan's, and answer every key a kept plan
/// names. Older records go.
fn plans(directory: &Path, keys: &[&str]) -> BTreeSet<String> {
    let mut named = keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>();
    named.sort_unstable();
    named.dedup();
    let records = directory.join(PLANS);
    if let Ok(bytes) = serde_json::to_vec(&named) {
        let record = records.join(crate::extract::engine::hex(&Sha256::digest(&bytes)));
        if fs::read(&record).is_ok_and(|kept| kept == bytes) {
            let _ = fs::File::open(&record)
                .and_then(|file| file.set_modified(std::time::SystemTime::now()));
        } else {
            let temporary = records.join(format!(".plan.{}", std::process::id()));
            let _ = fs::create_dir_all(&records)
                .and_then(|()| fs::write(&temporary, &bytes))
                .and_then(|()| fs::rename(&temporary, &record));
        }
    }
    let mut recorded = fs::read_dir(&records)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect::<Vec<_>>();
    recorded.sort_unstable_by(|left, right| right.cmp(left));
    let mut kept = named.into_iter().collect::<BTreeSet<_>>();
    for (index, (_, record)) in recorded.into_iter().enumerate() {
        if index >= KEPT_PLANS {
            let _ = fs::remove_file(record);
        } else if let Some(keys) = fs::read(&record)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        {
            kept.extend(keys);
        }
    }
    kept
}

/// Retire the staged entries whose key no kept plan names, as the driver
/// retires the tables a probe did not ask for. A leased entry stays, and no
/// lock is waited for (D608.7).
fn retire(directory: &Path, kept: &BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with('.') && !kept.contains(name) {
            home::evict(directory, name);
        }
    }
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
        /// Why `publish` records no build inputs, when capturing them
        /// found a reason.
        unshared: Option<crate::input::bindings::Unshared>,
    },
    /// A table read from the home, with the results its publisher's build
    /// gave.
    Read {
        /// The bindings directory the table was read from: rawr's home,
        /// or the project's for a path closure.
        directory: PathBuf,
        key: String,
        crate_name: String,
        scripts: BTreeMap<String, Vec<Script>>,
        inputs: crate::input::bindings::Inputs,
    },
    /// A crate's intrinsic entry this run captured, whose closure runs a
    /// build script: published to rawr's home under its machine-wide key
    /// once the project's build gives its packages the outputs the nightly
    /// did (#403).
    Inventory {
        home: PathBuf,
        key: String,
        packages: Box<[Box<str>]>,
        entry: Box<crate::applicability::inventory::Entry>,
    },
    /// Such an entry read from rawr's home: refused, and evicted, when the
    /// project's build gives its packages other outputs.
    InventoryRead {
        home: PathBuf,
        key: String,
        crate_name: String,
        packages: Box<[Box<str>]>,
        scripts: crate::applicability::store::Scripts,
    },
}

impl Settled {
    fn key(&self) -> &str {
        match self {
            Self::Extracted { key, .. }
            | Self::Read { key, .. }
            | Self::Inventory { key, .. }
            | Self::InventoryRead { key, .. } => key,
        }
    }

    /// Each build script's outputs this entry was made under, by package.
    fn recorded(&self) -> Vec<(&str, &[Script])> {
        match self {
            Self::Extracted {
                nightly: Some(scripts),
                ..
            }
            | Self::Read { scripts, .. } => scripts
                .iter()
                .map(|(package, outputs)| (package.as_str(), outputs.as_slice()))
                .collect(),
            Self::InventoryRead {
                packages, scripts, ..
            } => packages
                .iter()
                .filter_map(|package| Some((package.as_ref(), scripts.get(package)?)))
                .collect(),
            Self::Extracted { nightly: None, .. } | Self::Inventory { .. } => Vec::new(),
        }
    }

    /// Whether a package this entry was made under runs again (D742).
    fn reruns(&self, verdicts: &BTreeMap<String, rerun::Verdict>) -> bool {
        self.recorded()
            .iter()
            .any(|(package, _)| verdicts.get(*package) == Some(&rerun::Verdict::Rerun))
    }

    /// Remove this entry's table, so preparing again misses it: a table
    /// this run extracted waits in `staged`.
    fn evict(&self, staged: &Path) {
        match self {
            Self::Extracted { key, .. } => home::evict(staged, key),
            Self::Read { directory, key, .. } => home::evict(directory, key),
            Self::InventoryRead { home, key, .. } => home::evict(home, key),
            Self::Inventory { .. } => {}
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
    /// The shared nightly build directory extraction reads (D700), where a
    /// stale run of a build script is cleaned (D742).
    pub(crate) nightly: Option<PathBuf>,
}

/// Why settling stops the run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Build scripts gave rawr's nightly build other outputs than this
    /// project's build, and the nightly has not run them again since. Their
    /// runs there are cleaned: preparing again extracts with fresh ones
    /// (D742).
    #[error(
        "the build-script output of {} differs from this project's build. rawr cleaned \
         its nightly run: run again to extract with this project's output",
        names(.0)
    )]
    Rerun(Box<[Box<str>]>),
    /// A shared table or fact whose build-script outputs differ from this
    /// project's build, removed from rawr's home.
    #[error("{0}")]
    Refused(String),
}

impl Error {
    /// Whether preparing again can succeed: it can after a rerun's clean.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        matches!(self, Self::Rerun(_))
    }
}

impl Settlement {
    /// Explain a failed adapter build when its build scripts gave the
    /// extraction nightly other outputs than the project's build. A
    /// difference the nightly has not run again since may be a stale run
    /// in the shared directory: it is cleaned, and the error says to
    /// prepare again (D742).
    ///
    /// # Errors
    ///
    /// [`Error::Rerun`] when preparing again extracts afresh.
    pub fn drift_diagnostic(&self) -> Result<Option<String>, Error> {
        let Some(rawr) = &self.rawr else {
            return Ok(None);
        };
        let Some(stable) = runner_scripts(rawr) else {
            return Ok(None);
        };
        let verdicts = judge(rawr, self.nightly.as_deref(), &self.entries, Some(&stable));
        let rerun = reruns(&verdicts);
        if !rerun.is_empty() {
            for entry in &self.entries {
                if entry.reruns(&verdicts) {
                    entry.evict(&staged(rawr));
                }
            }
            return Err(Error::Rerun(rerun));
        }
        for entry in &self.entries {
            let crate_name = match entry {
                Settled::Extracted { publish, .. } => &publish.meta.crate_name,
                Settled::Read { crate_name, .. } => crate_name,
                Settled::Inventory { .. } | Settled::InventoryRead { .. } => continue,
            };
            if let Some((package, recorded)) = entry
                .recorded()
                .into_iter()
                .find(|(package, _)| verdicts.contains_key(*package))
            {
                let difference = difference(recorded, stable.get(package).map(Vec::as_slice))
                    .unwrap_or(Difference::Missing);
                return Ok(Some(format!(
                    "the build script of `{}` gives the extraction nightly other output than \
                     this project's build, after running it again too ({difference}): the API \
                     of `{crate_name}` differs between the extraction nightly and the \
                     project's build",
                    package_name(package),
                )));
            }
        }
        Ok(None)
    }

    /// Publish `entry`, a crate's intrinsic facts this run kept with the
    /// project, to rawr's `home` under `key` once the project's build in
    /// `rawr` has given `packages` the build-script outputs it was captured
    /// with (#403).
    pub fn inventory(
        &mut self,
        rawr: &Path,
        home: &Path,
        key: &[u8; 32],
        packages: &[Box<str>],
        entry: &crate::applicability::inventory::Entry,
    ) {
        let entry = entry.clone_rekeyed(*key);
        self.rawr.get_or_insert_with(|| rawr.to_path_buf());
        self.entries.push(Settled::Inventory {
            home: home.to_path_buf(),
            key: crate::extract::engine::hex(key),
            packages: packages.into(),
            entry: Box::new(entry),
        });
    }

    /// Check `entry`, read from rawr's `home` under `key`, against what the
    /// project's build in `rawr` gives `packages` (#403).
    pub fn inventory_read(
        &mut self,
        rawr: &Path,
        home: &Path,
        key: &[u8; 32],
        packages: &[Box<str>],
        entry: &crate::applicability::inventory::Entry,
    ) {
        self.rawr.get_or_insert_with(|| rawr.to_path_buf());
        self.entries.push(Settled::InventoryRead {
            home: home.to_path_buf(),
            key: crate::extract::engine::hex(key),
            crate_name: entry.crate_name().to_owned(),
            packages: packages.into(),
            scripts: entry.scripts().clone(),
        });
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
        if self.nightly.is_none() {
            self.nightly = other.nightly;
        }
    }

    /// Publish newly extracted tables only when the nightly and project's
    /// build-script outputs agree. A table whose outputs differ, whether
    /// extracted, staged or shared, is a miss once (D742); drift after that
    /// leaves it local and names the package. A shared table must still
    /// agree with this project.
    ///
    /// # Errors
    ///
    /// [`Error::Rerun`] when a difference is cleaned and preparing again
    /// extracts afresh. [`Error::Refused`] for a table read from the home
    /// whose build scripts give this project's build other results, evicted
    /// when its lease is free.
    pub fn settle(self) -> Result<(), Error> {
        let Some(rawr) = &self.rawr else {
            return Ok(());
        };
        let stable = runner_scripts(rawr);
        self.settle_with(stable)
    }

    /// [`Settlement::settle`] after `rawr check`, whose project build is the
    /// `cargo check` beside it, as [`Observed`] read it when it ended
    /// (D609.4, #661).
    ///
    /// # Errors
    ///
    /// As [`Settlement::settle`].
    pub fn settle_checked(self, observed: &Observed) -> Result<(), Error> {
        self.settle_with(observed.0.clone())
    }

    fn settle_with(self, stable: Option<BTreeMap<String, Vec<Script>>>) -> Result<(), Error> {
        let Some(rawr) = self.rawr.clone() else {
            return Ok(());
        };
        if self.entries.is_empty() {
            return Ok(());
        }
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
        let verdicts = judge(&rawr, self.nightly.as_deref(), &entries, stable.as_ref());
        let rerun = reruns(&verdicts);
        let staged = staged(&rawr);
        let project = |package: &str| {
            stable
                .as_ref()
                .and_then(|stable| stable.get(package))
                .map(Vec::as_slice)
        };
        // Said once the run settles: a run prepared again settles again.
        let mut notes = Vec::new();
        for entry in entries {
            if entry.reruns(&verdicts) {
                entry.evict(&staged);
                continue;
            }
            match entry {
                Settled::Extracted {
                    key,
                    packages,
                    nightly,
                    mut publish,
                    unshared,
                    ..
                } => {
                    let crate_name = &publish.meta.crate_name;
                    let not_sharing = |reason: &dyn std::fmt::Display| {
                        format!(
                            "rawr: not sharing the bindings of `{crate_name}` with other \
                             projects: {reason}"
                        )
                    };
                    if publish.meta.inputs.is_none() {
                        // The project's own crates, and crates that reach
                        // them, stay with the project (D608.4).
                        match unshared {
                            Some(crate::input::bindings::Unshared::Project) => {}
                            Some(reason) => notes.push(not_sharing(&reason)),
                            None => notes.push(not_sharing(&"its build inputs were not recorded")),
                        }
                        continue;
                    }
                    let Some(nightly) = nightly else {
                        notes.push(not_sharing(
                            &"the build-script outputs of rawr's nightly build are missing or \
                              unreadable",
                        ));
                        continue;
                    };
                    let differs = packages.iter().find_map(|package| {
                        let difference = match nightly.get(package) {
                            Some(recorded) => difference(recorded, project(package))?,
                            None => Difference::Missing,
                        };
                        Some((package, difference))
                    });
                    if let Some((package, difference)) = differs {
                        let name = package_name(package);
                        notes.push(not_sharing(&format_args!(
                            "the build script of `{name}` gives rawr's nightly build other \
                             output than this project's build: {difference}"
                        )));
                        continue;
                    }
                    publish.meta.scripts = nightly;
                    publish_entry(&key, &publish, &staged);
                }
                Settled::Read {
                    directory,
                    key,
                    crate_name,
                    scripts,
                    ..
                } => {
                    // This project's own extraction, with drift its rerun
                    // confirmed or outputs it cannot compare: extracting
                    // again would give it back.
                    if directory == staged {
                        continue;
                    }
                    let differs = scripts.iter().find_map(|(package, recorded)| {
                        Some((package, difference(recorded, project(package))?))
                    });
                    let Some((package, difference)) = differs else {
                        continue;
                    };
                    home::evict(&directory, &key);
                    let name = package_name(package);
                    refused.get_or_insert_with(|| {
                        if verdicts.contains_key(package) {
                            format!(
                                "the shared bindings of `{crate_name}` were extracted where the \
                                 build script of `{name}` gave other output than this project's \
                                 build gives ({difference}), as rawr's nightly build does after \
                                 running it again. rawr removed them: run again to extract them \
                                 here",
                            )
                        } else {
                            format!(
                                "the shared bindings of `{crate_name}` need the output of the \
                                 build script of `{name}`, which this project's build did not \
                                 give. rawr removed them: run again to extract them here",
                            )
                        }
                    });
                }
                Settled::Inventory {
                    home,
                    packages,
                    entry,
                    ..
                } => {
                    let agrees = stable.as_ref().is_some_and(|stable| {
                        entry.scripts_agree(
                            &packages,
                            &crate::applicability::store::Scripts::from_map(stable.clone()),
                        )
                    });
                    if agrees {
                        let _ = crate::applicability::store::publish(&home, &entry);
                    }
                }
                Settled::InventoryRead {
                    home,
                    key,
                    crate_name,
                    packages,
                    scripts,
                } => {
                    let project = crate::applicability::store::Scripts::from_map(
                        stable.clone().unwrap_or_default(),
                    );
                    if scripts.agree(&packages, &project) {
                        continue;
                    }
                    home::evict(&home, &key);
                    refused.get_or_insert_with(|| {
                        format!(
                            "the shared facts of `{crate_name}` were captured with other \
                             build-script output than this project's build gives, or without \
                             it. rawr removed them: run again to capture them here",
                        )
                    });
                }
            }
        }
        if let Some(bindings) = &self.bindings {
            let bindings = bindings.clone();
            std::thread::spawn(move || home::prune(&bindings, IDLE));
        }
        if !rerun.is_empty() {
            return Err(Error::Rerun(rerun));
        }
        for note in notes {
            eprintln!("{note}");
        }
        refused.map_or(Ok(()), |refused| Err(Error::Refused(refused)))
    }
}

/// The verdict on each package whose build script gave one of `entries`
/// other outputs than the project's build gave (`stable`), judged once each
/// against the record in `rawr`. A package to run again is marked in the
/// `nightly` directory, whose next build cleans it; without that directory
/// nothing runs again.
fn judge(
    rawr: &Path,
    nightly: Option<&Path>,
    entries: &[Settled],
    stable: Option<&BTreeMap<String, Vec<Script>>>,
) -> BTreeMap<String, rerun::Verdict> {
    let Some(stable) = stable else {
        return BTreeMap::new();
    };
    let mut differ = BTreeMap::<&str, (&[Script], Vec<Script>, Difference)>::new();
    let mut agreed = Vec::new();
    for entry in entries {
        for (package, recorded) in entry.recorded() {
            let Some(project) = stable.get(package) else {
                continue;
            };
            match difference(recorded, Some(project)) {
                None => agreed.push(package),
                Some(difference) => {
                    differ.entry(package).or_insert_with(|| {
                        let seen = project.iter().map(|run| run.seen_by(recorded)).collect();
                        (recorded, seen, difference)
                    });
                }
            }
        }
    }
    let mut record = rerun::Record::read(rawr);
    for package in agreed {
        if !differ.contains_key(package) {
            record.agreed(package);
        }
    }
    let stale = nightly.map(rerun::Stale::of);
    let verdicts = differ
        .into_iter()
        .map(|(package, (recorded, project, difference))| {
            let verdict = match &stale {
                Some(stale) => {
                    let verdict = record.judge(package, recorded, &project);
                    if verdict == rerun::Verdict::Rerun {
                        stale.mark(package);
                        eprintln!(
                            "rawr: the build script of `{}` gave rawr's nightly build other \
                             output than this project's build ({difference}): running it again \
                             in rawr's nightly build",
                            package_name(package),
                        );
                    }
                    verdict
                }
                None => rerun::Verdict::Drift,
            };
            (package.to_owned(), verdict)
        })
        .collect();
    record.write();
    verdicts
}

/// The names of the packages `verdicts` runs again.
fn reruns(verdicts: &BTreeMap<String, rerun::Verdict>) -> Box<[Box<str>]> {
    let mut names = verdicts
        .iter()
        .filter(|(_, verdict)| **verdict == rerun::Verdict::Rerun)
        .map(|(package, _)| Box::from(package_name(package)))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    names.into_boxed_slice()
}

/// `names` as a message lists them.
fn names(names: &[Box<str>]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn runner_scripts(rawr: &Path) -> Option<BTreeMap<String, Vec<Script>>> {
    let bytes = fs::read(rawr.join("runner").join(RUNNER_SCRIPTS)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Publish `publish`'s table where it settles, as that entry's own and only
/// copy: the driver's copy moves in, or goes when the entry already holds
/// the same bytes. An entry whose table and metadata are unchanged is not
/// written. A settled table is no longer staged.
fn publish_entry(key: &str, publish: &Publish, staged: &Path) {
    let Ok(bytes) = serde_json::to_vec(&publish.meta) else {
        return;
    };
    let kept = table(&publish.bindings, key);
    let own = publish.table == kept;
    // What the entry holds now, under a lease released before publishing.
    let (same_meta, same_table) =
        home::hold(&publish.bindings, key).map_or((false, false), |_lease| {
            (
                meta_in(&publish.bindings.join(key)).as_ref() == Some(&publish.meta),
                own || same_bytes(&publish.table, &kept),
            )
        });
    let published = match (same_table, same_meta) {
        (true, true) => Ok(()),
        (true, false) => home::publish_moving(&publish.bindings, key, &kept, &[(META, &bytes)]),
        (false, _) => {
            home::publish_moving(&publish.bindings, key, &publish.table, &[(META, &bytes)])
        }
    };
    if published.is_ok() {
        if !own {
            let _ = fs::remove_file(&publish.table);
        }
        home::evict(staged, key);
    }
}

/// Whether the files at `left` and `right` hold the same bytes.
fn same_bytes(left: &Path, right: &Path) -> bool {
    let length = |path: &Path| fs::metadata(path).map(|metadata| metadata.len()).ok();
    length(left).is_some_and(|bytes| length(right) == Some(bytes))
        && digest(left).is_some_and(|bytes| digest(right) == Some(bytes))
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

/// `rawr cache clean`: remove shared bindings and inventories unused for an
/// hour, of every extraction protocol, and each protocol's directory that no
/// installed rawr uses once it has gone an hour unused.
pub fn clean(cargo_home: &Path) {
    format::clean(&crate::store::home(cargo_home));
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
            files: BTreeMap::from([("generated.rs".to_owned(), "digest".to_owned())]),
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
                _ => {
                    script.files.insert(
                        "generated.rs".to_owned(),
                        "other generated bytes".to_owned(),
                    );
                }
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
        if !home::alone(
            "bindings::tests::an_entry_appears_whole_and_a_lease_blocks_its_replacement",
        ) {
            return;
        }
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
        if !home::alone("bindings::tests::removal_by_last_use_skips_recent_and_leased_entries") {
            return;
        }
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
        assert_ne!(left.files, changed.files);
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
        if !home::alone("bindings::tests::a_table_publishes_only_with_matching_nightly_scripts_and_an_incompatible_hit_stops_the_run") {
            return;
        }
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
                unshared: None,
            }
        };
        Settlement {
            entries: vec![extracted("first"), extracted("second")],
            bindings: Some(bindings.clone()),
            rawr: Some(first.clone()),
            nightly: None,
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
            directory: bindings.clone(),
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
            nightly: None,
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
            nightly: None,
        }
        .settle()
        .unwrap_err()
        .to_string();
        assert!(refused.contains("`demo`"), "{refused}");
        assert!(super::read(&bindings, "demo").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    /// A table the driver wrote under `extraction`, settling in `bindings`
    /// under no build scripts.
    fn driver_table(extraction: &Path, bindings: &Path, key: &str, bytes: &[u8]) -> Settled {
        fs::create_dir_all(extraction).unwrap();
        let table = extraction.join(key);
        let temporary = extraction.join(format!(".{key}.tmp"));
        fs::write(&temporary, bytes).unwrap();
        fs::rename(&temporary, &table).unwrap();
        extracted(bindings, key, table)
    }

    /// The table at `table`, settling in `bindings` under no build scripts.
    fn extracted(bindings: &Path, key: &str, table: PathBuf) -> Settled {
        Settled::Extracted {
            key: key.to_owned(),
            packages: Vec::new(),
            inputs_packages: Vec::new(),
            nightly: Some(BTreeMap::new()),
            publish: Box::new(Publish {
                bindings: bindings.to_path_buf(),
                table,
                meta: meta(BTreeMap::new()),
            }),
            unshared: None,
        }
    }

    fn modified(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    /// staged/ keeps the entries of the plans it staged for last, as the
    /// driver keeps one probe's tables: an entry no kept plan names goes,
    /// as one of an older protocol does, unless a reader holds it (#1354).
    #[test]
    fn staging_retires_the_entries_no_kept_plan_names_and_skips_a_leased_one() {
        if !home::alone("bindings::tests::staging_retires_the_entries_no_kept_plan_names_and_skips_a_leased_one") {
            return;
        }
        let root = scratch("retire");
        let staged = root.join("staged");
        let extraction = root.join("extraction");
        let bindings = root.join("bindings");
        let inputs = crate::input::bindings::Inputs::fixture;
        let meta = serde_json::to_vec(&meta(BTreeMap::new())).unwrap();
        home::publish(
            &staged,
            "older-protocol",
            &[("table.json", b"units"), ("meta.json", &meta)],
        )
        .unwrap();
        let present = |key: &str| staged.join(key).exists();
        // Each plan is older than the next.
        let age = || {
            for record in fs::read_dir(staged.join(super::PLANS)).unwrap().flatten() {
                let file = fs::File::open(record.path()).unwrap();
                let modified = file.metadata().unwrap().modified().unwrap();
                file.set_modified(modified - Duration::from_secs(60 * 60))
                    .unwrap();
            }
        };
        for key in ["a", "b", "c", "d"] {
            let entry = driver_table(&extraction, &bindings, key, key.as_bytes());
            super::stage(&staged, &[key], &[(&entry, inputs())]);
            age();
        }
        assert!(["a", "b", "c", "d"].into_iter().all(present));
        assert!(!present("older-protocol"));
        // A fifth plan leaves the first unnamed, but its reader keeps it.
        let reader = super::read(&staged, "a").unwrap();
        let entry = driver_table(&extraction, &bindings, "e", b"e");
        super::stage(&staged, &["e"], &[(&entry, inputs())]);
        assert!(["a", "b", "c", "d", "e"].into_iter().all(present));
        drop(reader);
        super::stage(&staged, &["e"], &[(&entry, inputs())]);
        assert!(!present("a"));
        assert!(["b", "c", "d", "e"].into_iter().all(present));
        let records = fs::read_dir(staged.join(super::PLANS)).unwrap().count();
        assert_eq!(records, super::KEPT_PLANS);
        fs::remove_dir_all(root).unwrap();
    }

    /// One copy per key (#1355): staged/ keeps metadata naming the table
    /// the driver wrote, staging it again unchanged writes nothing, and
    /// settling moves it into its entry, after which an unchanged table
    /// and metadata are not published again.
    #[test]
    fn a_second_staging_or_settling_of_unchanged_tables_writes_nothing() {
        if !home::alone(
            "bindings::tests::a_second_staging_or_settling_of_unchanged_tables_writes_nothing",
        ) {
            return;
        }
        let root = scratch("unchanged");
        let rawr = runner(&root, &BTreeMap::new());
        let staged = super::staged(&rawr);
        let extraction = rawr.join("extraction");
        let bindings = root.join("bindings");
        let inputs = crate::input::bindings::Inputs::fixture;
        let entry = driver_table(&extraction, &bindings, "demo", b"units");
        super::stage(&staged, &["demo"], &[(&entry, inputs())]);
        assert!(!staged.join("demo").join("table.json").exists());
        let read = super::read(&staged, "demo").unwrap();
        assert_eq!(read.table, b"units");
        assert_eq!(read.path, extraction.join("demo"));
        drop(read);
        let generation = home::generation(&staged, "demo").unwrap();
        let written = modified(&staged.join("demo").join("meta.json"));
        super::stage(&staged, &["demo"], &[(&entry, inputs())]);
        assert_eq!(home::generation(&staged, "demo").unwrap(), generation);
        assert_eq!(modified(&staged.join("demo").join("meta.json")), written);
        // A table the driver wrote again is staged again; one changed
        // behind staging's back is not read.
        let entry = driver_table(&extraction, &bindings, "demo", b"more units");
        super::stage(&staged, &["demo"], &[(&entry, inputs())]);
        assert_ne!(home::generation(&staged, "demo").unwrap(), generation);
        assert_eq!(super::read(&staged, "demo").unwrap().table, b"more units");
        fs::write(extraction.join("demo"), b"other units").unwrap();
        assert!(super::read(&staged, "demo").is_none());
        let settle = |entry: Settled| {
            Settlement {
                entries: vec![entry],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
                nightly: None,
            }
            .settle()
            .unwrap();
        };
        // Settling moves the driver's table into the entry, and the table
        // is staged no more.
        settle(driver_table(&extraction, &bindings, "demo", b"units"));
        assert_eq!(super::read(&bindings, "demo").unwrap().table, b"units");
        assert!(!extraction.join("demo").exists());
        assert!(!staged.join("demo").exists());
        let generation = home::generation(&bindings, "demo").unwrap();
        let kept = super::table(&bindings, "demo");
        let written = modified(&kept);
        // The driver read the settled table, or wrote the same bytes again:
        // nothing is published, and its copy goes.
        settle(extracted(&bindings, "demo", kept.clone()));
        assert_eq!(home::generation(&bindings, "demo").unwrap(), generation);
        settle(driver_table(&extraction, &bindings, "demo", b"units"));
        assert_eq!(home::generation(&bindings, "demo").unwrap(), generation);
        assert_eq!(modified(&kept), written);
        assert!(!extraction.join("demo").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_extracted_table_is_staged_for_the_next_demand_and_settles_where_it_did() {
        if !home::alone("bindings::tests::an_extracted_table_is_staged_for_the_next_demand_and_settles_where_it_did") {
            return;
        }
        let root = scratch("staged");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let nightly = BTreeMap::from([(package.clone(), vec![script("nightly")])]);
        // The project's build gives other results than the nightly did.
        let rawr = runner(
            &root,
            &BTreeMap::from([(package.clone(), vec![script("stable")])]),
        );
        let staged = super::staged(&rawr);
        let home = root.join("home");
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let extracted = |key: &str, external: &str, nightly: Option<_>| {
            // A table only this project replays may name its own sources,
            // which shared publication refuses.
            let mut meta = meta(BTreeMap::new());
            meta.inputs = None;
            meta.externals.push(External {
                crate_name: external.to_owned(),
                stable_crate_id: "0".to_owned(),
                unit_key: None,
            });
            Settled::Extracted {
                key: key.to_owned(),
                packages: vec![package.clone()],
                inputs_packages: vec![package.clone()],
                nightly,
                publish: Box::new(Publish {
                    bindings: home.clone(),
                    table: table.clone(),
                    meta,
                }),
                unshared: None,
            }
        };
        // Two demands' probes named different external crates: the staged
        // table keeps both, with the nightly's results.
        let inputs = crate::input::bindings::Inputs::fixture;
        let first = extracted("demo", "first", Some(nightly.clone()));
        super::stage(&staged, &["demo"], &[(&first, inputs())]);
        let second = extracted("demo", "second", Some(nightly.clone()));
        super::stage(&staged, &["demo"], &[(&second, inputs())]);
        // A table with no nightly results is probed again rather than
        // replayed.
        let unscripted = extracted("no-nightly", "first", None);
        super::stage(&staged, &["demo"], &[(&unscripted, inputs())]);
        assert!(super::read(&staged, "no-nightly").is_none());
        let entry = super::read(&staged, "demo").unwrap();
        assert_eq!(entry.table, b"units");
        assert_eq!(entry.meta.scripts, nightly);
        assert_eq!(entry.meta.inputs, Some(inputs()));
        let externals = entry
            .meta
            .externals
            .iter()
            .map(|external| external.crate_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(externals, ["second", "first"]);
        drop(entry);
        // Replaying it is the project's own extraction: nothing is refused
        // or removed, and nothing reaches the home.
        Settlement {
            entries: vec![Settled::Read {
                directory: staged.clone(),
                key: "demo".to_owned(),
                crate_name: "demo".to_owned(),
                scripts: nightly,
                inputs: crate::input::bindings::Inputs::fixture(),
            }],
            bindings: Some(home.clone()),
            rawr: Some(rawr),
            nightly: None,
        }
        .settle()
        .unwrap();
        assert!(super::read(&staged, "demo").is_some());
        assert!(super::read(&home, "demo").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nightly_drift_or_missing_evidence_never_publishes_a_table() {
        if !home::alone(
            "bindings::tests::nightly_drift_or_missing_evidence_never_publishes_a_table",
        ) {
            return;
        }
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
        generated.files.insert(
            "generated.rs".to_owned(),
            "different-generated-source".to_owned(),
        );
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
                    unshared: None,
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
                nightly: None,
            };
            let diagnostic = settlement.drift_diagnostic().unwrap();
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
                unshared: None,
            }],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr.clone()),
            nightly: None,
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
                unshared: None,
            }],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr),
            nightly: None,
        }
        .settle()
        .unwrap();
        assert!(super::read(&bindings, "no-scripts").is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_project_outputs_refuse_a_shared_table_before_execution() {
        if !home::alone(
            "bindings::tests::missing_project_outputs_refuse_a_shared_table_before_execution",
        ) {
            return;
        }
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
                    directory: bindings.clone(),
                    key: "key".to_owned(),
                    crate_name: "fixture".to_owned(),
                    scripts: scripts.clone(),
                    inputs: crate::input::bindings::Inputs::fixture(),
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr),
                nightly: None,
            }
            .settle()
            .unwrap_err()
            .to_string();
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
        if !home::alone(
            "bindings::tests::actual_cargo_outputs_publish_only_when_both_toolchains_agree",
        ) {
            return;
        }
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
                    unshared: None,
                }],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
                nightly: None,
            };
            assert_eq!(
                settlement.drift_diagnostic().unwrap().is_some(),
                mode != "match"
            );
            settlement.settle().unwrap();
            assert_eq!(
                super::read(&bindings, mode).is_some(),
                mode == "match",
                "{mode}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// A build script that leaves files rustc never reads, holding its
    /// build's own paths, as a native build's objects and CMake cache do,
    /// still shares: two projects in their own target directories agree on
    /// the generated file the compile read. A script whose read file really
    /// differs shares nothing, and the difference names that file.
    #[test]
    fn outputs_with_per_build_paths_share_and_a_different_read_file_does_not() {
        if !home::alone(
            "bindings::tests::outputs_with_per_build_paths_share_and_a_different_read_file_does_not",
        ) {
            return;
        }
        let root = scratch("per-build-paths");
        let project = root.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\n[package]\nname = \"per-build-paths\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            project.join("src/lib.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n\
             pub const GENERATED: &str = env!(\"GENERATED\");\n",
        )
        .unwrap();
        fs::write(
            project.join("build.rs"),
            r#"fn main() {
    println!("cargo:rerun-if-env-changed=RAWR_TEST_OUTPUT");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let width = if std::env::var("RAWR_TEST_OUTPUT").as_deref() == Ok("generated") { 2 } else { 1 };
    std::fs::write(output.join("generated.rs"), format!("pub type Generated = [u8; {width}];\n")).unwrap();
    std::fs::create_dir_all(output.join("native")).unwrap();
    std::fs::write(output.join("native/objects.o"), output.display().to_string()).unwrap();
    println!("cargo:rustc-env=GENERATED={}", output.join("generated.rs").display());
}
"#,
        )
        .unwrap();
        let build = |target: &str, mode: &str| {
            let output = std::process::Command::new("cargo")
                .args(["check", "--offline", "--message-format=json"])
                .current_dir(&project)
                .env("CARGO_TARGET_DIR", root.join(target))
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
        // rawr's nightly build: its compile read only `generated.rs`.
        let mut nightly = super::scripts_from_messages(&build("nightly", "match")).unwrap();
        let read = std::collections::BTreeSet::from(["generated.rs".to_owned()]);
        for outputs in nightly.values_mut() {
            for output in outputs {
                assert!(output.files.contains_key("native/objects.o"));
                output.read(&read);
            }
        }
        let packages = nightly.keys().cloned().collect::<Vec<_>>();
        assert_eq!(packages.len(), 1);
        let bindings = root.join("bindings");
        let rawr = root.join("rawr");
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let settle = |entry: Settled, observed: &super::Observed| {
            Settlement {
                entries: vec![entry],
                bindings: Some(bindings.clone()),
                rawr: Some(rawr.clone()),
                nightly: None,
            }
            .settle_checked(observed)
        };
        let extracted = |key: &str| Settled::Extracted {
            key: key.to_owned(),
            packages: packages.clone(),
            inputs_packages: packages.clone(),
            nightly: Some(nightly.clone()),
            publish: Box::new(Publish {
                bindings: bindings.clone(),
                table: table.clone(),
                meta: meta(BTreeMap::new()),
            }),
            unshared: None,
        };
        // The first project publishes.
        let first = super::Observed::read(&build("first", "match"));
        settle(extracted("shared"), &first).unwrap();
        let recorded = super::read(&bindings, "shared").unwrap().meta.scripts;
        assert_eq!(recorded, nightly);
        // The second, in its own target directory, reads them.
        let second = super::Observed::read(&build("second", "match"));
        let read_entry = || Settled::Read {
            directory: bindings.clone(),
            key: "shared".to_owned(),
            crate_name: "per-build-paths".to_owned(),
            scripts: recorded.clone(),
            inputs: crate::input::bindings::Inputs::fixture(),
        };
        settle(read_entry(), &second).unwrap();
        assert!(super::read(&bindings, "shared").is_some());
        // A project whose script writes another `generated.rs` shares
        // nothing: its extraction is not published, and the shared table
        // is refused, naming the file.
        let differs = super::Observed::read(&build("differs", "generated"));
        let package = &packages[0];
        assert_eq!(
            super::difference(
                &nightly[package],
                differs.0.as_ref().unwrap().get(package).map(Vec::as_slice),
            ),
            Some(super::Difference::File("generated.rs".to_owned()))
        );
        settle(extracted("differs"), &differs).unwrap();
        assert!(super::read(&bindings, "differs").is_none());
        let refused = settle(read_entry(), &differs).unwrap_err().to_string();
        assert!(refused.contains("`generated.rs`"), "{refused}");
        assert!(super::read(&bindings, "shared").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    /// A table whose build script gave the nightly other outputs than the
    /// project's build is a miss once: its staged copy goes, the nightly's
    /// run is marked to clean, and the run prepares again. The same
    /// difference after that run is drift, reported without another clean,
    /// and stays so until an output changes (D742).
    #[test]
    fn a_difference_cleans_the_nightly_run_once_and_then_is_drift() {
        // Staging leases entries, as the tests that spawn alone do.
        if !home::alone(
            "bindings::tests::a_difference_cleans_the_nightly_run_once_and_then_is_drift",
        ) {
            return;
        }
        let root = scratch("rerun");
        let bindings = root.join("bindings");
        let nightly = root.join("nightly");
        let package = "registry+https://index#demo@1.0.0".to_owned();
        let rawr = runner(
            &root,
            &BTreeMap::from([(package.clone(), vec![script("project")])]),
        );
        let staged = super::staged(&rawr);
        let table = root.join("table");
        fs::write(&table, b"units").unwrap();
        let extracted = |output: &str| Settled::Extracted {
            key: "demo".to_owned(),
            packages: vec![package.clone()],
            inputs_packages: vec![package.clone()],
            nightly: Some(BTreeMap::from([(package.clone(), vec![script(output)])])),
            publish: Box::new(Publish {
                bindings: bindings.clone(),
                table: table.clone(),
                meta: meta(BTreeMap::new()),
            }),
            unshared: None,
        };
        let settlement = |entry: Settled| Settlement {
            entries: vec![entry],
            bindings: Some(bindings.clone()),
            rawr: Some(rawr.clone()),
            nightly: Some(nightly.clone()),
        };
        let stale = super::Stale::of(&nightly);
        super::stage(
            &staged,
            &["demo"],
            &[(
                &extracted("other"),
                crate::input::bindings::Inputs::fixture(),
            )],
        );
        assert!(super::read(&staged, "demo").is_some());
        let rerun = settlement(extracted("other")).settle().unwrap_err();
        assert!(rerun.is_stale());
        assert!(rerun.to_string().contains("`demo`"), "{rerun}");
        assert!(
            super::read(&staged, "demo").is_none(),
            "the stale table stayed staged"
        );
        assert_eq!(stale.pending(), ["demo"]);
        stale.cleaned("demo");
        // The run again gave the same: the toolchains differ. Nothing is
        // cleaned again, now or in a later run.
        for _ in 0..2 {
            settlement(extracted("other")).settle().unwrap();
            assert!(stale.pending().is_empty());
            assert!(super::read(&bindings, "demo").is_none());
        }
        // A shared table made where the script gave yet another output may
        // be a stale run: it goes, and the run prepares again.
        let read = |output: &str| Settled::Read {
            directory: bindings.clone(),
            key: "demo".to_owned(),
            crate_name: "fixture".to_owned(),
            scripts: BTreeMap::from([(package.clone(), vec![script(output)])]),
            inputs: crate::input::bindings::Inputs::fixture(),
        };
        home::publish(&bindings, "demo", &[("table.json", b"units")]).unwrap();
        let rerun = settlement(read("third")).settle().unwrap_err();
        assert!(rerun.is_stale());
        assert!(!bindings.join("demo").exists());
        assert_eq!(stale.pending(), ["demo"]);
        stale.cleaned("demo");
        // The run again agrees: the table publishes, and the difference
        // is forgotten.
        settlement(extracted("project")).settle().unwrap();
        assert!(super::read(&bindings, "demo").is_some());
        assert!(!rawr.join(super::rerun::RECORD).exists());
        // A failed build asks for the rerun once, then names the drift.
        let failed = settlement(extracted("fourth"));
        assert!(failed.drift_diagnostic().unwrap_err().is_stale());
        assert_eq!(stale.pending(), ["demo"]);
        let drift = failed.drift_diagnostic().unwrap().unwrap();
        assert!(
            drift.contains("`demo`") && drift.contains("`fixture`"),
            "{drift}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
