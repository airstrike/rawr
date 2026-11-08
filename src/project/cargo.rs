//! A Cargo package as Cargo describes it. Its targets come from `cargo
//! metadata`, and what one target builds with on the host (the crates its
//! code names, the features it enables and its profile's panic strategy)
//! from Cargo's unit graph, asked with the command's feature flags. Rawr
//! infers none of these from `Cargo.toml` itself. Each answer is kept while
//! the files Cargo read for it are unchanged.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub mod cfg;
pub mod features;

/// The workspace `cargo metadata` describes from one manifest: its
/// members, each with its library and binary targets.
#[derive(Debug)]
pub struct Workspace {
    packages: Box<[Package]>,
    /// Cargo's answer, for the fields a package inherits.
    json: serde_json::Value,
}

/// One package of a workspace, with the targets Cargo builds of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    manifest: PathBuf,
    /// The workspace root's manifest, beside the lockfile.
    workspace: PathBuf,
    name: Box<str>,
    targets: Box<[Target]>,
    /// `[package] default-run`.
    default_run: Option<Box<str>>,
}

/// A library or binary target of a package, as Cargo names and finds it:
/// `src/lib.rs`, `src/main.rs`, `src/bin/*.rs`, `src/bin/*/main.rs`, a
/// `[lib]` or a `[[bin]]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    name: Box<str>,
    kind: Kind,
    /// Canonical crate root.
    path: PathBuf,
}

/// The kind of a target Rawr runs from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// The package's library.
    Library,
    /// One of its binaries.
    Binary,
}

/// What one target builds with on the host, from Cargo's unit graph: the
/// crates its code reaches by name (rustc's `--extern`s), with every
/// `[target.'cfg(..)']` table and feature evaluated as Cargo does, the
/// features it enables, and its profile's panic strategy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unit {
    crates: Box<[Box<str>]>,
    /// The crates the package's build script reaches by name.
    build_crates: Box<[Box<str>]>,
    features: features::Enabled,
    panic: Panic,
}

/// A profile's `panic` setting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Panic {
    /// A panic unwinds; an uncaught one exits with status 101.
    #[default]
    Unwind,
    /// A panic aborts the process.
    Abort,
}

/// Cargo could not describe a package.
#[derive(Debug)]
pub enum Error {
    /// Cargo could not be started.
    Spawn {
        /// The Cargo command, as `cargo metadata`.
        command: &'static str,
        /// Why it could not start.
        source: std::io::Error,
    },
    /// Cargo refused the package.
    Failed {
        /// The Cargo command.
        command: &'static str,
        /// What Cargo printed on stderr.
        stderr: Box<str>,
    },
    /// Cargo's answer is not what this version of Cargo prints.
    Malformed {
        /// The Cargo command.
        command: &'static str,
        /// What is missing from the answer.
        reason: &'static str,
    },
}

const METADATA: &str = "cargo metadata";
const UNIT_GRAPH: &str = "cargo build --unit-graph";
const PRINT_CFG: &str = "rustc --print cfg";

/// One kept `cargo metadata` answer, by the manifest it was asked from.
struct Described {
    manifest: PathBuf,
    inputs: Inputs,
    workspace: Arc<Workspace>,
}

/// One kept unit-graph answer, by its package's manifest, its target and
/// the feature flags it was asked with.
struct Built {
    manifest: PathBuf,
    target: Target,
    selection: features::Selection,
    inputs: Inputs,
    unit: Unit,
}

/// What an answer was computed from: the texts of files (`None` when
/// absent) and the listings of the directories Cargo's target discovery
/// reads. An answer is fresh while these read the same.
#[derive(Debug, Eq, PartialEq)]
struct Inputs {
    files: Box<[(PathBuf, Option<String>)]>,
    listings: Box<[(PathBuf, Box<[OsString]>)]>,
}

static DESCRIBED: Mutex<Vec<Described>> = Mutex::new(Vec::new());
static BUILT: Mutex<Vec<Built>> = Mutex::new(Vec::new());

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { command, source } => {
                write!(formatter, "could not run `{command}`: {source}")
            }
            Self::Failed { stderr, .. } => formatter.write_str(
                stderr
                    .trim_end()
                    .strip_prefix("error: ")
                    .unwrap_or(stderr.trim_end()),
            ),
            Self::Malformed { command, reason } => {
                write!(
                    formatter,
                    "`{command}` printed an answer Rawr can't read: {reason}"
                )
            }
        }
    }
}

