//! rawr's own runtime, compiled once per machine (D620).
//!
//! A runner needs the `rawr` crate and its dependencies. Compiled in each
//! project's build directory, they cost about 85 s on every project's first
//! `rawr run` (T280). Instead, the user's cargo compiles them once into
//! `runtime/<key>` in rawr's home, on the project's toolchain, and each
//! runner links that copy: `extern crate rawr as __rawr_internal;` in its
//! `main.rs`, and link search paths from its build script (D614).
//!
//! The key covers everything that changes the compiled runtime: rawr's
//! version and sources (which carry the lockfile its dependencies resolve
//! from), the full `rustc -vV` of the project's compiler, which names its
//! commit and host target, the runtime's manifest with its features and
//! profile, the opt-level setting, and the hashing seed rawr's build script
//! reads. A key that misses compiles a new entry, so a runner never links a
//! copy compiled for another key. Entries go by last use (D608.7).
//!
//! A project that can't link the shared copy compiles rawr in its own build
//! directory, as before, and says why:
//! - a runner that links rawr-iced, which depends on the project's iced and
//!   so on rawr from the project's build (D601);
//! - a crate both the project and rawr name whose types cross between them,
//!   such as `futures_core::Stream`, which rustc reports as two versions of
//!   one crate (D613.3);
//! - a native static library that both link.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use sha2::{Digest as _, Sha256};
use toml_edit::{DocumentMut, Item, Table};

use crate::cargo::config::INTERNAL_ALIAS;
use crate::node;

const DOMAIN: &[u8] = b"rawr.runtime/v1";
/// The runtime's own package, which only depends on `rawr`.
const PACKAGE: &str = "rawr-runtime";
/// The generated runner's package in its lockfile.
const RUNNER: &str = "rawr-generated-runner";
/// The user setting that lowers the runtime's opt-level (T302).
pub const OPT_LEVEL: &str = "RAWR_RUNTIME_OPT_LEVEL";
/// The opt-level of the `rawr` crate when [`OPT_LEVEL`] is unset.
const DEFAULT_OPT_LEVEL: &str = "3";
/// In `<target>/rawr`: the runner this project could not link to the
/// shared runtime, and why.
const LOCAL: &str = "runtime-local";
/// How long an entry must sit unused before it may be removed.
const IDLE: Duration = Duration::from_secs(24 * 60 * 60);
/// How many recently used entries are kept besides the one in use.
const KEPT: usize = 3;
/// In the runner's directory: the pruned lockfile's digest, then the lock
/// cargo settled it to.
const SETTLED: &str = "rawr-settled-lock";

/// One compiled runtime, leased for as long as a runner build links it.
pub(crate) struct Runtime {
    lib: PathBuf,
    stamp: PathBuf,
    key: String,
    native: BTreeSet<String>,
    /// The project's cargo, which settles the runner's lockfile.
    cargo: PathBuf,
    /// A shared lock on the entry: cleanup never removes it while held.
    _lease: File,
}

/// The opt-level the runtime's `rawr` crate compiles at: [`OPT_LEVEL`] when
/// it is set, else 3. `Err` names a value cargo does not take.
pub(crate) fn opt_level(environment: &crate::cargo::config::Environment) -> Result<Item, String> {
    let value = environment
        .entries()
        .find_map(|(key, value)| (key == OPT_LEVEL).then_some(value))
        .unwrap_or(DEFAULT_OPT_LEVEL)
        .trim();
    match value {
        "0" | "1" | "2" | "3" => Ok(toml_edit::value(i64::from(
            value.as_bytes()[0].saturating_sub(b'0'),
        ))),
        "s" | "z" => Ok(toml_edit::value(value)),
        _ => Err(format!(
            "{OPT_LEVEL} is `{value}`, and it takes 0, 1, 2, 3, s or z"
        )),
    }
}

