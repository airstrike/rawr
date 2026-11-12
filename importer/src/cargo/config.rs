//! Canonical Cargo dependency and configuration provenance.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use semver::VersionReq;
use sha2::{Digest as _, Sha256};
use toml_edit::DocumentMut;

use super::{digest, normalized_absolute, Digest, Limits};
use crate::process::ToolPaths;
use crate::{Error, Result};

pub(crate) mod env;
pub(crate) mod projection;

const MAX_ENVIRONMENT_ENTRIES: usize = 4_096;
const MAX_ENVIRONMENT_BYTES: usize = 256 * 1024 * 1024;
const MAX_VALUE_BYTES: usize = 1024 * 1024;
const IMPORTER_OVERRIDDEN: [&str; 15] = [
    "CARGO_TARGET_DIR",
    "CARGO_INCREMENTAL",
    "RUSTC",
    "RUSTDOC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTDOC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_JOBS",
    "CARGO_MAKEFLAGS",
    "MAKEFLAGS",
    "MFLAGS",
    "NUM_JOBS",
];
const WRAPPER_ENVIRONMENT: [&str; 4] = [
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
];
/// The dynamic loader's search path on the ELF hosts the importer supports.
pub(crate) const LOADER_PATH: &str = "LD_LIBRARY_PATH";
const INTERNAL_ENVIRONMENT_PREFIX: &str = "RAWR_IMPORT_";
const UNSTABLE_ENVIRONMENT_PREFIX: &str = "CARGO_UNSTABLE_";
pub(crate) const INTERNAL_ALIAS: &str = "__rawr_internal";

type EnvironmentValue = (Box<str>, Box<str>);

#[cfg(test)]
std::thread_local! {
    static OWNED_PROVENANCE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_INPUT_CONFIG_CANDIDATES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One role occupied by a dependency in the final runner universe.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Role {
    /// An unchanged Rust library selected by Rawr-side configuration.
    Library,
    /// Rawr runtime support linked into the eventual runner.
    Runtime,
    /// Rawr compiler support linked into the eventual runner.
    Compiler,
}

/// A nonempty canonical role proof for one dependency identity.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Roles(u8);

/// One unresolved dependency specification promoted only by `Request::build`.
#[derive(Clone)]
pub struct Dependency {
    alias: Box<str>,
    package: Option<Box<str>>,
    source: Source,
    features: Box<[Box<str>]>,
    default_features: bool,
    roles: Roles,
}

/// An exact environment replayed after ambient state is cleared.
#[derive(Clone, Default)]
pub struct Environment {
    values: Box<[EnvironmentValue]>,
}

/// Proof that every retained Cargo authority excludes unstable build-std.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NoBuildStd {
    private: (),
}

impl NoBuildStd {
    pub(crate) const fn admits_pinned_omission(self) -> bool {
        let Self { private: () } = self;
        true
    }

    #[cfg(test)]
    pub(crate) const fn fixture() -> Self {
        Self { private: () }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Source {
    Path(PathBuf),
    Registry {
        manifest: Box<str>,
        requirement: VersionReq,
    },
    Git {
        url: Box<str>,
        reference: GitReference,
        /// A `version` beside `git`: Cargo checks it against the Git package.
        version: Option<Box<str>>,
    },
}

/// What a Git dependency names in its repository, as its manifest spells it.
/// Cargo pins the commit each one resolves to in `Cargo.lock`, so rawr
/// declares the same reference and Cargo's lock decides the commit.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GitReference {
    /// `branch = "..."`.
    Branch(Box<str>),
    /// `tag = "..."`.
    Tag(Box<str>),
    /// `rev = "..."`: a commit hash, short or full, or another reference.
    Rev(Box<str>),
    /// No `branch`, `tag` or `rev`: the repository's default branch.
    DefaultBranch,
}

impl GitReference {
    /// The manifest key and value that declare this reference.
    #[must_use]
    pub fn key_value(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Branch(name) => Some(("branch", name)),
            Self::Tag(name) => Some(("tag", name)),
            Self::Rev(name) => Some(("rev", name)),
            Self::DefaultBranch => None,
        }
    }

    /// Split a Cargo Git source id, `git+<url>[?<kind>=<name>][#<commit>]`,
    /// into its URL and reference, as `cargo metadata` prints it.
    #[must_use]
    pub fn parse_source(source: &str) -> Option<(&str, Self)> {
        let location = source.strip_prefix("git+")?;
        let location = location
            .split_once('#')
            .map_or(location, |(location, _)| location);
        let Some((url, query)) = location.split_once('?') else {
            return Some((location, Self::DefaultBranch));
        };
        let mut reference = Self::DefaultBranch;
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;
            reference = match key {
                "branch" => Self::Branch(value.into()),
                "tag" => Self::Tag(value.into()),
                "rev" => Self::Rev(value.into()),
                _ => continue,
            };
        }
        Some((url, reference))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Selected {
    alias: Box<str>,
    package: Option<Box<str>>,
    source: Source,
    features: Box<[Box<str>]>,
    default_features: bool,
    roles: Roles,
}

#[derive(Clone, Debug)]
enum State {
    Absent,
    Present(Digest),
}

#[derive(Clone, Debug)]
struct Candidate {
    path: PathBuf,
    state: State,
}

#[derive(Clone, Debug)]
pub(crate) struct Provenance {
    candidates: Box<[Candidate]>,
    explicit: Box<[PathBuf]>,
    environment: Environment,
    projection: projection::Projection,
    /// The merged `[env]` of every file Cargo reads for the project.
    env: env::Table,
    no_build_std: NoBuildStd,
    digest: Digest,
    /// Project/hierarchical `build.rustc`, resolved during the existing parse.
    /// Selected-authority configs used only by extraction cannot select the
    /// project's runner compiler.
    compiler: Option<PathBuf>,
}

/// A compiler selection derived from Cargo authority. The default rustup
/// lookup must become the project's exact sysroot executable before a build
/// changes directory; an explicitly configured program retains its semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Compiler {
    configured: Option<PathBuf>,
}

impl Compiler {
    pub(crate) fn configured(&self) -> Option<&Path> {
        self.configured.as_deref()
    }
}

impl Roles {
    const fn new(role: Role) -> Self {
        Self(role.bit())
    }

    const fn and(self, role: Role) -> Self {
        Self(self.0 | role.bit())
    }

    const fn library() -> Self {
        Self::new(Role::Library)
    }

    const fn rawr() -> Self {
        Self::new(Role::Runtime).and(Role::Compiler)
    }

    /// Test whether this proof contains a role.
    pub const fn contains(self, role: Role) -> bool {
        self.0 & role.bit() != 0
    }

    /// Iterate the canonical role order.
    pub fn iter(self) -> impl Iterator<Item = Role> {
        [Role::Library, Role::Runtime, Role::Compiler]
            .into_iter()
            .filter(move |role| self.contains(*role))
    }
}

impl fmt::Debug for Roles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl Role {
    /// The stable canonical role spelling used by content identities.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Runtime => "runtime",
            Self::Compiler => "compiler",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Library => 1,
            Self::Runtime => 2,
            Self::Compiler => 4,
        }
    }
}

impl Dependency {
    /// Begin a local-path dependency specification.
    pub fn path(alias: &str, path: &Path) -> Result<Self> {
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Path(path.to_path_buf()),
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
        })
    }

    /// Begin a registry dependency with a semantic Cargo version requirement.
    pub fn registry(alias: &str, version: &str) -> Result<Self> {
        let requirement = VersionReq::parse(version).map_err(|error| Error::Request {
            reason: format!(
                "failed to parse the version requirement `{version}` for dependency \
                 `{alias}`: {error}"
            )
            .into_boxed_str(),
        })?;
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Registry {
                manifest: version.into(),
                requirement,
            },
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
        })
    }

    /// Begin a Git dependency specification. Cargo locks the commit its
    /// reference resolves to, in the project's `Cargo.lock`.
    pub fn git(alias: &str, url: &str, reference: GitReference) -> Result<Self> {
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Git {
                url: url.into(),
                reference,
                version: None,
            },
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
        })
    }

    /// Add the `version` written beside `git`. Cargo checks the requirement
    /// against the Git package's version, so rawr passes it through as written.
    pub fn git_version(mut self, version: &str) -> Self {
        let Source::Git {
            version: retained, ..
        } = &mut self.source
        else {
            panic!("a `version` beside `git` belongs to a Git dependency");
        };
        *retained = Some(version.into());
        self
    }

    /// Select a package name when it differs from the Rawr alias.
    pub fn package(mut self, package: &str) -> Result<Self> {
        if !valid_package_name(package) {
            return Err(Error::Request {
                reason: format!(
                    "invalid package name `{package}` for dependency `{}`",
                    self.alias
                )
                .into_boxed_str(),
            });
        }
        self.package = Some(package.into());
        Ok(self)
    }

    /// Seal an order-independent Cargo feature set and default-feature policy.
    pub fn features<I, S>(mut self, features: I, default_features: bool) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut retained = Vec::new();
        for feature in features {
            let feature = feature.as_ref();
            if feature.is_empty() {
                return Err(Error::Request {
                    reason: format!("dependency `{}` names an empty feature", self.alias)
                        .into_boxed_str(),
                });
            }
            retained.push(Box::<str>::from(feature));
        }
        retained.sort_unstable();
        retained.dedup();
        self.features = retained.into_boxed_slice();
        self.default_features = default_features;
        Ok(self)
    }

    pub(crate) fn rawr(path: &Path) -> Result<Self> {
        // `sync` makes script values `Send`, so a script future is a `Send`
        // std future any runtime polls (D513.3). It changes no unit the
        // project builds: only rawr's own proc macro sees it.
        // `debugging` lets `rawr mcp` read the running program's values
        // (D675). It adds no dependency, so the project's units are the
        // same, and the runtime compiles once per rawr version either way
        // (D620). A run that `rawr mcp` didn't start registers no
        // debugger: each step checks that one `Option` and goes on, as
        // `rawr run` does for a program without dependencies.
        let features: Box<[Box<str>]> = Box::from([
            Box::<str>::from("debugging"),
            Box::<str>::from("rust-import"),
            Box::<str>::from("std"),
            Box::<str>::from("sync"),
        ]);
        Ok(Self {
            alias: validated_alias(INTERNAL_ALIAS)?,
            package: Some("rawr".into()),
            source: Source::Path(path.to_path_buf()),
            features,
            default_features: false,
            roles: Roles::rawr(),
        })
    }

    pub(crate) fn seal(self, project_root: &Path) -> Result<Selected> {
        let source = match self.source {
            Source::Path(path) => {
                let path = if path.is_absolute() {
                    path
                } else {
                    project_root.join(path)
                };
                Source::Path(super::resolved_directory("dependency", &path)?)
            }
            source => source,
        };
        Ok(Selected {
            alias: self.alias,
            package: self.package,
            source,
            features: self.features,
            default_features: self.default_features,
            roles: self.roles,
        })
    }
}

