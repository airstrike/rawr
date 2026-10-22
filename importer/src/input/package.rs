//! Exact, replayable pinned-Cargo package membership.

use std::path::{Component, Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::proof::{Entry, Role, TreeFile};
use super::{Allocation, Usage};
use crate::cargo::{self, toolchain};
use crate::process::{self, Program};
use crate::{Error, Result};

pub(in crate::input) mod observed;

#[cfg(test)]
mod selection;

const VIRTUAL_VCS: &str = ".cargo_vcs_info.json";
const VIRTUAL_LOCK: &str = "Cargo.lock";
const VIRTUAL_ORIGINAL_MANIFEST: &str = "Cargo.toml.orig";

#[cfg(test)]
thread_local! {
    static OWNED_PACKAGE_FILE_PATHS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_PATH_SETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_GIT_SETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_REGISTRY_SETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) enum Set {
    Path(Listed),
    Registry(Registry),
    Git(Git),
}

pub(crate) struct Listed {
    base_usage: Usage,
    usage: Usage,
    package_id: Box<str>,
    root: PathBuf,
    cargo: process::Cargo,
    command: process::Command,
    files: Box<[PathBuf]>,
}

pub(crate) struct Registry {
    base_usage: Usage,
    usage: Usage,
    package_id: Box<str>,
    source: Box<str>,
    checksum: [u8; 32],
    root: PathBuf,
    marker: PathBuf,
    marker_digest: [u8; 32],
}

/// Historical registry fields without an input-proof constructor.
pub(crate) struct Stored {
    pub(crate) base_usage: Usage,
    pub(crate) usage: Usage,
    pub(crate) package_id: Box<str>,
    pub(crate) source: Box<str>,
    pub(crate) checksum: [u8; 32],
    pub(crate) root: PathBuf,
    pub(crate) marker: PathBuf,
    pub(crate) marker_digest: [u8; 32],
}

pub(crate) struct Git {
    listed: Listed,
    source: Box<str>,
    oid: Box<str>,
}

pub(super) const REGISTRY_MARKER_SCHEMA: u32 = 1;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    v: u32,
}

fn marker(bytes: &[u8]) -> std::result::Result<Marker, serde_json::Error> {
    serde_json::from_slice(bytes)
}

impl Set {
    #[cfg(test)]
    pub(super) fn fixture_stored_registry() -> Self {
        Self::Registry(Registry {
            base_usage: Usage { items: 2, bytes: 3 },
            usage: Usage {
                items: 11,
                bytes: 127,
            },
            package_id: "registry+fixture#pkg@1.2.3".into(),
            source: "registry+fixture".into(),
            checksum: [7; 32],
            root: PathBuf::from("/unobserved/registry/pkg"),
            marker: PathBuf::from("/unobserved/registry/pkg/.cargo-ok"),
            marker_digest: [8; 32],
        })
    }