/// Use the shared runtime for the runner whose generated `manifest` and
/// `lock` are given, compiling it first on a miss. `None` when this runner
/// links rawr from the project's build instead, which has said why.
pub(crate) fn prepare(
    context: &crate::context::Context,
    manifest: &[u8],
    lock: &[u8],
) -> Result<Option<Runtime>, node::Error> {
    let transform = |reason: String| node::Error::Transform(reason.into_boxed_str());
    let text = std::str::from_utf8(manifest).map_err(|error| transform(error.to_string()))?;
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| transform(error.to_string()))?;
    let Some(dependency) = document
        .get("dependencies")
        .and_then(|table| table.get(INTERNAL_ALIAS))
        .and_then(Item::as_table_like)
    else {
        return Ok(None);
    };
    let Some(root) = dependency
        .get("path")
        .and_then(Item::as_str)
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    let lock_text = std::str::from_utf8(lock).map_err(|error| transform(error.to_string()))?;
    if let Some(dependent) = rawr_dependent(lock_text) {
        crate::progress::note(
            "runtime-local",
            &format!(
                "rawr: this runner links `{dependent}`, which depends on rawr, so rawr compiles \
                 in this project"
            ),
        );
        return Ok(None);
    }
    let marker = context.rawr().join(LOCAL);
    let runner = runner_digest(manifest, lock);
    if let Ok(recorded) = fs::read_to_string(&marker)
        && let Some((digest, reason)) = recorded.split_once('\n')
    {
        if digest == runner {
            crate::progress::note(
                "runtime-local",
                &format!("rawr: {}; rawr compiles in this project", reason.trim()),
            );
            return Ok(None);
        }
        let _ = fs::remove_file(&marker);
    }
    let environment = context.environment();
    let level = opt_level(environment).map_err(transform)?;
    let Some(toolchain) = Toolchain::for_context(context)? else {
        crate::progress::note(
            "runtime-local",
            "rawr: this context has no captured project compiler, so rawr compiles in this project",
        );
        return Ok(None);
    };
    let wrapper = wrapper_manifest(&root, dependency, &level);
    let source_lock = fs::read(root.join("Cargo.lock")).map_err(|source| node::Error::Io {
        operation: "read the lockfile of rawr's runtime",
        path: root.join("Cargo.lock"),
        source,
    })?;
    let development = crate::host::DIGEST == [0; 32];
    let mut key = Sha256::new();
    for part in [
        DOMAIN,
        &crate::host::DIGEST,
        // A development build has no source digest: its checkout names it,
        // and cargo decides what is fresh there.
        if development {
            root.as_os_str().as_encoded_bytes()
        } else {
            b""
        },
        &Sha256::digest(&source_lock),
        toolchain.version.as_bytes(),
        wrapper.as_bytes(),
    ] {
        key.update((part.len() as u64).to_le_bytes());
        key.update(part);
    }
    // rawr's build script bakes this seed into the crate.
    for name in ["RAWR_HASHING_SEED", "RAWR_AHASH_SEED"] {
        let value = environment
            .entries()
            .find_map(|(key, value)| (key == name).then_some(value))
            .unwrap_or_default();
        key.update((value.len() as u64).to_le_bytes());
        key.update(value.as_bytes());
    }
    let key = hex(&key.finalize());
    let runtimes = runtimes()?;
    let entry = runtimes.join(&key[..16]);
    let runtime = ensure(
        &entry,
        &key,
        &wrapper,
        &source_lock,
        &toolchain,
        environment,
        development,
    )?;
    let keep = entry.clone();
    let _ = std::thread::Builder::new()
        .name("rawr-runtime-cleanup".into())
        .spawn(move || prune(&runtimes, &[keep.as_path()], IDLE, KEPT));
    Ok(Some(runtime))
}

/// Record that the runner of `manifest` and `lock` could not link the shared
/// runtime, and why, so its next build goes straight to the project's own.
/// A changed runner tries the shared runtime again.
pub(crate) fn record_local(rawr: &Path, manifest: &[u8], lock: &[u8], reason: &str) {
    crate::progress::note(
        "runtime-local",
        &format!("rawr: {reason}; rawr compiles in this project"),
    );
    let _ = fs::create_dir_all(rawr);
    let _ = fs::write(
        rawr.join(LOCAL),
        format!("{}\n{reason}\n", runner_digest(manifest, lock)),
    );
}

fn runner_digest(manifest: &[u8], lock: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update((manifest.len() as u64).to_le_bytes());
    digest.update(manifest);
    digest.update(lock);
    hex(&digest.finalize())
}

/// Why a failed runner build could not link the shared runtime, from rustc's
/// and the linker's messages: a crate both name whose types cross, or a
/// symbol both define.
pub(crate) fn link_failure(stderr: &str) -> Option<String> {
    for marker in [
        "multiple different versions of crate `",
        "two different versions of crate `",
    ] {
        if let Some(at) = stderr.find(marker) {
            let name = stderr[at + marker.len()..].split('`').next().unwrap_or("?");
            return Some(format!(
                "`{name}` crosses between this project and rawr's shared runtime, which each \
                 compile their own"
            ));
        }
    }
    stderr.contains("duplicate symbol").then(|| {
        "a native symbol is defined both by this project and by rawr's shared runtime".to_owned()
    })
}

impl Runtime {
    /// The runner's manifest without its `rawr` dependency and that crate's
    /// profile.
    pub(crate) fn manifest(&self, text: &str) -> Option<String> {
        let mut document = text.parse::<DocumentMut>().ok()?;
        let dependency = document
            .get_mut("dependencies")?
            .as_table_like_mut()?
            .remove(INTERNAL_ALIAS)?;
        let root = dependency
            .as_table_like()
            .and_then(|table| table.get("path"))
            .and_then(Item::as_str)
            .map(str::to_owned);
        if let Some(profiles) = document
            .get_mut("profile")
            .and_then(Item::as_table_like_mut)
        {
            for (_, profile) in profiles.iter_mut() {
                let Some(packages) = profile.get_mut("package").and_then(Item::as_table_like_mut)
                else {
                    continue;
                };
                let names = packages
                    .iter()
                    .map(|(name, _)| name.to_owned())
                    .filter(|name| names_rawr(name, root.as_deref()))
                    .collect::<Vec<_>>();
                for name in names {
                    packages.remove(&name);
                }
            }
        }
        Some(document.to_string())
    }

    /// The runner's lockfile without `rawr` and what only it needs. Cargo
    /// settles it before the `--locked` build (`settle_lock`).
    pub(crate) fn lock(&self, text: &str) -> Option<String> {
        prune_lock(text)
    }

    /// The lock cargo settled the runner's `pruned` lock to at an earlier
    /// build, for the same `manifest`: the lock this build writes instead.
    pub(crate) fn settled_lock(work: &Path, manifest: &[u8], pruned: &[u8]) -> Option<String> {
        let recorded = fs::read_to_string(work.join(SETTLED)).ok()?;
        let (digest, settled) = recorded.split_once('\n')?;
        (digest == runner_digest(manifest, pruned)).then(|| settled.to_owned())
    }