impl Environment {
    /// Capture the complete current Unicode process environment.
    pub fn capture() -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut total = 0_usize;
        for (key, value) in std::env::vars_os() {
            let key = key.into_string().map_err(|key| Error::Request {
                reason: format!("the environment variable {key:?} has a name that is not Unicode")
                    .into_boxed_str(),
            })?;
            let value = value.into_string().map_err(|_| Error::Request {
                reason: format!("the environment variable `{key}` has a value that is not Unicode")
                    .into_boxed_str(),
            })?;
            insert_owned_environment(&mut values, &mut total, key, value)?;
        }
        Ok(Self {
            values: environment_values(values)?,
        })
    }

    /// Seal an explicit deterministic environment.
    pub fn from_pairs<I, K, V>(values: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut unique = BTreeMap::new();
        let mut total = 0_usize;
        for (key, value) in values {
            let key = key.as_ref();
            let value = value.as_ref();
            insert_environment(&mut unique, &mut total, key, value)?;
        }
        Ok(Self {
            values: environment_values(unique)?,
        })
    }

    /// Replace one sealed environment entry.
    pub fn setting(self, key: &str, value: &str) -> Result<Self> {
        let _ = validate_environment_entry(&BTreeMap::new(), 0, key, value)?;
        let mut values = self.values.into_vec();
        values.retain(|(existing, _)| existing.as_ref() != key);
        let retained_bytes = values.iter().fold(0_usize, |total, (key, value)| {
            total.saturating_add(key.len()).saturating_add(value.len())
        });
        let total = retained_bytes
            .checked_add(key.len())
            .and_then(|total| total.checked_add(value.len()))
            .unwrap_or(usize::MAX);
        if total > MAX_ENVIRONMENT_BYTES {
            return Err(Error::Limit {
                resource: "Cargo environment bytes",
                limit: MAX_ENVIRONMENT_BYTES,
                actual: total,
            });
        }
        if values.len().saturating_add(1) > MAX_ENVIRONMENT_ENTRIES {
            return Err(Error::Limit {
                resource: "Cargo environment entries",
                limit: MAX_ENVIRONMENT_ENTRIES,
                actual: values.len().saturating_add(1),
            });
        }
        values.push((key.into(), value.into()));
        values.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        Ok(Self {
            values: values.into_boxed_slice(),
        })
    }

    /// Remove one entry from the sealed environment, preserving exact absence.
    pub fn removing(self, key: &str) -> Result<Self> {
        if key.is_empty() || key.contains('=') || key.contains('\0') {
            return Err(Error::Request {
                reason: format!("invalid environment variable name `{key}`").into_boxed_str(),
            });
        }
        let mut values = self.values.into_vec();
        values.retain(|(existing, _)| existing.as_ref() != key);
        Ok(Self {
            values: values.into_boxed_slice(),
        })
    }

    /// Construct an explicit empty environment.
    pub fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn command_values(
        &self,
        target_dir: &Path,
        rustc: &Path,
        rustdoc: &Path,
    ) -> Result<Box<[(OsString, OsString)]>> {
        self.command_values_with_limits(
            target_dir,
            rustc,
            rustdoc,
            MAX_ENVIRONMENT_ENTRIES,
            MAX_ENVIRONMENT_BYTES,
        )
    }

    fn command_values_with_limits(
        &self,
        target_dir: &Path,
        rustc: &Path,
        rustdoc: &Path,
        entry_limit: usize,
        byte_limit: usize,
    ) -> Result<Box<[(OsString, OsString)]>> {
        let loader = self.loader_path(rustc);
        // Cargo gets the home it would use, its symlinks resolved, so the
        // registry paths it reports are the canonical ones rawr reads: a
        // home reached through a symlink is the directory it names (#1754).
        let home = self.context_cargo_home().ok().flatten();
        let retained = self.values.iter().filter(|(key, _)| {
            !IMPORTER_OVERRIDDEN.contains(&key.as_ref())
                && !(loader.is_some() && key.as_ref() == LOADER_PATH)
                && !(home.is_some() && key.as_ref() == "CARGO_HOME")
        });
        let retained_items = retained.clone().count();
        let items = retained_items
            .saturating_add(4)
            .saturating_add(usize::from(loader.is_some()))
            .saturating_add(usize::from(home.is_some()));
        if items > entry_limit {
            return Err(Error::Limit {
                resource: "Cargo environment entries",
                limit: entry_limit,
                actual: items,
            });
        }
        let bytes = retained
            .clone()
            .fold(target_dir.as_os_str().len(), |total, (key, value)| {
                total.saturating_add(key.len()).saturating_add(value.len())
            })
            .saturating_add("CARGO_TARGET_DIR".len())
            .saturating_add("CARGO_INCREMENTAL".len())
            .saturating_add(1)
            .saturating_add("RUSTC".len())
            .saturating_add(rustc.as_os_str().len())
            .saturating_add("RUSTDOC".len())
            .saturating_add(rustdoc.as_os_str().len())
            .saturating_add(
                loader
                    .as_ref()
                    .map_or(0, |value| LOADER_PATH.len().saturating_add(value.len())),
            )
            .saturating_add(home.as_ref().map_or(0, |home| {
                "CARGO_HOME".len().saturating_add(home.as_os_str().len())
            }));
        if bytes > byte_limit {
            return Err(Error::Limit {
                resource: "Cargo environment bytes",
                limit: byte_limit,
                actual: bytes,
            });
        }
        let mut values = Vec::with_capacity(items);
        for (key, value) in retained {
            values.push((OsString::from(key.as_ref()), OsString::from(value.as_ref())));
        }
        values.push(("CARGO_TARGET_DIR".into(), target_dir.as_os_str().to_owned()));
        // Incremental, as `cargo check` is: Cargo gives it to path packages
        // alone, so an edit to the project's crate checks what changed, not
        // the whole crate again (#1772). Registry crates build as before.
        values.push(("CARGO_INCREMENTAL".into(), "1".into()));
        values.push(("RUSTC".into(), rustc.as_os_str().to_owned()));
        values.push(("RUSTDOC".into(), rustdoc.as_os_str().to_owned()));
        if let Some(loader) = loader {
            values.push((LOADER_PATH.into(), loader));
        }
        if let Some(home) = home {
            values.push(("CARGO_HOME".into(), home.into_os_string()));
        }
        values.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        Ok(values.into_boxed_slice())
    }

    /// The `LD_LIBRARY_PATH` a rustup proxy would give the toolchain's Cargo:
    /// the toolchain's `lib` ahead of the caller's value. The importer runs
    /// that Cargo directly, without the proxy. Cargo puts the sysroot's
    /// `lib/rustlib/<host>/lib` first on this path when it runs a build
    /// script. With rustc-dev installed, that directory holds a second
    /// `librustc_driver`, and that copy's `$ORIGIN/../lib` RUNPATH has no
    /// libLLVM. A build script that runs `$RUSTC` (libc's does) loads that
    /// copy, because `LD_LIBRARY_PATH` comes before RUNPATH. It then fails to
    /// load libLLVM unless the toolchain's own `lib` is also on the path.
    /// macOS needs nothing. Cargo sets `DYLD_FALLBACK_LIBRARY_PATH` there, and
    /// the loader consults it only after rustc's `@rpath`.
    pub(crate) fn loader_path(&self, rustc: &Path) -> Option<OsString> {
        if cfg!(target_vendor = "apple") {
            return None;
        }
        let mut value = rustc.parent()?.parent()?.join("lib").into_os_string();
        if let Some(caller) = self.value(LOADER_PATH).filter(|caller| !caller.is_empty()) {
            value.push(":");
            value.push(caller);
        }
        Some(value)
    }

    /// One captured variable.
    pub(crate) fn value_of(&self, key: &str) -> Option<&str> {
        self.value(key)
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find_map(|(name, value)| (name.as_ref() == key).then_some(value.as_ref()))
    }

    pub(crate) fn entries(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_ref(), value.as_ref()))
    }

    pub(crate) fn cargo_home(&self) -> Result<Option<PathBuf>> {
        let path = if let Some(cargo_home) = self.value("CARGO_HOME") {
            Some(PathBuf::from(cargo_home))
        } else {
            self.value("HOME")
                .map(|home| PathBuf::from(home).join(".cargo"))
        };
        path.map(|path| normalized_absolute("Cargo home", &path).map(resolved))
            .transpose()
    }

    pub(crate) fn context_cargo_home(&self) -> Result<Option<PathBuf>> {
        let (source, path) = if let Some(cargo_home) = self.value("CARGO_HOME") {
            ("CARGO_HOME", PathBuf::from(cargo_home))
        } else if let Some(home) = self.value("HOME") {
            ("HOME", PathBuf::from(home))
        } else {
            return Ok(None);
        };
        if path.as_os_str().is_empty() {
            return Err(Error::Request {
                reason: format!("the environment variable {source} is set but empty")
                    .into_boxed_str(),
            });
        }
        if !path.is_absolute() {
            return Err(Error::Request {
                reason: format!(
                    "the environment variable {source} is `{}`, which is not an absolute path",
                    path.display()
                )
                .into_boxed_str(),
            });
        }
        let path = normalized_absolute("Cargo home", &path)?;
        Ok(Some(resolved(if source == "HOME" {
            path.join(".cargo")
        } else {
            path
        })))
    }
}