    #[cfg(test)]
    pub(super) fn fixture_stored_listed(git: bool) -> Self {
        let command = process::Command::cargo(
            Vec::new(),
            if git {
                process::CargoAction::PackageGit
            } else {
                process::CargoAction::PackagePath
            },
            Vec::new(),
            PathBuf::from("/unobserved/package"),
            Box::default(),
        )
        .expect("fixture package command is sealed without running it");
        let listed = Listed {
            base_usage: Usage { items: 2, bytes: 3 },
            usage: Usage {
                items: 11,
                bytes: 127,
            },
            package_id: "fixture#pkg@1.2.3".into(),
            root: PathBuf::from("/unobserved/package"),
            cargo: process::Cargo::fixture(),
            command,
            files: vec![PathBuf::from("/unobserved/package/Cargo.toml")].into_boxed_slice(),
        };
        if git {
            Self::Git(Git {
                listed,
                source: "git+fixture".into(),
                oid: "abc".into(),
            })
        } else {
            Self::Path(listed)
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "path capture atomically binds exact Cargo replay, package identity, and the transferred allocation proof"
    )]
    pub(crate) fn capture_path(
        cargo_commit: &str,
        package_id: &str,
        manifest: &Path,
        cargo: process::Cargo,
        command: process::Command,
        bytes: &[u8],
        base_usage: Usage,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(super::input("cargo-commit-skew", cargo_commit));
        }
        let mut allocation = Allocation::from_usage(base_usage, limits);
        allocation.items(1)?;
        allocation.bytes(bytes.len())?;
        allocation.path(manifest)?;
        let manifest = super::canonical_file("fallback package manifest", manifest)?;
        let root = manifest
            .parent()
            .ok_or_else(|| super::input("package-root", manifest.display()))?;
        allocation.string(package_id)?;
        allocation.path(root)?;
        let files = parse(
            cargo_commit,
            package_id,
            root,
            bytes,
            limits,
            &mut allocation,
        )?;
        #[cfg(test)]
        OWNED_PATH_SETS.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self::Path(Listed {
            base_usage,
            usage: allocation.usage(),
            package_id: package_id.into(),
            root: root.to_path_buf(),
            cargo,
            command,
            files,
        }))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Git capture adds exact source identity to the same atomic path-capture proof bundle"
    )]
    pub(crate) fn capture_git(
        cargo_commit: &str,
        package: &cargo::unit::Package,
        manifest: &Path,
        cargo: process::Cargo,
        command: process::Command,
        bytes: &[u8],
        base_usage: Usage,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let cargo::unit::PackageSource::Git { cargo_id, oid } = package.source() else {
            return Err(super::input("package-source-shape", package.id()));
        };
        let Self::Path(listed) = Self::capture_path(
            cargo_commit,
            package.id(),
            manifest,
            cargo,
            command,
            bytes,
            base_usage,
            limits,
        )?
        else {
            return Err(super::input("package-source-shape", package.id()));
        };
        let mut allocation = Allocation::from_usage(listed.usage, limits);
        allocation.string(cargo_id)?;
        allocation.string(oid)?;
        let mut listed = listed;
        listed.usage = allocation.usage();
        #[cfg(test)]
        OWNED_GIT_SETS.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self::Git(Git {
            listed,
            source: cargo_id.as_ref().into(),
            oid: oid.as_ref().into(),
        }))
    }

    pub(crate) fn capture_registry(
        cargo_commit: &str,
        package: &cargo::unit::Package,
        cargo_home: &Path,
        base_usage: Usage,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(super::input("cargo-commit-skew", cargo_commit));
        }
        let cargo::unit::PackageSource::Registry { cargo_id, checksum } = package.source() else {
            return Err(super::input("package-source-shape", package.id()));
        };
        let checksum = checksum
            .ok_or_else(|| super::input("registry-checksum-authority-absent", package.id()))?;
        let exact_package_id = package
            .id()
            .strip_prefix(cargo_id.as_ref())
            .and_then(|suffix| suffix.strip_prefix('#'))
            .and_then(|suffix| suffix.split_once('@'))
            .is_some_and(|(name, version)| name == package.name() && version == package.version());
        if !exact_package_id {
            return Err(super::input(
                "registry-package-id-source-mismatch",
                package.id(),
            ));
        }
        let mut allocation = Allocation::from_usage(base_usage, limits);
        allocation.path(cargo_home)?;
        let cargo_home = super::canonical_directory("Cargo home", cargo_home)?;
        allocation.items(1)?;
        allocation.bytes(
            cargo_home
                .as_os_str()
                .len()
                .saturating_add("/registry/src".len()),
        )?;
        let source_root_candidate = cargo_home.join("registry/src");
        let source_root =
            super::canonical_directory("Cargo registry source root", &source_root_candidate)?;
        allocation.path(package.root())?;
        let root = super::canonical_directory("registry package root", package.root())?;
        let relative = root
            .strip_prefix(&source_root)
            .map_err(|_| super::input("registry-root-outside-cargo-home", root.display()))?;
        if !registry_relative_shape(relative, package.name(), package.version())
            || package.manifest() != root.join("Cargo.toml")
        {
            return Err(super::input("registry-root-shape", root.display()));
        }
        allocation.items(1)?;
        allocation.bytes(root.as_os_str().len().saturating_add("/.cargo-ok".len()))?;
        let marker_candidate = root.join(".cargo-ok");
        let marker = super::canonical_file(
            "Cargo completed registry extraction marker",
            &marker_candidate,
        )?;
        let marker_digest = validate_registry_marker(&marker, limits, &mut allocation)?;
        allocation.string(package.id())?;
        allocation.string(cargo_id)?;
        allocation.path(&root)?;
        allocation.path(&marker)?;
        allocation.items(2)?;
        allocation.bytes(64)?;
        #[cfg(test)]
        OWNED_REGISTRY_SETS.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self::Registry(Registry {
            base_usage,
            usage: allocation.usage(),
            package_id: package.id().into(),
            source: cargo_id.as_ref().into(),
            checksum,
            root,
            marker,
            marker_digest,
        }))
    }

    pub(super) fn entry(
        &self,
        replay: bool,
        allocation: &mut Allocation,
        limits: &cargo::Limits,
    ) -> Result<Entry> {
        match self {
            Self::Path(listed) => listed.entry(replay, allocation, limits),
            Self::Registry(registry) => registry.entry(allocation, limits),
            Self::Git(git) => git.entry(replay, allocation, limits),
        }
    }

    pub(crate) const fn base_usage(&self) -> Usage {
        match self {
            Self::Path(listed) => listed.base_usage,
            Self::Registry(registry) => registry.base_usage,
            Self::Git(git) => git.listed.base_usage,
        }
    }

    pub(crate) const fn usage(&self) -> Usage {
        match self {
            Self::Path(listed) => listed.usage,
            Self::Registry(registry) => registry.usage,
            Self::Git(git) => git.listed.usage,
        }
    }
}

