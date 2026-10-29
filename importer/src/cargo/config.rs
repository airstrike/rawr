//! Canonical Cargo dependency and configuration provenance.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use semver::VersionReq;
use sha2::{Digest as _, Sha256};
use toml_edit::DocumentMut;

use super::{
    canonical_directory, canonical_file, digest, normalized_absolute, Digest, Limits,
    RequestAllocation,
};
use crate::process::ToolPaths;
use crate::{Error, Result};

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
const LOADER_PATH: &str = "LD_LIBRARY_PATH";
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
    /// A selected framework profile package.
    Profile,
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
    allocation: RequestAllocation,
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
        rev: Box<str>,
    },
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
        [Role::Library, Role::Runtime, Role::Compiler, Role::Profile]
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
            Self::Profile => "profile",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Library => 1,
            Self::Runtime => 2,
            Self::Compiler => 4,
            Self::Profile => 8,
        }
    }
}

impl Dependency {
    /// Begin a local-path dependency specification.
    pub fn path(alias: &str, path: &Path, limits: &Limits) -> Result<Self> {
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(1)?;
        allocation.string(alias)?;
        allocation.path(path)?;
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Path(path.to_path_buf()),
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
            allocation,
        })
    }

    /// Begin a registry dependency with a semantic Cargo version requirement.
    pub fn registry(alias: &str, version: &str, limits: &Limits) -> Result<Self> {
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(1)?;
        allocation.string(alias)?;
        allocation.string(version)?;
        let requirement = VersionReq::parse(version).map_err(|error| Error::Request {
            field: "dependency version",
            reason: error.to_string().into_boxed_str(),
        })?;
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Registry {
                manifest: validated_nonempty("dependency version", version)?,
                requirement,
            },
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
            allocation,
        })
    }

    /// Begin an exact full-revision Git dependency specification.
    pub fn git(alias: &str, url: &str, revision: &str, limits: &Limits) -> Result<Self> {
        let valid = revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(Error::Request {
                field: "dependency git revision",
                reason: "must be a full 40-digit hexadecimal commit".into(),
            });
        }
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(1)?;
        allocation.string(alias)?;
        allocation.string(url)?;
        allocation.string(revision)?;
        Ok(Self {
            alias: validated_library_alias(alias)?,
            package: None,
            source: Source::Git {
                url: validated_nonempty("dependency git URL", url)?,
                rev: revision.to_ascii_lowercase().into_boxed_str(),
            },
            features: Box::default(),
            default_features: true,
            roles: Roles::library(),
            allocation,
        })
    }

    /// Begin an exact local framework-profile package selected by its host.
    #[doc(hidden)]
    pub fn profile_path(alias: &str, package: &str, path: &Path, limits: &Limits) -> Result<Self> {
        if !valid_package_name(package) {
            return Err(Error::Request {
                field: "profile package",
                reason: format!("`{package}` is not a Cargo package name").into_boxed_str(),
            });
        }
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(1)?;
        allocation.string(alias)?;
        allocation.string(package)?;
        allocation.path(path)?;
        Ok(Self {
            alias: validated_alias(alias)?,
            package: Some(package.into()),
            source: Source::Path(path.to_path_buf()),
            features: Box::default(),
            default_features: true,
            roles: Roles::new(Role::Profile),
            allocation,
        })
    }

    /// Select a package name when it differs from the Rawr alias.
    pub fn package(mut self, package: &str) -> Result<Self> {
        if !valid_package_name(package) {
            return Err(Error::Request {
                field: "dependency package",
                reason: format!("`{package}` is not a Cargo package name").into_boxed_str(),
            });
        }
        self.allocation.string(package)?;
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
            self.allocation.string(feature)?;
            retained.try_reserve(1).map_err(|error| Error::Request {
                field: "dependency features",
                reason: error.to_string().into_boxed_str(),
            })?;
            let feature = validated_nonempty("dependency feature", feature)?;
            retained.push(feature);
        }
        retained.sort_unstable();
        retained.dedup();
        self.features = retained.into_boxed_slice();
        self.default_features = default_features;
        Ok(self)
    }

    pub(crate) fn rawr(path: &Path, limits: &Limits) -> Result<Self> {
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(4)?;
        allocation.string(INTERNAL_ALIAS)?;
        allocation.string("rawr")?;
        allocation.path(path)?;
        allocation.string("rust-import")?;
        allocation.string("std")?;
        allocation.string("sync")?;
        // `sync` makes script values `Send`, so a script future is a `Send`
        // std future any runtime polls (D513.3). It changes no unit the
        // project builds: only rawr's own proc macro sees it.
        let features: Box<[Box<str>]> = Box::from([
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
            allocation,
        })
    }

    pub(crate) const fn allocation(&self) -> RequestAllocation {
        self.allocation
    }

    pub(crate) fn seal(
        self,
        project_root: &Path,
        allocation: &mut RequestAllocation,
    ) -> Result<Selected> {
        let source = match self.source {
            Source::Path(path) => {
                let path = if path.is_absolute() {
                    path
                } else {
                    allocation.path(project_root)?;
                    project_root.join(path)
                };
                allocation.path(&path)?;
                Source::Path(canonical_directory("dependency", &path)?)
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
        for (key, value) in env::vars_os() {
            let key = key.into_string().map_err(|_| Error::Request {
                field: "Cargo environment",
                reason: "contains a non-Unicode variable name".into(),
            })?;
            let value = value.into_string().map_err(|_| Error::Request {
                field: "Cargo environment",
                reason: format!("variable `{key}` contains a non-Unicode value").into_boxed_str(),
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
        values.try_reserve(1).map_err(|error| Error::Request {
            field: "Cargo environment",
            reason: format!("failed to reserve environment entry: {error}").into_boxed_str(),
        })?;
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
                field: "Cargo environment",
                reason: format!("invalid environment variable `{key}`").into_boxed_str(),
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
        let retained = self.values.iter().filter(|(key, _)| {
            !IMPORTER_OVERRIDDEN.contains(&key.as_ref())
                && !(loader.is_some() && key.as_ref() == LOADER_PATH)
        });
        let retained_items = retained.clone().count();
        let items = retained_items
            .saturating_add(4)
            .saturating_add(usize::from(loader.is_some()));
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
            );
        if bytes > byte_limit {
            return Err(Error::Limit {
                resource: "Cargo environment bytes",
                limit: byte_limit,
                actual: bytes,
            });
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(items)
            .map_err(|error| Error::Request {
                field: "Cargo environment",
                reason: format!("failed to reserve command environment: {error}").into_boxed_str(),
            })?;
        for (key, value) in retained {
            values.push((OsString::from(key.as_ref()), OsString::from(value.as_ref())));
        }
        values.push(("CARGO_TARGET_DIR".into(), target_dir.as_os_str().to_owned()));
        values.push(("CARGO_INCREMENTAL".into(), "0".into()));
        values.push(("RUSTC".into(), rustc.as_os_str().to_owned()));
        values.push(("RUSTDOC".into(), rustdoc.as_os_str().to_owned()));
        if let Some(loader) = loader {
            values.push((LOADER_PATH.into(), loader));
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
    fn loader_path(&self, rustc: &Path) -> Option<OsString> {
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
        path.map(|path| normalized_absolute("Cargo home", &path))
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
            return Err(Error::Path {
                field: "Cargo home",
                path,
                reason: format!("{source} is explicitly empty").into_boxed_str(),
            });
        }
        if !path.is_absolute() {
            return Err(Error::Path {
                field: "Cargo home",
                path,
                reason: format!("{source} must be absolute").into_boxed_str(),
            });
        }
        let path = normalized_absolute("Cargo home", &path)?;
        Ok(Some(if source == "HOME" {
            path.join(".cargo")
        } else {
            path
        }))
    }
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
            field: "Cargo environment",
            reason: format!("invalid environment entry `{key}`").into_boxed_str(),
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
            field: "Cargo environment authority",
            reason: format!("environment variable `{key}` conflicts with importer control")
                .into_boxed_str(),
        });
    }
    if values.contains_key(key) {
        return Err(Error::Request {
            field: "Cargo environment",
            reason: format!("duplicate environment variable `{key}`").into_boxed_str(),
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
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(values.len())
        .map_err(|error| Error::Request {
            field: "Cargo environment allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    for (key, value) in values {
        retained.push((key.into_boxed_str(), value.into_boxed_str()));
    }
    Ok(retained.into_boxed_slice())
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
                        rev: left_rev,
                    },
                    Source::Git {
                        url: right_url,
                        rev: right_rev,
                    },
                ) => left_url == right_url && left_rev == right_rev,
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

    /// Borrow the semantic values sealed by this exact configuration discovery.
    pub(crate) fn projection(&self) -> &projection::Projection {
        &self.projection
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

    #[cfg(test)]
    pub(crate) fn discover(
        project_root: &Path,
        authority_manifest: Option<&Path>,
        environment: Environment,
        target: &str,
        profile: &str,
        parallelism: crate::process::parallelism::Proof,
        limits: &Limits,
    ) -> Result<Self> {
        let mut allocation = super::RequestAllocation::new(limits);
        Self::discover_budgeted(
            project_root,
            authority_manifest,
            environment,
            target,
            profile,
            parallelism,
            limits,
            &mut allocation,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "provenance is one proof bundle over all selected Cargo authorities and its allocation ledger"
    )]
    pub(crate) fn discover_budgeted(
        project_root: &Path,
        authority_manifest: Option<&Path>,
        environment: Environment,
        target: &str,
        profile: &str,
        parallelism: crate::process::parallelism::Proof,
        limits: &Limits,
        allocation: &mut super::RequestAllocation,
    ) -> Result<Self> {
        let projection = projection::Projection::derive_governed_budgeted(
            &environment,
            target,
            profile,
            parallelism,
            limits,
            allocation,
        )?;
        let mut candidates = Vec::new();
        let project_end = append_hierarchy_candidates(&mut candidates, project_root, allocation)?;
        let authority_end = if let Some(authority_root) = authority_manifest.and_then(Path::parent)
        {
            append_hierarchy_candidates(&mut candidates, authority_root, allocation)?
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
                field: "Cargo config authority",
                reason:
                    "project and selected Cargo authority expose different hierarchical configs"
                        .into(),
            });
        }
        let explicit = if !project_has_config {
            allocation.items(authority_extra_count)?;
            let mut explicit = Vec::new();
            explicit
                .try_reserve_exact(authority_extra_count)
                .map_err(|error| Error::Request {
                    field: "Cargo config authority allocation",
                    reason: error.to_string().into_boxed_str(),
                })?;
            for candidate in authority.iter().filter(|candidate| {
                matches!(candidate.state, State::Present(_))
                    && !project.iter().any(|project| project.path == candidate.path)
            }) {
                allocation.path(&candidate.path)?;
                explicit.push(candidate.path.clone());
            }
            explicit
        } else {
            Vec::new()
        };

        if let Some(cargo_home) = environment.cargo_home()? {
            let home_start = candidates.len();
            append_pair_candidates(&mut candidates, &cargo_home, allocation)?;
            reject_pair_conflict(&candidates[home_start..], "Cargo home config")?;
        }
        candidates.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        candidates.dedup_by(|left, right| left.path == right.path);

        let mut compiler = None::<(usize, PathBuf)>;
        let cargo_home = environment.cargo_home()?;
        for candidate in &candidates {
            if matches!(candidate.state, State::Present(_)) {
                let bytes =
                    read_config_budgeted(&candidate.path, crate::process::TAIL_BYTES, allocation)?;
                // A byte is a conservative upper bound for every retained TOML
                // node and byte.  Prove the complete owned DOM before parsing.
                allocation.retained(bytes.len(), bytes.len())?;
                let text = std::str::from_utf8(&bytes).map_err(|error| Error::Schema {
                    document: "Cargo config",
                    reason: error.to_string().into_boxed_str(),
                })?;
                let document = text.parse::<DocumentMut>().map_err(|error| Error::Schema {
                    document: "Cargo config",
                    reason: error.to_string().into_boxed_str(),
                })?;
                validate_config_authority(&document, &candidate.path)?;
                if let Some(value) = document
                    .get("build")
                    .and_then(toml_edit::Item::as_table_like)
                    .and_then(|build| build.get("rustc"))
                {
                    let value = value.as_str().ok_or_else(|| Error::Request {
                        field: "Cargo project compiler",
                        reason: "build.rustc must be a program path string".into(),
                    })?;
                    let directory = candidate.path.parent().ok_or_else(|| Error::Request {
                        field: "Cargo project compiler",
                        reason: "config has no containing directory".into(),
                    })?;
                    let base = directory.parent().ok_or_else(|| Error::Request {
                        field: "Cargo project compiler",
                        reason: "config has no relative-path base".into(),
                    })?;
                    let priority = if directory.file_name() == Some(std::ffi::OsStr::new(".cargo"))
                        && project_root.starts_with(base)
                    {
                        Some(base.components().count() + 1)
                    } else if cargo_home.as_deref() == Some(directory) {
                        Some(0)
                    } else {
                        None
                    };
                    if let Some(priority) = priority
                        && compiler.as_ref().is_none_or(|(kept, _)| priority > *kept)
                    {
                        let path = compiler_path(base, value);
                        allocation.path(&path)?;
                        compiler = Some((priority, path));
                    }
                }
            }
        }

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
        allocation.retained(1, digest_bytes)?;
        let mut digest_input = Vec::new();
        digest_input
            .try_reserve_exact(digest_bytes)
            .map_err(|error| Error::Request {
                field: "Cargo config provenance digest allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
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
        // The present-config count is an accounting target recorded on
        // the request account; every candidate was already charged above.
        allocation.exceeded(
            "Cargo config files",
            limits.packages,
            candidates
                .iter()
                .filter(|candidate| matches!(&candidate.state, State::Present(_)))
                .count(),
        );
        #[cfg(test)]
        OWNED_PROVENANCE.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self {
            candidates: candidates.into_boxed_slice(),
            explicit: explicit.into_boxed_slice(),
            environment,
            projection,
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

    #[cfg(test)]
    pub(crate) fn input_config(
        &self,
        mut retained: RequestAllocation,
    ) -> Result<crate::input::config::Config> {
        self.input_config_accounted(&mut retained)
    }

    /// Retain baseline configuration through the caller's original request account.
    pub(crate) fn input_config_accounted(
        &self,
        allocation: &mut RequestAllocation,
    ) -> Result<crate::input::config::Config> {
        let mut cargo_env_overrides = false;
        for candidate in &self.candidates {
            if matches!(&candidate.state, State::Present(_)) {
                let bytes =
                    read_config_budgeted(&candidate.path, crate::process::TAIL_BYTES, allocation)?;
                allocation.retained(bytes.len(), bytes.len())?;
                let text = std::str::from_utf8(&bytes).map_err(|error| Error::Schema {
                    document: "Cargo config",
                    reason: error.to_string().into_boxed_str(),
                })?;
                let document = text.parse::<DocumentMut>().map_err(|error| Error::Schema {
                    document: "Cargo config",
                    reason: error.to_string().into_boxed_str(),
                })?;
                validate_config_authority(&document, &candidate.path)?;
                cargo_env_overrides |= document.get("env").is_some();
            }
        }
        allocation.items(self.candidates.len())?;
        for candidate in &self.candidates {
            allocation.path(&candidate.path)?;
        }
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
        let projection = self.projection.clone_budgeted(allocation)?;
        Ok(crate::input::config::Config {
            candidates: candidates.into_boxed_slice(),
            cargo_env_overrides,
            provenance: *self.digest.as_bytes(),
            projection,
            _allocation: *allocation,
        })
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
            field: "Cargo compiler wrapper",
            reason: format!("config `{}` defines a wrapper chain", path.display()).into_boxed_str(),
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
            field: "Cargo build-std authority",
            reason: format!(
                "config `{}` defines unsupported build-std authority",
                path.display()
            )
            .into_boxed_str(),
        });
    }
    if unstable.is_some_and(|table| table.contains_key("advanced-env")) {
        return Err(Error::Request {
            field: "Cargo advanced environment authority",
            reason: format!(
                "config `{}` enables unsupported advanced-env authority",
                path.display()
            )
            .into_boxed_str(),
        });
    }
    Ok(())
}

fn append_hierarchy_candidates(
    candidates: &mut Vec<Candidate>,
    start: &Path,
    allocation: &mut RequestAllocation,
) -> Result<usize> {
    let count = start.ancestors().count().saturating_mul(2);
    allocation.items(count)?;
    candidates
        .try_reserve_exact(count)
        .map_err(|error| Error::Request {
            field: "Cargo config hierarchy allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    for directory in start.ancestors() {
        let pair_start = candidates.len();
        append_candidate(candidates, directory, ".cargo/config.toml", allocation)?;
        append_candidate(candidates, directory, ".cargo/config", allocation)?;
        reject_pair_conflict(&candidates[pair_start..], "Cargo config authority")?;
    }
    Ok(candidates.len())
}

fn append_pair_candidates(
    candidates: &mut Vec<Candidate>,
    directory: &Path,
    allocation: &mut RequestAllocation,
) -> Result<()> {
    allocation.items(2)?;
    candidates
        .try_reserve_exact(2)
        .map_err(|error| Error::Request {
            field: "Cargo config pair allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    append_candidate(candidates, directory, "config.toml", allocation)?;
    append_candidate(candidates, directory, "config", allocation)
}

fn append_candidate(
    candidates: &mut Vec<Candidate>,
    directory: &Path,
    relative: &str,
    allocation: &mut RequestAllocation,
) -> Result<()> {
    let bytes = directory
        .as_os_str()
        .len()
        .saturating_add(1)
        .saturating_add(relative.len());
    allocation.retained(1, bytes)?;
    let path = directory.join(relative);
    reject_symlink_components(&path)?;
    let state = candidate_state_budgeted(&path, MAX_VALUE_BYTES, allocation)?;
    candidates.push(Candidate { path, state });
    Ok(())
}

fn reject_pair_conflict(candidates: &[Candidate], field: &'static str) -> Result<()> {
    if candidates
        .iter()
        .all(|candidate| matches!(&candidate.state, State::Present(_)))
    {
        return Err(Error::Request {
            field,
            reason: "both `config.toml` and legacy `config` exist at one hierarchy level".into(),
        });
    }
    Ok(())
}

fn candidate_state(path: &Path, limit: usize) -> Result<State> {
    candidate_state_inner(path, limit, None)
}

fn candidate_state_budgeted(
    path: &Path,
    limit: usize,
    allocation: &mut RequestAllocation,
) -> Result<State> {
    candidate_state_inner(path, limit, Some(allocation))
}

fn candidate_state_inner(
    path: &Path,
    limit: usize,
    allocation: Option<&mut RequestAllocation>,
) -> Result<State> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Path {
            field: "Cargo config candidate",
            path: path.to_path_buf(),
            reason: "must not be a symlink".into(),
        }),
        Ok(metadata) if metadata.is_file() => {
            let canonical = canonical_file("Cargo config", path)?;
            if canonical != path {
                return Err(Error::Path {
                    field: "Cargo config candidate",
                    path: path.to_path_buf(),
                    reason: "contains a symlink or non-canonical ancestor".into(),
                });
            }
            let bytes = if let Some(allocation) = allocation {
                read_config_budgeted(path, limit, allocation)?
            } else {
                read_config(path, limit)?
            };
            Ok(State::Present(digest(&bytes)))
        }
        Ok(_) => Err(Error::Path {
            field: "Cargo config candidate",
            path: path.to_path_buf(),
            reason: "exists but is not a file".into(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::Absent),
        Err(source) => Err(Error::Io {
            operation: "inspect Cargo config candidate",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn reject_symlink_components(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => continue,
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Path {
                    field: "Cargo config candidate",
                    path: current,
                    reason: "contains a symlink component".into(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect Cargo config ancestor",
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn read_config(path: &Path, limit: usize) -> Result<Vec<u8>> {
    read_config_inner(path, limit, None)
}

fn read_config_budgeted(
    path: &Path,
    limit: usize,
    allocation: &mut RequestAllocation,
) -> Result<Vec<u8>> {
    read_config_inner(path, limit, Some(allocation))
}

fn read_config_inner(
    path: &Path,
    limit: usize,
    allocation: Option<&mut RequestAllocation>,
) -> Result<Vec<u8>> {
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
    if let Some(allocation) = allocation {
        allocation.retained(1, length)?;
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
            field: "dependency alias",
            reason: format!("`{alias}` is not a Rust identifier").into_boxed_str(),
        })
    }
}

fn validated_library_alias(alias: &str) -> Result<Box<str>> {
    if alias == INTERNAL_ALIAS {
        return Err(Error::Request {
            field: "dependency alias",
            reason: format!("`{alias}` is reserved for Rawr's internal package").into_boxed_str(),
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

fn validated_nonempty(field: &'static str, value: &str) -> Result<Box<str>> {
    if value.is_empty() || value.len() > MAX_VALUE_BYTES {
        Err(Error::Request {
            field,
            reason: "must contain between 1 byte and 1 MiB".into(),
        })
    } else {
        Ok(value.into())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::Path;
    use std::rc::Rc;

    use super::projection::Projection;
    use super::{
        validate_config_authority, Candidate, Dependency, Environment, NoBuildStd, Provenance,
        State, LOADER_PATH, OWNED_INPUT_CONFIG_CANDIDATES, OWNED_PROVENANCE,
    };
    use crate::cargo::{digest, Limits, RequestAllocation};
    use crate::Error;
    use toml_edit::DocumentMut;

    /// A target one unit under an exact total refuses nothing; the
    /// account records the axis with that target and a high water equal to
    /// the exact total the exercise charged.
    fn assert_exceeded(
        record: crate::exceedance::Record,
        name: &str,
        target: usize,
        high_water: usize,
    ) {
        let axis = record
            .axis(name)
            .unwrap_or_else(|| panic!("{name} records the lowered target"));
        assert_eq!(axis.target, target as u64);
        assert_eq!(axis.high_water, high_water as u64);
        assert!(axis.first_requested > axis.target);
        assert!(axis.first_requested <= axis.high_water);
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
    fn input_config_accounted_keeps_prior_usage_and_exact_remaining_capacity() {
        let path = std::env::temp_dir().join(format!(
            "rawr-input-config-account-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id(),
        ));
        let source = b"[env]\nCONFIG_VALUE = 'present'\n";
        fs::write(&path, source).expect("present config publishes");
        let provenance = input_config_provenance(&path, State::Present(digest(source)));
        let standard = Limits::standard();
        let mut prior = RequestAllocation::new(&standard);
        prior.retained(7, 31).expect("earlier request work");
        let mut measured = prior;
        let actual = provenance
            .input_config_accounted(&mut measured)
            .expect("the original request account retains config");
        assert!(measured.items > prior.items);
        assert!(measured.bytes > prior.bytes);
        assert_eq!(actual._allocation, measured);
        let legacy = provenance
            .input_config(prior)
            .expect("legacy wrapper delegates");
        assert_eq!(actual.candidates, legacy.candidates);
        assert_eq!(actual.projection, legacy.projection);
        assert_eq!(actual.provenance, legacy.provenance);
        assert_eq!(actual.cargo_env_overrides, legacy.cargo_env_overrides);
        assert_eq!(actual._allocation, legacy._allocation);
        assert!(actual.cargo_env_overrides);
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
        // The exact remaining capacity records nothing; one unit less
        // still admits the config, charges the same totals, and records the
        // lowered axis with the measured total as its high water.
        for (item_floor, byte_floor, axis) in [
            (0, 0, None),
            (1, 0, Some(crate::cargo::REQUEST_ITEMS_RESOURCE)),
            (0, 1, Some(crate::cargo::REQUEST_BYTES_RESOURCE)),
        ] {
            let mut limits = standard;
            limits.items = measured.items - item_floor;
            limits.manifest_bytes = measured.bytes - byte_floor;
            let mut allocation = RequestAllocation::new(&limits);
            allocation
                .retained(prior.items, prior.bytes)
                .expect("same earlier request work");
            let config = provenance
                .input_config_accounted(&mut allocation)
                .expect("the exact and lowered targets both admit the config");
            assert_eq!(
                (allocation.items, allocation.bytes),
                (measured.items, measured.bytes)
            );
            assert_eq!(config._allocation, allocation);
            match axis {
                None => assert!(!allocation.exceedances().is_exceeded()),
                Some(name) if name == crate::cargo::REQUEST_ITEMS_RESOURCE => {
                    assert_exceeded(allocation.exceedances(), name, limits.items, measured.items);
                }
                Some(name) => {
                    assert_exceeded(
                        allocation.exceedances(),
                        name,
                        limits.manifest_bytes,
                        measured.bytes,
                    );
                }
            }
        }
        fs::remove_file(path).expect("present config retires");
    }

    #[test]
    fn input_config_accounted_records_exceedance_before_candidate_ownership() {
        let path = Path::new("/unobserved/config.toml");
        let provenance = input_config_provenance(path, State::Absent);
        for bytes in [false, true] {
            let mut limits = Limits::standard();
            if bytes {
                limits.manifest_bytes = 31 + path.as_os_str().len() - 1;
            } else {
                limits.items = 8;
            }
            let mut allocation = RequestAllocation::new(&limits);
            allocation.retained(7, 31).expect("earlier request work");
            OWNED_INPUT_CONFIG_CANDIDATES.with(|owned| owned.set(0));
            // The candidate path charge crosses the target before the
            // candidates are owned; the account records that first exceeding
            // charge, keeps charging, and ownership still proceeds.
            let config = provenance
                .input_config_accounted(&mut allocation)
                .expect("the exceeded target still admits the candidates");
            assert_eq!(OWNED_INPUT_CONFIG_CANDIDATES.with(Cell::get), 1);
            assert_eq!(config._allocation, allocation);
            let (name, first_requested, total) = if bytes {
                (
                    crate::cargo::REQUEST_BYTES_RESOURCE,
                    31 + path.as_os_str().len(),
                    allocation.bytes,
                )
            } else {
                (crate::cargo::REQUEST_ITEMS_RESOURCE, 9, allocation.items)
            };
            let axis = allocation
                .exceedances()
                .axis(name)
                .copied()
                .expect("the candidate path charge records its axis");
            assert_eq!(axis.first_requested, first_requested as u64);
            assert_eq!(axis.high_water, total as u64);
            assert!(allocation.items > 8 && allocation.bytes > 31);
        }
    }

    #[test]
    fn input_config_accounted_keeps_read_and_parse_charges_on_schema_failure() {
        let path = std::env::temp_dir().join(format!(
            "rawr-input-config-failure-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id(),
        ));
        for source in [b"\xff".as_slice(), b"key = ".as_slice()] {
            fs::write(&path, source).expect("invalid config publishes");
            let provenance = input_config_provenance(&path, State::Present(digest(source)));
            let limits = Limits::standard();
            let mut allocation = RequestAllocation::new(&limits);
            allocation.retained(7, 31).expect("earlier request work");
            OWNED_INPUT_CONFIG_CANDIDATES.with(|owned| owned.set(0));
            assert!(matches!(
                provenance.input_config_accounted(&mut allocation),
                Err(Error::Schema {
                    document: "Cargo config",
                    ..
                })
            ));
            assert_eq!(
                (allocation.items, allocation.bytes),
                (8 + source.len(), 31 + 2 * source.len())
            );
            assert_eq!(OWNED_INPUT_CONFIG_CANDIDATES.with(Cell::get), 0);
        }
        fs::remove_file(path).expect("invalid config retires");
    }

    #[test]
    fn feature_iterator_records_exceedance_past_the_retained_item_target() {
        const EXTRA: usize = 6;
        let mut limits = Limits::standard();
        limits.items = 4;
        let exact = Dependency::registry("dep", "1", &limits)
            .expect("dependency occupies three retained items")
            .features(["feature"], true)
            .expect("one feature reaches the exact retained-item target");
        assert!(!exact.allocation().exceedances().is_exceeded());

        // A finite feature list past the item target is retained in
        // full; the account records the items axis from the fifth item up to
        // the truthful high water of every retained feature.
        let generated = Rc::new(Cell::new(0_usize));
        let counter = Rc::clone(&generated);
        let features = std::iter::from_fn(move || {
            let next = counter.get();
            if next == EXTRA {
                return None;
            }
            counter.set(next + 1);
            Some(format!("feature-{next}"))
        });
        let dependency = Dependency::registry("dep", "1", &limits)
            .expect("dependency occupies three retained items")
            .features(features, true)
            .expect("every feature past the target is still retained");
        assert_eq!(generated.get(), EXTRA, "the whole finite list is consumed");
        assert_eq!(dependency.features.len(), EXTRA);
        let items = dependency
            .allocation()
            .exceedances()
            .axis(crate::cargo::REQUEST_ITEMS_RESOURCE)
            .copied()
            .expect("the feature list records the items axis");
        assert_eq!(
            (items.target, items.first_requested, items.high_water),
            (4, 5, 3 + EXTRA as u64)
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
            "aarch64-apple-darwin",
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
            "aarch64-apple-darwin",
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
            "aarch64-apple-darwin",
            "dev",
            parallelism,
            &limits,
        )
        .expect("semantic provenance discovers");
        fs::remove_dir(path).expect("test config root is removed");
        assert_eq!(baseline.digest, semantic.digest);
        assert_ne!(baseline.projection, semantic.projection);
    }

    #[test]
    fn deep_config_hierarchy_uses_one_exact_prospective_request_ledger() {
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
            let mut allocation = crate::cargo::RequestAllocation::new(limits);
            OWNED_PROVENANCE.with(|owned| owned.set(0));
            let provenance = Provenance::discover_budgeted(
                &root,
                None,
                Environment::empty(),
                "aarch64-apple-darwin",
                "dev",
                parallelism,
                limits,
                &mut allocation,
            )?;
            Ok::<_, crate::Error>((allocation, provenance))
        };
        let standard = Limits::standard();
        let (usage, _) = exercise(&standard).expect("standard deep hierarchy measures");
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, _) = exercise(&exact).expect("exact deep hierarchy ledger succeeds");
        assert_eq!(OWNED_PROVENANCE.with(Cell::get), 1);
        assert!(!exact_usage.exceedances().is_exceeded());
        // One unit under the exact ledger refuses nothing and still owns
        // the provenance; the account records the lowered axis at the exact total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, _) =
            exercise(&item_floor).expect("one item under the deep ledger still discovers");
        assert_eq!(OWNED_PROVENANCE.with(Cell::get), 1);
        assert_exceeded(
            item_usage.exceedances(),
            crate::cargo::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, _) =
            exercise(&byte_floor).expect("one byte under the deep ledger still discovers");
        assert_eq!(OWNED_PROVENANCE.with(Cell::get), 1);
        assert_exceeded(
            byte_usage.exceedances(),
            crate::cargo::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
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
            assert!(format!("{error}").contains("importer control"));
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
        assert!(format!("{error}").contains("wrapper chain"));
    }

    /// Two registry dependencies with the same requirement and no
    /// `package` key are two packages; one package under two aliases is
    /// one unit.
    #[test]
    fn dependencies_share_a_unit_only_when_they_name_one_package() {
        let limits = Limits::standard();
        let seal = |dependency: Dependency| {
            dependency
                .seal(Path::new("/"), &mut RequestAllocation::new(&limits))
                .unwrap()
        };
        let itoa = seal(Dependency::registry("itoa", "1", &limits).unwrap());
        let semver = seal(Dependency::registry("semver", "1", &limits).unwrap());
        assert!(!itoa.shares_cargo_unit(&semver));
        let renamed = seal(
            Dependency::registry("numbers", "1", &limits)
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
                crate::cargo::toolchain::HOST,
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