/// The directory a Cargo home names, its symlinks resolved as Cargo follows
/// them. One that doesn't exist yet keeps its spelling.
fn resolved(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn insert_environment(
    values: &mut BTreeMap<String, String>,
    total: &mut usize,
    key: &str,
    value: &str,
) -> Result<()> {
    let next_total = validate_environment_entry(values, *total, key, value)?;
    let _ = values.insert(key.to_owned(), value.to_owned());
    *total = next_total;
    Ok(())
}

fn insert_owned_environment(
    values: &mut BTreeMap<String, String>,
    total: &mut usize,
    key: String,
    value: String,
) -> Result<()> {
    let next_total = validate_environment_entry(values, *total, &key, &value)?;
    let _ = values.insert(key, value);
    *total = next_total;
    Ok(())
}

fn validate_environment_entry(
    values: &BTreeMap<String, String>,
    total: usize,
    key: &str,
    value: &str,
) -> Result<usize> {
    if key.is_empty() || key.contains('=') || key.contains('\0') || value.contains('\0') {
        return Err(Error::Request {
            reason: format!("invalid environment variable `{key}`").into_boxed_str(),
        });
    }
    if key.len() > MAX_VALUE_BYTES || value.len() > MAX_VALUE_BYTES {
        return Err(Error::Limit {
            resource: "Cargo environment value bytes",
            limit: MAX_VALUE_BYTES,
            actual: key.len().max(value.len()),
        });
    }
    if WRAPPER_ENVIRONMENT.contains(&key)
        || key.starts_with(INTERNAL_ENVIRONMENT_PREFIX)
        || key.starts_with(UNSTABLE_ENVIRONMENT_PREFIX)
    {
        return Err(Error::Request {
            reason: format!(
                "the environment variable `{key}` is not supported yet; unset it and run again"
            )
            .into_boxed_str(),
        });
    }
    if values.contains_key(key) {
        return Err(Error::Request {
            reason: format!("the environment variable `{key}` is set twice").into_boxed_str(),
        });
    }
    let entries = values.len().saturating_add(1);
    if entries > MAX_ENVIRONMENT_ENTRIES {
        return Err(Error::Limit {
            resource: "Cargo environment entries",
            limit: MAX_ENVIRONMENT_ENTRIES,
            actual: entries,
        });
    }
    let next_total = total
        .checked_add(key.len())
        .and_then(|length| length.checked_add(value.len()))
        .unwrap_or(usize::MAX);
    if next_total > MAX_ENVIRONMENT_BYTES {
        return Err(Error::Limit {
            resource: "Cargo environment bytes",
            limit: MAX_ENVIRONMENT_BYTES,
            actual: next_total,
        });
    }
    Ok(next_total)
}

fn environment_values(values: BTreeMap<String, String>) -> Result<Box<[EnvironmentValue]>> {
    Ok(values
        .into_iter()
        .map(|(key, value)| (key.into_boxed_str(), value.into_boxed_str()))
        .collect())
}

impl fmt::Debug for Environment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Keys<'environment>(&'environment [EnvironmentValue]);
        impl fmt::Debug for Keys<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_list()
                    .entries(self.0.iter().map(|(key, _)| key.as_ref()))
                    .finish()
            }
        }
        formatter
            .debug_struct("Environment")
            .field("keys", &Keys(&self.values))
            .finish()
    }
}

impl Selected {
    pub(crate) fn alias(&self) -> &str {
        &self.alias
    }

    pub(crate) fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    pub(crate) fn source(&self) -> &Source {
        &self.source
    }

    pub(crate) fn features(&self) -> &[Box<str>] {
        &self.features
    }

    pub(crate) const fn default_features(&self) -> bool {
        self.default_features
    }

    pub(crate) const fn roles(&self) -> Roles {
        self.roles
    }

    pub(crate) fn shares_cargo_unit(&self, other: &Self) -> bool {
        // The package each names: its `package`, else its alias. Two
        // dependencies with no `package` key are two packages.
        self.package().unwrap_or(self.alias()) == other.package().unwrap_or(other.alias())
            && self.features == other.features
            && self.default_features == other.default_features
            && match (&self.source, &other.source) {
                (Source::Path(left), Source::Path(right)) => left == right,
                (
                    Source::Registry {
                        requirement: left, ..
                    },
                    Source::Registry {
                        requirement: right, ..
                    },
                ) => left == right,
                (
                    Source::Git {
                        url: left_url,
                        reference: left_reference,
                        version: left_version,
                    },
                    Source::Git {
                        url: right_url,
                        reference: right_reference,
                        version: right_version,
                    },
                ) => {
                    left_url == right_url
                        && left_reference == right_reference
                        && left_version == right_version
                }
                (Source::Path(_), Source::Registry { .. } | Source::Git { .. })
                | (Source::Registry { .. }, Source::Path(_) | Source::Git { .. })
                | (Source::Git { .. }, Source::Path(_) | Source::Registry { .. }) => false,
            }
    }
}

impl Provenance {
    /// Cargo's project compiler selection (D642): RUSTC, then the config
    /// environment variable, then the nearest hierarchical value. Program
    /// paths containing a separator resolve relative to their definition;
    /// bare program names are left for the sealed PATH.
    pub(crate) fn compiler(&self, project: &Path) -> Compiler {
        let configured = self
            .environment
            .value("RUSTC")
            .or_else(|| self.environment.value("CARGO_BUILD_RUSTC"))
            .map(|value| compiler_path(project, value))
            .or_else(|| self.compiler.clone());
        Compiler { configured }
    }

    pub(crate) fn environment(&self) -> &Environment {
        &self.environment
    }

    /// The target triple Cargo builds the project for.
    pub(crate) fn target(&self) -> &str {
        self.projection.target()
    }

    /// Borrow the semantic values sealed by this exact configuration discovery.
    pub(crate) fn projection(&self) -> &projection::Projection {
        &self.projection
    }

    /// Borrow the `[env]` table Cargo gives the processes it runs.
    pub(crate) fn env(&self) -> &env::Table {
        &self.env
    }

    /// Borrow config candidates in their original discovery order.
    pub(crate) fn input_candidates(
        &self,
    ) -> impl ExactSizeIterator<Item = (&Path, Option<&[u8; 32]>)> {
        self.candidates.iter().map(|candidate| {
            let digest = match &candidate.state {
                State::Absent => None,
                State::Present(digest) => Some(digest.as_bytes()),
            };
            (candidate.path.as_path(), digest)
        })
    }