    /// Settle the runner's lockfile in `work`, written from the `pruned`
    /// lock and `manifest`, to what cargo resolves without rawr, unless an
    /// earlier build did. Pruning walks the lock's edges, which name a
    /// shared package's dependencies at the features rawr turned on too:
    /// ahash keeps `const-random` from rawr's `compile-time-rng`. Cargo
    /// drops what no remaining feature needs and keeps every version, so
    /// the build's `--locked` holds.
    pub(crate) fn settle_lock(
        &self,
        work: &Path,
        project: &Path,
        environment: &crate::cargo::config::Environment,
        manifest: &[u8],
        pruned: &[u8],
    ) -> Result<(), node::Error> {
        if Self::settled_lock(work, manifest, pruned).is_some() {
            return Ok(());
        }
        let output = Command::new(&self.cargo)
            .args(["update", "--workspace", "--offline", "--quiet"])
            .arg("--manifest-path")
            .arg(work.join("Cargo.toml"))
            .current_dir(project)
            .env_clear()
            .envs(environment.entries())
            .stdin(Stdio::null())
            .output()
            .map_err(|source| node::Error::Io {
                operation: "settle the runner's lockfile",
                path: self.cargo.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(node::Error::Transform(
                format!(
                    "cargo could not settle the runner's lockfile in {}: {}",
                    work.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                )
                .into_boxed_str(),
            ));
        }
        let lock = work.join("Cargo.lock");
        let settled = fs::read_to_string(&lock).map_err(|source| node::Error::Io {
            operation: "read the runner's settled lockfile",
            path: lock,
            source,
        })?;
        let record = format!("{}\n{settled}", runner_digest(manifest, pruned));
        write_changed(&work.join(SETTLED), record.as_bytes())
    }

    /// The runner's build script, which also points rustc at the runtime.
    pub(crate) fn build_script(&self, text: &str) -> Option<String> {
        let end = text.rfind('}')?;
        let lib = self.lib.to_str()?;
        let stamp = self.stamp.to_str()?;
        Some(format!(
            "{}    // rawr's runtime, compiled once in rawr's home ({key}).\n    \
             println!(\"cargo:rustc-link-search=crate={{}}\", {lib:?});\n    \
             println!(\"cargo:rustc-link-search=dependency={{}}\", {lib:?});\n    \
             println!(\"cargo:rerun-if-changed={{}}\", {stamp:?});\n{}",
            &text[..end],
            &text[end..],
            key = &self.key[..16],
        ))
    }

    /// The runner's `main.rs`, which links the runtime under the name the
    /// generated code uses.
    pub(crate) fn main(&self, text: &str) -> String {
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            if !line.starts_with("#![") {
                break;
            }
            at += line.len();
        }
        format!(
            "{}extern crate rawr as {INTERNAL_ALIAS};\n{}",
            &text[..at],
            &text[at..]
        )
    }

    /// Why a successful runner build still can't keep the shared runtime:
    /// a static library that a build script of the project's links too.
    /// `messages` is the runner build's cargo JSON.
    pub(crate) fn native_clash(&self, messages: &[u8]) -> Option<String> {
        let linked = static_libraries(messages);
        let both = linked.intersection(&self.native).next()?;
        Some(format!(
            "the native library `{both}` is linked both by this project and by rawr's shared \
             runtime"
        ))
    }
}

/// Where a runner that compiled rawr in the project took rawr's sources
/// from: the hosts in rawr's home and this rawr's own. Their units in the
/// project's build directory are stale once its runner links the shared
/// runtime.
pub(crate) fn host_sources() -> Vec<PathBuf> {
    let mut sources = Vec::new();
    if let Ok(runtimes) = runtimes()
        && let Some(home) = runtimes.parent()
    {
        sources.push(home.join("hosts"));
    }
    if let Ok(root) = crate::host::root() {
        sources.push(fs::canonicalize(&root).unwrap_or(root));
    }
    sources
}

/// A runner manifest that compiles rawr in the project, with the `rawr`
/// crate at the opt-level setting. `None` when it already is.
pub(crate) fn with_opt_level(text: &str, level: &Item) -> Option<String> {
    let mut document = text.parse::<DocumentMut>().ok()?;
    let root = document
        .get("dependencies")?
        .get(INTERNAL_ALIAS)?
        .get("path")?
        .as_str()?
        .to_owned();
    let mut changed = false;
    for (_, profile) in document.get_mut("profile")?.as_table_like_mut()?.iter_mut() {
        let Some(packages) = profile.get_mut("package").and_then(Item::as_table_like_mut) else {
            continue;
        };
        for (name, table) in packages.iter_mut() {
            if !names_rawr(&name, Some(&root)) {
                continue;
            }
            if let Some(table) = table.as_table_like_mut()
                && !table.get("opt-level").is_some_and(|current| {
                    current.as_integer() == level.as_integer() && current.as_str() == level.as_str()
                })
            {
                table.insert("opt-level", level.clone());
                changed = true;
            }
        }
    }
    changed.then(|| document.to_string())
}

/// Whether a `[profile.*.package]` key names the `rawr` package at `root`.
fn names_rawr(name: &str, root: Option<&str>) -> bool {
    let Some((source, package)) = name.split_once('#') else {
        return false;
    };
    let package = package.split_once('@').map_or(package, |(name, _)| name);
    package == "rawr"
        && root.is_some_and(|root| {
            source
                .strip_prefix("path+file://")
                .is_some_and(|path| Path::new(path) == Path::new(root))
        })
}

