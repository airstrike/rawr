//! Conservative, transactional creation of a starter Cargo crate marked
//! `// rawr`.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// The exact `src/main.rs` installed by [`create`].
pub const COUNTER_SOURCE: &str = include_str!("../assets/counter/src/main.rs");

/// The `Cargo.toml` [`create`] writes for the package `name`. Its iced
/// names `advanced` and `sipper`, the two iced features Rawr's runner adds,
/// and every widget Rawr's facade offers, so a widget can be added while
/// the program runs and the runner builds the same iced as `cargo build`.
#[must_use]
pub fn manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\niced = {{ git = \"https://github.com/iced-rs/iced\", rev = \"{}\", \
         features = [{}] }}\n",
        rawr_iced::native::ICED_REVISION,
        ICED_FEATURES
            .iter()
            .map(|feature| format!("\"{feature}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The iced features [`manifest`] names. The image widget comes without
/// codecs, as Rawr's runner builds it: iced's `image` would add every codec
/// the `image` crate has, AVIF's encoder among them.
pub const ICED_FEATURES: [&str; 7] = [
    "advanced",
    "canvas",
    "image-without-codecs",
    "qr_code",
    "selector",
    "sipper",
    "svg",
];

/// The `rust-toolchain.toml` [`create`] writes: the importer's pinned
/// nightly, the compiler Rawr builds the project's runner and reads its
/// dependencies with, so `cargo build` uses it too.
#[cfg(feature = "native-runner")]
#[must_use]
pub fn toolchain() -> String {
    format!(
        "[toolchain]\nchannel = \"{}\"\ncomponents = [\"rust-src\", \"rustfmt\", \"clippy\"]\n",
        rawr_import::cargo::toolchain::CHANNEL
    )
}

/// The `.cargo/config.toml` [`create`] writes: `build.target` names the
/// host (Cargo's `host-tuple`, so the file holds on any machine), as Rawr's
/// builds do, so `cargo build` and `rawr run` compile the same units into
/// `target/<host>/debug` and each reuses the other's.
#[must_use]
pub fn cargo_config() -> String {
    "[build]\ntarget = \"host-tuple\"\n".to_owned()
}

/// The Cargo package name for a crate in the directory `directory`, as
/// `cargo new` would accept it: lowercase, with each character that is not
/// an ASCII letter, digit, `-` or `_` replaced by `-`. `None` when the
/// result is empty or does not start with a letter or `_`.
#[must_use]
pub fn package_name(directory: &str) -> Option<String> {
    let name = directory
        .chars()
        .map(|character| match character.to_ascii_lowercase() {
            kept @ ('a'..='z' | '0'..='9' | '-' | '_') => kept,
            _ => '-',
        })
        .collect::<String>();
    name.starts_with(|first: char| first.is_ascii_lowercase() || first == '_')
        .then_some(name)
}

/// A newly initialized Rawr crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Created {
    root: PathBuf,
}

impl Created {
    /// Return the absolute, lexically normalized project root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Failure while validating or initializing a project destination.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    CurrentDirectory(io::Error),
    UnsafeTarget {
        path: PathBuf,
        reason: &'static str,
    },
    InvalidName(PathBuf),
    Inspect {
        path: PathBuf,
        source: io::Error,
    },
    ExistingFile(PathBuf),
    ExistingSymlink(PathBuf),
    ExistingNonEmpty(PathBuf),
    UnsafeAncestor {
        path: PathBuf,
        kind: EntryKind,
    },
    Initialize {
        root: PathBuf,
        path: PathBuf,
        operation: Operation,
        source: io::Error,
        cleanup: Box<[CleanupFailure]>,
    },
}

#[derive(Clone, Copy, Debug)]
enum EntryKind {
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    CreateDirectory,
    WriteManifest,
    WriteToolchain,
    WriteCargoConfig,
    WriteSource,
}

#[derive(Debug)]
struct CleanupFailure {
    path: PathBuf,
    source: io::Error,
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
struct Plan {
    root: PathBuf,
    name: String,
    directories: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct LedgerEntry {
    path: PathBuf,
    kind: CreatedKind,
}

#[derive(Clone, Copy, Debug)]
enum CreatedKind {
    File,
    Directory,
}

#[derive(Debug)]
struct NewFileError {
    source: io::Error,
    created: bool,
}

trait FileSystem {
    fn inspect(&self, path: &Path) -> io::Result<Entry>;
    fn directory_is_empty(&self, path: &Path) -> io::Result<bool>;
    fn create_directory(&self, path: &Path) -> io::Result<()>;
    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<(), NewFileError>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_directory(&self, path: &Path) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug)]
struct OsFileSystem;

/// Create a crate at `path`, resolving relative input against the current directory.
pub fn create(path: PathBuf) -> Result<Created, Error> {
    let current = std::env::current_dir().map_err(|source| Error {
        kind: ErrorKind::CurrentDirectory(source),
    })?;
    create_at(&OsFileSystem, &current, &path)
}

fn create_at(
    file_system: &impl FileSystem,
    current: &Path,
    requested: &Path,
) -> Result<Created, Error> {
    let plan = Plan::validate(file_system, current, requested)?;
    let mut ledger = Vec::new();

    for directory in &plan.directories {
        if let Err(source) = file_system.create_directory(directory) {
            return Err(initialization_failure(
                file_system,
                &plan.root,
                directory,
                Operation::CreateDirectory,
                source,
                &ledger,
            ));
        }
        ledger.push(LedgerEntry {
            path: directory.clone(),
            kind: CreatedKind::Directory,
        });
    }

    let source_directory = plan.root.join("src");
    if let Err(source) = file_system.create_directory(&source_directory) {
        return Err(initialization_failure(
            file_system,
            &plan.root,
            &source_directory,
            Operation::CreateDirectory,
            source,
            &ledger,
        ));
    }
    ledger.push(LedgerEntry {
        path: source_directory.clone(),
        kind: CreatedKind::Directory,
    });
    #[cfg(feature = "native-runner")]
    {
        let config_directory = plan.root.join(".cargo");
        if let Err(source) = file_system.create_directory(&config_directory) {
            return Err(initialization_failure(
                file_system,
                &plan.root,
                &config_directory,
                Operation::CreateDirectory,
                source,
                &ledger,
            ));
        }
        ledger.push(LedgerEntry {
            path: config_directory,
            kind: CreatedKind::Directory,
        });
    }

    let files = [
        (
            plan.root.join("Cargo.toml"),
            manifest(&plan.name),
            Operation::WriteManifest,
        ),
        #[cfg(feature = "native-runner")]
        (
            plan.root.join("rust-toolchain.toml"),
            toolchain(),
            Operation::WriteToolchain,
        ),
        #[cfg(feature = "native-runner")]
        (
            plan.root.join(".cargo/config.toml"),
            cargo_config(),
            Operation::WriteCargoConfig,
        ),
        (
            source_directory.join("main.rs"),
            COUNTER_SOURCE.to_owned(),
            Operation::WriteSource,
        ),
    ];
    for (path, contents, operation) in files {
        if let Err(failure) = file_system.write_new(&path, contents.as_bytes()) {
            if failure.created {
                ledger.push(LedgerEntry {
                    path: path.clone(),
                    kind: CreatedKind::File,
                });
            }
            return Err(initialization_failure(
                file_system,
                &plan.root,
                &path,
                operation,
                failure.source,
                &ledger,
            ));
        }
        ledger.push(LedgerEntry {
            path,
            kind: CreatedKind::File,
        });
    }

    Ok(Created { root: plan.root })
}

impl Plan {
    fn validate(
        file_system: &impl FileSystem,
        current: &Path,
        requested: &Path,
    ) -> Result<Self, Error> {
        if requested.as_os_str().is_empty() {
            return Err(unsafe_target(requested, "the target path is empty"));
        }

        let joined = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            current.join(requested)
        };
        let root = normalize_absolute(&joined).ok_or_else(|| {
            unsafe_target(
                &joined,
                "the target cannot be resolved beneath the filesystem root",
            )
        })?;

        if root.parent().is_none() {
            return Err(unsafe_target(
                &root,
                "the filesystem root cannot be initialized as a project",
            ));
        }
        let name = root
            .file_name()
            .and_then(|name| package_name(&name.to_string_lossy()))
            .ok_or_else(|| Error {
                kind: ErrorKind::InvalidName(root.clone()),
            })?;

        let mut ancestors = root
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
            .collect::<Vec<_>>();
        ancestors.reverse();
        let mut directories = Vec::new();

        for ancestor in ancestors {
            let entry = file_system.inspect(ancestor).map_err(|source| Error {
                kind: ErrorKind::Inspect {
                    path: ancestor.to_path_buf(),
                    source,
                },
            })?;
            let is_target = ancestor == root;

            match (is_target, entry) {
                (_, Entry::Missing) => directories.push(ancestor.to_path_buf()),
                (true, Entry::Directory) => {
                    let empty = file_system
                        .directory_is_empty(&root)
                        .map_err(|source| Error {
                            kind: ErrorKind::Inspect {
                                path: root.clone(),
                                source,
                            },
                        })?;
                    if !empty {
                        return Err(Error {
                            kind: ErrorKind::ExistingNonEmpty(root),
                        });
                    }
                }
                (true, Entry::File) => {
                    return Err(Error {
                        kind: ErrorKind::ExistingFile(root),
                    });
                }
                (true, Entry::Symlink) => {
                    return Err(Error {
                        kind: ErrorKind::ExistingSymlink(root),
                    });
                }
                (true, Entry::Other) => {
                    return Err(unsafe_target(
                        &root,
                        "the existing target is not a regular directory",
                    ));
                }
                (false, Entry::Directory) => {}
                (false, Entry::File) => {
                    return Err(unsafe_ancestor(ancestor, EntryKind::File));
                }
                (false, Entry::Symlink) => {
                    return Err(unsafe_ancestor(ancestor, EntryKind::Symlink));
                }
                (false, Entry::Other) => {
                    return Err(unsafe_ancestor(ancestor, EntryKind::Other));
                }
            }
        }

        Ok(Self {
            root,
            name,
            directories,
        })
    }
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    let mut prefix = None;
    let mut rooted = false;
    let mut components = Vec::<OsString>::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value.as_os_str().to_os_string()),
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop()?;
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }

    if !rooted {
        return None;
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    normalized.push(std::path::MAIN_SEPARATOR.to_string());
    normalized.extend(components);
    Some(normalized)
}

fn unsafe_target(path: &Path, reason: &'static str) -> Error {
    Error {
        kind: ErrorKind::UnsafeTarget {
            path: path.to_path_buf(),
            reason,
        },
    }
}

fn unsafe_ancestor(path: &Path, kind: EntryKind) -> Error {
    Error {
        kind: ErrorKind::UnsafeAncestor {
            path: path.to_path_buf(),
            kind,
        },
    }
}

fn initialization_failure(
    file_system: &impl FileSystem,
    root: &Path,
    path: &Path,
    operation: Operation,
    source: io::Error,
    ledger: &[LedgerEntry],
) -> Error {
    let cleanup = ledger
        .iter()
        .rev()
        .filter_map(|entry| {
            let result = match entry.kind {
                CreatedKind::File => file_system.remove_file(&entry.path),
                CreatedKind::Directory => file_system.remove_directory(&entry.path),
            };
            result.err().map(|source| CleanupFailure {
                path: entry.path.clone(),
                source,
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Error {
        kind: ErrorKind::Initialize {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            operation,
            source,
            cleanup,
        },
    }
}

impl FileSystem for OsFileSystem {
    fn inspect(&self, path: &Path) -> io::Result<Entry> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Entry::Missing),
            Err(source) => return Err(source),
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Ok(Entry::Symlink);
        }
        if file_type.is_file() {
            return Ok(Entry::File);
        }
        if file_type.is_dir() {
            return Ok(Entry::Directory);
        }
        Ok(Entry::Other)
    }

    fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
        Ok(fs::read_dir(path)?.next().is_none())
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<(), NewFileError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|source| NewFileError {
                source,
                created: false,
            })?;
        file.write_all(contents).map_err(|source| NewFileError {
            source,
            created: true,
        })
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_directory(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => formatter.write_str("a file"),
            Self::Symlink => formatter.write_str("a symlink"),
            Self::Other => formatter.write_str("an unsupported filesystem entry"),
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory => formatter.write_str("create directory"),
            Self::WriteManifest => formatter.write_str("write Cargo.toml"),
            Self::WriteToolchain => formatter.write_str("write rust-toolchain.toml"),
            Self::WriteCargoConfig => formatter.write_str("write .cargo/config.toml"),
            Self::WriteSource => formatter.write_str("write starter source"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::CurrentDirectory(source) => {
                write!(formatter, "cannot resolve the current directory: {source}")
            }
            ErrorKind::UnsafeTarget { path, reason } => {
                write!(
                    formatter,
                    "unsafe project target `{}`: {reason}",
                    path.display()
                )
            }
            ErrorKind::InvalidName(path) => write!(
                formatter,
                "`{}` cannot name a Cargo package. \
                 A package name must start with a letter or `_`. \
                 Pick a directory name that does.",
                path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()
            ),
            ErrorKind::Inspect { path, source } => write!(
                formatter,
                "cannot inspect project target `{}`: {source}",
                path.display()
            ),
            ErrorKind::ExistingFile(path) => write!(
                formatter,
                "refusing to overwrite existing file `{}`",
                path.display()
            ),
            ErrorKind::ExistingSymlink(path) => write!(
                formatter,
                "refusing existing project-target symlink `{}`",
                path.display()
            ),
            ErrorKind::ExistingNonEmpty(path) => write!(
                formatter,
                "refusing non-empty project target `{}`",
                path.display()
            ),
            ErrorKind::UnsafeAncestor { path, kind } => write!(
                formatter,
                "unsafe project target: ancestor `{}` is {kind}",
                path.display()
            ),
            ErrorKind::Initialize {
                root,
                path,
                operation,
                source,
                cleanup,
            } => {
                write!(
                    formatter,
                    "could not {operation} `{}` while initializing `{}`: {source}",
                    path.display(),
                    root.display()
                )?;
                if cleanup.is_empty() {
                    formatter.write_str(
                        "; all paths created by this invocation were removed (a pre-existing empty target, if any, was left intact)",
                    )
                } else {
                    formatter.write_str("; rollback was incomplete; inspect and remove only:")?;
                    for failure in cleanup {
                        write!(
                            formatter,
                            " `{}` ({})",
                            failure.path.display(),
                            failure.source
                        )?;
                    }
                    Ok(())
                }
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::CurrentDirectory(source)
            | ErrorKind::Inspect { source, .. }
            | ErrorKind::Initialize { source, .. } => Some(source),
            ErrorKind::UnsafeTarget { .. }
            | ErrorKind::InvalidName(_)
            | ErrorKind::ExistingFile(_)
            | ErrorKind::ExistingSymlink(_)
            | ErrorKind::ExistingNonEmpty(_)
            | ErrorKind::UnsafeAncestor { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{FileSystem, NewFileError, OsFileSystem};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct FailAfterWriting;

    struct FailAfterWritingAndCleanup;

    impl FileSystem for FailAfterWriting {
        fn inspect(&self, path: &Path) -> io::Result<super::Entry> {
            OsFileSystem.inspect(path)
        }

        fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
            OsFileSystem.directory_is_empty(path)
        }

        fn create_directory(&self, path: &Path) -> io::Result<()> {
            OsFileSystem.create_directory(path)
        }

        fn write_new(&self, path: &Path, contents: &[u8]) -> Result<(), NewFileError> {
            OsFileSystem.write_new(path, contents)?;
            Err(NewFileError {
                source: io::Error::other("injected failure after source creation"),
                created: true,
            })
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            OsFileSystem.remove_file(path)
        }

        fn remove_directory(&self, path: &Path) -> io::Result<()> {
            OsFileSystem.remove_directory(path)
        }
    }

    impl FileSystem for FailAfterWritingAndCleanup {
        fn inspect(&self, path: &Path) -> io::Result<super::Entry> {
            OsFileSystem.inspect(path)
        }

        fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
            OsFileSystem.directory_is_empty(path)
        }

        fn create_directory(&self, path: &Path) -> io::Result<()> {
            OsFileSystem.create_directory(path)
        }

        fn write_new(&self, path: &Path, contents: &[u8]) -> Result<(), NewFileError> {
            FailAfterWriting.write_new(path, contents)
        }

        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            Err(io::Error::other("injected cleanup failure"))
        }

        fn remove_directory(&self, path: &Path) -> io::Result<()> {
            OsFileSystem.remove_directory(path)
        }
    }

    #[test]
    fn partial_failure_removes_only_paths_owned_by_the_transaction() {
        let sandbox = temporary_path("rollback");
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("new").join("nested").join("counter");

        let error = super::create_at(&FailAfterWriting, &sandbox, &target).unwrap_err();

        assert!(!target.exists());
        assert!(sandbox.is_dir());
        assert!(error
            .to_string()
            .contains("all paths created by this invocation were removed"));
        std::fs::remove_dir(&sandbox).unwrap();
    }

    #[test]
    fn rollback_leaves_a_pre_existing_empty_target_intact() {
        let sandbox = temporary_path("existing-root");
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("counter");
        std::fs::create_dir(&target).unwrap();

        super::create_at(&FailAfterWriting, &sandbox, &target).unwrap_err();

        assert!(target.is_dir());
        assert!(std::fs::read_dir(&target).unwrap().next().is_none());
        std::fs::remove_dir(&target).unwrap();
        std::fs::remove_dir(&sandbox).unwrap();
    }

    #[test]
    fn cleanup_failure_reports_the_exact_recoverable_paths() {
        let sandbox = temporary_path("cleanup-report");
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("counter");
        std::fs::create_dir(&target).unwrap();

        let error = super::create_at(&FailAfterWritingAndCleanup, &sandbox, &target).unwrap_err();
        let report = error.to_string();

        assert!(report.contains("rollback was incomplete"));
        assert!(report.contains(&target.join("Cargo.toml").display().to_string()));
        std::fs::remove_file(target.join("Cargo.toml")).unwrap();
        std::fs::remove_dir(&target).unwrap();
        std::fs::remove_dir(&sandbox).unwrap();
    }

    #[test]
    fn a_new_crate_is_a_cargo_toml_and_a_marked_main_rs() {
        let sandbox = temporary_path("crate");
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("My Counter");

        let created = super::create_at(&OsFileSystem, &sandbox, &target).unwrap();

        assert_eq!(created.root(), target);
        let manifest = std::fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert_eq!(manifest, super::manifest("my-counter"));
        assert!(manifest.starts_with("[package]\nname = \"my-counter\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"));
        assert!(manifest.contains(&format!(
            "iced = {{ git = \"https://github.com/iced-rs/iced\", rev = \"{}\", features = [\"advanced\", \"canvas\", \"image-without-codecs\", \"qr_code\", \"selector\", \"sipper\", \"svg\"] }}",
            rawr_iced::native::ICED_REVISION
        )));
        #[cfg(feature = "native-runner")]
        let toolchain = std::fs::read_to_string(target.join("rust-toolchain.toml")).unwrap();
        #[cfg(feature = "native-runner")]
        assert_eq!(
            toolchain,
            format!(
                "[toolchain]\nchannel = \"{}\"\ncomponents = [\"rust-src\", \"rustfmt\", \"clippy\"]\n",
                rawr_import::cargo::toolchain::CHANNEL
            )
        );
        #[cfg(feature = "native-runner")]
        assert_eq!(
            std::fs::read_to_string(target.join(".cargo/config.toml")).unwrap(),
            super::cargo_config()
        );
        let source = std::fs::read_to_string(target.join("src/main.rs")).unwrap();
        assert_eq!(source, super::COUNTER_SOURCE);
        assert!(rawr::project::is_marked(&source));
        assert!(matches!(
            rawr::project::Target::discover(&target).unwrap(),
            rawr::project::Target::Project(_)
        ));
        // `rawr check` passes on it; `cargo build` is checked by hand, as it
        // fetches iced.
        #[cfg(feature = "native-runner")]
        assert_eq!(
            crate::check::execute(Some(target.clone())).unwrap(),
            target.join("src/main.rs")
        );
        std::fs::remove_dir_all(&sandbox).unwrap();
    }

    #[test]
    fn package_names_follow_what_cargo_new_accepts() {
        assert_eq!(super::package_name("counter").as_deref(), Some("counter"));
        assert_eq!(super::package_name("My App").as_deref(), Some("my-app"));
        assert_eq!(super::package_name("snake_case-1").as_deref(), Some("snake_case-1"));
        assert_eq!(super::package_name("_x.y").as_deref(), Some("_x-y"));
        assert_eq!(super::package_name("café").as_deref(), Some("caf-"));
        assert_eq!(super::package_name("1up"), None);
        assert_eq!(super::package_name("-dash"), None);
        assert_eq!(super::package_name(""), None);
    }

    #[test]
    fn a_directory_that_names_no_package_is_refused_before_writing() {
        let sandbox = temporary_path("bad-name");
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("2048");

        let error = super::create_at(&OsFileSystem, &sandbox, &target).unwrap_err();

        assert_eq!(
            error.to_string(),
            "`2048` cannot name a Cargo package. A package name must start with a letter or `_`. Pick a directory name that does."
        );
        assert!(!target.exists());
        std::fs::remove_dir(&sandbox).unwrap();
    }

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rawr-new-unit-{label}-{}-{id}", std::process::id()))
    }
}