    /// Discover the config Cargo reads for `project_root`. `target` is a
    /// target the caller selects; without one, the target is Cargo's own:
    /// `CARGO_BUILD_TARGET`, then the nearest `build.target`, then the host.
    pub(crate) fn discover(
        project_root: &Path,
        authority_manifest: Option<&Path>,
        environment: Environment,
        target: Option<&str>,
        profile: &str,
        parallelism: crate::process::parallelism::Proof,
        limits: &Limits,
    ) -> Result<Self> {
        let mut candidates = Vec::new();
        let project_end = append_hierarchy_candidates(&mut candidates, project_root)?;
        let authority_end = if let Some(authority_root) = authority_manifest.and_then(Path::parent)
        {
            append_hierarchy_candidates(&mut candidates, authority_root)?
        } else {
            project_end
        };
        let (project, authority) = candidates.split_at(project_end);
        let authority = &authority[..authority_end.saturating_sub(project_end)];
        let project_has_config = project
            .iter()
            .any(|candidate| matches!(candidate.state, State::Present(_)));
        let authority_extra_count = authority
            .iter()
            .filter(|candidate| {
                matches!(candidate.state, State::Present(_))
                    && !project.iter().any(|project| project.path == candidate.path)
            })
            .count();
        if project_has_config && authority_extra_count != 0 {
            return Err(Error::Request {
                reason: format!(
                    "Cargo reads different `.cargo/config.toml` files for `{}` than for the \
                     project directory `{}`",
                    authority_manifest
                        .map_or(project_root, |path| path)
                        .display(),
                    project_root.display(),
                )
                .into_boxed_str(),
            });
        }
        let explicit = if !project_has_config {
            let mut explicit = Vec::with_capacity(authority_extra_count);
            for candidate in authority.iter().filter(|candidate| {
                matches!(candidate.state, State::Present(_))
                    && !project.iter().any(|project| project.path == candidate.path)
            }) {
                explicit.push(candidate.path.clone());
            }
            explicit
        } else {
            Vec::new()
        };

        if let Some(cargo_home) = environment.cargo_home()? {
            let home_start = candidates.len();
            append_pair_candidates(&mut candidates, &cargo_home)?;
            prefer_extensionless(&mut candidates, home_start);
        }
        candidates.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        candidates.dedup_by(|left, right| left.path == right.path);

        let mut compiler = None::<(usize, PathBuf)>;
        let mut configured_target = None::<(usize, Box<str>)>;
        let cargo_home = environment.cargo_home()?;
        let mut env_sources = Vec::new();
        for candidate in &candidates {
            if matches!(candidate.state, State::Present(_)) {
                let bytes = read_config(&candidate.path, crate::process::TAIL_BYTES)?;
                // A byte is a conservative upper bound for every retained TOML
                // node and byte.  Prove the complete owned DOM before parsing.
                let mut document = parse_config(&candidate.path, &bytes)?;
                validate_config_authority(&document, &candidate.path)?;
                if let Some(item) = document.remove("env")
                    && let Some(rank) = env_rank(
                        &candidate.path,
                        project_root,
                        &explicit,
                        cargo_home.as_deref(),
                    )
                {
                    env_sources.push(env::Source {
                        rank,
                        path: &candidate.path,
                        item,
                    });
                }
                let directory = candidate
                    .path
                    .parent()
                    .expect("a Cargo config file has a directory");
                let base = directory
                    .parent()
                    .expect("a Cargo config file's directory has a parent");
                let priority = if directory.file_name() == Some(std::ffi::OsStr::new(".cargo"))
                    && project_root.starts_with(base)
                {
                    Some(base.components().count() + 1)
                } else if cargo_home.as_deref() == Some(directory) {
                    Some(0)
                } else {
                    None
                };
                let build = document
                    .get("build")
                    .and_then(toml_edit::Item::as_table_like);
                if let Some(value) = build.and_then(|build| build.get("rustc")) {
                    let value = value.as_str().ok_or_else(|| Error::Request {
                        reason: format!(
                            "invalid configuration for key `build.rustc`: expected a string \
                             in `{}`",
                            candidate.path.display()
                        )
                        .into_boxed_str(),
                    })?;
                    if let Some(priority) = priority
                        && compiler.as_ref().is_none_or(|(kept, _)| priority > *kept)
                    {
                        let path = compiler_path(base, value);
                        compiler = Some((priority, path));
                    }
                }
                if let Some(value) = build.and_then(|build| build.get("target"))
                    && let Some(priority) = priority
                    && configured_target
                        .as_ref()
                        .is_none_or(|(kept, _)| priority > *kept)
                {
                    configured_target = Some((priority, build_target(value, &candidate.path)?));
                }
            }
        }

        let env = env::Table::merge(&mut env_sources)?;
        drop(env_sources);

        // Cargo reads `host-tuple` as the host's own triple.
        let target = match target
            .or_else(|| environment.value("CARGO_BUILD_TARGET"))
            .or(configured_target
                .as_ref()
                .map(|(_, target)| target.as_ref()))
        {
            None | Some("host-tuple") => crate::cargo::toolchain::HOST,
            Some(target) => target,
        };
        let projection = projection::Projection::derive_governed(
            &environment,
            target,
            profile,
            parallelism,
            limits,
        )?;

        let digest_bytes = candidates.iter().fold(0_usize, |total, candidate| {
            total
                .saturating_add(8)
                .saturating_add(candidate.path.as_os_str().len())
                .saturating_add(8)
                .saturating_add(match candidate.state {
                    State::Absent => b"absent".len(),
                    State::Present(_) => b"present".len(),
                })
                .saturating_add(if matches!(candidate.state, State::Present(_)) {
                    8 + 32
                } else {
                    0
                })
        });
        let mut digest_input = Vec::with_capacity(digest_bytes);
        for candidate in &candidates {
            append_framed(
                &mut digest_input,
                candidate.path.as_os_str().as_encoded_bytes(),
            );
            match &candidate.state {
                State::Absent => append_framed(&mut digest_input, b"absent"),
                State::Present(file_digest) => {
                    append_framed(&mut digest_input, b"present");
                    append_framed(&mut digest_input, file_digest.as_bytes());
                }
            }
        }
        #[cfg(test)]
        OWNED_PROVENANCE.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self {
            candidates: candidates.into_boxed_slice(),
            explicit: explicit.into_boxed_slice(),
            environment,
            projection,
            env,
            no_build_std: NoBuildStd { private: () },
            digest: digest(&digest_input),
            compiler: compiler.map(|(_, path)| path),
        })
    }

    pub(crate) fn revalidate(&self, limits: &Limits, command: &'static str) -> Result<()> {
        self.projection
            .revalidate(&self.environment, command, limits)?;
        for candidate in &self.candidates {
            let current = candidate_state(&candidate.path, crate::process::TAIL_BYTES)?;
            let equal = match (&candidate.state, current) {
                (State::Absent, State::Absent) => true,
                (State::Present(expected), State::Present(actual)) => *expected == actual,
                (State::Absent, State::Present(_)) | (State::Present(_), State::Absent) => false,
            };
            if !equal {
                return Err(Error::ResolutionDrift {
                    command,
                    field: "Cargo config provenance",
                });
            }
        }
        Ok(())
    }

    pub(crate) fn command_environment(
        &self,
        target_dir: &Path,
        native: &impl ToolPaths,
    ) -> Result<Box<[(OsString, OsString)]>> {
        let (rustc, rustdoc) = native.cargo_tools();
        self.environment.command_values(target_dir, rustc, rustdoc)
    }

    /// [`Self::command_environment`] for the check whose units extraction
    /// reads, with rawr's own driver as the rustc wrapper when generated
    /// crates are `recorded` (D866): it records their native instances of
    /// marked generics and runs rustc itself for every other unit, so no
    /// other unit's fingerprint or output changes. The user's wrappers are
    /// refused ([`validate_config_authority`]), so it is the only one.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn recording_environment(
        &self,
        target_dir: &Path,
        native: &impl ToolPaths,
        recorded: &[PathBuf],
    ) -> Result<Box<[(OsString, OsString)]>> {
        let environment = self.command_environment(target_dir, native)?;
        if recorded.is_empty() {
            return Ok(environment);
        }
        let roots = recorded
            .iter()
            .map(|root| fs::canonicalize(root).unwrap_or_else(|_| root.clone()))
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let mut environment = environment.into_vec();
        environment.push((
            OsString::from("RUSTC_WRAPPER"),
            native.driver().as_os_str().to_owned(),
        ));
        environment.push((OsString::from(crate::record::ROOTS), OsString::from(roots)));
        Ok(environment.into_boxed_slice())
    }

    pub(crate) fn explicit_configs(&self) -> &[PathBuf] {
        &self.explicit
    }

    pub(crate) fn cargo_home(&self) -> Result<Option<PathBuf>> {
        self.environment.cargo_home()
    }

    pub(crate) const fn digest(&self) -> Digest {
        self.digest
    }

    pub(crate) const fn no_build_std(&self) -> NoBuildStd {
        self.no_build_std
    }

    pub(crate) const fn projection_schema(&self) -> u32 {
        projection::schema()
    }

    pub(crate) fn universe_context(&self) -> Digest {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.cargo.config-context/v1");
        digest.update(self.projection.universe_digest().as_bytes());
        let present = self
            .candidates
            .iter()
            .filter_map(|candidate| match candidate.state {
                State::Absent => None,
                State::Present(digest) => Some(digest),
            })
            .collect::<Vec<_>>();
        digest.update((present.len() as u64).to_le_bytes());
        for value in present {
            digest.update(value.as_bytes());
        }
        Digest(digest.finalize().into())
    }

    /// The baseline configuration a normal build retains.
    pub(crate) fn input_config(&self) -> Result<crate::input::config::Config> {
        let mut candidates = Vec::new();
        candidates
            .try_reserve_exact(self.candidates.len())
            .map_err(|source| Error::Allocation {
                operation: "baseline Cargo config candidates",
                source,
            })?;
        #[cfg(test)]
        OWNED_INPUT_CONFIG_CANDIDATES.with(|owned| owned.set(owned.get() + 1));
        for candidate in &self.candidates {
            let digest = match &candidate.state {
                State::Absent => None,
                State::Present(digest) => Some(*digest.as_bytes()),
            };
            candidates.push((candidate.path.clone(), digest));
        }
        let projection = self.projection.try_clone()?;
        Ok(crate::input::config::Config {
            candidates: candidates.into_boxed_slice(),
            env: self.env.clone(),
            provenance: *self.digest.as_bytes(),
            projection,
        })
    }
}

/// Where Cargo merges `path`'s `[env]`, or `None` when Cargo doesn't read the
/// file for the project. A `--config` file outranks every discovered file.
fn env_rank(
    path: &Path,
    project_root: &Path,
    explicit: &[PathBuf],
    cargo_home: Option<&Path>,
) -> Option<env::Rank> {
    if let Some(index) = explicit.iter().position(|explicit| explicit == path) {
        return Some(env::Rank::Cli(index));
    }
    let directory = path.parent()?;
    let base = directory.parent()?;
    if directory.file_name() == Some(std::ffi::OsStr::new(".cargo"))
        && project_root.starts_with(base)
    {
        Some(env::Rank::Hierarchy(base.components().count()))
    } else if cargo_home == Some(directory) {
        Some(env::Rank::Home)
    } else {
        None
    }
}

/// A config file's `build.target`: one target triple. Cargo also takes a
/// list of them and builds for each; rawr builds a program for one.
fn build_target(value: &toml_edit::Item, path: &Path) -> Result<Box<str>> {
    let invalid = || Error::Request {
        reason: format!(
            "invalid configuration for key `build.target`: expected a string in `{}`",
            path.display()
        )
        .into_boxed_str(),
    };
    if let Some(target) = value.as_str() {
        return Ok(target.into());
    }
    let list = value.as_array().ok_or_else(invalid)?;
    match list.iter().collect::<Vec<_>>().as_slice() {
        [target] => target.as_str().map(Box::from).ok_or_else(invalid),
        _ => Err(Error::Request {
            reason: format!(
                "rawr builds a program for one target, and `{}` sets `build.target` to {} targets",
                path.display(),
                list.len()
            )
            .into_boxed_str(),
        }),
    }
}