/// A package in the runner's lockfile, other than the runner, that depends
/// on `rawr`, such as rawr-iced.
fn rawr_dependent(lock: &str) -> Option<String> {
    let document = lock.parse::<DocumentMut>().ok()?;
    let packages = document.get("package")?.as_array_of_tables()?;
    packages
        .iter()
        .filter(|package| package.get("name").and_then(Item::as_str) != Some(RUNNER))
        .find(|package| {
            package
                .get("dependencies")
                .and_then(Item::as_array)
                .is_some_and(|dependencies| {
                    dependencies.iter().any(|dependency| {
                        dependency
                            .as_str()
                            .and_then(|text| text.split_whitespace().next())
                            == Some("rawr")
                    })
                })
        })
        .and_then(|package| package.get("name").and_then(Item::as_str))
        .map(str::to_owned)
}

/// `lock` without the runner's `rawr` dependency and every package that only
/// `rawr` reached.
fn prune_lock(lock: &str) -> Option<String> {
    let mut document = lock.parse::<DocumentMut>().ok()?;
    let packages = document.get_mut("package")?.as_array_of_tables_mut()?;
    let identity = |table: &Table| {
        (
            table
                .get("name")
                .and_then(Item::as_str)
                .unwrap_or_default()
                .to_owned(),
            table
                .get("version")
                .and_then(Item::as_str)
                .unwrap_or_default()
                .to_owned(),
            table
                .get("source")
                .and_then(Item::as_str)
                .map(str::to_owned),
        )
    };
    let root = packages.iter().position(|package| {
        package.get("name").and_then(Item::as_str) == Some(RUNNER)
            && !package.contains_key("source")
    })?;
    if let Some(dependencies) = packages
        .get_mut(root)?
        .get_mut("dependencies")
        .and_then(Item::as_array_mut)
    {
        dependencies.retain(|dependency| {
            dependency
                .as_str()
                .and_then(|text| text.split_whitespace().next())
                != Some("rawr")
        });
    }
    let identities = packages.iter().map(identity).collect::<Vec<_>>();
    let edges = packages
        .iter()
        .map(|package| {
            package
                .get("dependencies")
                .and_then(Item::as_array)
                .map(|dependencies| {
                    dependencies
                        .iter()
                        .filter_map(|dependency| dependency.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    // A dependency is `name`, `name version` or `name version (source)`.
    let find = |dependency: &str| {
        let mut parts = dependency.splitn(3, ' ');
        let name = parts.next().unwrap_or_default();
        let version = parts.next();
        let source = parts
            .next()
            .map(|source| source.trim_start_matches('(').trim_end_matches(')'));
        identities
            .iter()
            .position(|(candidate, candidate_version, candidate_source)| {
                candidate == name
                    && version.is_none_or(|version| version == candidate_version)
                    && source.is_none_or(|source| candidate_source.as_deref() == Some(source))
            })
    };
    let mut reached = vec![false; identities.len()];
    let mut queue = VecDeque::from([root]);
    reached[root] = true;
    while let Some(index) = queue.pop_front() {
        for dependency in &edges[index] {
            if let Some(next) = find(dependency)
                && !reached[next]
            {
                reached[next] = true;
                queue.push_back(next);
            }
        }
    }
    let mut kept = toml_edit::ArrayOfTables::new();
    for (index, package) in packages.iter().enumerate() {
        if reached[index] {
            kept.push(package.clone());
        }
    }
    *packages = kept;
    Some(document.to_string())
}

/// The project's compiler, as its `cargo build` finds it.
struct Toolchain {
    /// `rustc -vV`: its release, commit, host target and LLVM.
    version: String,
    rustc: PathBuf,
    cargo: PathBuf,
}

/// The compiler the project's own Cargo build selects, including rustup's
/// directory override. Shared binding replay binds this before using an API.
pub(crate) fn compiler_identity(
    project: &Path,
    config: &crate::cargo::config::Provenance,
) -> Result<String, node::Error> {
    Toolchain::query_program(
        project,
        config.environment(),
        config.compiler(project).configured().map(Path::to_path_buf),
    )
    .map(|toolchain| toolchain.version)
}

impl Toolchain {
    fn for_context(context: &crate::context::Context) -> Result<Option<Self>, node::Error> {
        context
            .compiler()
            .map(|compiler| {
                Self::query_program(
                    context.project(),
                    context.environment(),
                    compiler.configured().map(Path::to_path_buf),
                )
            })
            .transpose()
    }

    fn query_program(
        project: &Path,
        environment: &crate::cargo::config::Environment,
        configured: Option<PathBuf>,
    ) -> Result<Self, node::Error> {
        let rustc = configured.clone().unwrap_or_else(|| PathBuf::from("rustc"));
        let run = |arguments: &[&str]| -> Result<String, node::Error> {
            let output = Command::new(&rustc)
                .args(arguments)
                .current_dir(project)
                .env_clear()
                .envs(environment.entries())
                .stdin(Stdio::null())
                .output()
                .map_err(|source| node::Error::Io {
                    operation: "ask the project's rustc for its version",
                    path: rustc.clone(),
                    source,
                })?;
            if !output.status.success() {
                return Err(node::Error::Transform(
                    format!(
                        "`rustc {}` failed in {}: {}",
                        arguments.join(" "),
                        project.display(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    )
                    .into_boxed_str(),
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        };
        let version = run(&["-vV"])?;
        let sysroot = PathBuf::from(run(&["--print", "sysroot"])?.trim());
        let suffix = std::env::consts::EXE_SUFFIX;
        let exact = sysroot.join("bin").join(format!("rustc{suffix}"));
        let rustc = configured.unwrap_or_else(|| {
            if exact.is_file() {
                exact
            } else {
                PathBuf::from("rustc")
            }
        });
        let cargo = sysroot.join("bin").join(format!("cargo{suffix}"));
        let cargo = if cargo.is_file() {
            cargo
        } else {
            PathBuf::from("cargo")
        };
        Ok(Self {
            version,
            rustc,
            cargo,
        })
    }
}

/// The runtime's manifest: `rawr` as the runner names it, at a fixed profile
/// with the opt-level setting for the `rawr` crate.
fn wrapper_manifest(root: &Path, dependency: &dyn toml_edit::TableLike, level: &Item) -> String {
    let mut rawr = toml_edit::InlineTable::new();
    rawr.insert("path", root.to_string_lossy().as_ref().into());
    for key in ["default-features", "features"] {
        if let Some(value) = dependency.get(key).and_then(Item::as_value) {
            rawr.insert(key, value.clone());
        }
    }
    let mut document = DocumentMut::new();
    let mut package = Table::new();
    package.insert("name", toml_edit::value(PACKAGE));
    package.insert("version", toml_edit::value("0.0.0"));
    package.insert("edition", toml_edit::value("2024"));
    package.insert("publish", toml_edit::value(false));
    document.insert("package", Item::Table(package));
    let mut lib = Table::new();
    lib.insert("path", toml_edit::value("lib.rs"));
    document.insert("lib", Item::Table(lib));
    let mut dependencies = Table::new();
    dependencies.insert("rawr", toml_edit::value(rawr));
    document.insert("dependencies", Item::Table(dependencies));
    // One fixed profile for every project: nothing a project sets reaches
    // the runtime, so the key needs none of it. Nothing is stripped, so
    // cargo never runs `rust-objcopy`, which some toolchains ship broken.
    let mut dev = Table::new();
    dev.insert("debug", toml_edit::value(0));
    dev.insert("incremental", toml_edit::value(false));
    dev.insert("strip", toml_edit::value(false));
    let mut own = Table::new();
    own.insert("opt-level", level.clone());
    let mut packages = Table::new();
    packages.insert("rawr", Item::Table(own));
    dev.insert("package", Item::Table(packages));
    let mut profile = Table::new();
    profile.insert("dev", Item::Table(dev));
    document.insert("profile", Item::Table(profile));
    document.insert("workspace", Item::Table(Table::new()));
    document.to_string()
}

/// `runtime` in rawr's home.
fn runtimes() -> Result<PathBuf, node::Error> {
    let cargo_home = match std::env::var_os("CARGO_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("HOME").ok_or(node::Error::State(
            "rawr's runtime needs CARGO_HOME or HOME",
        ))?)
        .join(".cargo"),
    };
    Ok(crate::store::home(&cargo_home).join("runtime"))
}

/// The entry for `key`, compiled first when it has none, and leased.
fn ensure(
    entry: &Path,
    key: &str,
    wrapper: &str,
    source_lock: &[u8],
    toolchain: &Toolchain,
    environment: &crate::cargo::config::Environment,
    development: bool,
) -> Result<Runtime, node::Error> {
    let io = |operation: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| node::Error::Io {
            operation,
            path,
            source,
        }
    };
    let lease = entry.join(".lock");
    // Cleanup may rename an idle entry away between these steps; a second
    // attempt finds no entry and compiles one.
    let lock = (0..2)
        .find_map(|_| {
            fs::create_dir_all(entry).ok()?;
            fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lease)
                .ok()
        })
        .ok_or_else(|| node::Error::Io {
            operation: "open the lock of rawr's runtime",
            path: lease.clone(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })?;
    lock.lock().map_err(io("lock rawr's runtime", &lease))?;
    let stamp = entry.join("built");
    let lib = entry.join("lib");
    let built = fs::read_to_string(&stamp).is_ok_and(|text| text.trim() == key)
        && fs::read_to_string(entry.join("native")).is_ok();
    if development || !built {
        compile(entry, key, wrapper, source_lock, toolchain, environment)?;
    }
    let native = fs::read_to_string(entry.join("native"))
        .map_err(io("read the native libraries of rawr's runtime", entry))?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    lock.unlock().map_err(io("unlock rawr's runtime", &lease))?;
    lock.lock_shared()
        .map_err(io("lease rawr's runtime", &lease))?;
    // The last use, which cleanup orders entries by.
    let _ = File::open(entry).and_then(|directory| directory.set_modified(SystemTime::now()));
    Ok(Runtime {
        lib,
        stamp,
        key: key.to_owned(),
        native,
        cargo: toolchain.cargo.clone(),
        _lease: lock,
    })
}

/// Compile the runtime in `entry` with the project's toolchain and publish
/// its libraries in `entry/lib`.
fn compile(
    entry: &Path,
    key: &str,
    wrapper: &str,
    source_lock: &[u8],
    toolchain: &Toolchain,
    environment: &crate::cargo::config::Environment,
) -> Result<(), node::Error> {
    let io = |operation: &'static str, path: PathBuf| {
        move |source| node::Error::Io {
            operation,
            path,
            source,
        }
    };
    crate::progress::note(
        "runtime-compile",
        "rawr: compiling rawr's runtime for this toolchain; later projects reuse it",
    );
    write_changed(&entry.join("Cargo.toml"), wrapper.as_bytes())?;
    write_changed(&entry.join("lib.rs"), b"")?;
    // The runtime resolves from the lockfile rawr was released with. Cargo
    // drops what the runtime doesn't need and changes no version.
    let source = entry.join("source.lock");
    if fs::read(&source).ok().as_deref() != Some(source_lock) {
        fs::write(entry.join("Cargo.lock"), source_lock).map_err(io(
            "write the lockfile of rawr's runtime",
            entry.join("Cargo.lock"),
        ))?;
        fs::write(&source, source_lock).map_err(io("record rawr's lockfile", source.clone()))?;
    }
    let target = entry.join("target");
    let build = toml_edit::Value::from(target.to_string_lossy().as_ref()).to_string();
    let mut command = Command::new(&toolchain.cargo);
    command
        .args(["build", "--message-format=json-render-diagnostics"])
        .arg("--config")
        .arg(format!("build.build-dir={build}"))
        .arg("--manifest-path")
        .arg(entry.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .current_dir(entry)
        .env_clear()
        .envs(environment.entries().filter(|(key, _)| !overrides(key)))
        // An empty value overrides every other source of rustflags.
        .env("CARGO_ENCODED_RUSTFLAGS", "")
        .env("RUSTC", &toolchain.rustc)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(io(
        "launch the build of rawr's runtime",
        toolchain.cargo.clone(),
    ))?;
    let stderr = child.stderr.take();
    let log = entry.join("build.log");
    let errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut text);
        }
        text
    });
    let mut messages = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
            crate::progress::observe(true, b"\n");
            messages.extend_from_slice(&line);
            messages.push(b'\n');
        }
    }
    let status = child.wait().map_err(io(
        "wait for the build of rawr's runtime",
        toolchain.cargo.clone(),
    ))?;
    let errors = errors.join().unwrap_or_default();
    let _ = fs::write(&log, &errors);
    if !status.success() {
        let text = String::from_utf8_lossy(&errors);
        let tail = text
            .lines()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(node::Error::Transform(
            format!(
                "rawr's runtime failed to compile ({status}); cargo's output is in {}:\n{tail}",
                log.display()
            )
            .into_boxed_str(),
        ));
    }
    publish(entry, key, &messages)
}

