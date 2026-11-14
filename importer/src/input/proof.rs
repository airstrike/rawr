//! Canonical content proof and bounded filesystem evaluation.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::fingerprint::Fingerprint;
use super::package;
use super::stored::view::View;
use super::walk::{self, Ancestors};
use super::Allocation;
use crate::cargo;
use crate::{Error, Result};

pub(in crate::input) mod protected;

pub(super) const SCHEMA: u32 = 10;

/// Canonical freshness evidence consumed when a captured graph is published.
#[derive(Clone, Serialize)]
pub struct Proof {
    schema: u32,
    entries: Box<[Entry]>,
    fingerprint: Fingerprint,
    // Derived only from associated owners. The exact serialized proof and its
    // historical codec deliberately retain their existing representation.
    #[serde(skip)]
    native: Option<super::native::Inputs>,
}

/// One filesystem identity whose changes can invalidate a native proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Watch {
    /// Observe one exact file or currently absent path through its parent.
    Exact(PathBuf),
    /// Observe a recursively captured package or declared-input tree.
    Tree(PathBuf),
}

/// Exact final-form harness bytes transformed into a generated runner crate.
pub(crate) struct Harness {
    manifest: Box<[u8]>,
    lock: Box<[u8]>,
}

pub(super) enum Recipe {
    Workspace {
        roots: Box<[PathBuf]>,
    },
    File {
        role: Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    Path {
        role: Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    Directory {
        role: Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    DepInfo {
        package_id: Box<str>,
        path: PathBuf,
        package_root: PathBuf,
        out_directories: Box<[PathBuf]>,
    },
    SavedOutput {
        package_id: Box<str>,
        out_dir: PathBuf,
    },
    Package(package::Set),
    Environment {
        package_id: Box<str>,
        execution: Box<str>,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    EnvironmentProjection {
        schema: u32,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    Fact {
        role: Role,
        package_id: Option<Box<str>>,
        name: Box<str>,
        value: Box<str>,
    },
    BuildMessage {
        package_id: Box<str>,
        execution: Box<str>,
        name: Box<str>,
        value: Box<str>,
    },
    Directive {
        package_id: Box<str>,
        execution: Box<str>,
        index: usize,
        value: Box<str>,
    },
}

impl Recipe {
    pub(super) fn view(&self) -> View<'_, package::Set> {
        match self {
            Self::Workspace { roots, .. } => View::Workspace { roots },
            Self::File {
                role,
                package_id,
                path,
            } => View::File {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::Path {
                role,
                package_id,
                path,
            } => View::Path {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::Directory {
                role,
                package_id,
                path,
            } => View::Directory {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            } => View::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            },
            Self::SavedOutput {
                package_id,
                out_dir,
            } => View::SavedOutput {
                package_id,
                out_dir,
            },
            Self::Package(package) => View::Package(package),
            Self::Environment {
                package_id,
                execution,
                key,
                value,
            } => View::Environment {
                package_id,
                execution,
                key,
                value: value.as_deref(),
            },
            Self::EnvironmentProjection { schema, key, value } => View::EnvironmentProjection {
                schema: *schema,
                key,
                value: value.as_deref(),
            },
            Self::Fact {
                role,
                package_id,
                name,
                value,
            } => View::Fact {
                role: *role,
                package_id: package_id.as_deref(),
                name,
                value,
            },
            Self::BuildMessage {
                package_id,
                execution,
                name,
                value,
            } => View::BuildMessage {
                package_id,
                execution,
                name,
                value,
            },
            Self::Directive {
                package_id,
                execution,
                index,
                value,
            } => View::Directive {
                package_id,
                execution,
                index: *index,
                value,
            },
        }
    }

    pub(super) fn file(role: Role, path: &Path, allocation: &mut Allocation) -> Result<Self> {
        allocation.path(path)?;
        Ok(Self::File {
            role,
            package_id: None,
            path: path.to_path_buf(),
        })
    }

    pub(super) fn package_file(
        role: Role,
        package_id: &str,
        path: &Path,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.path(path)?;
        Ok(Self::File {
            role,
            package_id: Some(package_id.into()),
            path: path.to_path_buf(),
        })
    }

    pub(super) fn path(role: Role, path: &Path, allocation: &mut Allocation) -> Result<Self> {
        allocation.path(path)?;
        Ok(Self::Path {
            role,
            package_id: None,
            path: path.to_path_buf(),
        })
    }

    pub(super) fn package_path(
        role: Role,
        package_id: &str,
        path: &Path,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.path(path)?;
        Ok(Self::Path {
            role,
            package_id: Some(package_id.into()),
            path: path.to_path_buf(),
        })
    }

    pub(super) fn package_directory(
        role: Role,
        package_id: &str,
        path: &Path,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.path(path)?;
        Ok(Self::Directory {
            role,
            package_id: Some(package_id.into()),
            path: path.to_path_buf(),
        })
    }

    pub(super) fn fact(
        role: Role,
        name: &str,
        value: &str,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(name)?;
        allocation.string(value)?;
        Ok(Self::Fact {
            role,
            package_id: None,
            name: name.into(),
            value: value.into(),
        })
    }

    pub(super) fn package_fact(
        role: Role,
        package_id: &str,
        name: &str,
        value: &str,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.string(name)?;
        allocation.string(value)?;
        Ok(Self::Fact {
            role,
            package_id: Some(package_id.into()),
            name: name.into(),
            value: value.into(),
        })
    }

    pub(super) fn build_message(
        package_id: &str,
        execution: &str,
        name: &str,
        value: &str,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.string(execution)?;
        allocation.string(name)?;
        allocation.string(value)?;
        Ok(Self::BuildMessage {
            package_id: package_id.into(),
            execution: execution.into(),
            name: name.into(),
            value: value.into(),
        })
    }

    pub(super) fn environment(
        package_id: &str,
        execution: &str,
        key: &str,
        value: Option<&str>,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.string(execution)?;
        allocation.string(key)?;
        if let Some(value) = value {
            allocation.string(value)?;
        }
        Ok(Self::Environment {
            package_id: package_id.into(),
            execution: execution.into(),
            key: key.into(),
            value: value.map(Box::from),
        })
    }

    pub(super) fn projection(
        schema: u32,
        key: &str,
        value: Option<&str>,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(key)?;
        if let Some(value) = value {
            allocation.string(value)?;
        }
        Ok(Self::EnvironmentProjection {
            schema,
            key: key.into(),
            value: value.map(Box::from),
        })
    }

    pub(super) fn directive(
        package_id: &str,
        execution: &str,
        index: usize,
        value: &str,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.string(execution)?;
        allocation.string(value)?;
        Ok(Self::Directive {
            package_id: package_id.into(),
            execution: execution.into(),
            index,
            value: value.into(),
        })
    }

    pub(super) fn saved_output(
        package_id: &str,
        out_dir: &Path,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.path(out_dir)?;
        Ok(Self::SavedOutput {
            package_id: package_id.into(),
            out_dir: out_dir.to_path_buf(),
        })
    }

    pub(super) fn dep_info(
        package_id: &str,
        path: &Path,
        package_root: &Path,
        out_directories: &[PathBuf],
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.string(package_id)?;
        allocation.path(path)?;
        allocation.path(package_root)?;
        allocation.items(out_directories.len())?;
        for directory in out_directories {
            allocation.path(directory)?;
        }
        let mut directories = Vec::new();
        directories
            .try_reserve_exact(out_directories.len())
            .map_err(|error| Error::Input {
                code: "input-recipe-allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
        for directory in out_directories {
            directories.push(directory.to_path_buf());
        }
        Ok(Self::DepInfo {
            package_id: package_id.into(),
            path: path.to_path_buf(),
            package_root: package_root.to_path_buf(),
            out_directories: directories.into_boxed_slice(),
        })
    }
}

/// A config file reached through a symlink is also watched where the link
/// points now: an edit there changes what Cargo reads. The link itself is the
/// entry's own path, so retargeting it is seen there.
fn followed(entry: &Entry) -> Option<Watch> {
    let Entry::File {
        role: Role::Config,
        path,
        ..
    } = entry
    else {
        return None;
    };
    let path = Path::new(path.as_ref());
    fs::canonicalize(path)
        .ok()
        .filter(|target| target != path)
        .map(Watch::Exact)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Authority,
    Harness,
    Config,
    Toolchain,
    Target,
    Profile,
    PackageManifest,
    DependencySource,
    DepInfo,
    BuildScript,
    DeclaredInput,
    DefaultPackageSource,
    OutDirectory,
    BuiltOutput,
    BuildMessage,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Entry {
    Workspace {
        paths: Box<[Box<str>]>,
        digest: [u8; 32],
    },
    File {
        role: Role,
        #[serde(skip_serializing_if = "Option::is_none")]
        package_id: Option<Box<str>>,
        path: Box<str>,
        bytes: usize,
        digest: [u8; 32],
    },
    Directory {
        role: Role,
        #[serde(skip_serializing_if = "Option::is_none")]
        package_id: Option<Box<str>>,
        path: Box<str>,
        directories: Box<[TreeDirectory]>,
        files: Box<[TreeFile]>,
    },
    Registry {
        role: Role,
        package_id: Box<str>,
        source: Box<str>,
        checksum: [u8; 32],
        root: Box<str>,
        marker_schema: u32,
        marker_digest: [u8; 32],
    },
    Git {
        role: Role,
        package_id: Box<str>,
        source: Box<str>,
        oid: Box<str>,
        root: Box<str>,
        files: Box<[TreeFile]>,
    },
    Missing {
        role: Role,
        #[serde(skip_serializing_if = "Option::is_none")]
        package_id: Option<Box<str>>,
        path: Box<str>,
    },
    Environment {
        package_id: Box<str>,
        execution: Box<str>,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    EnvironmentProjection {
        schema: u32,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    Fact {
        role: Role,
        #[serde(skip_serializing_if = "Option::is_none")]
        package_id: Option<Box<str>>,
        name: Box<str>,
        value: Box<str>,
    },
    BuildMessage {
        package_id: Box<str>,
        execution: Box<str>,
        name: Box<str>,
        source: Option<Box<str>>,
        value: Box<str>,
    },
    Directive {
        package_id: Box<str>,
        execution: Box<str>,
        index: usize,
        value: Box<str>,
    },
}

impl Entry {
    /// Whether this entry is one of the generated runner's own files, which
    /// Rawr writes into its store.
    fn generated(&self) -> bool {
        matches!(
            self,
            Self::File {
                role: Role::Harness,
                ..
            } | Self::Missing {
                role: Role::Harness,
                ..
            }
        )
    }

    /// Whether a build wrote what this entry proves: the generated runner's
    /// own files, or a path under one of `outputs`. A workspace is one when
    /// each of its paths is.
    fn written(&self, outputs: &[PathBuf]) -> bool {
        let under = |path: &str| {
            let path = crate::location::resolved(Path::new(path));
            outputs.iter().any(|output| path.starts_with(output))
        };
        match self {
            _ if self.generated() => true,
            Self::Workspace { paths, .. } => {
                !paths.is_empty() && paths.iter().all(|path| under(path))
            }
            Self::File { path, .. } | Self::Directory { path, .. } | Self::Missing { path, .. } => {
                under(path)
            }
            _ => false,
        }
    }

    /// What this entry proves, as a person names it: a file by its path,
    /// a package by its id, a variable by its name.
    fn subject(&self) -> String {
        match self {
            Self::Workspace { .. } => "the Cargo workspace".to_owned(),
            Self::File { path, .. } | Self::Directory { path, .. } | Self::Missing { path, .. } => {
                format!("`{path}`")
            }
            Self::Registry { package_id, .. } | Self::Git { package_id, .. } => {
                format!("the package `{package_id}`")
            }
            Self::Environment { key, .. } | Self::EnvironmentProjection { key, .. } => {
                format!("the environment variable `{key}`")
            }
            Self::Fact { name, .. } => format!("the toolchain's `{name}`"),
            Self::BuildMessage { package_id, .. } | Self::Directive { package_id, .. } => {
                format!("the build script output of `{package_id}`")
            }
        }
    }
}

#[derive(Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProtectedEntry {
    GeneratedFile {
        package: Box<str>,
        execution: ProtectedExecution,
        path: Box<str>,
        bytes: usize,
        digest: [u8; 32],
    },
    File {
        role: Role,
        source: ProtectedSource,
        bytes: usize,
        digest: [u8; 32],
    },
    OutputDirectory {
        package: Box<str>,
        execution: ProtectedExecution,
        directories: Box<[TreeDirectory]>,
        files: Box<[TreeFile]>,
    },
    SourceDirectory {
        package: Box<str>,
        directories: Box<[TreeDirectory]>,
        files: Box<[TreeFile]>,
    },
    Registry {
        role: Role,
        package: Box<str>,
        source: Box<str>,
        checksum: [u8; 32],
        marker_schema: u32,
        marker_digest: [u8; 32],
    },
    Git {
        role: Role,
        package: Box<str>,
        source: Box<str>,
        oid: Box<str>,
        files: Box<[TreeFile]>,
    },
    Missing {
        role: Role,
        source: ProtectedSource,
    },
    Environment {
        package: Box<str>,
        execution: ProtectedExecution,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    Fact {
        role: Role,
        package: Box<str>,
        name: Box<str>,
        value: Box<str>,
    },
    BuildMessage {
        package: Box<str>,
        execution: ProtectedExecution,
        name: Box<str>,
        value: Box<str>,
    },
    Directive {
        package: Box<str>,
        execution: ProtectedExecution,
        index: usize,
        value: Box<str>,
    },
}

/// One authenticated compiler edge from the package whose execution consumed
/// an input to the package namespace that physically owns its canonical path.
#[derive(Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ProtectedSource {
    consumer: Box<str>,
    owner: Box<str>,
    path: Box<str>,
}

#[derive(Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProtectedExecution {
    Artifact {
        target: Box<str>,
        target_kinds: Box<[Box<str>]>,
        crate_types: Box<[Box<str>]>,
        crate_name: Box<str>,
        features: Box<[Box<str>]>,
        profile: Box<[u8]>,
        platform: ArtifactPlatform,
        role: ArtifactRole,
    },
    BuildScript {
        target: Box<str>,
        target_kinds: Box<[Box<str>]>,
        crate_types: Box<[Box<str>]>,
        crate_name: Box<str>,
        features: Box<[Box<str>]>,
        profile: Box<[u8]>,
        platform: ArtifactPlatform,
    },
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactPlatform {
    Host,
    Target,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactRole {
    BuildScript,
    ProcMacro,
    Normal,
}

#[derive(Clone, Copy)]
enum ExecutionJoin<T> {
    One(T),
    Ambiguous,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct TreeFile {
    pub(crate) path: Box<str>,
    pub(crate) bytes: usize,
    pub(crate) digest: [u8; 32],
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct TreeDirectory {
    pub(crate) path: Box<str>,
}

/// Buffer a canonical projection with checked arithmetic. The buffer is
/// bounded only by `try_reserve`, and a count overflow is a genuine failure.
struct Canonical {
    bytes: Vec<u8>,
}

/// Hash the canonical proof while counting its bytes with checked arithmetic;
/// an overflow of the count is a genuine failure.
struct Hashing {
    hasher: Sha256,
    bytes: usize,
    overflow: bool,
}

impl Proof {
    pub(in crate::input) fn contains_workspace(&self, expected: &Entry) -> bool {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Workspace { .. }))
            .eq(std::iter::once(expected))
    }
    /// Move entries after stored replay has joined its matching current owner.
    #[cfg(feature = "rustc-driver")]
    pub(super) fn from_replayed_parts(
        schema: u32,
        entries: Box<[Entry]>,
        fingerprint: Fingerprint,
        native: super::native::Inputs,
    ) -> Self {
        Self {
            schema,
            entries,
            fingerprint,
            native: Some(native),
        }
    }

    /// The proof of a run that replayed shared bindings: the files it read
    /// (the project's manifest and lock, the final-form harness), joined with
    /// the evaluated crate input witnesses a watcher observes (D645).
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn shared<'proof>(
        entries: Box<[Entry]>,
        native: [u8; 32],
        witnesses: impl IntoIterator<Item = &'proof Self>,
    ) -> Self {
        let entries = entries
            .into_vec()
            .into_iter()
            .chain(
                witnesses
                    .into_iter()
                    .flat_map(|proof| proof.entries.iter().cloned()),
            )
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let canonical = serde_json::to_vec(&(SCHEMA, &entries)).unwrap_or_default();
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.shared-bindings/v1");
        fingerprint.update(&canonical);
        Self {
            schema: SCHEMA,
            entries,
            fingerprint: Fingerprint::from_bytes(fingerprint.finalize().into()),
            native: Some(super::native::Inputs::shared(native)),
        }
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture() -> Self {
        Self {
            schema: SCHEMA,
            entries: Box::default(),
            fingerprint: Fingerprint::from_bytes([0x51; 32]),
            native: Some(super::native::Inputs::fixture([0x51; 32])),
        }
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture_runner_harness_manifest(root: &Path) -> Result<PathBuf> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| Error::Input {
                code: "runner-harness-fixture-root",
                reason: "the importer workspace root is absent".into(),
            })?;
        let workspace = workspace.to_str().ok_or_else(|| Error::Input {
            code: "runner-harness-fixture-root",
            reason: "the importer workspace root is not Unicode".into(),
        })?;
        let manifest = format!(
            "[workspace]\nresolver = \"3\"\n\n[package]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies.__rawr_internal]\npackage = \"rawr\"\npath = {workspace:?}\ndefault-features = false\nfeatures = [\"debugging\", \"rust-import\", \"std\", \"sync\"]\n"
        );
        let harness = root.join("runner-harness");
        fs::create_dir_all(&harness).map_err(|source| Error::Io {
            operation: "create runner harness fixture",
            path: harness.clone(),
            source,
        })?;
        let manifest_path = harness.join("Cargo.toml");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)
        {
            Ok(mut output) => {
                output
                    .write_all(manifest.as_bytes())
                    .map_err(|source| Error::Io {
                        operation: "write runner harness fixture manifest",
                        path: manifest_path.clone(),
                        source,
                    })?
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&manifest_path).map_err(|source| Error::Io {
                    operation: "read existing runner harness fixture manifest",
                    path: manifest_path.clone(),
                    source,
                })?;
                if existing.as_slice() != manifest.as_bytes() {
                    return Err(Error::Input {
                        code: "runner-harness-fixture-drift",
                        reason: "the existing runner harness manifest changed".into(),
                    });
                }
            }
            Err(source) => {
                return Err(Error::Io {
                    operation: "create runner harness fixture manifest",
                    path: manifest_path,
                    source,
                });
            }
        }
        Ok(manifest_path)
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture_runner_harness(root: &Path) -> Result<Self> {
        let manifest_path = Self::fixture_runner_harness_manifest(root)?;
        let lock_path = manifest_path.with_file_name("Cargo.lock");
        let manifest = fs::read(&manifest_path).map_err(|source| Error::Io {
            operation: "read runner harness fixture manifest",
            path: manifest_path.clone(),
            source,
        })?;
        let lock = fs::read(&lock_path).map_err(|source| Error::Io {
            operation: "read runner harness fixture lock",
            path: lock_path.clone(),
            source,
        })?;
        let manifest_path = manifest_path.to_str().ok_or_else(|| Error::Input {
            code: "runner-harness-fixture-path",
            reason: "the runner harness manifest path is not Unicode".into(),
        })?;
        let lock_path = lock_path.to_str().ok_or_else(|| Error::Input {
            code: "runner-harness-fixture-path",
            reason: "the runner harness lock path is not Unicode".into(),
        })?;
        let manifest_digest: [u8; 32] = Sha256::digest(&manifest).into();
        let lock_digest: [u8; 32] = Sha256::digest(&lock).into();
        let entries = Box::from([
            Entry::File {
                role: Role::Harness,
                package_id: None,
                path: manifest_path.into(),
                bytes: manifest.len(),
                digest: manifest_digest,
            },
            Entry::File {
                role: Role::Harness,
                package_id: None,
                path: lock_path.into(),
                bytes: lock.len(),
                digest: lock_digest,
            },
        ]);
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.runner-harness-fixture/v1");
        fingerprint.update(manifest_digest);
        fingerprint.update(lock_digest);
        let fingerprint = fingerprint.finalize().into();
        Ok(Self {
            schema: SCHEMA,
            entries,
            fingerprint: Fingerprint::from_bytes(fingerprint),
            native: Some(super::native::Inputs::fixture(fingerprint)),
        })
    }

    pub(crate) const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    pub(crate) fn native(&self) -> Result<&super::native::Inputs> {
        self.native.as_ref().ok_or_else(|| Error::Input {
            code: "native-input-association-missing",
            reason: "exact proof has no associated native input projection".into(),
        })
    }

    pub(super) fn bind_native(&mut self, native: super::native::Inputs) {
        self.native = Some(native);
    }

    /// The input that changed from `older` to this proof, in one line, as
    /// "`src/theme/button.rs` changed": the first entry that is new,
    /// changed or gone. `None` when every entry is the same.
    ///
    /// What a build wrote is never named: an entry under `outputs`, the
    /// directories builds write ([`crate::location::outputs_here`]), and
    /// the generated runner's own files, which each build places in an
    /// execution of its own under Rawr's store (#1890). Their place moves
    /// with every build, and what they hold follows from the inputs named.
    pub(crate) fn change_from(&self, older: &Self, outputs: &[PathBuf]) -> Option<String> {
        let changed = |newer: &[Entry], older: &[Entry]| {
            newer
                .iter()
                .filter(|entry| !entry.written(outputs))
                .find(|entry| !older.contains(entry))
                .map(Entry::subject)
        };
        changed(&self.entries, &older.entries)
            .or_else(|| changed(&older.entries, &self.entries))
            .map(|subject| format!("{subject} changed"))
    }

    pub(super) fn stored_parts(&self) -> (u32, &[Entry], Fingerprint) {
        (self.schema, &self.entries, self.fingerprint)
    }

    pub(super) fn into_stored_parts(self) -> (u32, Box<[Entry]>, Fingerprint) {
        (self.schema, self.entries, self.fingerprint)
    }

    #[cfg(test)]
    pub(super) fn fixture_stored(entries: Box<[Entry]>) -> Self {
        let canonical = serde_json::to_vec(&(SCHEMA, &entries)).expect("fixture proof serializes");
        Self {
            schema: SCHEMA,
            fingerprint: Fingerprint::from_bytes(Sha256::digest(canonical).into()),
            native: None,
            entries,
        }
    }

    /// What this proof read, entry by entry.
    #[cfg(any(test, feature = "rustc-driver"))]
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Clone only bounded filesystem identities required by the production
    /// watcher; environment and scalar proof entries remain non-filesystem
    /// freshness evidence.
    ///
    /// The generated runner's own files are Rawr's writes, never a save,
    /// and are not watched (#1890).
    pub fn watch(&self) -> impl Iterator<Item = Watch> + '_ {
        self.entries
            .iter()
            .filter(|entry| !entry.generated())
            .flat_map(|entry| {
                let paths = match entry {
                    Entry::Workspace { paths, .. } => Some(paths.as_ref()),
                    _ => None,
                };
                paths
                    .into_iter()
                    .flatten()
                    .map(|path| Watch::Exact(PathBuf::from(path.as_ref())))
                    .chain(match entry {
                        Entry::Workspace { .. } => None,
                        Entry::File { path, .. } | Entry::Missing { path, .. } => {
                            Some(Watch::Exact(PathBuf::from(path.as_ref())))
                        }
                        Entry::Directory { path, .. }
                        | Entry::Registry { root: path, .. }
                        | Entry::Git { root: path, .. } => {
                            Some(Watch::Tree(PathBuf::from(path.as_ref())))
                        }
                        Entry::Environment { .. }
                        | Entry::EnvironmentProjection { .. }
                        | Entry::Fact { .. }
                        | Entry::BuildMessage { .. }
                        | Entry::Directive { .. } => None,
                    })
                    .chain(followed(entry))
            })
    }

    /// Each path package's name and directory, from the Cargo package ids
    /// the proof's entries carry: `path+file:///dir#name@version`, or
    /// `path+file:///dir#version` when the directory is named as the
    /// package.
    fn path_packages(&self) -> Vec<(String, String)> {
        let mut packages = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::File { package_id, .. }
                | Entry::Directory { package_id, .. }
                | Entry::Missing { package_id, .. }
                | Entry::Fact { package_id, .. } => package_id.as_deref(),
                Entry::Registry { package_id, .. }
                | Entry::Git { package_id, .. }
                | Entry::Environment { package_id, .. }
                | Entry::BuildMessage { package_id, .. }
                | Entry::Directive { package_id, .. } => Some(package_id.as_ref()),
                Entry::Workspace { .. } | Entry::EnvironmentProjection { .. } => None,
            })
            .filter_map(|id| {
                let (directory, fragment) = id.strip_prefix("path+file://")?.split_once('#')?;
                let name = match fragment.split_once('@') {
                    Some((name, _)) => name,
                    None => directory.rsplit('/').next()?,
                };
                Some((name.to_owned(), directory.to_owned()))
            })
            .collect::<Vec<_>>();
        packages.sort_unstable();
        packages.dedup();
        packages
    }

    /// The path packages a runner's sources may name: each of
    /// [`Self::path_packages`] but the harness's own, whose manifest and
    /// lock the runner is made from and which no runner depends on. Its
    /// directory is new for each harness a save seals, and a launch's proof
    /// names no such package, so with it each save's runner sources were new;
    /// without it a harness of the same manifest and lock renders the same
    /// sources (#1821).
    pub(crate) fn runner_path_packages(&self) -> Vec<(String, String)> {
        let harness = self.harness_directory();
        let mut packages = self.path_packages();
        packages.retain(|(_, directory)| Some(Path::new(directory)) != harness);
        packages
    }

    /// The directory of the harness's manifest.
    fn harness_directory(&self) -> Option<&Path> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::File {
                role: Role::Harness,
                path,
                ..
            } => {
                let path = Path::new(path.as_ref());
                (path.file_name() == Some("Cargo.toml".as_ref()))
                    .then(|| path.parent())
                    .flatten()
            }
            _ => None,
        })
    }

    /// The manifest of each path package the harness built but its own, as
    /// the proof recorded it. Under the runner's root each is a member of
    /// its workspace (`runner::mirror`), and Cargo resolves a member's
    /// lockfile entries with every optional and target-specific dependency
    /// its manifest names.
    pub(crate) fn runner_path_manifests(&self, limit: usize) -> Result<Vec<Box<[u8]>>> {
        let harness = self.harness_directory();
        let mut manifests = Vec::new();
        for entry in &self.entries {
            let Entry::File {
                role: Role::PackageManifest,
                package_id: Some(package_id),
                path,
                bytes,
                digest,
            } = entry
            else {
                continue;
            };
            let path = Path::new(path.as_ref());
            if !package_id.starts_with("path+file://") || path.parent() == harness {
                continue;
            }
            let retained = super::read_file("runner path package manifest", path, limit)?;
            if retained.len() != *bytes || Sha256::digest(&retained).as_slice() != digest {
                return Err(Error::ResolutionDrift {
                    command: "runner generation",
                    field: "path package manifest",
                });
            }
            manifests.push(retained.into_boxed_slice());
        }
        Ok(manifests)
    }

    /// What [`Self::runner_harness`] reads, by content: the harness's
    /// manifest's and lock's names, lengths and digests, which the bytes it
    /// reads must match. Where the harness is doesn't enter, so a new
    /// harness of the same manifest and lock digests the same (#1821).
    pub(crate) fn runner_harness_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.runner-harness/v1");
        for entry in &self.entries {
            let Entry::File {
                role: Role::Harness,
                path,
                bytes,
                digest: file,
                ..
            } = entry
            else {
                continue;
            };
            let name = match Path::new(path.as_ref())
                .file_name()
                .and_then(|name| name.to_str())
            {
                Some(name @ ("Cargo.toml" | "Cargo.lock")) => name,
                _ => continue,
            };
            digest.update((name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update((*bytes as u64).to_le_bytes());
            digest.update(file);
        }
        digest.finalize().into()
    }

    pub(crate) fn runner_harness(&self, limit: usize) -> Result<Harness> {
        let mut manifest = None;
        let mut lock = None;
        for entry in &self.entries {
            let Entry::File {
                role: Role::Harness,
                path,
                bytes,
                digest,
                ..
            } = entry
            else {
                continue;
            };
            let path = Path::new(path.as_ref());
            let target = match path.file_name().and_then(|name| name.to_str()) {
                Some("Cargo.toml") => &mut manifest,
                Some("Cargo.lock") => &mut lock,
                _ => continue,
            };
            if target.is_some() {
                return Err(Error::Input {
                    code: "runner-harness-cardinality",
                    reason: format!("duplicate harness file `{}`", path.display()).into_boxed_str(),
                });
            }
            let retained = super::read_file("runner harness file", path, limit)?;
            if retained.len() != *bytes || Sha256::digest(&retained).as_slice() != digest {
                return Err(Error::ResolutionDrift {
                    command: "runner generation",
                    field: "final-form harness file",
                });
            }
            *target = Some(retained.into_boxed_slice());
        }
        Ok(Harness {
            manifest: manifest.ok_or_else(|| Error::Input {
                code: "runner-harness-cardinality",
                reason: "final-form Cargo.toml is absent".into(),
            })?,
            lock: lock.ok_or_else(|| Error::Input {
                code: "runner-harness-cardinality",
                reason: "final-form Cargo.lock is absent".into(),
            })?,
        })
    }
}

pub(super) fn protected_digest_accounted(
    proof: &Proof,
    evidence: &super::build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    allocation: &mut Allocation,
) -> Result<cargo::universe::BuildProjection> {
    protected::project::legacy_accounted(&proof.entries, evidence, protected, target, allocation)
}

#[cfg(test)]
pub(super) fn protected_digest(
    proof: &Proof,
    evidence: &super::build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    allocation: &mut Allocation,
) -> Result<cargo::universe::BuildProjection> {
    protected_entries(&proof.entries, evidence, protected, target, allocation)
}

#[cfg(test)]
pub(super) fn protected_entries(
    entries: &[Entry],
    evidence: &super::build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    allocation: &mut Allocation,
) -> Result<cargo::universe::BuildProjection> {
    protected::project::legacy(entries, evidence, protected, target, allocation)
}

impl Harness {
    pub(crate) fn manifest(&self) -> &[u8] {
        &self.manifest
    }

    pub(crate) fn lock(&self) -> &[u8] {
        &self.lock
    }
}

pub(super) fn evaluate(
    recipes: &[Recipe],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Proof> {
    evaluate_known(recipes, BTreeMap::new(), limits, allocation)
}

/// [`evaluate`], taking the entry `known` holds for a recipe at its
/// position instead of observing that recipe again: a capture that parsed
/// a dep-info file for its own recipes gives its entry (#1846).
pub(super) fn evaluate_known(
    recipes: &[Recipe],
    mut known: BTreeMap<usize, Entry>,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Proof> {
    let mut entries = Vec::new();
    for (index, recipe) in recipes.iter().enumerate() {
        allocation.items(1)?;
        entries.try_reserve(1).map_err(|error| Error::Input {
            code: "input-proof-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
        let entry = match known.remove(&index) {
            Some(entry) => entry,
            None => entry(recipe, limits, allocation)?,
        };
        entries.push(entry);
    }
    seal(entries, allocation)
}

/// The proof of exactly `entries`, each once and in order: what
/// [`evaluate`] gives the recipes that observed them.
pub(super) fn seal(mut entries: Vec<Entry>, allocation: &mut Allocation) -> Result<Proof> {
    entries.sort_unstable();
    entries.dedup();
    let fingerprint = digest(&entries, allocation)?;
    Ok(Proof {
        schema: SCHEMA,
        entries: entries.into_boxed_slice(),
        fingerprint,
        native: None,
    })
}

/// The fingerprint a proof of exactly `entries` has: each entry once, in
/// order, as [`evaluate`] seals them. A caller that already holds each
/// entry's evaluation folds it here without observing any input again.
pub(super) fn fingerprint(
    entries: &mut Vec<&Entry>,
    allocation: &mut Allocation,
) -> Result<Fingerprint> {
    entries.sort_unstable();
    entries.dedup();
    digest(entries, allocation)
}

fn digest(entries: &[impl Serialize], allocation: &mut Allocation) -> Result<Fingerprint> {
    let mut canonical = Hashing {
        hasher: Sha256::new(),
        bytes: 0,
        overflow: false,
    };
    let result = serde_json::to_writer(&mut canonical, &(SCHEMA, entries));
    if canonical.overflow {
        return Err(allocation.overflow("input proof canonical bytes"));
    }
    result.map_err(|error| Error::Input {
        code: "input-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(Fingerprint::from_bytes(canonical.hasher.finalize().into()))
}

/// What one recipe observes now: the entry a proof of it holds.
pub(super) fn entry(
    recipe: &Recipe,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Entry> {
    Ok(match recipe.view() {
        View::Workspace { roots } => {
            let Recipe::Workspace { .. } = recipe else {
                return Err(super::input(
                    "workspace-recipe",
                    "missing original graph account",
                ));
            };
            super::workspace::observe(
                roots,
                &mut super::workspace::Account::borrow(allocation, limits),
            )?
        }
        View::File {
            role,
            package_id,
            path,
        } => file_entry(role, package_id, path, allocation)?,
        View::Path {
            role,
            package_id,
            path,
        } => path_entry(role, package_id, path, allocation, limits)?,
        View::Directory {
            role,
            package_id,
            path,
        } => directory_entry(role, package_id, path, allocation, limits)?,
        View::DepInfo {
            package_id,
            path,
            package_root,
            out_directories,
        } => super::replay::dep_info_entry(
            package_id,
            path,
            package_root,
            out_directories,
            allocation,
        )?,
        View::SavedOutput {
            package_id,
            out_dir,
        } => super::replay::saved_output_entry(package_id, out_dir, allocation)?,
        View::Package(set) => set.entry(allocation, limits)?,
        View::Environment {
            package_id,
            execution,
            key,
            value,
        } => {
            allocation.string(package_id)?;
            allocation.string(execution)?;
            allocation.string(key)?;
            if let Some(value) = value {
                allocation.string(value)?;
            }
            Entry::Environment {
                package_id: package_id.into(),
                execution: execution.into(),
                key: key.into(),
                value: value.map(Box::from),
            }
        }
        View::EnvironmentProjection { schema, key, value } => {
            allocation.string(key)?;
            if let Some(value) = value {
                allocation.string(value)?;
            }
            Entry::EnvironmentProjection {
                schema,
                key: key.into(),
                value: value.map(Box::from),
            }
        }
        View::Fact {
            role,
            package_id,
            name,
            value,
        } => {
            if let Some(package_id) = package_id {
                allocation.string(package_id)?;
            }
            allocation.string(name)?;
            allocation.string(value)?;
            Entry::Fact {
                role,
                package_id: package_id.map(Box::from),
                name: name.into(),
                value: value.into(),
            }
        }
        View::BuildMessage {
            package_id,
            execution,
            name,
            value,
        } => {
            allocation.string(package_id)?;
            allocation.string(execution)?;
            allocation.string(name)?;
            allocation.string(value)?;
            Entry::BuildMessage {
                package_id: package_id.into(),
                execution: execution.into(),
                name: name.into(),
                source: None,
                value: value.into(),
            }
        }
        View::Directive {
            package_id,
            execution,
            index,
            value,
        } => {
            allocation.string(package_id)?;
            allocation.string(execution)?;
            allocation.string(value)?;
            Entry::Directive {
                package_id: package_id.into(),
                execution: execution.into(),
                index,
                value: value.into(),
            }
        }
    })
}

fn file_entry(
    role: Role,
    package_id: Option<&str>,
    path: &Path,
    allocation: &mut Allocation,
) -> Result<Entry> {
    allocation.path(path)?;
    // A config candidate keeps the path Cargo reads; Cargo follows a symlinked
    // `.cargo` directory or config file, and its content is read through it.
    let path = if matches!(role, Role::Config) {
        path.to_path_buf()
    } else {
        super::canonical_file("freshness file", path)?
    };
    let metadata = fs::metadata(&path).map_err(|source| Error::Io {
        operation: "inspect freshness file",
        path: path.clone(),
        source,
    })?;
    let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    allocation.bytes(bytes)?;
    let digest = super::freshness_digest("freshness file", &path, bytes)?;
    allocation.items(1)?;
    allocation.bytes(path.as_os_str().len())?;
    if let Some(package_id) = package_id {
        allocation.string(package_id)?;
    }
    let text = super::path_text("freshness file", &path)?;
    Ok(Entry::File {
        role,
        package_id: package_id.map(Box::from),
        path: text.into(),
        bytes,
        digest,
    })
}

fn path_entry(
    role: Role,
    package_id: Option<&str>,
    path: &Path,
    allocation: &mut Allocation,
    limits: &cargo::Limits,
) -> Result<Entry> {
    allocation.path(path)?;
    let normalized = super::normalized_absolute(path)?;
    match fs::metadata(&normalized) {
        Ok(metadata) if metadata.is_file() => file_entry(role, package_id, &normalized, allocation),
        Ok(metadata) if metadata.is_dir() => {
            directory_entry(role, package_id, &normalized, allocation, limits)
        }
        Ok(_) => Err(super::input(
            "unsupported-input-shape",
            normalized.display(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            allocation.items(1)?;
            allocation.bytes(normalized.as_os_str().len())?;
            if let Some(package_id) = package_id {
                allocation.string(package_id)?;
            }
            let text = super::path_text("missing freshness path", &normalized)?;
            Ok(Entry::Missing {
                role,
                package_id: package_id.map(Box::from),
                path: text.into(),
            })
        }
        Err(source) => Err(Error::Io {
            operation: "inspect freshness path",
            path: normalized,
            source,
        }),
    }
}

pub(super) fn directory_entry(
    role: Role,
    package_id: Option<&str>,
    path: &Path,
    allocation: &mut Allocation,
    limits: &cargo::Limits,
) -> Result<Entry> {
    allocation.path(path)?;
    let tree = role == Role::DefaultPackageSource;
    let root = super::canonical_directory("freshness directory", path)?;
    allocation.items(1)?;
    let mut pending = Vec::new();
    allocation.items(1)?;
    allocation.path(&root)?;
    reserve(&mut pending)?;
    pending.push((
        root.clone(),
        PathBuf::new(),
        0_usize,
        Ancestors::root(root.clone()),
    ));
    let mut directories = Vec::new();
    let mut files = Vec::new();
    while let Some((directory, prefix, depth, ancestors)) = pending.pop() {
        super::limited("input directory depth", limits.depth, depth)?;
        let directory_entries = fs::read_dir(&directory).map_err(|source| Error::Io {
            operation: "read freshness directory",
            path: directory.clone(),
            source,
        })?;
        let mut children = Vec::new();
        for child in directory_entries {
            let child = child.map_err(|source| Error::Io {
                operation: "read freshness directory entry",
                path: directory.clone(),
                source,
            })?;
            allocation.items(1)?;
            reserve(&mut children)?;
            children.push(child);
        }
        children.sort_unstable_by_key(std::fs::DirEntry::file_name);
        for child in children.into_iter().rev() {
            let child_path = child.path();
            let file_type = child.file_type().map_err(|source| Error::Io {
                operation: "inspect freshness directory entry type",
                path: child_path.clone(),
                source,
            })?;
            // Cargo follows a link in the tree and reads what it resolves to;
            // one that resolves to nothing, or back to a directory the walk
            // is inside, has nothing to read.
            let followed = file_type.is_symlink();
            let (child_path, metadata) = if followed {
                let target = match fs::canonicalize(&child_path) {
                    Ok(target) => target,
                    Err(error) if walk::dangling(&error) => continue,
                    Err(source) => {
                        return Err(Error::Io {
                            operation: "resolve freshness directory link",
                            path: child_path,
                            source,
                        });
                    }
                };
                let metadata = fs::metadata(&target).map_err(|source| Error::Io {
                    operation: "inspect freshness directory link target",
                    path: target.clone(),
                    source,
                })?;
                (target, metadata)
            } else {
                let metadata = child.metadata().map_err(|source| Error::Io {
                    operation: "inspect freshness directory entry",
                    path: child_path.clone(),
                    source,
                })?;
                (child_path, metadata)
            };
            if followed && metadata.is_dir() && ancestors.encloses(&directory, &child_path) {
                continue;
            }
            if tree {
                let omitted =
                    walk::package_omits(&child.file_name(), depth, metadata.is_dir(), || {
                        allocation.items(1)?;
                        allocation.path(&child_path)?;
                        Ok::<_, Error>(child_path.join("Cargo.toml").exists())
                    })?;
                // Cargo's listing skips what is neither a file nor a
                // directory, and so does a package's walk.
                if omitted || !metadata.is_dir() && !metadata.is_file() {
                    continue;
                }
            }
            allocation.items(1)?;
            allocation.bytes(
                prefix
                    .as_os_str()
                    .len()
                    .saturating_add(1)
                    .saturating_add(child.file_name().len()),
            )?;
            let spelled = prefix.join(child.file_name());
            let relative: Box<str> = super::path_text("freshness tree entry", &spelled)?.into();
            if metadata.is_dir() {
                let child_depth = depth.saturating_add(1);
                super::limited("input directory depth", limits.depth, child_depth)?;
                allocation.items(1)?;
                allocation.path(&child_path)?;
                let ancestors = if followed {
                    allocation.items(1)?;
                    allocation.path(&directory)?;
                    allocation.path(&child_path)?;
                    ancestors.follow(&directory, child_path.clone())
                } else {
                    Rc::clone(&ancestors)
                };
                reserve(&mut pending)?;
                pending.push((child_path, spelled, child_depth, ancestors));
                allocation.items(1)?;
                reserve(&mut directories)?;
                directories.push(TreeDirectory { path: relative });
            } else if metadata.is_file() {
                let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                allocation.bytes(bytes)?;
                let digest = super::freshness_digest("freshness tree file", &child_path, bytes)?;
                allocation.items(1)?;
                reserve(&mut files)?;
                files.push(TreeFile {
                    path: relative,
                    bytes,
                    digest,
                });
            } else {
                return Err(super::input(
                    "unsupported-input-shape",
                    child_path.display(),
                ));
            }
        }
    }
    directories.sort_unstable();
    files.sort_unstable();
    allocation.items(1)?;
    allocation.bytes(root.as_os_str().len())?;
    if let Some(package_id) = package_id {
        allocation.string(package_id)?;
    }
    Ok(Entry::Directory {
        role,
        package_id: package_id.map(Box::from),
        path: super::path_text("freshness directory", &root)?.into(),
        directories: directories.into_boxed_slice(),
        files: files.into_boxed_slice(),
    })
}

fn reserve<T>(values: &mut Vec<T>) -> Result<()> {
    values.try_reserve(1).map_err(|error| Error::Input {
        code: "input-proof-allocation",
        reason: error.to_string().into_boxed_str(),
    })
}

impl Write for Canonical {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().checked_add(bytes.len()).is_none() {
            return Err(io::Error::other("canonical projection byte count overflow"));
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Hashing {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(actual) = self.bytes.checked_add(bytes.len()) else {
            self.overflow = true;
            return Err(io::Error::other(
                "input proof canonical byte count overflow",
            ));
        };
        self.hasher.update(bytes);
        self.bytes = actual;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        evaluate, protected_digest, Entry, Proof, ProtectedEntry, Recipe, Role, Watch, SCHEMA,
    };
    use crate::cargo::unit::ArtifactProfile;
    use crate::cargo::{self, Limits};
    use crate::input::build::{Artifact, Evidence, Key, Package, Script};
    use crate::input::fingerprint::Fingerprint;
    use crate::input::{Allocation, Usage};
    use crate::Error;

    const TARGET: &str = "aarch64-apple-darwin";

    fn artifact_profile() -> ArtifactProfile {
        ArtifactProfile::from_build(&serde_json::json!({
            "opt_level": "0",
            "debuginfo": 2,
            "debug_assertions": true,
            "overflow_checks": true,
            "test": false,
        }))
        .expect("fixture artifact profile is valid")
    }

    #[allow(clippy::too_many_arguments)]
    fn artifact(
        package_id: &str,
        root: &Path,
        dep_info: &Path,
        target: &str,
        target_kind: &str,
        crate_type: &str,
        features: &[&str],
    ) -> Artifact {
        Artifact {
            package_id: package_id.into(),
            outputs: Box::default(),
            dep_info: dep_info.to_path_buf(),
            key: Key {
                package_id: package_id.into(),
                target: root.join(target),
                target_kinds: Box::from([Box::from(target_kind)]),
                crate_types: Box::from([Box::from(crate_type)]),
                crate_name: "rawr_iced".into(),
                features: features
                    .iter()
                    .map(|feature| Box::<str>::from(*feature))
                    .collect(),
            },
            profile: artifact_profile(),
            fresh: false,
        }
    }

    fn package(root: &Path, build_script: bool) -> Package {
        Package {
            manifest: root.join("Cargo.toml"),
            root: root.to_path_buf(),
            build_script: build_script.then(|| root.join("build.rs")),
        }
    }

    fn package_id(root: &Path, name: &str) -> String {
        format!("path+file://{}#{name}@0.1.0", root.display())
    }

    fn projection(
        root: &Path,
        dep_info: &Path,
        target: &str,
        target_kind: &str,
        crate_type: &str,
        features: &[&str],
        artifact_count: usize,
    ) -> crate::Result<cargo::universe::BuildProjection> {
        let limits = Limits::standard();
        let package_id = format!("path+file://{}#rawr-iced@0.1.0", root.display());
        let out_dir = dep_info
            .parent()
            .unwrap_or(dep_info)
            .join("build/rawr-iced/out");
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::Environment {
                package_id: package_id.clone().into_boxed_str(),
                execution: dep_info.to_string_lossy().into_owned().into_boxed_str(),
                key: "OUT_DIR".into(),
                value: Some(out_dir.to_string_lossy().into_owned().into_boxed_str()),
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let mut artifacts = Vec::new();
        for _ in 0..artifact_count {
            artifacts.push(artifact(
                &package_id,
                root,
                dep_info,
                target,
                target_kind,
                crate_type,
                features,
            ));
        }
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [(
                package_id.clone().into(),
                Package {
                    manifest: root.join("Cargo.toml"),
                    root: root.to_path_buf(),
                    build_script: None,
                },
            )]
            .into(),
            scripts: Box::from([Script {
                compiler: None,
                package_id: package_id.clone().into_boxed_str(),
                out_dir,
                cfg: Box::default(),
                environment: Box::default(),
            }]),
            artifacts: artifacts.into_boxed_slice(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &package_id,
            "rawr-iced",
            "0.1.0",
            "",
            root.to_path_buf(),
        );
        protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
    }

    fn cross_package_projection(
        base: &Path,
        consumer_name: &str,
        owner_name: &str,
        digest: [u8; 32],
    ) -> (cargo::universe::BuildProjection, String, String) {
        let limits = Limits::standard();
        let consumer_root = base.join(consumer_name);
        let owner_root = base.join(owner_name);
        let consumer_id = package_id(&consumer_root, consumer_name);
        let owner_id = package_id(&owner_root, owner_name);
        let path = owner_root.join("src/profile/resolution/august-29-v17.lock");
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::DependencySource,
                package_id: Some(consumer_id.clone().into_boxed_str()),
                path: path.to_string_lossy().into_owned().into_boxed_str(),
                bytes: 85_344,
                digest,
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [
                (
                    consumer_id.clone().into_boxed_str(),
                    package(&consumer_root, false),
                ),
                (
                    owner_id.clone().into_boxed_str(),
                    package(&owner_root, false),
                ),
            ]
            .into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &consumer_id,
            consumer_name,
            "0.1.0",
            "",
            consumer_root,
        )
        .with_fixture_package(&owner_id, owner_name, "0.1.0", "", owner_root);
        let consumer = protected
            .package_token(&consumer_id)
            .expect("the protected consumer has a token");
        let owner = protected
            .package_token(&owner_id)
            .expect("the protected source owner has a token");
        let projection = protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
        .expect("the protected cross-package source edge seals");
        (projection, consumer, owner)
    }

    fn build_script_projection(
        root: &Path,
        placement: &str,
        features: &[&str],
        artifact_count: usize,
    ) -> crate::Result<cargo::universe::BuildProjection> {
        let limits = Limits::standard();
        let package_id = package_id(root, "rawr-import");
        let target_root = PathBuf::from(format!("/private/tmp/{placement}/target/debug"));
        let dep_info = target_root
            .join("deps")
            .join(format!("build_script_build-{placement}.d"));
        let out_dir = target_root
            .join("build")
            .join(format!("rawr-import-{placement}"))
            .join("out");
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([
                Entry::Directory {
                    role: Role::OutDirectory,
                    package_id: Some(package_id.clone().into_boxed_str()),
                    path: out_dir.to_string_lossy().into_owned().into_boxed_str(),
                    directories: Box::default(),
                    files: Box::default(),
                },
                Entry::BuildMessage {
                    package_id: package_id.clone().into_boxed_str(),
                    execution: out_dir.to_string_lossy().into_owned().into_boxed_str(),
                    name: "saved-output".into(),
                    source: Some(
                        out_dir
                            .parent()
                            .unwrap_or(&out_dir)
                            .join("output")
                            .to_string_lossy()
                            .into_owned()
                            .into_boxed_str(),
                    ),
                    value: {
                        use sha2::{Digest as _, Sha256};
                        format!("{:x}", Sha256::digest(br#"{"directives":["rerun-if-changed=build.rs"],"paths":["build.rs"],"environment":[]}"#)).into()
                    },
                },
                Entry::BuildMessage {
                    package_id: package_id.clone().into_boxed_str(),
                    execution: out_dir.to_string_lossy().into_owned().into_boxed_str(),
                    name: "cfg".into(),
                    source: None,
                    value: "feature=\"same\"".into(),
                },
                Entry::Directive {
                    package_id: package_id.clone().into_boxed_str(),
                    execution: out_dir.to_string_lossy().into_owned().into_boxed_str(),
                    index: 0,
                    value: "rerun-if-changed=build.rs".into(),
                },
            ]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let artifacts = (0..artifact_count)
            .map(|_| {
                let mut artifact = artifact(
                    &package_id,
                    root,
                    &dep_info,
                    "build.rs",
                    "custom-build",
                    "bin",
                    features,
                );
                artifact.key.crate_name = "build_script_build".into();
                artifact
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [(package_id.clone().into_boxed_str(), package(root, true))].into(),
            scripts: Box::from([Script {
                compiler: (artifact_count == 1).then_some((0, false)),
                package_id: package_id.clone().into_boxed_str(),
                out_dir,
                cfg: Box::default(),
                environment: Box::default(),
            }]),
            artifacts,
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &package_id,
            "rawr-import",
            "0.1.0",
            "",
            root.to_path_buf(),
        );
        protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
    }

    #[test]
    fn protected_cross_package_source_binds_both_endpoints_and_is_root_independent() {
        let (left, consumer, owner) = cross_package_projection(
            Path::new("/private/tmp/left"),
            "rawr-import",
            "rawr-iced",
            [7; 32],
        );
        let (right, right_consumer, right_owner) = cross_package_projection(
            Path::new("/private/tmp/right"),
            "rawr-import",
            "rawr-iced",
            [7; 32],
        );
        assert_eq!(consumer, right_consumer);
        assert_eq!(owner, right_owner);
        assert_eq!(
            left, right,
            "absolute package placement is not shared meaning"
        );

        let (_, entries): (u32, Box<[ProtectedEntry]>) =
            serde_json::from_slice(left.bytes()).expect("the projection decodes");
        let [ProtectedEntry::File {
            role: Role::DependencySource,
            source,
            bytes: 85_344,
            digest,
        }] = entries.as_ref()
        else {
            panic!("one cross-package dependency-source edge is retained");
        };
        assert_eq!(source.consumer.as_ref(), consumer);
        assert_eq!(source.owner.as_ref(), owner);
        assert_ne!(source.consumer, source.owner);
        assert_eq!(
            source.path.as_ref(),
            format!("$PACKAGE/{owner}/src/profile/resolution/august-29-v17.lock")
        );
        assert_eq!(*digest, [7; 32]);

        let (changed_consumer, _, _) = cross_package_projection(
            Path::new("/private/tmp/right"),
            "rawr-api",
            "rawr-iced",
            [7; 32],
        );
        let (changed_owner, _, _) = cross_package_projection(
            Path::new("/private/tmp/right"),
            "rawr-import",
            "rawr-profile-assets",
            [7; 32],
        );
        let (changed_bytes, _, _) = cross_package_projection(
            Path::new("/private/tmp/right"),
            "rawr-import",
            "rawr-iced",
            [8; 32],
        );
        assert_ne!(left, changed_consumer);
        assert_ne!(left, changed_owner);
        assert_ne!(left, changed_bytes);
    }

    #[test]
    fn cross_package_edges_do_not_collapse_and_structural_roles_remain_self_owned() {
        let limits = Limits::standard();
        let first_root = PathBuf::from("/fixture/first");
        let second_root = PathBuf::from("/fixture/second");
        let owner_root = PathBuf::from("/fixture/owner");
        let first_id = package_id(&first_root, "rawr-first");
        let second_id = package_id(&second_root, "rawr-second");
        let owner_id = package_id(&owner_root, "rawr-owner");
        let source = owner_root.join("shared/input.rs");
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [
                (
                    first_id.clone().into_boxed_str(),
                    package(&first_root, false),
                ),
                (
                    second_id.clone().into_boxed_str(),
                    package(&second_root, false),
                ),
                (
                    owner_id.clone().into_boxed_str(),
                    package(&owner_root, false),
                ),
            ]
            .into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &first_id,
            "rawr-first",
            "0.1.0",
            "",
            first_root,
        )
        .with_fixture_package(&second_id, "rawr-second", "0.1.0", "", second_root)
        .with_fixture_package(&owner_id, "rawr-owner", "0.1.0", "", owner_root.clone());
        let entry = |package_id: &str, role| Entry::File {
            role,
            package_id: Some(package_id.into()),
            path: source.to_string_lossy().into_owned().into_boxed_str(),
            bytes: 4,
            digest: [9; 32],
        };
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([
                entry(&first_id, Role::DependencySource),
                entry(&second_id, Role::DependencySource),
            ]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let projection = protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
        .expect("two protected consumers retain two source-use edges");
        let (_, entries): (u32, Box<[ProtectedEntry]>) =
            serde_json::from_slice(projection.bytes()).expect("the projection decodes");
        assert_eq!(entries.len(), 2);
        let mut consumers = entries
            .iter()
            .map(|entry| match entry {
                ProtectedEntry::File { source, .. } => source.consumer.as_ref(),
                _ => panic!("only protected file edges are retained"),
            })
            .collect::<Vec<_>>();
        consumers.sort_unstable();
        let mut expected_consumers = [
            protected
                .package_token(&first_id)
                .expect("first consumer has a token"),
            protected
                .package_token(&second_id)
                .expect("second consumer has a token"),
        ];
        expected_consumers.sort_unstable();
        assert_eq!(consumers, expected_consumers);

        let mut structural_packages = evidence.packages.clone();
        structural_packages
            .get_mut(first_id.as_str())
            .expect("the first package exists")
            .manifest = owner_root.join("Cargo.toml");
        let structural = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: structural_packages,
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let manifest = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::PackageManifest,
                package_id: Some(first_id.into_boxed_str()),
                path: owner_root
                    .join("Cargo.toml")
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
                bytes: 4,
                digest: [9; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        assert!(matches!(
            protected_digest(
                &manifest,
                &structural,
                &protected,
                TARGET,
                &mut Allocation::new(&limits),
            ),
            Err(Error::Input {
                code: "protected-build-package-owner-conflict",
                ..
            })
        ));
    }

    #[test]
    fn causal_package_ownership_excludes_a_nested_unprotected_project() {
        let limits = Limits::standard();
        let protected_root = Path::new("/fixture/rawr");
        let project_root = protected_root.join("target/rawr/cache/resolution/project");
        let protected_id = package_id(protected_root, "rawr");
        let project_id = package_id(&project_root, "rawr-generated-import");
        let manifest = project_root.join("Cargo.toml");
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [
                (
                    protected_id.clone().into_boxed_str(),
                    package(protected_root, false),
                ),
                (
                    project_id.clone().into_boxed_str(),
                    package(&project_root, false),
                ),
            ]
            .into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &protected_id,
            "rawr",
            "0.1.0",
            "",
            protected_root.to_path_buf(),
        );
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::PackageManifest,
                package_id: Some(project_id.clone().into_boxed_str()),
                path: manifest.to_string_lossy().into_owned().into_boxed_str(),
                bytes: 17,
                digest: [7; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let projection = protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
        .expect("an authenticated unprotected package is excluded");
        assert_eq!(projection.bytes(), b"[4,[]]\n");

        let unprotected_consumer = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::DependencySource,
                package_id: Some(project_id.into_boxed_str()),
                path: protected_root
                    .join("src/lib.rs")
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
                bytes: 17,
                digest: [7; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let projection = protected_digest(
            &unprotected_consumer,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
        .expect("an unprotected consumer acquires no shared authority");
        assert_eq!(projection.bytes(), b"[4,[]]\n");

        let conflict = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::DependencySource,
                package_id: Some(protected_id.into_boxed_str()),
                path: manifest.to_string_lossy().into_owned().into_boxed_str(),
                bytes: 17,
                digest: [7; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        assert!(matches!(
            protected_digest(
                &conflict,
                &evidence,
                &protected,
                TARGET,
                &mut Allocation::new(&limits),
            ),
            Err(Error::Input {
                code: "protected-build-source-owner-unprotected",
                ..
            })
        ));
    }

    #[test]
    fn protected_child_ownership_is_distinct_and_ambiguous_roots_reject() {
        let limits = Limits::standard();
        let outer_root = Path::new("/fixture/rawr");
        let child_root = outer_root.join("importer");
        let outer_id = package_id(outer_root, "rawr");
        let child_id = package_id(&child_root, "rawr-import");
        let source = child_root.join("src/lib.rs");
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [
                (
                    outer_id.clone().into_boxed_str(),
                    package(outer_root, false),
                ),
                (
                    child_id.clone().into_boxed_str(),
                    package(&child_root, false),
                ),
            ]
            .into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &outer_id,
            "rawr",
            "0.1.0",
            "",
            outer_root.to_path_buf(),
        )
        .with_fixture_package(&child_id, "rawr-import", "0.1.0", "", child_root.clone());
        let child_token = protected
            .package_token(&child_id)
            .expect("protected child has a token");
        let outer_token = protected
            .package_token(&outer_id)
            .expect("protected outer package has a token");
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::DependencySource,
                package_id: Some(child_id.into_boxed_str()),
                path: source.to_string_lossy().into_owned().into_boxed_str(),
                bytes: 3,
                digest: [4; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let projection = protected_digest(
            &proof,
            &evidence,
            &protected,
            TARGET,
            &mut Allocation::new(&limits),
        )
        .expect("the causal protected child owns its source");
        let text = std::str::from_utf8(projection.bytes()).expect("projection is UTF-8 JSON");
        assert!(text.contains(&child_token));
        assert!(!text.contains(&outer_token));

        let alias_id = package_id(&child_root, "rawr-import-alias");
        let mut ambiguous_packages = evidence.packages.clone();
        ambiguous_packages.insert(alias_id.into_boxed_str(), package(&child_root, false));
        let ambiguous = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: ambiguous_packages,
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        assert!(matches!(
            protected_digest(
                &proof,
                &ambiguous,
                &protected,
                TARGET,
                &mut Allocation::new(&limits),
            ),
            Err(Error::Input {
                code: "protected-build-package-owner-ambiguous",
                ..
            })
        ));
    }

    #[test]
    fn protected_external_declared_input_without_stable_owner_rejects() {
        let limits = Limits::standard();
        let root = Path::new("/fixture/importer");
        let package_id = package_id(root, "rawr-import");
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [(package_id.clone().into_boxed_str(), package(root, true))].into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            &package_id,
            "rawr-import",
            "0.1.0",
            "",
            root.to_path_buf(),
        );
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::Missing {
                role: Role::DeclaredInput,
                package_id: Some(package_id.into_boxed_str()),
                path: "/external/generated/config.rs".into(),
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        assert!(matches!(
            protected_digest(
                &proof,
                &evidence,
                &protected,
                TARGET,
                &mut Allocation::new(&limits),
            ),
            Err(Error::Input {
                code: "protected-build-external-input-unmapped",
                ..
            })
        ));

        let missing_owner = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::Missing {
                role: Role::DependencySource,
                package_id: None,
                path: root
                    .join("missing.rs")
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        assert!(matches!(
            protected_digest(
                &missing_owner,
                &evidence,
                &protected,
                TARGET,
                &mut Allocation::new(&limits),
            ),
            Err(Error::Input {
                code: "protected-build-package-owner-missing",
                ..
            })
        ));
    }

    #[test]
    fn build_script_fingerprint_paths_are_replaced_by_semantic_execution() {
        let left = build_script_projection(
            Path::new("/private/tmp/a/source/importer"),
            "aaaaaaaaaaaaaaaa",
            &["default"],
            1,
        )
        .expect("left build-script projection derives");
        let right = build_script_projection(
            Path::new("/private/tmp/b/source/importer"),
            "bbbbbbbbbbbbbbbb",
            &["default"],
            1,
        )
        .expect("right build-script projection derives");
        assert_eq!(left, right);

        let changed = build_script_projection(
            Path::new("/private/tmp/b/source/importer"),
            "bbbbbbbbbbbbbbbb",
            &["changed"],
            1,
        )
        .expect("semantic build-script drift derives");
        assert_ne!(left, changed);

        for (count, code) in [
            (0, "protected-build-script-artifact-unmapped"),
            (2, "protected-build-script-artifact-unmapped"),
        ] {
            assert!(matches!(
                build_script_projection(
                    Path::new("/private/tmp/a/source/importer"),
                    "aaaaaaaaaaaaaaaa",
                    &["default"],
                    count,
                ),
                Err(Error::Input { code: actual, .. }) if actual == code
            ));
        }
    }

    #[test]
    fn protected_projection_owns_one_canonical_record_and_charges_it_once() {
        let limits = Limits::standard();
        let root = Path::new("/fixture/importer");
        let package_id = "path+file:///fixture/importer#rawr-import@0.1.0";
        let proof = Proof {
            schema: SCHEMA,
            entries: Box::from([Entry::File {
                role: Role::DependencySource,
                package_id: Some(package_id.into()),
                path: "/fixture/importer/src/lib.rs".into(),
                bytes: 3,
                digest: [7; 32],
            }]),
            fingerprint: Fingerprint::from_bytes([0; 32]),
            native: None,
        };
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: [(
                package_id.into(),
                Package {
                    manifest: root.join("Cargo.toml"),
                    root: root.to_path_buf(),
                    build_script: None,
                },
            )]
            .into(),
            scripts: Box::default(),
            artifacts: Box::default(),
            environment: Box::default(),
        };
        let protected = cargo::universe::MetadataProof::fixture_package(
            package_id,
            "rawr-import",
            "0.1.0",
            "",
            root.to_path_buf(),
        );
        let mut allocation = Allocation::new(&limits);
        let projection = protected_digest(&proof, &evidence, &protected, TARGET, &mut allocation)
            .expect("protected projection seals once");
        assert!(projection.bytes().ends_with(b"\n"));
        assert_eq!(allocation.usage.items, 2);
        assert_eq!(allocation.usage.bytes, projection.bytes().len());
    }

    #[test]
    fn prior_projection_schema_cannot_match_current_proof_freshness() {
        assert_eq!(
            SCHEMA, 10,
            "workspace determinants advance the input-proof wire shape"
        );
        let limits = Limits::standard();
        let evaluate_projection = |schema| {
            let recipe = Recipe::EnvironmentProjection {
                schema,
                key: "rawr.context.input_proof_bytes".into(),
                value: Some("536870912".into()),
            };
            evaluate(
                &[recipe],
                &limits,
                &mut Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits),
            )
            .expect("the projection freshness fixture evaluates")
        };
        let current = evaluate_projection(9);
        let prior = evaluate_projection(8);
        assert_ne!(current.fingerprint(), prior.fingerprint());
    }

    #[test]
    fn declared_arbitrary_environment_changes_execution_freshness() {
        let recipe = |value: Option<&str>| Recipe::Environment {
            package_id: "path+file:///fixture#fixture@0.1.0".into(),
            execution: "/fixture/target/build/fixture/out".into(),
            key: "ARBITRARY_DECLARED_KEY".into(),
            value: value.map(Box::from),
        };
        let limits = Limits::standard();
        let unset = evaluate(&[recipe(None)], &limits, &mut Allocation::new(&limits))
            .expect("unset declared environment evaluates");
        let empty = evaluate(&[recipe(Some(""))], &limits, &mut Allocation::new(&limits))
            .expect("empty declared environment evaluates");
        let changed = evaluate(
            &[recipe(Some("changed"))],
            &limits,
            &mut Allocation::new(&limits),
        )
        .expect("changed declared environment evaluates");
        assert_ne!(unset.fingerprint(), empty.fingerprint());
        assert_ne!(empty.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn protected_environment_uses_semantic_artifact_not_target_placement() {
        let left_root = Path::new("/private/tmp/r3a-1/source/iced");
        let right_root = Path::new("/private/tmp/r3b-1/source/iced");
        let left_dep = Path::new(
            "/private/tmp/generated-a/authoritative-target/aarch64-apple-darwin/debug/deps/rawr_iced-aaaaaaaaaaaaaaaa.d",
        );
        let right_dep = Path::new(
            "/private/tmp/generated-b/authoritative-target/aarch64-apple-darwin/debug/deps/rawr_iced-bbbbbbbbbbbbbbbb.d",
        );
        let left = projection(
            left_root,
            left_dep,
            "src/lib.rs",
            "lib",
            "lib",
            &["advanced"],
            1,
        )
        .expect("left protected projection derives");
        let right = projection(
            right_root,
            right_dep,
            "src/lib.rs",
            "lib",
            "lib",
            &["advanced"],
            1,
        )
        .expect("right protected projection derives");
        assert_eq!(left, right);
    }

    #[test]
    fn protected_environment_semantic_artifact_drift_cannot_match() {
        let root = Path::new("/private/tmp/r3a-1/source/iced");
        let target_dep = Path::new(
            "/private/tmp/generated/authoritative-target/aarch64-apple-darwin/debug/deps/rawr_iced-a.d",
        );
        let host_dep =
            Path::new("/private/tmp/generated/authoritative-target/debug/deps/rawr_iced-b.d");
        let baseline = projection(
            root,
            target_dep,
            "src/lib.rs",
            "lib",
            "lib",
            &["advanced"],
            1,
        )
        .expect("baseline protected projection derives");
        for changed in [
            projection(
                root,
                target_dep,
                "src/other.rs",
                "lib",
                "lib",
                &["advanced"],
                1,
            ),
            projection(
                root,
                target_dep,
                "src/lib.rs",
                "lib",
                "lib",
                &["advanced", "changed"],
                1,
            ),
            projection(
                root,
                target_dep,
                "src/lib.rs",
                "custom-build",
                "bin",
                &["advanced"],
                1,
            ),
            projection(root, host_dep, "src/lib.rs", "lib", "lib", &["advanced"], 1),
        ] {
            assert_ne!(
                baseline,
                changed.expect("changed semantic projection derives")
            );
        }
    }

    #[test]
    fn protected_environment_rejects_unmapped_and_ambiguous_artifacts() {
        let root = PathBuf::from("/private/tmp/r3a-1/source/iced");
        let dep_info = Path::new(
            "/private/tmp/generated/authoritative-target/aarch64-apple-darwin/debug/deps/rawr_iced-a.d",
        );
        let missing = projection(
            &root,
            Path::new("/private/tmp/generated/unmapped.d"),
            "src/lib.rs",
            "lib",
            "lib",
            &[],
            0,
        );
        assert!(matches!(
            missing,
            Err(Error::Input {
                code: "protected-environment-artifact-unmapped",
                ..
            })
        ));
        let ambiguous = projection(&root, dep_info, "src/lib.rs", "lib", "lib", &[], 2);
        assert!(matches!(
            ambiguous,
            Err(Error::Input {
                code: "protected-environment-artifact-ambiguous",
                ..
            })
        ));
    }

    /// #520: a config file reached through a symlinked `.cargo` keeps the
    /// path Cargo reads, and the watcher also observes where the link points.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_config_keeps_its_path_and_watches_its_target() {
        let root = std::env::temp_dir().join(format!(
            "rawr-proof-config-link-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("shared")).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        std::fs::write(root.join("shared/config.toml"), b"[net]\n").unwrap();
        std::os::unix::fs::symlink(root.join("shared"), root.join(".cargo")).unwrap();
        let spelled = root.join(".cargo/config.toml");
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let recipe = Recipe::path(Role::Config, &spelled, &mut allocation).unwrap();
        let proof = evaluate(&[recipe], &limits, &mut allocation)
            .expect("Cargo follows a symlinked .cargo");
        assert!(matches!(
            &proof.entries[..],
            [Entry::File { role: Role::Config, path, bytes: 6, .. }]
                if path.as_ref() == spelled.to_str().unwrap()
        ));
        let watched = proof.watch().collect::<Vec<_>>();
        assert!(
            watched
                == [
                    super::Watch::Exact(spelled.clone()),
                    super::Watch::Exact(root.join("shared/config.toml")),
                ]
        );
        let recipe = Recipe::path(Role::DeclaredInput, &spelled, &mut allocation).unwrap();
        let proof = evaluate(&[recipe], &limits, &mut allocation)
            .expect("a declared input through a symlink is its target");
        let target = root.join("shared/config.toml");
        assert!(matches!(
            &proof.entries[..],
            [Entry::File { role: Role::DeclaredInput, path, bytes: 6, .. }]
                if path.as_ref() == target.to_str().unwrap()
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A build script's `rerun-if-changed` path reached through symlinks (an
    /// SDK path under `xcrun --show-sdk-path`'s link) is the file or directory
    /// the links resolve to, as Cargo reads it. Its entry names the target,
    /// and the recipe keeps the spelling, so retargeting a link changes the
    /// entry even to a file of the same bytes, and so does editing the
    /// target. A tree is walked through its links as Cargo walks it.
    #[cfg(unix)]
    #[test]
    fn a_declared_input_through_symlinks_is_its_target_and_a_retarget_changes_it() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "rawr-proof-declared-link-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("SDKs/MacOSX15.4.sdk/usr/include")).unwrap();
        std::fs::create_dir_all(root.join("SDKs/MacOSX26.2.sdk/usr/include")).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        for sdk in ["MacOSX15.4.sdk", "MacOSX26.2.sdk"] {
            std::fs::write(
                root.join("SDKs").join(sdk).join("usr/include/stdio.h"),
                b"int puts(const char *);\n",
            )
            .unwrap();
        }
        symlink("MacOSX15.4.sdk", root.join("SDKs/MacOSX15.sdk")).unwrap();
        let header = root.join("SDKs/MacOSX15.sdk/usr/include/stdio.h");
        let sdk = root.join("SDKs/MacOSX15.sdk");
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let observe = |path: &Path, allocation: &mut Allocation| {
            let recipe =
                Recipe::package_path(Role::DeclaredInput, "pkg", path, allocation).unwrap();
            let proof = evaluate(&[recipe], &limits, allocation)
                .expect("an input through a symlink is its target");
            proof.entries.into_vec().pop().unwrap()
        };
        let before = observe(&header, &mut allocation);
        let target = root.join("SDKs/MacOSX15.4.sdk/usr/include/stdio.h");
        assert!(matches!(
            &before,
            Entry::File { path, bytes: 24, .. } if path.as_ref() == target.to_str().unwrap()
        ));
        let Entry::Directory {
            path,
            directories,
            files,
            ..
        } = observe(&sdk, &mut allocation)
        else {
            panic!("a symlinked directory is a tree");
        };
        assert_eq!(
            path.as_ref(),
            root.join("SDKs/MacOSX15.4.sdk").to_str().unwrap()
        );
        assert_eq!(directories.len(), 2);
        assert!(matches!(&files[..], [file] if file.path.as_ref() == "usr/include/stdio.h"));

        // The link points elsewhere, at a file of the same bytes.
        std::fs::remove_file(root.join("SDKs/MacOSX15.sdk")).unwrap();
        symlink("MacOSX26.2.sdk", root.join("SDKs/MacOSX15.sdk")).unwrap();
        let retargeted = observe(&header, &mut allocation);
        assert!(retargeted != before, "a retargeted link is a changed input");
        // The target's content changes.
        std::fs::write(
            root.join("SDKs/MacOSX26.2.sdk/usr/include/stdio.h"),
            b"int puts(const char *s);\n",
        )
        .unwrap();
        assert!(observe(&header, &mut allocation) != retargeted);

        // Inside a tree, links are followed; one back to a directory the
        // walk is inside, and one naming nothing, have nothing to read.
        let tree = root.join("tree");
        std::fs::create_dir(&tree).unwrap();
        symlink(root.join("SDKs/MacOSX15.sdk"), tree.join("sdk")).unwrap();
        symlink(&header, tree.join("header.h")).unwrap();
        symlink(&tree, tree.join("loop")).unwrap();
        symlink(root.join("absent"), tree.join("dangling")).unwrap();
        let Entry::Directory {
            directories, files, ..
        } = observe(&tree, &mut allocation)
        else {
            panic!("a tree with links");
        };
        let directories = directories
            .iter()
            .map(|directory| directory.path.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(directories, ["sdk", "sdk/usr", "sdk/usr/include"]);
        let files = files
            .iter()
            .map(|file| file.path.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(files, ["header.h", "sdk/usr/include/stdio.h"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A restart names the native input that changed: an edited file, a
    /// new one, one gone, or a changed variable.
    #[test]
    fn a_changed_proof_names_the_input_that_changed() {
        let file = |path: &str, digest: u8| Entry::File {
            role: Role::DependencySource,
            package_id: None,
            path: path.into(),
            bytes: 1,
            digest: [digest; 32],
        };
        let variable = |value: &str| Entry::EnvironmentProjection {
            schema: 1,
            key: "CC".into(),
            value: Some(value.into()),
        };
        let proof = |entries: Vec<Entry>| Proof::fixture_stored(entries.into_boxed_slice());
        let before = proof(vec![file("/p/src/main.rs", 1), variable("cc")]);
        assert_eq!(before.change_from(&before, &[]), None);
        assert_eq!(
            proof(vec![file("/p/src/main.rs", 2), variable("cc")])
                .change_from(&before, &[])
                .as_deref(),
            Some("`/p/src/main.rs` changed")
        );
        assert_eq!(
            proof(vec![file("/p/src/main.rs", 1)])
                .change_from(&before, &[])
                .as_deref(),
            Some("the environment variable `CC` changed")
        );
        assert_eq!(
            proof(vec![
                file("/p/src/main.rs", 1),
                file("/p/src/new.rs", 1),
                variable("cc")
            ])
            .change_from(&before, &[])
            .as_deref(),
            Some("`/p/src/new.rs` changed")
        );
    }

    /// A restart never names what a build wrote (#1890): the generated
    /// runner's manifest and lock, which each build places in an execution
    /// of its own under Rawr's store, nor a file under a target directory.
    /// The native input that changed beside them is named, and they are
    /// not watched.
    #[test]
    fn a_changed_proof_names_no_file_a_build_wrote() {
        let file = |role: Role, path: &str, digest: u8| Entry::File {
            role,
            package_id: None,
            path: path.into(),
            bytes: 1,
            digest: [digest; 32],
        };
        let built = |execution: &str, source: u8| {
            let generated = format!("/p/target/rawr/store/executions/{execution}/generated/final");
            Proof::fixture_stored(Box::from([
                file(Role::Harness, &format!("{generated}/Cargo.toml"), 1),
                file(Role::Harness, &format!("{generated}/Cargo.lock"), 1),
                file(
                    Role::DefaultPackageSource,
                    &format!("{generated}/src/main.rs"),
                    1,
                ),
                file(Role::DependencySource, "/p/src/main.rs", source),
            ]))
        };
        let outputs = [PathBuf::from("/p/target")];
        let before = built("a", 1);
        assert_eq!(built("b", 1).change_from(&before, &outputs), None);
        assert_eq!(
            built("b", 2).change_from(&before, &outputs).as_deref(),
            Some("`/p/src/main.rs` changed")
        );
        let watched = before.watch().collect::<Vec<_>>();
        assert!(
            !watched.iter().any(|watch| matches!(
                watch,
                Watch::Exact(path) if path.ends_with("Cargo.lock") || path.ends_with("Cargo.toml")
            )),
            "{watched:?}"
        );
        assert!(watched.contains(&Watch::Exact(PathBuf::from("/p/src/main.rs"))));
    }
}