fn registry_relative_shape(relative: &Path, name: &str, version: &str) -> bool {
    let mut components = relative.components();
    let registry = components.next();
    let package = components.next();
    registry.is_some_and(|component| !component.as_os_str().is_empty())
        && package.is_some_and(|component| {
            component
                .as_os_str()
                .to_str()
                .and_then(|package| package.strip_prefix(name))
                .and_then(|suffix| suffix.strip_prefix('-'))
                == Some(version)
        })
        && components.next().is_none()
}

impl Listed {
    fn entry(
        &self,
        replay: bool,
        allocation: &mut Allocation,
        limits: &cargo::Limits,
    ) -> Result<Entry> {
        let replayed;
        let files = if replay {
            let bytes = self
                .cargo
                // The child capture is physical (`CAPTURE_BYTES` stdout,
                // `TAIL_BYTES` stderr), never sized from the JSON accounting target.
                .run(&self.command, process::Limits::capture())?
                .checked(Program::Cargo)?;
            allocation.items(1)?;
            allocation.bytes(bytes.len())?;
            let current = parse(
                toolchain::supported_cargo_commit(),
                &self.package_id,
                &self.root,
                &bytes,
                limits,
                allocation,
            )?;
            if current != self.files {
                return Err(Error::Input {
                    code: "package-file-set-changed",
                    reason: self.package_id.clone(),
                });
            }
            replayed = current;
            replayed.as_ref()
        } else {
            self.files.as_ref()
        };

        let mut entries = Vec::new();
        for path in files {
            allocation.exceeded(
                "input proof filesystem items",
                limits.items,
                allocation.usage.items.saturating_add(1),
            );
            allocation.items(1)?;
            entries.try_reserve(1).map_err(|error| Error::Input {
                code: "package-file-set-allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
            // A package file is an artifact-class read bound to the physical
            // capture bound; its size against the JSON target only records.
            let contents = super::read_file_budgeted(
                "fallback package file",
                path,
                crate::process::CAPTURE_BYTES,
                allocation,
            )?;
            allocation.exceeded("fallback package file", limits.json_bytes, contents.len());
            let bytes = contents.len();
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| super::input("package-file-set-escape", path.display()))?;
            allocation.items(1)?;
            allocation.bytes(relative.as_os_str().len())?;
            let relative = super::path_text("fallback package file", relative)?;
            entries.push(TreeFile {
                path: relative.into(),
                bytes,
                digest: Sha256::digest(contents).into(),
            });
        }
        allocation.path(&self.root)?;
        allocation.string(&self.package_id)?;
        let root = super::path_text("fallback package root", &self.root)?;
        Ok(Entry::Directory {
            role: Role::DefaultPackageSource,
            package_id: Some(self.package_id.clone()),
            path: root.into(),
            directories: Box::default(),
            files: entries.into_boxed_slice(),
        })
    }
}

impl Registry {
    pub(super) fn into_stored(self) -> Stored {
        let Self {
            base_usage,
            usage,
            package_id,
            source,
            checksum,
            root,
            marker,
            marker_digest,
        } = self;
        Stored {
            base_usage,
            usage,
            package_id,
            source,
            checksum,
            root,
            marker,
            marker_digest,
        }
    }