/// Whether an environment variable would change how the runtime compiles in
/// a way its key does not cover: the project's target, build directory,
/// rustflags and profile. The runtime takes its own.
fn overrides(key: &str) -> bool {
    matches!(
        key,
        "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "CARGO_BUILD_RUSTFLAGS"
            | "CARGO_TARGET_DIR"
            | "CARGO_BUILD_TARGET_DIR"
            | "CARGO_BUILD_BUILD_DIR"
            | "CARGO_BUILD_TARGET"
            | "CARGO_INCREMENTAL"
            | "RUSTC"
    ) || key.starts_with("CARGO_PROFILE_")
        || (key.starts_with("CARGO_TARGET_") && key.ends_with("_RUSTFLAGS"))
}

/// Link the libraries cargo's `messages` name into a fresh `entry/lib`, so
/// rustc finds exactly one `rawr`, and record the static libraries the
/// runtime's build scripts link. The stamp goes last.
fn publish(entry: &Path, key: &str, messages: &[u8]) -> Result<(), node::Error> {
    let io = |operation: &'static str, path: PathBuf| {
        move |source| node::Error::Io {
            operation,
            path,
            source,
        }
    };
    let mut files = BTreeMap::new();
    for line in messages.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let library = message
            .pointer("/target/kind")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|kinds| {
                kinds
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|kind| {
                        matches!(
                            kind,
                            "lib" | "rlib" | "dylib" | "proc-macro" | "staticlib" | "cdylib"
                        )
                    })
            });
        if !library {
            continue;
        }
        for file in message
            .get("filenames")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            let path = PathBuf::from(file);
            if let Some(name) = path.file_name() {
                files.insert(name.to_owned(), path);
            }
        }
    }
    let rawr = files
        .keys()
        .filter(|name| {
            let name = name.to_string_lossy();
            name.starts_with("librawr-") && name.ends_with(".rlib")
        })
        .count();
    if rawr != 1 {
        return Err(node::Error::Transform(
            format!("rawr's runtime build named {rawr} `rawr` libraries, not one").into_boxed_str(),
        ));
    }
    let id = std::process::id();
    let staging = entry.join(format!(".lib-{id}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(io("stage rawr's runtime", staging.clone()))?;
    for (name, path) in &files {
        let to = staging.join(name);
        if fs::hard_link(path, &to).is_err() {
            fs::copy(path, &to).map_err(io("stage a library of rawr's runtime", path.clone()))?;
        }
    }
    let lib = entry.join("lib");
    let retired = entry.join(format!(".lib-old-{id}"));
    if lib.exists() {
        fs::rename(&lib, &retired).map_err(io("retire rawr's runtime", lib.clone()))?;
    }
    fs::rename(&staging, &lib).map_err(io("publish rawr's runtime", lib.clone()))?;
    let _ = fs::remove_dir_all(&retired);
    let native = static_libraries(messages)
        .into_iter()
        .map(|name| name + "\n")
        .collect::<String>();
    fs::write(entry.join("native"), native).map_err(io(
        "record the native libraries of rawr's runtime",
        entry.join("native"),
    ))?;
    let partial = entry.join(format!(".built-{id}"));
    fs::write(&partial, format!("{key}\n")).map_err(io("stamp rawr's runtime", partial.clone()))?;
    fs::rename(&partial, entry.join("built"))
        .map_err(io("stamp rawr's runtime", entry.join("built")))
}

/// The static libraries the build scripts in cargo's JSON `messages` link.
fn static_libraries(messages: &[u8]) -> BTreeSet<String> {
    let mut libraries = BTreeSet::new();
    for line in messages.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str)
            != Some("build-script-executed")
        {
            continue;
        }
        for library in message
            .get("linked_libs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            if let Some((kind, name)) = library.split_once('=')
                && kind.starts_with("static")
            {
                libraries.insert(name.split(':').next().unwrap_or(name).to_owned());
            }
        }
    }
    libraries
}

