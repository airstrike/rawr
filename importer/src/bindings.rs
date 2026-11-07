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

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::extract::engine::home;

mod rerun;

pub(crate) use rerun::Stale;

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
/// project's own sources. Best effort: a table not staged is probed again.
pub(crate) fn stage(directory: &Path, settled: &[(&Settled, crate::input::bindings::Inputs)]) {
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
        let Ok(table) = fs::read(&publish.table) else {
            continue;
        };
        let mut meta = publish.meta.clone();
        meta.scripts = nightly.clone();
        meta.inputs = Some(inputs.clone());
        if let Some(kept) = read(directory, key)
            && kept.meta.crate_hash == meta.crate_hash
        {
            for external in kept.meta.externals {
                if !meta.externals.contains(&external) {
                    meta.externals.push(external);
                }
            }
        }
        let Ok(bytes) = serde_json::to_vec(&meta) else {
            continue;
        };
        let _ = home::publish(directory, key, &[("table.json", &table), (META, &bytes)]);
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
            if let Some((package, _)) = entry
                .recorded()
                .into_iter()
                .find(|(package, _)| verdicts.contains_key(*package))
            {
                return Ok(Some(format!(
                    "the build script of `{}` gives the extraction nightly other output than \
                     this project's build, after running it again too: the API of \
                     `{crate_name}` differs between the extraction nightly and the project's \
                     toolchain",
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
    /// `cargo check` beside it: `messages` are that build's JSON messages,
    /// whose `build-script-executed` lines give the project toolchain's
    /// build-script outputs (D609.4, #661).
    ///
    /// # Errors
    ///
    /// As [`Settlement::settle`].
    pub fn settle_checked(self, messages: &[u8]) -> Result<(), Error> {
        let stable = scripts_from_messages(messages);
        self.settle_with(stable)
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
        let project = |package: &str| stable.as_ref().and_then(|stable| stable.get(package));
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
                    ..
                } => {
                    if publish.meta.inputs.is_none() {
                        eprintln!("rawr: not sharing the bindings of `{}` with other projects: its build inputs were not recorded", publish.meta.crate_name);
                        continue;
                    }
                    let Some(nightly) = nightly else {
                        eprintln!("rawr: not sharing the bindings of `{}` with other projects: the nightly build's build-script outputs are missing or unreadable", publish.meta.crate_name);
                        continue;
                    };
                    let differs = packages.iter().find(|package| {
                        let recorded = nightly.get(*package);
                        recorded.is_none() || recorded != project(package)
                    });
                    if let Some(package) = differs {
                        let name = package_name(package);
                        if verdicts.contains_key(package) {
                            eprintln!("rawr: not sharing the bindings of `{}` with other projects: the build script of `{name}` gives the nightly build other output than this project's build, so the two toolchains differ", publish.meta.crate_name);
                        } else {
                            eprintln!("rawr: not sharing the bindings of `{}` with other projects: the output of the build script of `{name}` is missing from the nightly build or this project's build", publish.meta.crate_name);
                        }
                        continue;
                    }
                    publish.meta.scripts = nightly;
                    publish_entry(&key, &publish);
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
                    let differs = scripts
                        .iter()
                        .find(|(package, recorded)| project(package) != Some(*recorded));
                    let Some((package, _)) = differs else {
                        continue;
                    };
                    home::evict(&directory, &key);
                    let name = package_name(package);
                    refused.get_or_insert_with(|| {
                        if verdicts.contains_key(package) {
                            format!(
                                "the shared bindings of `{crate_name}` were extracted where the \
                                 build script of `{name}` gave other output than this project's \
                                 build gives, as rawr's nightly build does after running it \
                                 again. rawr removed them: run again to extract them here",
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
    let mut differ = BTreeMap::<&str, (&[Script], &[Script])>::new();
    let mut agreed = Vec::new();
    for entry in entries {
        for (package, recorded) in entry.recorded() {
            let Some(project) = stable.get(package) else {
                continue;
            };
            if recorded == project.as_slice() {
                agreed.push(package);
            } else {
                differ.entry(package).or_insert((recorded, project));
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
        .map(|(package, (recorded, project))| {
            let verdict = match &stale {
                Some(stale) => {
                    let verdict = record.judge(package, recorded, project);
                    if verdict == rerun::Verdict::Rerun {
                        stale.mark(package);
                        eprintln!(
                            "rawr: the build-script output of `{}` differs from this project's \
                             build: running it again in rawr's nightly build",
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
            }
        };
        // Two demands' probes named different external crates: the staged
        // table keeps both, with the nightly's results.
        let inputs = crate::input::bindings::Inputs::fixture;
        let first = extracted("demo", "first", Some(nightly.clone()));
        super::stage(&staged, &[(&first, inputs())]);
        let second = extracted("demo", "second", Some(nightly.clone()));
        super::stage(&staged, &[(&second, inputs())]);
        // A table with no nightly results is probed again rather than
        // replayed.
        let unscripted = extracted("no-nightly", "first", None);
        super::stage(&staged, &[(&unscripted, inputs())]);
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

    /// A table whose build script gave the nightly other outputs than the
    /// project's build is a miss once: its staged copy goes, the nightly's
    /// run is marked to clean, and the run prepares again. The same
    /// difference after that run is drift, reported without another clean,
    /// and stays so until an output changes (D742).
    #[test]
    fn a_difference_cleans_the_nightly_run_once_and_then_is_drift() {
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
