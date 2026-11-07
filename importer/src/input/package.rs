//! The files Cargo watches for a build script that declares no `rerun-if`
//! input: the whole package.

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::proof::{Entry, Role};
use super::{Allocation, Usage};
use crate::cargo::{self, toolchain};
use crate::{Error, Result};

pub(in crate::input) mod observed;

#[cfg(test)]
mod selection;

#[cfg(test)]
thread_local! {
    static OWNED_REGISTRY_SETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A package whose build script Cargo reruns when any of its files changes.
pub(crate) enum Set {
    /// A path or Git package, read as its whole source tree.
    Tree(Tree),
    /// A registry package, which Cargo never changes once extracted.
    Registry(Registry),
}

/// A path or Git package's source tree. Its entry walks the tree as Cargo's
/// listing does, keeping every file Cargo could select: a change to
/// any of them, or a file added or removed, changes the entry, so no
/// `cargo package --list` is needed to replay it.
pub(crate) struct Tree {
    base_usage: Usage,
    usage: Usage,
    package_id: Box<str>,
    root: PathBuf,
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
    pub(super) fn fixture_stored_tree() -> Self {
        Self::Tree(Tree {
            base_usage: Usage { items: 2, bytes: 3 },
            usage: Usage {
                items: 11,
                bytes: 127,
            },
            package_id: "path+file:///unobserved/package#pkg@1.2.3".into(),
            root: PathBuf::from("/unobserved/package"),
        })
    }

    /// A path or Git package whose manifest is `manifest`; a registry
    /// package is captured by [`Self::capture_registry`].
    pub(crate) fn capture_tree(
        cargo_commit: &str,
        package: &cargo::unit::Package,
        manifest: &Path,
        base_usage: Usage,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(super::input("cargo-commit-skew", cargo_commit));
        }
        let mut allocation = Allocation::from_usage(base_usage, limits);
        allocation.path(manifest)?;
        let manifest = super::canonical_file("fallback package manifest", manifest)?;
        let root = manifest
            .parent()
            .ok_or_else(|| super::input("package-root", manifest.display()))?;
        allocation.string(package.id())?;
        allocation.path(root)?;
        Ok(Self::Tree(Tree {
            base_usage,
            usage: allocation.usage(),
            package_id: package.id().into(),
            root: root.to_path_buf(),
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
        let marker_digest = validate_registry_marker(&marker, &mut allocation)?;
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
        allocation: &mut Allocation,
        limits: &cargo::Limits,
    ) -> Result<Entry> {
        match self {
            Self::Tree(tree) => tree.entry(allocation, limits),
            Self::Registry(registry) => registry.entry(allocation),
        }
    }

    pub(crate) const fn base_usage(&self) -> Usage {
        match self {
            Self::Tree(tree) => tree.base_usage,
            Self::Registry(registry) => registry.base_usage,
        }
    }

    pub(crate) const fn usage(&self) -> Usage {
        match self {
            Self::Tree(tree) => tree.usage,
            Self::Registry(registry) => registry.usage,
        }
    }
}

impl Tree {
    /// The package and root a stored record walks again.
    pub(super) fn into_stored(self) -> (Box<str>, PathBuf) {
        (self.package_id, self.root)
    }

    fn entry(&self, allocation: &mut Allocation, limits: &cargo::Limits) -> Result<Entry> {
        super::proof::directory_entry(
            Role::DefaultPackageSource,
            Some(&self.package_id),
            &self.root,
            allocation,
            limits,
        )
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

    fn entry(&self, allocation: &mut Allocation) -> Result<Entry> {
        let marker_digest = validate_registry_marker(&self.marker, allocation)?;
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

fn validate_registry_marker(path: &Path, allocation: &mut Allocation) -> Result<[u8; 32]> {
    let bytes = super::read_file_budgeted(
        "Cargo completed registry extraction marker",
        path,
        1024,
        allocation,
    )?;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{registry_relative_shape, Set, OWNED_REGISTRY_SETS, REGISTRY_MARKER_SCHEMA};
    use crate::cargo::toolchain;
    use crate::cargo::unit::{Package, PackageSource};
    use crate::cargo::Limits;
    use crate::input::proof::Entry;
    use crate::input::{Allocation, Usage};
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

    fn reset_ownership_counters() {
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
            .entry(&mut allocation, &Limits::standard())
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
            set.entry(&mut allocation, &Limits::standard()),
            Err(Error::Input {
                code: "registry-marker-changed",
                ..
            })
        ));
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
        // One item below the exact aggregate still completes with the exact
        // figure charged.
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

    /// A path package's tree keeps every file Cargo's listing could select,
    /// ignored and hidden ones included, and leaves out its `.git`, its own
    /// `target` and a nested package. A file added anywhere else changes it.
    #[test]
    fn a_package_tree_keeps_what_cargo_could_list_and_leaves_out_the_rest() {
        let fixture = Fixture::new("tree");
        let root = fixture.root.join("package");
        for (path, contents) in [
            ("Cargo.toml", "[package]\nname = \"tree\"\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
            (".gitignore", "*.log\n"),
            ("build.log", "ignored\n"),
            (".hidden", "hidden\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("target/debug/out", "built\n"),
            ("nested/Cargo.toml", "[package]\nname = \"nested\"\n"),
            ("nested/src/lib.rs", "pub fn g() {}\n"),
            ("assets/target/kept", "kept\n"),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let package = Package::fixture(
            "path+file:///fixture/package#tree@0.1.0",
            "tree",
            "0.1.0",
            root.clone(),
            PackageSource::Path {
                cargo_id: "path+file:///fixture/package".into(),
            },
        );
        let limits = Limits::standard();
        let set = Set::capture_tree(
            toolchain::supported_cargo_commit(),
            &package,
            &root.join("Cargo.toml"),
            Usage { items: 0, bytes: 0 },
            &limits,
        )
        .unwrap();
        let entry = |set: &Set| set.entry(&mut Allocation::new(&limits), &limits).unwrap();
        let Entry::Directory {
            directories, files, ..
        } = entry(&set)
        else {
            panic!("a package tree is a directory entry");
        };
        let directories: Vec<&str> = directories.iter().map(|value| &*value.path).collect();
        let files: Vec<&str> = files.iter().map(|value| &*value.path).collect();
        assert_eq!(directories, ["assets", "assets/target", "src"]);
        assert_eq!(
            files,
            [
                ".gitignore",
                ".hidden",
                "Cargo.toml",
                "assets/target/kept",
                "build.log",
                "src/lib.rs"
            ]
        );
        let before = entry(&set);
        fs::write(root.join("target/debug/more"), "built\n").unwrap();
        fs::write(root.join("nested/src/more.rs"), "\n").unwrap();
        fs::write(root.join(".git/index"), "index\n").unwrap();
        assert!(entry(&set) == before);
        fs::write(root.join("src/new.rs"), "\n").unwrap();
        assert!(entry(&set) != before);
    }
}