fn compiler_path(base: &Path, value: &str) -> PathBuf {
    if value.contains('/') || value.contains('\\') {
        base.join(value)
    } else {
        PathBuf::from(value)
    }
}

fn validate_config_authority(document: &DocumentMut, path: &Path) -> Result<()> {
    let build = document
        .get("build")
        .and_then(toml_edit::Item::as_table_like);
    let configured_build = build.is_some_and(|table| {
        table.contains_key("rustc-wrapper") || table.contains_key("rustc-workspace-wrapper")
    });
    let configured_environment = document
        .get("env")
        .and_then(toml_edit::Item::as_table_like)
        .is_some_and(|table| {
            WRAPPER_ENVIRONMENT
                .iter()
                .any(|name| table.contains_key(name))
        });
    if configured_build || configured_environment {
        return Err(Error::Request {
            reason: format!(
                "rawr can't build Rust with a `rustc` wrapper, and `{}` sets one",
                path.display()
            )
            .into_boxed_str(),
        });
    }
    let unstable = document
        .get("unstable")
        .and_then(toml_edit::Item::as_table_like);
    let build_std = unstable.is_some_and(|table| {
        table.contains_key("build-std") || table.contains_key("build-std-features")
    });
    if build_std {
        return Err(Error::Request {
            reason: format!(
                "rawr can't build Rust with `unstable.build-std`, and `{}` sets it",
                path.display()
            )
            .into_boxed_str(),
        });
    }
    if unstable.is_some_and(|table| table.contains_key("advanced-env")) {
        return Err(Error::Request {
            reason: format!(
                "rawr can't build Rust with `unstable.advanced-env`, and `{}` sets it",
                path.display()
            )
            .into_boxed_str(),
        });
    }
    Ok(())
}

fn append_hierarchy_candidates(candidates: &mut Vec<Candidate>, start: &Path) -> Result<usize> {
    let count = start.ancestors().count().saturating_mul(2);
    candidates.reserve_exact(count);
    for directory in start.ancestors() {
        let pair_start = candidates.len();
        append_candidate(candidates, directory, ".cargo/config.toml")?;
        append_candidate(candidates, directory, ".cargo/config")?;
        prefer_extensionless(candidates, pair_start);
    }
    Ok(candidates.len())
}

fn append_pair_candidates(candidates: &mut Vec<Candidate>, directory: &Path) -> Result<()> {
    append_candidate(candidates, directory, "config.toml")?;
    append_candidate(candidates, directory, "config")
}

fn append_candidate(
    candidates: &mut Vec<Candidate>,
    directory: &Path,
    relative: &str,
) -> Result<()> {
    let path = directory.join(relative);
    let state = candidate_state(&path, MAX_VALUE_BYTES)?;
    candidates.push(Candidate { path, state });
    Ok(())
}

/// One Cargo config file's TOML, failing as Cargo words it.
fn parse_config(path: &Path, bytes: &[u8]) -> Result<DocumentMut> {
    let unparsable = |error: &dyn fmt::Display| Error::Request {
        reason: format!(
            "could not parse TOML configuration in `{}`: {error}",
            path.display()
        )
        .into_boxed_str(),
    };
    std::str::from_utf8(bytes)
        .map_err(|error| unparsable(&error))?
        .parse::<DocumentMut>()
        .map_err(|error| unparsable(&error))
}

/// When one directory has both `config.toml` and `config` (the pair pushed
/// from `start`), Cargo warns and reads only `config`. Drop `config.toml` so
/// rawr reads what Cargo reads; `config`'s own state still notices if it
/// disappears and `config.toml` takes over.
fn prefer_extensionless(candidates: &mut Vec<Candidate>, start: usize) {
    let pair = &candidates[start..];
    debug_assert!(pair.len() == 2 && pair[0].path.extension().is_some());
    if pair
        .iter()
        .all(|candidate| matches!(&candidate.state, State::Present(_)))
    {
        candidates.remove(start);
    }
}