impl Error {
    /// Whether Cargo is not installed: the command could not be found.
    #[must_use]
    pub fn is_not_installed(&self) -> bool {
        matches!(self, Self::Spawn { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::Failed { .. } | Self::Malformed { .. } => None,
        }
    }
}

impl Workspace {
    /// The workspace of the manifest at `manifest`, as `cargo metadata
    /// --no-deps` describes it.
    pub fn of(manifest: &Path) -> Result<Arc<Self>, Error> {
        let manifest = canonical(manifest);
        let kept = lock(&DESCRIBED)
            .iter()
            .find(|kept| kept.manifest == manifest)
            .map(|kept| (Arc::clone(&kept.workspace), kept.inputs.read_again()));
        if let Some((workspace, now)) = kept {
            let fresh = lock(&DESCRIBED)
                .iter()
                .any(|kept| kept.manifest == manifest && kept.inputs == now);
            if fresh {
                return Ok(workspace);
            }
        }
        let workspace = Arc::new(Self::ask(&manifest)?);
        let inputs = workspace.inputs();
        let mut described = lock(&DESCRIBED);
        described.retain(|kept| kept.manifest != manifest);
        described.push(Described {
            manifest,
            inputs,
            workspace: Arc::clone(&workspace),
        });
        Ok(workspace)
    }

    /// The workspace's members.
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    /// The member whose manifest is `manifest`.
    #[must_use]
    pub fn package(&self, manifest: &Path) -> Option<&Package> {
        let manifest = canonical(manifest);
        self.packages
            .iter()
            .find(|package| package.manifest == manifest)
    }

    /// The member and its library or binary target whose crate root is
    /// the file at `path`.
    #[must_use]
    pub fn target_at(&self, path: &Path) -> Option<(&Package, &Target)> {
        let path = canonical(path);
        self.packages.iter().find_map(|package| {
            package
                .targets
                .iter()
                .find(|target| target.path == path)
                .map(|target| (package, target))
        })
    }

    /// Cargo's description of the member whose manifest is `manifest`, as
    /// `cargo metadata` prints it.
    pub(crate) fn package_json(&self, manifest: &Path) -> Option<&serde_json::Value> {
        let manifest = canonical(manifest);
        self.json["packages"].as_array()?.iter().find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| canonical(Path::new(path)) == manifest)
        })
    }

    fn ask(manifest: &Path) -> Result<Self, Error> {
        let output = cargo()
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--offline",
            ])
            .arg("--manifest-path")
            .arg(manifest)
            .output()
            .map_err(|source| Error::Spawn {
                command: METADATA,
                source,
            })?;
        let json = answer(METADATA, &output)?;
        let malformed = |reason| Error::Malformed {
            command: METADATA,
            reason,
        };
        let workspace = json["workspace_root"]
            .as_str()
            .map(|root| canonical(&Path::new(root).join("Cargo.toml")))
            .ok_or_else(|| malformed("no `workspace_root`"))?;
        let packages = json["packages"]
            .as_array()
            .ok_or_else(|| malformed("no `packages`"))?
            .iter()
            .map(|package| {
                Package::from_json(package, &workspace).ok_or_else(|| malformed("a package"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { packages, json })
    }

    /// What Cargo read to describe the workspace: each member's manifest
    /// and the root's, and the directories its target discovery lists.
    fn inputs(&self) -> Inputs {
        let mut files = self
            .packages
            .iter()
            .flat_map(|package| [package.manifest.clone(), package.workspace.clone()])
            .collect::<Vec<_>>();
        files.sort_unstable();
        files.dedup();
        let mut listings = Vec::new();
        for package in &self.packages {
            let source = package.directory().join("src");
            listings.push(source.clone());
            let binaries = source.join("bin");
            for name in listing(&binaries) {
                let directory = binaries.join(name);
                if directory.is_dir() {
                    listings.push(directory);
                }
            }
            listings.push(binaries);
        }
        Inputs::read(files, listings)
    }
}

impl Inputs {
    /// The texts of `files` and the listings of `directories`, read now.
    fn read(files: Vec<PathBuf>, directories: Vec<PathBuf>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|path| {
                    let text = std::fs::read_to_string(&path).ok();
                    (path, text)
                })
                .collect(),
            listings: directories
                .into_iter()
                .map(|directory| {
                    let names = listing(&directory);
                    (directory, names)
                })
                .collect(),
        }
    }

    /// The same files and directories, read now.
    fn read_again(&self) -> Self {
        Self::read(
            self.files.iter().map(|(path, _)| path.clone()).collect(),
            self.listings
                .iter()
                .map(|(directory, _)| directory.clone())
                .collect(),
        )
    }
}