fn write_changed(path: &Path, bytes: &[u8]) -> Result<(), node::Error> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    fs::write(path, bytes).map_err(|source| node::Error::Io {
        operation: "write rawr's runtime manifest",
        path: path.to_path_buf(),
        source,
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// `rawr cache clean`: remove idle runtimes beyond the most recently used.
pub fn clean(cargo_home: &Path) {
    prune(
        &crate::store::home(cargo_home).join("runtime"),
        &[],
        IDLE,
        KEPT,
    );
}

/// Best effort: remove entries other than `keep` that are both unused for
/// `idle` and older than the `kept` most recently used. An entry a runner
/// build leases is skipped, and nothing here waits for a lock or reads free
/// disk. Each goes by being renamed aside first, so a partial removal is
/// never taken for an entry and is finished by a later cleanup.
fn prune(runtimes: &Path, keep: &[&Path], idle: Duration, kept: usize) {
    let Ok(entries) = fs::read_dir(runtimes) else {
        return;
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(".retired-") {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        if name.starts_with('.')
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            || keep.contains(&path.as_path())
        {
            continue;
        }
        let used = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|_| SystemTime::now());
        let idle = used.elapsed().is_ok_and(|age| age > idle);
        candidates.push((used, idle, name, path));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    for (_, idle, name, path) in candidates.into_iter().skip(kept) {
        if !idle {
            continue;
        }
        let Ok(lock) = File::open(path.join(".lock")) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let retired = runtimes.join(format!(".retired-{name}-{}", std::process::id()));
        let renamed = fs::rename(&path, &retired).is_ok();
        drop(lock);
        if renamed {
            let _ = fs::remove_dir_all(&retired);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::SystemTime;

    const LOCK: &str = r#"version = 4

[[package]]
name = "itoa"
version = "1.0.15"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "rawr"
version = "0.3.3"
dependencies = [
 "itoa",
 "smartstring",
]

[[package]]
name = "rawr-generated-runner"
version = "0.0.0"
dependencies = [
 "itoa",
 "rawr",
]

[[package]]
name = "smartstring"
version = "1.0.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    fn runtime() -> super::Runtime {
        let root = scratch("lease");
        fs::create_dir_all(&root).unwrap();
        super::Runtime {
            lib: PathBuf::from("/home/rawr/runtime/0123/lib"),
            stamp: PathBuf::from("/home/rawr/runtime/0123/built"),
            key: "0123456789abcdef0123".into(),
            native: ["sha2-asm".to_owned()].into(),
            cargo: PathBuf::from("cargo"),
            _lease: fs::File::create(root.join("lock")).unwrap(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-runtime-{name}-{}", std::process::id()))
    }

    #[test]
    fn default_compiler_keeps_the_project_toolchain_after_changing_directory() {
        use std::process::Command;

        use crate::cargo::config;

        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("rawr-runtime-toolchain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let runtime = root.join("runtime");
        let home = root.join("cargo-home");
        for directory in [&project, &runtime, &home] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(
            project.join("rust-toolchain.toml"),
            "[toolchain]\nchannel='stable'\n",
        )
        .unwrap();
        fs::write(
            runtime.join("rust-toolchain.toml"),
            format!(
                "[toolchain]\nchannel='{}'\n",
                crate::cargo::toolchain::CHANNEL
            ),
        )
        .unwrap();
        let captured = config::Environment::capture().unwrap();
        let proxy_directory = captured.cargo_home().unwrap().unwrap().join("bin");
        let captured_path = captured
            .entries()
            .find_map(|(key, value)| (key == "PATH").then_some(value))
            .unwrap();
        let path = std::env::join_paths(
            std::iter::once(proxy_directory).chain(std::env::split_paths(captured_path)),
        )
        .unwrap();
        let environment = config::Environment::from_pairs(captured.entries().filter(|(key, _)| {
            !matches!(*key, "RUSTUP_TOOLCHAIN" | "RUSTC" | "CARGO_BUILD_RUSTC")
        }))
        .unwrap()
        .setting("CARGO_HOME", home.to_str().unwrap())
        .unwrap()
        .setting("PATH", path.to_str().unwrap())
        .unwrap();
        let limits = crate::cargo::Limits::standard();
        let provenance = config::Provenance::discover(
            &project,
            None,
            environment.clone(),
            crate::cargo::toolchain::HOST,
            "dev",
            crate::process::parallelism::Proof::discover().unwrap(),
            &limits,
        )
        .unwrap();
        let context = crate::context::Context::capture_config(
            &project,
            &provenance,
            crate::cargo::toolchain::HOST,
            &limits,
        )
        .unwrap();
        assert!(context.compiler().unwrap().configured().is_none());
        let toolchain = super::Toolchain::for_context(&context).unwrap().unwrap();
        assert!(toolchain.rustc.is_absolute(), "{:?}", toolchain.rustc);
        assert_eq!(
            toolchain.version,
            super::compiler_identity(&project, &provenance).unwrap()
        );
        let run = |program: &std::path::Path| {
            let output = Command::new(program)
                .arg("-vV")
                .current_dir(&runtime)
                .env_clear()
                .envs(environment.entries())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        assert_eq!(run(&toolchain.rustc), toolchain.version);
        assert_ne!(
            run(std::path::Path::new("rustc")),
            toolchain.version,
            "the proxy follows the runtime directory's different toolchain"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_runner_lock_keeps_only_what_the_runner_reaches_without_rawr() {
        let pruned = super::prune_lock(LOCK).unwrap();
        assert!(pruned.contains("name = \"itoa\""), "{pruned}");
        assert!(!pruned.contains("name = \"rawr\""), "{pruned}");
        assert!(!pruned.contains("smartstring"), "{pruned}");
        assert!(!pruned.contains(" \"rawr\","), "{pruned}");
        assert_eq!(super::rawr_dependent(LOCK), None);
        let iced = LOCK.replace(
            "[[package]]\nname = \"smartstring\"",
            "[[package]]\nname = \"rawr-iced\"\nversion = \"0.3.3\"\ndependencies = [\n \"rawr\",\n]\n\n[[package]]\nname = \"smartstring\"",
        );
        assert_eq!(super::rawr_dependent(&iced).as_deref(), Some("rawr-iced"));
    }

    #[test]
    fn the_runner_links_the_runtime_in_place_of_its_rawr_dependency() {
        let runtime = runtime();
        let manifest = "[package]\nname = \"rawr-generated-runner\"\n\n[dependencies]\n\
            __rawr_internal = { package = \"rawr\", path = \"/hosts/a\", default-features = false, features = [\"std\"] }\n\
            itoa = \"1\"\n\n\
            [profile.dev.package.\"path+file:///hosts/a#rawr@0.3.3\"]\nopt-level = 3\n\n\
            [profile.dev.package.itoa]\nopt-level = 1\n";
        let rewired = runtime.manifest(manifest).unwrap();
        assert!(!rewired.contains("__rawr_internal"), "{rewired}");
        assert!(!rewired.contains("#rawr@"), "{rewired}");
        assert!(rewired.contains("[profile.dev.package.itoa]"), "{rewired}");
        assert!(rewired.contains("itoa = \"1\""), "{rewired}");

        let main = runtime.main("#![forbid(unsafe_code)]\nstatic A: u8 = 0;\n");
        assert_eq!(
            main,
            "#![forbid(unsafe_code)]\nextern crate rawr as __rawr_internal;\nstatic A: u8 = 0;\n"
        );
        let script = runtime
            .build_script("fn main() {\n    println!(\"cargo:rerun-if-changed=x\");\n}\n")
            .unwrap();
        assert!(
            script.contains(
                "println!(\"cargo:rustc-link-search=crate={}\", \"/home/rawr/runtime/0123/lib\");"
            ),
            "{script}"
        );
        assert!(script.ends_with("}\n"), "{script}");
    }

    #[test]
    fn crossing_types_and_shared_native_libraries_are_named() {
        let stderr = "error[E0277]: the trait bound `Ticks: Stream` is not satisfied\n\
            note: there are multiple different versions of crate `futures_core` in the dependency graph\n";
        let reason = super::link_failure(stderr).unwrap();
        assert!(reason.contains("`futures_core`"), "{reason}");
        assert_eq!(super::link_failure("error[E0308]: mismatched types"), None);
        let messages = br#"{"reason":"build-script-executed","package_id":"x","linked_libs":["static=sha2-asm","dylib=System"],"linked_paths":[],"cfgs":[],"env":[],"out_dir":"/o"}
"#;
        let clash = runtime().native_clash(messages).unwrap();
        assert!(clash.contains("`sha2-asm`"), "{clash}");
        assert_eq!(runtime().native_clash(b"{}\n"), None);
    }

    #[test]
    fn the_opt_level_setting_takes_cargo_s_values() {
        let environment = |value: &str| {
            crate::cargo::config::Environment::from_pairs([(super::OPT_LEVEL, value)]).unwrap()
        };
        let none = crate::cargo::config::Environment::from_pairs::<_, &str, &str>([]).unwrap();
        assert_eq!(
            super::opt_level(&none).unwrap().as_integer(),
            Some(3),
            "the default stays 3"
        );
        assert_eq!(
            super::opt_level(&environment("1")).unwrap().as_integer(),
            Some(1)
        );
        assert_eq!(
            super::opt_level(&environment("s")).unwrap().as_str(),
            Some("s")
        );
        assert!(super::opt_level(&environment("fast")).is_err());
    }

    #[test]
    fn cleanup_removes_idle_entries_beyond_the_newest_and_skips_leased_ones() {
        let runtimes = scratch("prune");
        let _ = fs::remove_dir_all(&runtimes);
        let now = SystemTime::now();
        let ages = [
            ("current", 9),
            ("newest", 0),
            ("old1", 2),
            ("old2", 3),
            ("leased", 4),
        ];
        for (name, days) in ages {
            fs::create_dir_all(runtimes.join(name).join("lib")).unwrap();
            fs::write(runtimes.join(name).join(".lock"), b"").unwrap();
            fs::File::open(runtimes.join(name))
                .and_then(|dir| dir.set_modified(now - super::IDLE * days))
                .unwrap();
        }
        let leased = fs::File::open(runtimes.join("leased/.lock")).unwrap();
        leased.lock_shared().unwrap();
        let current = runtimes.join("current");
        super::prune(&runtimes, &[current.as_path()], super::IDLE, 1);
        let mut left = fs::read_dir(&runtimes)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(left, ["current", "leased", "newest"]);
        drop(leased);
        fs::remove_dir_all(runtimes).unwrap();
    }
}