fn candidate_state(path: &Path, limit: usize) -> Result<State> {
    // Cargo follows a symlinked `.cargo` directory or config file. Each check
    // reads through the link as it is now, so a changed link or target is seen.
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            let bytes = read_config(path, limit)?;
            Ok(State::Present(digest(&bytes)))
        }
        Ok(_) => Err(Error::Path {
            field: "Cargo config candidate",
            path: path.to_path_buf(),
            reason: "is not a file".into(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::Absent),
        Err(source) => Err(Error::Io {
            operation: "inspect Cargo config candidate",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_config(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut file = File::open(path).map_err(|source| Error::Io {
        operation: "open Cargo config",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| Error::Io {
        operation: "inspect Cargo config handle",
        path: path.to_path_buf(),
        source,
    })?;
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > limit {
        return Err(Error::Limit {
            resource: "Cargo config bytes",
            limit,
            actual: length,
        });
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: "Cargo config bytes",
            source,
        })?;
    bytes.resize(length, 0);
    if let Err(source) = file.read_exact(&mut bytes) {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(Error::ResolutionDrift {
                command: "Cargo config read",
                field: "Cargo config provenance",
            });
        }
        return Err(Error::Io {
            operation: "read Cargo config",
            path: path.to_path_buf(),
            source,
        });
    }
    let mut extra = [0_u8; 1];
    let extra = file.read(&mut extra).map_err(|source| Error::Io {
        operation: "probe Cargo config growth",
        path: path.to_path_buf(),
        source,
    })?;
    if extra != 0 {
        return Err(Error::ResolutionDrift {
            command: "Cargo config read",
            field: "Cargo config provenance",
        });
    }
    Ok(bytes)
}

fn append_framed(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn validated_alias(alias: &str) -> Result<Box<str>> {
    let mut characters = alias.chars();
    let valid = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if valid {
        Ok(alias.into())
    } else {
        Err(Error::Request {
            reason: format!("invalid dependency name `{alias}`: a crate name is a Rust identifier")
                .into_boxed_str(),
        })
    }
}

fn validated_library_alias(alias: &str) -> Result<Box<str>> {
    if alias == INTERNAL_ALIAS {
        return Err(Error::Request {
            reason: format!("the dependency name `{alias}` is reserved by rawr").into_boxed_str(),
        });
    }
    validated_alias(alias)
}

fn valid_package_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::Path;

    use super::projection::Projection;
    use super::{
        validate_config_authority, Candidate, Dependency, Environment, NoBuildStd, Provenance,
        State, LOADER_PATH, OWNED_INPUT_CONFIG_CANDIDATES, OWNED_PROVENANCE,
    };
    use crate::cargo::{digest, Limits};
    use crate::Error;
    use toml_edit::DocumentMut;

    /// An empty or relative CARGO_HOME is named as the variable it is (#483).
    #[test]
    fn an_unusable_cargo_home_names_the_variable() {
        let cases = [
            ("", "the environment variable CARGO_HOME is set but empty"),
            (
                "relative/home",
                "the environment variable CARGO_HOME is `relative/home`, which is not an \
                 absolute path",
            ),
        ];
        for (value, expected) in cases {
            let environment =
                Environment::from_pairs([("CARGO_HOME", value)]).expect("the environment seals");
            let error = environment
                .context_cargo_home()
                .expect_err("an unusable CARGO_HOME is refused");
            assert_eq!(error.to_string(), expected);
        }
    }

    fn input_config_provenance(path: &Path, state: State) -> Provenance {
        let limits = Limits::standard();
        let environment = Environment::from_pairs([("RUSTFLAGS", "-Copt-level=1")])
            .expect("fixture semantic environment seals");
        let projection = Projection::derive(&environment, "aarch64-apple-darwin", "dev", &limits)
            .expect("fixture projection derives");
        Provenance {
            candidates: Box::from([Candidate {
                path: path.to_path_buf(),
                state,
            }]),
            explicit: Box::default(),
            environment,
            projection,
            env: super::env::Table::fixture(&[("CONFIG_VALUE", "present", false)]),
            no_build_std: NoBuildStd { private: () },
            digest: digest(b"input config fixture"),
            compiler: None,
        }
    }

    #[test]
    fn input_candidates_borrow_original_order_paths_and_digest_storage() {
        let mut provenance = input_config_provenance(Path::new("/config/first"), State::Absent);
        provenance.candidates = Box::from([
            Candidate {
                path: "/config/first".into(),
                state: State::Absent,
            },
            Candidate {
                path: "/config/second".into(),
                state: State::Present(digest(b"second")),
            },
        ]);
        OWNED_INPUT_CONFIG_CANDIDATES.with(|owned| owned.set(0));
        let inputs = provenance.input_candidates();
        assert_eq!(inputs.len(), provenance.candidates.len());
        for ((path, value), candidate) in inputs.zip(&provenance.candidates) {
            assert!(std::ptr::eq(path, candidate.path.as_path()));
            match (&candidate.state, value) {
                (State::Absent, None) => {}
                (State::Present(expected), Some(actual)) => {
                    assert!(std::ptr::eq(actual, expected.as_bytes()));
                }
                _ => panic!("the exact candidate state is borrowed"),
            }
        }
        assert_eq!(OWNED_INPUT_CONFIG_CANDIDATES.with(Cell::get), 0);
    }

    #[test]
    fn input_config_copies_candidates_and_the_env_table() {
        let provenance = input_config_provenance(
            Path::new("/config/.cargo/config.toml"),
            State::Present(digest(b"present")),
        );
        let actual = provenance.input_config().expect("config is retained");
        assert_eq!(actual.provenance, *provenance.digest.as_bytes());
        assert_eq!(actual.env, provenance.env);
        assert_ne!(
            actual.candidates[0]
                .0
                .as_os_str()
                .as_encoded_bytes()
                .as_ptr(),
            provenance.candidates[0]
                .path
                .as_os_str()
                .as_encoded_bytes()
                .as_ptr()
        );
    }

    #[test]
    fn creating_previously_absent_config_invalidates_provenance() {
        let path = fs::canonicalize(std::env::temp_dir())
            .expect("the test temporary directory is canonical")
            .join(format!("rawr-absent-config-{}", std::process::id()));
        let provenance = Provenance {
            candidates: vec![Candidate {
                path: path.clone(),
                state: State::Absent,
            }]
            .into_boxed_slice(),
            explicit: Box::default(),
            environment: Environment::empty(),
            projection: Projection::derive(
                &Environment::empty(),
                "aarch64-apple-darwin",
                "dev",
                &Limits::standard(),
            )
            .expect("test projection derives"),
            env: super::env::Table::default(),
            no_build_std: NoBuildStd { private: () },
            digest: digest(b"test"),
            compiler: None,
        };
        fs::write(&path, b"[net]\noffline = true\n").expect("test config is created");
        let error = provenance
            .revalidate(&Limits::standard(), "test")
            .expect_err("absent-to-present config drift rejects");
        let _ = fs::remove_file(path);
        assert!(matches!(
            error,
            Error::ResolutionDrift {
                field: "Cargo config provenance",
                ..
            }
        ));
    }

    #[test]
    fn explicit_valid_cargo_home_survives_absent_root_hierarchy_candidates() {
        let fixture = std::env::temp_dir().join(format!(
            "rawr-cargo-home-provenance-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let project = fixture.join("project");
        let cargo_home = fixture.join("cargo-home");
        fs::create_dir_all(&project).expect("project root is created");
        fs::create_dir(&cargo_home).expect("Cargo home is created");
        let project = fs::canonicalize(project).expect("project root canonicalizes");
        let cargo_home = fs::canonicalize(cargo_home).expect("Cargo home canonicalizes");
        let cargo_home_text = cargo_home.to_str().expect("fixture path is Unicode");
        let environment = Environment::from_pairs([("CARGO_HOME", cargo_home_text)])
            .expect("explicit Cargo home seals");
        let parallelism =
            crate::process::parallelism::Proof::discover().expect("parallelism discovers");
        let provenance = Provenance::discover(
            &project,
            None,
            environment,
            Some("aarch64-apple-darwin"),
            "dev",
            parallelism,
            &Limits::standard(),
        )
        .expect("provenance retains absent candidates");

        assert_eq!(
            provenance.cargo_home().expect("Cargo home normalizes"),
            Some(cargo_home.clone())
        );
        assert!(provenance.candidates.iter().any(|candidate| {
            candidate.path == cargo_home.join("config.toml")
                && matches!(candidate.state, State::Absent)
        }));
        assert!(provenance.candidates.iter().any(|candidate| {
            candidate.path == Path::new("/.cargo/config.toml")
                && matches!(candidate.state, State::Absent)
        }));
        fs::remove_dir_all(fixture).expect("fixture is removed");
    }

    /// A canonical, empty fixture root that removes itself.
    struct Root(std::path::PathBuf);

    impl Root {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-config-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("fixture root is created");
            Self(fs::canonicalize(root).expect("fixture root canonicalizes"))
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn discover_in(project: &Path, cargo_home: &Path) -> crate::Result<Provenance> {
        let environment = Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home.to_str().expect("fixture path is Unicode"),
        )])
        .expect("Cargo home seals");
        Provenance::discover(
            project,
            None,
            environment,
            Some("aarch64-apple-darwin"),
            "dev",
            crate::process::parallelism::Proof::discover().expect("parallelism discovers"),
            &Limits::standard(),
        )
    }

    /// With both `config` and `config.toml` in one directory, Cargo warns
    /// and reads `config`. Rawr reads the same file and still notices when
    /// `config` goes away.
    #[test]
    fn config_beside_config_toml_reads_config_as_cargo_does() {
        let root = Root::new("pair");
        let project = root.0.join("project");
        let cargo_home = root.0.join("cargo-home");
        fs::create_dir_all(project.join(".cargo")).expect("project config directory");
        fs::create_dir_all(&cargo_home).expect("Cargo home");
        for directory in [project.join(".cargo"), cargo_home.clone()] {
            fs::write(directory.join("config.toml"), "[build]\nrustc='bin/toml'\n")
                .expect("config.toml writes");
            fs::write(directory.join("config"), "[build]\nrustc='bin/legacy'\n")
                .expect("config writes");
        }

        let provenance =
            discover_in(&project, &cargo_home).expect("Cargo accepts config beside config.toml");
        assert_eq!(
            provenance.compiler(&project).configured(),
            Some(project.join("bin/legacy").as_path())
        );
        for directory in [project.join(".cargo"), cargo_home.clone()] {
            assert!(provenance.candidates.iter().any(|candidate| {
                candidate.path == directory.join("config")
                    && matches!(candidate.state, State::Present(_))
            }));
            assert!(!provenance
                .candidates
                .iter()
                .any(|candidate| candidate.path == directory.join("config.toml")));
        }
        provenance
            .revalidate(&Limits::standard(), "test")
            .expect("unchanged configs stay fresh");

        fs::remove_file(project.join(".cargo/config")).expect("config is removed");
        assert!(matches!(
            provenance.revalidate(&Limits::standard(), "test"),
            Err(Error::ResolutionDrift {
                field: "Cargo config provenance",
                ..
            })
        ));
        let provenance =
            discover_in(&project, &cargo_home).expect("config.toml alone is read again");
        assert_eq!(
            provenance.compiler(&project).configured(),
            Some(project.join("bin/toml").as_path())
        );
    }

    /// Discovery merges `[env]` in Cargo's order: the nearest project file
    /// wins, Cargo home is last, and a relative value joins the directory
    /// above the `.cargo` that defines it.
    #[test]
    fn discovery_merges_env_tables_in_cargo_order() {
        let root = Root::new("env");
        let workspace = root.0.join("workspace");
        let project = workspace.join("project");
        let cargo_home = root.0.join("cargo-home");
        fs::create_dir_all(project.join(".cargo")).expect("project config directory");
        fs::create_dir_all(workspace.join(".cargo")).expect("workspace config directory");
        fs::create_dir_all(&cargo_home).expect("Cargo home");
        fs::write(
            project.join(".cargo/config.toml"),
            "[env]\nNEAR = 'project'\nTOOL = { value = 'bin/cc', relative = true }\n",
        )
        .expect("project config writes");
        fs::write(
            workspace.join(".cargo/config.toml"),
            "[env]\nNEAR = 'workspace'\nFAR = 'workspace'\n",
        )
        .expect("workspace config writes");
        fs::write(
            cargo_home.join("config.toml"),
            "[env]\nFAR = 'home'\nHOME_ONLY = 'home'\n",
        )
        .expect("home config writes");
        let provenance = discover_in(&project, &cargo_home).expect("[env] merges");
        let env = provenance.env();
        let tool = project.join("bin/cc");
        assert_eq!(env.value("NEAR", &[]), Some("project"));
        assert_eq!(env.value("FAR", &[]), Some("workspace"));
        assert_eq!(env.value("HOME_ONLY", &[]), Some("home"));
        assert_eq!(env.value("TOOL", &[]), tool.to_str());
        assert_eq!(
            provenance.input_config().expect("config is retained").env,
            *env
        );
    }

    /// Cargo follows a symlinked Cargo home; rawr reads the configs in the
    /// directory it names.
    #[cfg(unix)]
    #[test]
    fn symlinked_cargo_home_reads_the_directory_it_names() {
        let root = Root::new("home-link");
        let project = root.0.join("project");
        let cargo_home = root.0.join("cargo-home");
        let link = root.0.join("cargo-home-link");
        fs::create_dir_all(&project).expect("project");
        fs::create_dir_all(&cargo_home).expect("Cargo home");
        std::os::unix::fs::symlink(&cargo_home, &link).expect("Cargo home symlink");
        fs::write(cargo_home.join("config.toml"), "[net]\noffline = true\n")
            .expect("home config writes");

        let provenance =
            discover_in(&project, &link).expect("Cargo accepts a symlinked Cargo home");
        assert_eq!(
            provenance.cargo_home().expect("Cargo home resolves"),
            Some(cargo_home.clone())
        );
        assert!(provenance.candidates.iter().any(|candidate| {
            candidate.path == cargo_home.join("config.toml")
                && matches!(candidate.state, State::Present(_))
        }));
    }

    /// #520: Cargo follows a symlinked `.cargo` directory and a symlinked
    /// config file. rawr reads through both, and a changed target or a
    /// retargeted link makes the discovery stale.
    #[cfg(unix)]
    #[test]
    fn symlinked_cargo_directory_and_config_file_are_followed() {
        use std::os::unix::fs::symlink;

        let root = Root::new("dot-cargo-link");
        let project = root.0.join("project");
        let shared = root.0.join("shared");
        let cargo_home = root.0.join("cargo-home");
        fs::create_dir_all(project.join("member")).expect("project");
        fs::create_dir_all(&shared).expect("shared configs");
        fs::create_dir_all(&cargo_home).expect("Cargo home");
        fs::write(
            shared.join("config.toml"),
            "[build]\nrustc = \"bin/first\"\n",
        )
        .expect("shared config writes");
        fs::write(
            shared.join("other.toml"),
            "[build]\nrustc = \"bin/other\"\n",
        )
        .expect("other config writes");
        // The project's `.cargo` is a symlinked directory.
        symlink(&shared, project.join(".cargo")).expect(".cargo symlink");
        // The member's `.cargo/config.toml` is a symlinked file.
        fs::create_dir_all(project.join("member/.cargo")).expect("member .cargo");
        let member_config = project.join("member/.cargo/config.toml");
        symlink(shared.join("other.toml"), &member_config).expect("config symlink");

        let provenance =
            discover_in(&project, &cargo_home).expect("Cargo accepts a symlinked .cargo directory");
        // Config-relative paths stay relative to the directory Cargo found.
        assert_eq!(
            provenance.compiler(&project).configured(),
            Some(project.join("bin/first").as_path())
        );
        assert!(provenance.candidates.iter().any(|candidate| {
            candidate.path == project.join(".cargo/config.toml")
                && matches!(candidate.state, State::Present(_))
        }));
        provenance
            .revalidate(&Limits::standard(), "test")
            .expect("unchanged links stay fresh");
        let member = project.join("member");
        discover_in(&member, &cargo_home)
            .expect("Cargo accepts a symlinked config file")
            .revalidate(&Limits::standard(), "test")
            .expect("unchanged links stay fresh");

        let stale = |provenance: &Provenance| {
            matches!(
                provenance.revalidate(&Limits::standard(), "test"),
                Err(Error::ResolutionDrift {
                    field: "Cargo config provenance",
                    ..
                })
            )
        };
        // An edit to the target is seen through the link.
        fs::write(
            shared.join("config.toml"),
            "[build]\nrustc = \"bin/second\"\n",
        )
        .expect("shared config rewrites");
        assert!(stale(&provenance));

        // Retargeting the file link is seen too.
        let provenance = discover_in(&member, &cargo_home).expect("member discovers");
        fs::remove_file(&member_config).expect("config link is removed");
        symlink(shared.join("config.toml"), &member_config).expect("config link retargets");
        assert!(stale(&provenance));

        // A dangling link reads as absent, as Cargo reads it.
        let provenance = discover_in(&member, &cargo_home).expect("member rediscovers");
        fs::remove_file(&member_config).expect("config link is removed");
        symlink(shared.join("missing.toml"), &member_config).expect("dangling link");
        assert!(stale(&provenance));
        discover_in(&member, &cargo_home).expect("a dangling config link is absent");
    }

    #[test]
    fn config_digest_is_independent_of_semantic_environment_values() {
        let path = std::env::temp_dir().join(format!(
            "rawr-config-digest-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&path).expect("test config root is created");
        let path = fs::canonicalize(&path).expect("test config root is canonical");
        let limits = Limits::standard();
        let parallelism =
            crate::process::parallelism::Proof::discover().expect("test parallelism discovers");
        let baseline = Provenance::discover(
            &path,
            None,
            Environment::empty(),
            Some("aarch64-apple-darwin"),
            "dev",
            parallelism,
            &limits,
        )
        .expect("baseline provenance discovers");
        let semantic = Provenance::discover(
            &path,
            None,
            Environment::from_pairs([("RUSTFLAGS", "-Ctarget-cpu=native")])
                .expect("semantic environment seals"),
            Some("aarch64-apple-darwin"),
            "dev",
            parallelism,
            &limits,
        )
        .expect("semantic provenance discovers");
        fs::remove_dir(path).expect("test config root is removed");
        assert_eq!(baseline.digest, semantic.digest);
        assert_ne!(baseline.projection, semantic.projection);
    }

    /// The target is Cargo's: a target the caller selects, then
    /// `CARGO_BUILD_TARGET`, then the nearest `build.target`, whose
    /// `host-tuple` is the host, then the host.
    #[test]
    fn the_target_is_the_one_cargo_builds_for() {
        let root = std::env::temp_dir().join(format!(
            "rawr-config-target-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let project = root.join("outer/project");
        fs::create_dir_all(project.join(".cargo")).expect("project config directory");
        fs::create_dir_all(root.join("outer/.cargo")).expect("outer config directory");
        let project = fs::canonicalize(&project).expect("project is canonical");
        let outer = project
            .parent()
            .expect("outer directory")
            .join(".cargo/config.toml");
        let own = project.join(".cargo/config.toml");
        let limits = Limits::standard();
        let parallelism =
            crate::process::parallelism::Proof::discover().expect("test parallelism discovers");
        let target = |environment: Environment, selected: Option<&str>| {
            Provenance::discover(
                &project,
                None,
                environment,
                selected,
                "dev",
                parallelism,
                &limits,
            )
            .map(|provenance| provenance.target().to_owned())
        };
        let host = crate::cargo::toolchain::HOST;
        assert_eq!(target(Environment::empty(), None).unwrap(), host);
        fs::write(&outer, "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n").unwrap();
        fs::write(&own, "[build]\ntarget = \"wasm32-unknown-unknown\"\n").unwrap();
        assert_eq!(
            target(Environment::empty(), None).unwrap(),
            "wasm32-unknown-unknown"
        );
        assert_eq!(
            target(Environment::empty(), Some("aarch64-apple-darwin")).unwrap(),
            "aarch64-apple-darwin"
        );
        let environment = Environment::from_pairs([("CARGO_BUILD_TARGET", "i686-linux-android")])
            .expect("environment seals");
        assert_eq!(target(environment, None).unwrap(), "i686-linux-android");
        fs::write(&own, "[build]\ntarget = [\"host-tuple\"]\n").unwrap();
        assert_eq!(target(Environment::empty(), None).unwrap(), host);
        fs::write(&own, "[build]\ntarget = [\"a\", \"b\"]\n").unwrap();
        let error = target(Environment::empty(), None).unwrap_err().to_string();
        assert!(error.contains("for one target"), "{error}");
        fs::remove_dir_all(root).expect("test config root is removed");
    }

    #[test]
    fn deep_config_hierarchy_discovers_one_provenance() {
        let fixture = std::env::temp_dir().join(format!(
            "rawr-config-ledger-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&fixture).expect("config ledger fixture root is created");
        let mut root = fs::canonicalize(&fixture).expect("config ledger root canonicalizes");
        for depth in 0..12 {
            root.push(format!("level-{depth:02}"));
            fs::create_dir(&root).expect("deep config directory is created");
            let cargo = root.join(".cargo");
            fs::create_dir(&cargo).expect("deep Cargo config directory is created");
            fs::write(cargo.join("config.toml"), b"[net]\noffline = true\n")
                .expect("deep Cargo config can be written");
        }
        let parallelism =
            crate::process::parallelism::Proof::discover().expect("parallelism discovers");
        let exercise = |limits: &Limits| {
            OWNED_PROVENANCE.with(|owned| owned.set(0));
            let provenance = Provenance::discover(
                &root,
                None,
                Environment::empty(),
                Some("aarch64-apple-darwin"),
                "dev",
                parallelism,
                limits,
            )?;
            Ok::<_, crate::Error>(provenance)
        };
        exercise(&Limits::standard()).expect("the deep hierarchy discovers");
        assert_eq!(OWNED_PROVENANCE.with(Cell::get), 1);
        fs::remove_dir_all(fixture).expect("deep config fixture is removed");
    }

    #[test]
    fn importer_private_controls_reject_at_environment_seal() {
        for key in [
            "RAWR_IMPORT_PLAN",
            "RAWR_IMPORT_PROTOCOL",
            "RAWR_IMPORT_FUTURE_CONTROL",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
            "CARGO_UNSTABLE_BUILD_STD",
            "CARGO_UNSTABLE_PROFILE_RUSTFLAGS",
        ] {
            let error = Environment::from_pairs([(key, "unsupported")])
                .expect_err("private importer controls reject");
            assert!(format!("{error}").contains("is not supported yet; unset it and run again"));
        }
    }

    #[test]
    fn final_command_environment_bounds_include_both_tool_entries() {
        let environment = Environment::from_pairs([("USER_VALUE", "present")])
            .expect("the retained fixture environment seals");
        let target = Path::new("/target");
        let rustc = Path::new("/toolchain/bin/rustc");
        let rustdoc = Path::new("/toolchain/bin/rustdoc");
        let loader = (!cfg!(target_vendor = "apple")).then_some("/toolchain/lib");
        let entries = 5 + usize::from(loader.is_some());
        let exact_bytes = "USER_VALUE"
            .len()
            .saturating_add("present".len())
            .saturating_add("CARGO_TARGET_DIR".len())
            .saturating_add(target.as_os_str().len())
            .saturating_add("CARGO_INCREMENTAL".len())
            .saturating_add(1)
            .saturating_add("RUSTC".len())
            .saturating_add(rustc.as_os_str().len())
            .saturating_add("RUSTDOC".len())
            .saturating_add(rustdoc.as_os_str().len())
            .saturating_add(loader.map_or(0, |value| LOADER_PATH.len() + value.len()));

        let values = environment
            .command_values_with_limits(target, rustc, rustdoc, entries, exact_bytes)
            .expect("the exact count and byte boundary admits all fixed entries");
        assert_eq!(values.len(), entries);
        for key in [
            "CARGO_INCREMENTAL",
            "CARGO_TARGET_DIR",
            "RUSTC",
            "RUSTDOC",
            "USER_VALUE",
        ] {
            assert!(values.iter().any(|(actual, _)| actual == key));
        }
        assert!(matches!(
            environment.command_values_with_limits(target, rustc, rustdoc, entries - 1, exact_bytes),
            Err(Error::Limit {
                resource: "Cargo environment entries",
                limit,
                actual,
            }) if limit == entries - 1 && actual == entries
        ));
        let byte_limit = exact_bytes.saturating_sub(1);
        assert!(matches!(
            environment.command_values_with_limits(target, rustc, rustdoc, entries, byte_limit),
            Err(Error::Limit {
                resource: "Cargo environment bytes",
                limit: error_limit,
                actual,
            }) if error_limit == byte_limit && actual == exact_bytes
        ));
    }

    /// Cargo runs with the home it would use, resolved: a `CARGO_HOME`
    /// reached through a symlink and an unset one under a symlinked `HOME`
    /// give it the directory they name, so the registry paths it reports
    /// are canonical (#1754).
    #[cfg(unix)]
    #[test]
    fn cargo_runs_with_its_home_resolved() {
        let root = std::env::temp_dir().join(format!("rawr-command-home-{}", std::process::id()));
        let home = root.join("home");
        let cargo_home = home.join(".cargo");
        fs::create_dir_all(&cargo_home).expect("the fixture Cargo home creates");
        let link = root.join("home-link");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&home, &link).expect("the home symlink creates");
        let resolved = fs::canonicalize(&cargo_home).expect("the Cargo home resolves");
        let given = |pairs: &[(&str, &Path)]| {
            let environment = Environment::from_pairs(
                pairs
                    .iter()
                    .map(|(key, path)| (*key, path.to_str().expect("fixture paths are Unicode"))),
            )
            .expect("the fixture environment seals");
            environment
                .command_values(
                    Path::new("/target"),
                    Path::new("/toolchain/bin/rustc"),
                    Path::new("/toolchain/bin/rustdoc"),
                )
                .expect("the command environment builds")
                .iter()
                .filter(|(key, _)| key == "CARGO_HOME")
                .map(|(_, value)| std::path::PathBuf::from(value))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            given(&[("CARGO_HOME", &link.join(".cargo"))]),
            std::slice::from_ref(&resolved)
        );
        assert_eq!(given(&[("HOME", &link)]), [resolved]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_finds_the_toolchain_libraries_as_under_rustup() {
        let rustc = Path::new("/toolchain/bin/rustc");
        let rustdoc = Path::new("/toolchain/bin/rustdoc");
        let loader = |environment: Environment| {
            environment
                .command_values(Path::new("/target"), rustc, rustdoc)
                .expect("the command environment builds")
                .iter()
                .find(|(key, _)| key == LOADER_PATH)
                .map(|(_, value)| value.clone())
        };
        let caller = Environment::from_pairs([(LOADER_PATH, "/caller/lib")])
            .expect("the caller's loader path seals");
        let empty =
            Environment::from_pairs([(LOADER_PATH, "")]).expect("an empty loader path seals");
        if cfg!(target_vendor = "apple") {
            assert_eq!(loader(Environment::empty()), None);
            assert_eq!(loader(caller), Some("/caller/lib".into()));
        } else {
            assert_eq!(loader(Environment::empty()), Some("/toolchain/lib".into()));
            assert_eq!(loader(empty), Some("/toolchain/lib".into()));
            assert_eq!(loader(caller), Some("/toolchain/lib:/caller/lib".into()));
        }
    }

    #[test]
    fn every_build_std_config_value_shape_rejects() {
        for source in [
            "[unstable]\nbuild-std = false\n",
            "[unstable]\nbuild-std = \"std\"\n",
            "[unstable]\nbuild-std = [\"core\", \"alloc\"]\n",
            "[unstable]\nbuild-std-features = false\n",
            "[unstable]\nbuild-std-features = \"panic_immediate_abort\"\n",
            "[unstable]\nbuild-std-features = [\"panic_immediate_abort\"]\n",
            "unstable = { build-std = [\"core\", \"alloc\"] }\n",
            "unstable = { build-std-features = [\"panic_immediate_abort\"] }\n",
        ] {
            let document = source
                .parse::<DocumentMut>()
                .expect("build-std fixture is valid TOML");
            let error = validate_config_authority(&document, Path::new("/fixture/config.toml"))
                .expect_err("every build-std key rejects regardless of value type");
            assert!(format!("{error}").contains("build-std"));
        }
    }

    #[test]
    fn advanced_environment_config_rejects() {
        let document = "[unstable]\nadvanced-env = true\n"
            .parse::<DocumentMut>()
            .expect("advanced-env fixture is valid TOML");
        let error = validate_config_authority(&document, Path::new("/fixture/config.toml"))
            .expect_err("advanced-env changes the accepted environment schema");
        assert!(format!("{error}").contains("advanced-env"));
    }

    #[test]
    fn inline_build_wrapper_config_rejects() {
        let document = "build = { rustc-wrapper = \"/fixture/wrapper\" }\n"
            .parse::<DocumentMut>()
            .expect("inline wrapper fixture is valid TOML");
        let error = validate_config_authority(&document, Path::new("/fixture/config.toml"))
            .expect_err("inline build wrapper rejects");
        assert!(format!("{error}").contains("rawr can't build Rust with a `rustc` wrapper"));
    }

    /// Two registry dependencies with the same requirement and no
    /// `package` key are two packages; one package under two aliases is
    /// one unit.
    #[test]
    fn dependencies_share_a_unit_only_when_they_name_one_package() {
        let seal = |dependency: Dependency| dependency.seal(Path::new("/")).unwrap();
        let itoa = seal(Dependency::registry("itoa", "1").unwrap());
        let semver = seal(Dependency::registry("semver", "1").unwrap());
        assert!(!itoa.shares_cargo_unit(&semver));
        let renamed = seal(
            Dependency::registry("numbers", "1")
                .unwrap()
                .package("itoa")
                .unwrap(),
        );
        assert!(itoa.shares_cargo_unit(&renamed));
        assert!(renamed.shares_cargo_unit(&itoa));
    }

    /// The pinned Cargo's actual attempted rustc invocation is the oracle for
    /// precedence and definition-relative paths. The executable deliberately
    /// stops before compilation; no rustc wrapper or second config parser is
    /// involved in discovering the production value.
    #[cfg(unix)]
    #[test]
    fn project_compiler_matches_pinned_cargo_precedence_and_relative_paths() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::process::Command;

        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("rawr-compiler-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let home = root.join("home/.cargo");
        for directory in [
            root.join(".cargo"),
            project.join(".cargo"),
            project.join("src"),
            home.clone(),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\n[package]\nname='compiler-config'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(project.join("src/lib.rs"), "").unwrap();
        let tools = [
            root.join("bin/ancestor"),
            root.join("home/bin/home"),
            project.join("bin/project"),
            project.join("bin/build-env"),
            project.join("bin/rustc-env"),
        ];
        for tool in &tools {
            fs::create_dir_all(tool.parent().unwrap()).unwrap();
            fs::write(
                tool,
                "#!/bin/sh\nprintf '%s' \"$0\" > \"$COMPILER_LOG\"\nexit 91\n",
            )
            .unwrap();
            fs::set_permissions(tool, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(
            root.join(".cargo/config.toml"),
            "[build]\nrustc='bin/ancestor'\n",
        )
        .unwrap();
        fs::write(home.join("config.toml"), "[build]\nrustc='bin/home'\n").unwrap();
        let project_config = project.join(".cargo/config.toml");
        fs::write(&project_config, "[build]\nrustc='bin/project'\n").unwrap();
        let cargo = Command::new("rustup")
            .args([
                "which",
                "--toolchain",
                crate::cargo::toolchain::CHANNEL,
                "cargo",
            ])
            .output()
            .unwrap();
        assert!(
            cargo.status.success(),
            "{}",
            String::from_utf8_lossy(&cargo.stderr)
        );
        let cargo = String::from_utf8(cargo.stdout).unwrap();
        let log = root.join("compiler.log");
        let verify = |extra: &[(&str, &str)], expected: &Path| {
            let mut values = vec![
                ("CARGO_HOME".to_owned(), home.to_str().unwrap().to_owned()),
                (
                    "HOME".to_owned(),
                    root.join("home").to_str().unwrap().to_owned(),
                ),
                ("PATH".to_owned(), std::env::var("PATH").unwrap()),
                ("COMPILER_LOG".to_owned(), log.to_str().unwrap().to_owned()),
            ];
            values.extend(
                extra
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string())),
            );
            let environment = Environment::from_pairs(values).unwrap();
            let limits = Limits::standard();
            let provenance = Provenance::discover(
                &project,
                None,
                environment.clone(),
                Some(crate::cargo::toolchain::HOST),
                "dev",
                crate::process::parallelism::Proof::discover().unwrap(),
                &limits,
            )
            .unwrap();
            assert_eq!(provenance.compiler(&project).configured(), Some(expected));
            let context = crate::context::Context::capture_config(
                &project,
                &provenance,
                crate::cargo::toolchain::HOST,
                &limits,
            )
            .unwrap();
            assert_eq!(
                context.compiler().and_then(super::Compiler::configured),
                Some(expected)
            );
            let unbound = crate::context::Context::capture(
                &project,
                environment.clone(),
                crate::cargo::toolchain::HOST,
                &limits,
            )
            .unwrap();
            assert_ne!(context.identity(), unbound.identity());
            let _ = fs::remove_file(&log);
            let observed = Command::new(cargo.trim())
                .args(["check", "--offline"])
                .current_dir(&project)
                .env_clear()
                .envs(environment.entries())
                .output()
                .unwrap();
            assert!(!observed.status.success());
            assert_eq!(
                fs::read_to_string(&log).unwrap_or_else(|_| panic!(
                    "Cargo did not invoke fixture compiler: {}",
                    String::from_utf8_lossy(&observed.stderr)
                )),
                expected.to_str().unwrap()
            );
        };
        verify(&[], &tools[2]);
        fs::remove_file(&project_config).unwrap();
        verify(&[], &tools[0]);
        verify(&[("CARGO_BUILD_RUSTC", "bin/build-env")], &tools[3]);
        verify(
            &[
                ("CARGO_BUILD_RUSTC", "bin/build-env"),
                ("RUSTC", "bin/rustc-env"),
            ],
            &tools[4],
        );
        fs::remove_file(root.join(".cargo/config.toml")).unwrap();
        verify(&[], &tools[1]);
        fs::remove_dir_all(root).unwrap();
    }
}