impl Package {
    /// The package's manifest.
    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// The directory holding the manifest.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.manifest.parent().unwrap_or(&self.manifest)
    }

    /// `[package] name`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The package's library target.
    #[must_use]
    pub fn library(&self) -> Option<&Target> {
        self.targets
            .iter()
            .find(|target| target.kind == Kind::Library)
    }

    /// The package's binary targets.
    pub fn binaries(&self) -> impl Iterator<Item = &Target> {
        self.targets
            .iter()
            .filter(|target| target.kind == Kind::Binary)
    }

    /// The binary target named `name`, as `cargo run --bin <name>` picks it.
    #[must_use]
    pub fn binary(&self, name: &str) -> Option<&Target> {
        self.binaries().find(|target| &*target.name == name)
    }

    /// The binary `cargo run` runs: `default-run`, else the only one. With
    /// several and no `default-run`, Cargo asks for `--bin`, and there is
    /// none.
    #[must_use]
    pub fn run_binary(&self) -> Option<&Target> {
        if let Some(name) = &self.default_run {
            return self.binary(name);
        }
        let mut binaries = self.binaries();
        let first = binaries.next()?;
        binaries.next().is_none().then_some(first)
    }

    /// The target whose crate root is the file at `path`.
    #[must_use]
    pub fn target_at(&self, path: &Path) -> Option<&Target> {
        let path = canonical(path);
        self.targets.iter().find(|target| target.path == path)
    }

    fn from_json(json: &serde_json::Value, workspace: &Path) -> Option<Self> {
        let mut targets = Vec::new();
        for target in json["targets"].as_array()? {
            let kinds = target["kind"].as_array()?;
            let is = |kind: &str| kinds.iter().any(|value| value.as_str() == Some(kind));
            let kind = if is("bin") {
                Kind::Binary
            } else if ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"]
                .into_iter()
                .any(is)
            {
                Kind::Library
            } else {
                continue;
            };
            targets.push(Target {
                name: target["name"].as_str()?.into(),
                kind,
                path: canonical(Path::new(target["src_path"].as_str()?)),
            });
        }
        Some(Self {
            manifest: canonical(Path::new(json["manifest_path"].as_str()?)),
            workspace: workspace.to_path_buf(),
            name: json["name"].as_str()?.into(),
            targets: targets.into_boxed_slice(),
            default_run: json["default_run"].as_str().map(Box::from),
        })
    }
}

impl Target {
    /// The target's name: a binary's is what `--bin` takes.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name rustc gives the target's crate, which its paths start from.
    #[must_use]
    pub fn crate_name(&self) -> String {
        self.name.replace('-', "_")
    }

    /// Whether this is the package's library or one of its binaries.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The canonical crate root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Unit {
    /// What `target` of `package` builds with on the host, as `cargo
    /// build` builds it in the dev profile, with the process's feature
    /// flags ([`features::Selection::selected`]).
    pub fn of(package: &Package, target: &Target) -> Result<Self, Error> {
        let selection = features::Selection::selected();
        let files = || {
            let mut files = vec![
                package.workspace.clone(),
                package.workspace.with_file_name("Cargo.lock"),
                package.manifest.clone(),
            ];
            files.sort_unstable();
            files.dedup();
            files
        };
        let now = Inputs::read(files(), Vec::new());
        let kept = lock(&BUILT)
            .iter()
            .find(|kept| {
                kept.manifest == package.manifest
                    && kept.target == *target
                    && kept.selection == selection
                    && kept.inputs == now
            })
            .map(|kept| kept.unit.clone());
        if let Some(unit) = kept {
            return Ok(unit);
        }
        let unit = Self::ask(package, target, &selection)?;
        // Cargo may have written the lockfile, so the inputs are read after.
        let inputs = Inputs::read(files(), Vec::new());
        let mut built = lock(&BUILT);
        built.retain(|kept| !(kept.manifest == package.manifest && kept.target == *target));
        built.push(Built {
            manifest: package.manifest.clone(),
            target: target.clone(),
            selection,
            inputs,
            unit: unit.clone(),
        });
        Ok(unit)
    }

    /// The crates the target's code reaches by name: each dependency's
    /// crate name, as rustc's extern prelude has it.
    #[must_use]
    pub fn crates(&self) -> &[Box<str>] {
        &self.crates
    }

    /// A unit that links `crates` and unwinds, as a test's Cargo answer.
    #[cfg(test)]
    pub(crate) fn linking(crates: &[&str]) -> Self {
        Self {
            crates: crates.iter().copied().map(Box::from).collect(),
            build_crates: Box::default(),
            features: features::Enabled::default(),
            panic: Panic::Unwind,
        }
    }

    /// A unit that links `crates` for the target whose `rustc --print cfg`
    /// gives `lines`, as a test's Cargo answer.
    #[cfg(test)]
    pub(crate) fn linking_for(crates: &[&str], lines: &[&str]) -> Self {
        Self {
            features: features::Enabled::of_lines(lines.iter().copied()),
            ..Self::linking(crates)
        }
    }