    fn entry(&self, allocation: &mut Allocation, limits: &cargo::Limits) -> Result<Entry> {
        let marker_digest = validate_registry_marker(&self.marker, limits, allocation)?;
        if marker_digest != self.marker_digest {
            return Err(super::input(
                "registry-marker-changed",
                self.package_id.as_ref(),
            ));
        }
        allocation.string(&self.package_id)?;
        allocation.string(&self.source)?;
        allocation.path(&self.root)?;
        allocation.bytes(64)?;
        Ok(Entry::Registry {
            role: Role::DefaultPackageSource,
            package_id: self.package_id.clone(),
            source: self.source.clone(),
            checksum: self.checksum,
            root: super::path_text("registry package root", &self.root)?.into(),
            marker_schema: REGISTRY_MARKER_SCHEMA,
            marker_digest,
        })
    }
}

impl Git {
    fn entry(
        &self,
        replay: bool,
        allocation: &mut Allocation,
        limits: &cargo::Limits,
    ) -> Result<Entry> {
        let entry = self.listed.entry(replay, allocation, limits)?;
        let Entry::Directory {
            path: root, files, ..
        } = entry
        else {
            return Err(super::input(
                "git-package-entry-shape",
                self.listed.package_id.as_ref(),
            ));
        };
        allocation.string(&self.listed.package_id)?;
        allocation.string(&self.source)?;
        allocation.string(&self.oid)?;
        Ok(Entry::Git {
            role: Role::DefaultPackageSource,
            package_id: self.listed.package_id.clone(),
            source: self.source.clone(),
            oid: self.oid.clone(),
            root,
            files,
        })
    }
}

fn validate_registry_marker(
    path: &Path,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<[u8; 32]> {
    let bytes = super::read_file_budgeted(
        "Cargo completed registry extraction marker",
        path,
        1024,
        allocation,
    )?;
    allocation.exceeded(
        "registry extraction marker bytes",
        limits.string_bytes,
        bytes.len(),
    );
    // Preflight the owned marker parse before serde constructs the value.
    allocation.items(bytes.len())?;
    let marker = marker(&bytes).map_err(|error| Error::Input {
        code: "registry-marker-malformed",
        reason: error.to_string().into_boxed_str(),
    })?;
    if marker.v != REGISTRY_MARKER_SCHEMA {
        return Err(super::input("registry-marker-schema", path.display()));
    }
    Ok(Sha256::digest(bytes).into())
}

fn parse(
    cargo_commit: &str,
    package_id: &str,
    root: &Path,
    bytes: &[u8],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Box<[PathBuf]>> {
    if cargo_commit != toolchain::supported_cargo_commit() {
        return Err(super::input("cargo-commit-skew", cargo_commit));
    }
    // The file-list size is an accounting target recorded on the input
    // account; the physical bound is the child capture that produced it.
    allocation.exceeded(
        "Cargo package file-list bytes",
        limits.json_bytes,
        bytes.len(),
    );
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(super::input(
            "truncated-package-file-set",
            "Cargo package --list output does not end at a line boundary",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|error| Error::Input {
        code: "package-file-set-utf8",
        reason: error.to_string().into_boxed_str(),
    })?;
    let mut files = Vec::new();
    for line in text.split_terminator('\n') {
        allocation.exceeded(
            "Cargo package file-list entries",
            limits.items,
            files.len().saturating_add(1),
        );
        allocation.exceeded(
            "Cargo package file-list path",
            limits.string_bytes,
            line.len(),
        );
        let relative = Path::new(line);
        if line.is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(super::input("invalid-package-file-set-path", line));
        }
        if virtual_entry(cargo_commit, relative)? {
            continue;
        }
        allocation.items(2)?;
        allocation.bytes(
            root.as_os_str()
                .len()
                .saturating_add(1)
                .saturating_add(relative.as_os_str().len()),
        )?;
        files.try_reserve(1).map_err(|error| Error::Input {
            code: "package-file-set-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
        let candidate = root.join(relative);
        let path = super::canonical_file("Cargo package file-set entry", &candidate)?;
        #[cfg(test)]
        OWNED_PACKAGE_FILE_PATHS.with(|owned| owned.set(owned.get().saturating_add(1)));
        files.push(path);
    }
    files.sort_unstable();
    if files.is_empty() || files.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(super::input("invalid-package-file-set", package_id));
    }
    Ok(files.into_boxed_slice())
}

fn virtual_entry(cargo_commit: &str, relative: &Path) -> Result<bool> {
    if cargo_commit != toolchain::supported_cargo_commit() {
        return Err(super::input("cargo-commit-skew", cargo_commit));
    }
    Ok(relative == Path::new(VIRTUAL_VCS)
        || relative == Path::new(VIRTUAL_LOCK)
        || relative == Path::new(VIRTUAL_ORIGINAL_MANIFEST))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        registry_relative_shape, Set, OWNED_GIT_SETS, OWNED_PACKAGE_FILE_PATHS, OWNED_PATH_SETS,
        OWNED_REGISTRY_SETS, REGISTRY_MARKER_SCHEMA,
    };
    use crate::cargo::toolchain;
    use crate::cargo::unit::{Package, PackageSource};
    use crate::cargo::Limits;
    use crate::input::proof::Entry;
    use crate::input::{Allocation, Usage};
    use crate::process::{self, CargoAction};
    use crate::Error;

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
        cargo_home: PathBuf,
        package_root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-registry-{name}-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let cargo_home = root.join("cargo-home");
            let package_root = cargo_home.join("registry/src/index-fixture/crunchy-0.2.4");
            fs::create_dir_all(&package_root).expect("registry fixture root can be created");
            fs::write(
                package_root.join("Cargo.toml"),
                "[package]\nname = \"crunchy\"\nversion = \"0.2.4\"\n",
            )
            .expect("registry fixture manifest can be written");
            fs::write(package_root.join(".cargo-ok"), b"{\"v\":1}")
                .expect("registry marker can be written");
            Self {
                root: fs::canonicalize(root).expect("fixture root canonicalizes"),
                cargo_home: fs::canonicalize(cargo_home).expect("fixture Cargo home canonicalizes"),
                package_root: fs::canonicalize(package_root)
                    .expect("fixture package root canonicalizes"),
            }
        }

        fn package(&self, checksum: Option<[u8; 32]>) -> Package {
            let source = "registry+https://github.com/rust-lang/crates.io-index";
            Package::fixture(
                &format!("{source}#crunchy@0.2.4"),
                "crunchy",
                "0.2.4",
                self.package_root.clone(),
                PackageSource::Registry {
                    cargo_id: source.into(),
                    checksum,
                },
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn capture(fixture: &Fixture, package: &Package) -> Result<Set, Error> {
        Set::capture_registry(
            toolchain::supported_cargo_commit(),
            package,
            &fixture.cargo_home,
            Usage { items: 0, bytes: 0 },
            &Limits::standard(),
        )
    }

    fn limits_for(usage: Usage) -> Limits {
        let mut limits = Limits::standard();
        limits.items = usage.items;
        limits.json_bytes = usage.bytes;
        limits
    }

    fn package_command(root: &Path, action: CargoAction) -> process::Command {
        process::Command::cargo(
            Vec::new(),
            action,
            vec![OsString::from("--list")],
            root.to_path_buf(),
            Vec::new().into_boxed_slice(),
        )
        .expect("fixture package command seals")
    }

    fn long_file_list(fixture: &Fixture, count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for index in 0..count {
            let name = format!("member-{index:03}.rs");
            fs::write(
                fixture.package_root.join(&name),
                b"pub const VALUE: u8 = 1;\n",
            )
            .expect("package-list member can be written");
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(b'\n');
        }
        bytes
    }

    fn reset_ownership_counters() {
        OWNED_PACKAGE_FILE_PATHS.with(|owned| owned.set(0));
        OWNED_PATH_SETS.with(|owned| owned.set(0));
        OWNED_GIT_SETS.with(|owned| owned.set(0));
        OWNED_REGISTRY_SETS.with(|owned| owned.set(0));
    }

    #[test]
    fn registry_root_requires_exactly_two_components_without_collection() {
        assert!(!registry_relative_shape(
            Path::new("crunchy-0.2.4"),
            "crunchy",
            "0.2.4",
        ));
        assert!(registry_relative_shape(
            Path::new("index-fixture/crunchy-0.2.4"),
            "crunchy",
            "0.2.4",
        ));
        assert!(!registry_relative_shape(
            Path::new("extra/index-fixture/crunchy-0.2.4"),
            "crunchy",
            "0.2.4",
        ));
    }

    #[test]
    fn registry_proof_uses_checksum_and_marker_without_archive_or_checksum_sidecar() {
        let fixture = Fixture::new("complete");
        assert!(!fixture.package_root.join(".cargo-checksum.json").exists());
        assert!(!fixture
            .cargo_home
            .join("registry/cache/index-fixture/crunchy-0.2.4.crate")
            .exists());
        let set = capture(&fixture, &fixture.package(Some([0x44; 32])))
            .expect("Cargo metadata/lock plus completed extraction marker seal");
        assert!(matches!(set, Set::Registry(_)));
        let mut allocation = Allocation::with_limits(100, 4096, 1024);
        let entry = set
            .entry(false, &mut allocation, &Limits::standard())
            .expect("registry proof emits a freshness entry without a tree scan");
        let Entry::Registry {
            checksum,
            marker_schema,
            ..
        } = entry
        else {
            panic!("registry proof emitted a non-registry entry");
        };
        assert_eq!(checksum, [0x44; 32]);
        assert_eq!(marker_schema, REGISTRY_MARKER_SCHEMA);
    }

    #[test]
    fn registry_proof_rejects_missing_or_malformed_marker_and_missing_checksum() {
        let missing = Fixture::new("missing-marker");
        fs::remove_file(missing.package_root.join(".cargo-ok")).expect("marker can be removed");
        assert!(capture(&missing, &missing.package(Some([0x44; 32]))).is_err());

        let malformed = Fixture::new("malformed-marker");
        fs::write(malformed.package_root.join(".cargo-ok"), b"not-json")
            .expect("marker can be malformed");
        assert!(matches!(
            capture(&malformed, &malformed.package(Some([0x44; 32]))),
            Err(Error::Input {
                code: "registry-marker-malformed",
                ..
            })
        ));

        let checksum = Fixture::new("missing-checksum");
        assert!(matches!(
            capture(&checksum, &checksum.package(None)),
            Err(Error::Input {
                code: "registry-checksum-authority-absent",
                ..
            })
        ));
    }

    #[test]
    fn registry_proof_rejects_source_or_root_ambiguity_and_marker_drift() {
        let fixture = Fixture::new("authority");
        let mismatched = Package::fixture(
            "registry+https://other.invalid/index#crunchy@0.2.4",
            "crunchy",
            "0.2.4",
            fixture.package_root.clone(),
            PackageSource::Registry {
                cargo_id: "registry+https://github.com/rust-lang/crates.io-index".into(),
                checksum: Some([0x44; 32]),
            },
        );
        assert!(matches!(
            capture(&fixture, &mismatched),
            Err(Error::Input {
                code: "registry-package-id-source-mismatch",
                ..
            })
        ));

        let outside_root = fixture.root.join("vendor/crunchy-0.2.4");
        fs::create_dir_all(&outside_root).expect("vendor-shaped root can be created");
        fs::write(outside_root.join("Cargo.toml"), b"[package]\n")
            .expect("vendor-shaped manifest can be written");
        fs::write(outside_root.join(".cargo-ok"), b"{\"v\":1}")
            .expect("vendor-shaped marker can be written");
        let outside = Package::fixture(
            "registry+https://github.com/rust-lang/crates.io-index#crunchy@0.2.4",
            "crunchy",
            "0.2.4",
            fs::canonicalize(outside_root).expect("vendor root canonicalizes"),
            PackageSource::Registry {
                cargo_id: "registry+https://github.com/rust-lang/crates.io-index".into(),
                checksum: Some([0x44; 32]),
            },
        );
        assert!(capture(&fixture, &outside).is_err());

        let set = capture(&fixture, &fixture.package(Some([0x44; 32])))
            .expect("initial marker authority seals");
        fs::write(fixture.package_root.join(".cargo-ok"), b"{ \"v\": 1 }")
            .expect("valid marker can drift");
        let mut allocation = Allocation::with_limits(100, 4096, 1024);
        assert!(matches!(
            set.entry(true, &mut allocation, &Limits::standard()),
            Err(Error::Input {
                code: "registry-marker-changed",
                ..
            })
        ));
    }

    /// The file-list size, entry count and path length are accounting
    /// targets on the input account: the list still parses in full and each
    /// lowered axis records its truthful figure; nothing refuses.
    #[test]
    fn file_list_size_entries_and_paths_record_their_targets_without_refusal() {
        let root = Fixture::new("file-list-targets");
        fs::write(
            root.package_root.join("src.rs"),
            b"pub const VALUE: u8 = 1;\n",
        )
        .expect("path member can be written");
        let bytes = b"Cargo.toml\nsrc.rs\n";
        let exercise = |limits: &Limits| {
            let mut allocation = Allocation::new(limits);
            let files = super::parse(
                toolchain::supported_cargo_commit(),
                "path+file:///fixture#crunchy@0.2.4",
                Path::new(&root.package_root),
                bytes,
                limits,
                &mut allocation,
            )
            .expect("the file list parses under every accounting target");
            assert_eq!(files.len(), 2);
            allocation.exceedances()
        };
        let standard = Limits::standard();
        assert!(!exercise(&standard).is_exceeded());
        let mut size = standard;
        size.json_bytes = bytes.len() - 1;
        let axis = *exercise(&size)
            .axis("Cargo package file-list bytes")
            .expect("the list size records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (bytes.len() - 1) as u64,
                bytes.len() as u64,
                bytes.len() as u64
            )
        );
        let mut entries = standard;
        entries.items = 1;
        let axis = *exercise(&entries)
            .axis("Cargo package file-list entries")
            .expect("the entry count records its axis");
        assert_eq!((axis.target, axis.high_water), (1, 2));
        let mut paths = standard;
        paths.string_bytes = "src.rs".len();
        let axis = *exercise(&paths)
            .axis("Cargo package file-list path")
            .expect("the longest path records its axis");
        assert_eq!(
            (axis.target, axis.high_water),
            ("src.rs".len() as u64, "Cargo.toml".len() as u64)
        );
    }

    #[test]
    fn path_virtual_archive_entries_remain_excluded_by_pinned_package_list_parser() {
        let root = Fixture::new("path-list");
        fs::write(
            root.package_root.join("src.rs"),
            b"pub const VALUE: u8 = 1;\n",
        )
        .expect("path member can be written");
        for path in ["Cargo.toml.orig", ".cargo_vcs_info.json", "Cargo.lock"] {
            fs::write(root.package_root.join(path), b"virtual\n")
                .expect("virtual archive member can be written");
        }
        let bytes =
            b".cargo-ok\n.cargo_vcs_info.json\nCargo.lock\nCargo.toml\nCargo.toml.orig\nsrc.rs\n";
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let files = super::parse(
            toolchain::supported_cargo_commit(),
            "path+file:///fixture#crunchy@0.2.4",
            Path::new(&root.package_root),
            bytes,
            &limits,
            &mut allocation,
        )
        .expect("pinned package list excludes only Cargo virtual archive entries");
        assert!(files.iter().any(|path| path.ends_with(".cargo-ok")));
        assert!(files.iter().any(|path| path.ends_with("Cargo.toml")));
        assert!(files.iter().any(|path| path.ends_with("src.rs")));
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn long_path_and_git_lists_share_exact_usage_and_record_exceedance_at_final_ownership() {
        let fixture = Fixture::new("long-path-git");
        let bytes = long_file_list(&fixture, 48);
        let path_id = "path+file:///fixture#crunchy@0.2.4";
        let standard = Limits::standard();
        let path = Set::capture_path(
            toolchain::supported_cargo_commit(),
            path_id,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackagePath),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &standard,
        )
        .expect("standard envelope captures the long path list");
        let path_usage = path.usage();
        let exact = limits_for(path_usage);
        reset_ownership_counters();
        let set = Set::capture_path(
            toolchain::supported_cargo_commit(),
            path_id,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackagePath),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &exact,
        )
        .expect("the exact retained path-list envelope succeeds");
        assert_eq!(set.usage(), path_usage);
        assert_eq!(OWNED_PATH_SETS.with(std::cell::Cell::get), 1);
        let owned_paths = OWNED_PACKAGE_FILE_PATHS.with(std::cell::Cell::get);
        // One item below the exact envelope is an accounting target, not a
        // gate: the final path item is still owned and charged, so the retained
        // usage is the exact figure, one past the target.
        let mut below = exact;
        below.items = below.items.saturating_sub(1);
        reset_ownership_counters();
        let set = Set::capture_path(
            toolchain::supported_cargo_commit(),
            path_id,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackagePath),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &below,
        )
        .expect("an item target one below the exact usage records, never refuses");
        assert_eq!(set.usage(), path_usage);
        assert_eq!(set.usage().items, below.items + 1);
        assert_eq!(
            OWNED_PACKAGE_FILE_PATHS.with(std::cell::Cell::get),
            owned_paths
        );
        assert_eq!(OWNED_PATH_SETS.with(std::cell::Cell::get), 1);

        let oid = "0123456789abcdef0123456789abcdef01234567";
        let source = format!("git+https://example.invalid/repo?rev=main#{oid}");
        let package = Package::fixture(
            "git+https://example.invalid/repo?rev=main#crunchy@0.2.4",
            "crunchy",
            "0.2.4",
            fixture.package_root.clone(),
            PackageSource::Git {
                cargo_id: source.into(),
                oid: oid.into(),
            },
        );
        let git = Set::capture_git(
            toolchain::supported_cargo_commit(),
            &package,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackageGit),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &standard,
        )
        .expect("standard envelope captures the long Git list");
        let git_usage = git.usage();
        let exact = limits_for(git_usage);
        reset_ownership_counters();
        let set = Set::capture_git(
            toolchain::supported_cargo_commit(),
            &package,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackageGit),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &exact,
        )
        .expect("the exact retained Git envelope succeeds");
        assert_eq!(set.usage(), git_usage);
        assert_eq!(OWNED_GIT_SETS.with(std::cell::Cell::get), 1);
        // The Git identity strings are the final items: one below the exact
        // envelope records their charge past the target and completes ownership.
        let mut below = exact;
        below.items = below.items.saturating_sub(1);
        reset_ownership_counters();
        let set = Set::capture_git(
            toolchain::supported_cargo_commit(),
            &package,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackageGit),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &below,
        )
        .expect("an item target one below the exact usage records, never refuses");
        assert_eq!(set.usage(), git_usage);
        assert_eq!(set.usage().items, below.items + 1);
        assert_eq!(OWNED_PATH_SETS.with(std::cell::Cell::get), 1);
        assert_eq!(OWNED_GIT_SETS.with(std::cell::Cell::get), 1);
    }

    /// A replayed `cargo package --list` child is captured within the
    /// physical bounds (`CAPTURE_BYTES` stdout, `TAIL_BYTES` stderr), never
    /// within a capture sized from the Cargo JSON accounting target.
    #[test]
    fn replayed_package_list_capture_is_physical_and_independent_of_the_json_target() {
        use crate::process::test_launch::{Expected, FileLimits, Reply, Script, Step};

        let fixture = Fixture::new("replay-capture");
        let bytes = long_file_list(&fixture, 8);
        let path_id = "path+file:///fixture#crunchy@0.2.4";
        let mut limits = Limits::standard();
        // A JSON target far below the physical bound: a capture sized from it
        // would mismatch the scripted child's expected stream limits.
        limits.json_bytes = 4096;
        assert_ne!(limits.json_bytes, process::CAPTURE_BYTES);
        let set = Set::capture_path(
            toolchain::supported_cargo_commit(),
            path_id,
            &fixture.package_root.join("Cargo.toml"),
            process::Cargo::fixture(),
            package_command(&fixture.package_root, CargoAction::PackagePath),
            &bytes,
            Usage { items: 0, bytes: 0 },
            &limits,
        )
        .expect("the path list is captured");
        let launch_root = fixture.root.join("launch");
        fs::create_dir(&launch_root).expect("launch root can be created");
        let launch_root = fs::canonicalize(launch_root).expect("launch root canonicalizes");
        let expected = package_command(&fixture.package_root, CargoAction::PackagePath);
        let reply = bytes.clone();
        let step = Step::new(
            move |_| {
                Expected::new(
                    "/fixture/cargo".into(),
                    expected,
                    process::Limits::capture(),
                )
            },
            move |_| Reply::stdout(reply),
        );
        let mut allocation = Allocation::new(&limits);
        let entry = Script::new(
            launch_root,
            FileLimits {
                count: 1,
                bytes: 16,
            },
            vec![step],
        )
        .expect("launch script seals")
        .run(|| set.entry(true, &mut allocation, &limits))
        .expect("the replayed child is launched within the physical capture bounds");
        entry.expect("the replayed list matches the captured set");
    }

    #[test]
    fn registry_identity_and_marker_use_exact_aggregate_ownership() {
        let fixture = Fixture::new("registry-allocation");
        let package = fixture.package(Some([0x44; 32]));
        let standard = capture(&fixture, &package).expect("standard registry capture succeeds");
        let registry_usage = standard.usage();
        let exact = limits_for(registry_usage);
        reset_ownership_counters();
        let set = Set::capture_registry(
            toolchain::supported_cargo_commit(),
            &package,
            &fixture.cargo_home,
            Usage { items: 0, bytes: 0 },
            &exact,
        )
        .expect("exact registry identity and marker envelope succeeds");
        assert_eq!(set.usage(), registry_usage);
        assert_eq!(OWNED_REGISTRY_SETS.with(std::cell::Cell::get), 1);
        // One item below the exact aggregate is an accounting target; the
        // identity and marker ownership completes with the exact figure charged.
        let mut below = exact;
        below.items = below.items.saturating_sub(1);
        reset_ownership_counters();
        let set = Set::capture_registry(
            toolchain::supported_cargo_commit(),
            &package,
            &fixture.cargo_home,
            Usage { items: 0, bytes: 0 },
            &below,
        )
        .expect("an item target one below the exact usage records, never refuses");
        assert_eq!(set.usage(), registry_usage);
        assert_eq!(set.usage().items, below.items + 1);
        assert_eq!(OWNED_REGISTRY_SETS.with(std::cell::Cell::get), 1);
    }
}