    /// The features Cargo enables for the target.
    #[must_use]
    pub const fn features(&self) -> &features::Enabled {
        &self.features
    }

    /// The crates the package's build script reaches by name.
    #[must_use]
    pub fn build_crates(&self) -> &[Box<str>] {
        &self.build_crates
    }

    /// The profile's panic strategy.
    #[must_use]
    pub const fn panic(&self) -> Panic {
        self.panic
    }

    fn ask(
        package: &Package,
        target: &Target,
        selection: &features::Selection,
    ) -> Result<Self, Error> {
        let mut command = cargo();
        // The unit graph is a nightly feature of any Cargo.
        command
            .env("RUSTC_BOOTSTRAP", "1")
            .args(["build", "--unit-graph", "-Z", "unstable-options"])
            .arg("--manifest-path")
            .arg(&package.manifest)
            .args(selection.flags());
        match target.kind {
            Kind::Library => command.arg("--lib"),
            Kind::Binary => command.args(["--bin", &target.name]),
        };
        let output = command.output().map_err(|source| Error::Spawn {
            command: UNIT_GRAPH,
            source,
        })?;
        let json = answer(UNIT_GRAPH, &output)?;
        let malformed = |reason| Error::Malformed {
            command: UNIT_GRAPH,
            reason,
        };
        let units = json["units"]
            .as_array()
            .ok_or_else(|| malformed("no `units`"))?;
        let root = json["roots"]
            .as_array()
            .and_then(|roots| roots.first())
            .and_then(|index| unit(units, index))
            .ok_or_else(|| malformed("no root unit"))?;
        let dependencies = |of| depended(units, of).ok_or_else(|| malformed("a dependency"));
        let mut crates = Vec::new();
        let mut build_crates = Vec::new();
        for (depended, name) in dependencies(root)? {
            // A build script's run is a unit the package's code never
            // names; the script it runs depends on the build dependencies.
            if depended["mode"].as_str() != Some("run-custom-build") {
                crates.extend(name.map(Box::from));
                continue;
            }
            for (script, _) in dependencies(depended)? {
                if script["mode"].as_str() == Some("build") {
                    build_crates.extend(
                        dependencies(script)?
                            .into_iter()
                            .filter_map(|(_, name)| name.map(Box::from)),
                    );
                }
            }
        }
        for names in [&mut crates, &mut build_crates] {
            names.sort_unstable();
            names.dedup();
        }
        let panic = match root["profile"]["panic"].as_str() {
            Some("abort") => Panic::Abort,
            _ => Panic::Unwind,
        };
        let directory = package.manifest.parent().unwrap_or(Path::new("."));
        let target = cfg::Target::of_unit(directory, root).map_err(|error| match error {
            cfg::Error::Spawn(source) => Error::Spawn {
                command: PRINT_CFG,
                source,
            },
            cfg::Error::Failed(stderr) => Error::Failed {
                command: PRINT_CFG,
                stderr,
            },
        })?;
        Ok(Self {
            crates: crates.into_boxed_slice(),
            build_crates: build_crates.into_boxed_slice(),
            features: features::Enabled::of_unit(root).with_target(target),
            panic,
        })
    }
}

/// The unit at `index` of a unit graph's `units`.
fn unit<'a>(
    units: &'a [serde_json::Value],
    index: &serde_json::Value,
) -> Option<&'a serde_json::Value> {
    units.get(usize::try_from(index.as_u64()?).ok()?)
}

/// The units `of` depends on, each with the name its code reaches it by.
fn depended<'a>(
    units: &'a [serde_json::Value],
    of: &'a serde_json::Value,
) -> Option<Vec<(&'a serde_json::Value, Option<&'a str>)>> {
    of["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|dependency| {
            Some((
                unit(units, &dependency["index"])?,
                dependency["extern_crate_name"].as_str(),
            ))
        })
        .collect()
}

/// `$CARGO`, else `cargo`, with no stdin.
fn cargo() -> Command {
    let program = std::env::var_os("CARGO")
        .filter(|cargo| !cargo.is_empty())
        .unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    command
}

/// A Cargo command's JSON answer, or what it said when it refused.
fn answer(
    command: &'static str,
    output: &std::process::Output,
) -> Result<serde_json::Value, Error> {
    if !output.status.success() {
        return Err(Error::Failed {
            command,
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|_| Error::Malformed {
        command,
        reason: "its output is not JSON",
    })
}

/// The sorted names in `directory`, none when it is absent.
fn listing(directory: &Path) -> Box<[OsString]> {
    let mut names = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.into_boxed_slice()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn lock<T>(cell: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    cell.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
