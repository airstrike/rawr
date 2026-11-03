//! A `rawr-import-driver` for each nightly a project imports with.
//!
//! The driver links the compiler it reads metadata with, so one build serves
//! one toolchain. The pinned nightly's is the one beside `rawr-import`, built
//! or installed with it. Any other's is built from rawr's own driver sources
//! the first time a project names that toolchain, into rawr's home under
//! `drivers/<rustc commit>-<sources>`, and used from there on: a toolchain is
//! never built for twice, and a driver whose sources or toolchain changed is
//! a new directory. The same build writes that toolchain's std table beside
//! it, which rawr checks programs against.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::identity::Identity;
use crate::{Error, Result};

include!(concat!(env!("OUT_DIR"), "/driver_source.rs"));

const BINARY: &str = "rawr-import-driver";
const STD_TABLE: &str = "std-table";

/// A driver built for one toolchain, with its std table.
#[derive(Debug)]
pub(crate) struct Built {
    directory: PathBuf,
}

impl Built {
    /// The driver for `identity`, installed at `root`, under rawr's `home`,
    /// built first if there is none. `environment` is the import's.
    pub(crate) fn ensure(
        home: &Path,
        identity: &Identity,
        root: &Path,
        environment: &[(OsString, OsString)],
    ) -> Result<Self> {
        let drivers = home.join("drivers");
        let directory = drivers.join(key(identity));
        let built = Self { directory };
        if built.driver().is_file() {
            return Ok(built);
        }
        fs::create_dir_all(&drivers).map_err(|source| io("create", &drivers, source))?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(drivers.join(".lock"))
            .map_err(|source| io("lock", &drivers, source))?;
        let mut lock = fd_lock::RwLock::new(lock);
        let _guard = lock
            .write()
            .map_err(|source| io("lock", &drivers, source))?;
        if built.driver().is_file() {
            return Ok(built);
        }
        crate::progress::say(&format!(
            "Building rawr-import-driver for {} (once per toolchain)",
            identity.name()
        ));
        let staging = drivers.join(format!(".staging-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        let result = build(&staging, identity, root, environment).and_then(|()| {
            fs::rename(&staging, &built.directory)
                .map_err(|source| io("install", &built.directory, source))
        });
        let _ = fs::remove_dir_all(&staging);
        result.map(|()| built)
    }

    /// The driver executable.
    pub(crate) fn driver(&self) -> PathBuf {
        self.directory.join(BINARY)
    }

    /// The directory of the toolchain's std table (`paths.tsv`, `items.tsv`,
    /// `impls.tsv`).
    #[allow(dead_code, reason = "read once the std table loads at run time")]
    pub(crate) fn std_table(&self) -> PathBuf {
        self.directory.join(STD_TABLE)
    }
}

/// The directory name: the toolchain's rustc commit and the driver sources'
/// digest, each shortened.
fn key(identity: &Identity) -> String {
    let mut text = String::with_capacity(33);
    text.push_str(
        identity
            .rustc_commit()
            .get(..16)
            .unwrap_or(identity.rustc_commit()),
    );
    text.push('-');
    for byte in &DIGEST[..8] {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The manifest of the package the driver is built from: the driver alone,
/// with the importer's dependency requirements for the crates it uses.
const MANIFEST: &str = r#"[package]
name = "rawr-import-driver"
version = "0.0.0"
edition = "2024"
publish = false
build = "importer/build.rs"

[features]
default = ["rustc-driver"]
rustc-driver = []

[[bin]]
name = "rawr-import-driver"
path = "importer/src/driver.rs"
test = false

[dependencies]
libc = "0.2"
rmp-serde = "1.3.1"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["raw_value"] }
sha2 = "0.10"

[build-dependencies]
sha2 = "0.10"

[workspace]
"#;

/// Build the driver into `staging` with the toolchain's own Cargo and rustc,
/// then write its std table beside it.
fn build(
    staging: &Path,
    identity: &Identity,
    root: &Path,
    environment: &[(OsString, OsString)],
) -> Result<()> {
    let source = staging.join("source");
    write_sources(&source)?;
    let target = staging.join("target");
    let rustc = root.join("bin/rustc");
    let mut command = Command::new(root.join("bin/cargo"));
    command
        .args(["build", "--release", "--quiet", "--manifest-path"])
        .arg(source.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .current_dir(&source)
        .env_clear()
        .envs(
            environment
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.to_str(),
                        Some("CARGO_BUILD_TARGET" | "CARGO_TARGET_DIR" | "CARGO_BUILD_TARGET_DIR")
                    )
                })
                .cloned(),
        )
        .env("RUSTC", &rustc)
        .env("RUSTUP_TOOLCHAIN", identity.name())
        .env("CARGO_INCREMENTAL", "0");
    run(&mut command, "cargo build")?;
    let driver = staging.join(BINARY);
    fs::rename(target.join("release").join(BINARY), &driver)
        .map_err(|source| io("install", &driver, source))?;
    let _ = fs::remove_dir_all(&target);
    let _ = fs::remove_dir_all(&source);

    let table = staging.join(STD_TABLE);
    fs::create_dir_all(&table).map_err(|source| io("create", &table, source))?;
    let mut command = Command::new(&driver);
    command
        .arg("--std-table")
        .arg(&rustc)
        .arg(&table)
        .arg(super::HOST)
        .env_clear()
        .envs(environment.iter().cloned());
    if let Some((key, value)) = crate::process::driver_library(&rustc, environment) {
        command.env(key, value);
    }
    run(&mut command, "rawr-import-driver --std-table")
}

/// Write the driver's package: the embedded sources, or without them, the
/// same files from this checkout.
fn write_sources(source: &Path) -> Result<()> {
    let write = |relative: &str, bytes: &[u8]| {
        let path = source.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| io("create", parent, error))?;
        }
        fs::write(&path, bytes).map_err(|error| io("write", &path, error))
    };
    write("Cargo.toml", MANIFEST.as_bytes())?;
    if FILES.is_empty() {
        for relative in NAMES {
            let path = Path::new(CHECKOUT).join(relative);
            let bytes = fs::read(&path).map_err(|error| io("read", &path, error))?;
            write(relative, &bytes)?;
        }
    } else {
        for (relative, bytes) in FILES {
            write(relative, bytes)?;
        }
    }
    Ok(())
}

fn run(command: &mut Command, program: &'static str) -> Result<()> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|source| Error::Process {
            program,
            status: "failed to spawn".into(),
            stderr: source.to_string().into_boxed_str(),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Process {
            program,
            status: output.status.to_string().into_boxed_str(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .into_owned()
                .into_boxed_str(),
        })
    }
}

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::{key, MANIFEST};
    use crate::cargo::toolchain::Identity;

    /// The driver's package asks for each crate it uses what the importer
    /// asks for, so the lockfile pins the same versions.
    #[test]
    fn the_driver_manifest_asks_what_the_importer_does() {
        let importer = include_str!("../../../Cargo.toml")
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let driver = MANIFEST.parse::<toml_edit::DocumentMut>().unwrap();
        let requirement = |item: &toml_edit::Item| {
            item.as_str()
                .or_else(|| item.get("version").and_then(toml_edit::Item::as_str))
                .map(str::to_owned)
        };
        for (name, item) in driver["dependencies"].as_table().unwrap() {
            assert_eq!(
                requirement(item),
                requirement(&importer["dependencies"][name]),
                "{name}"
            );
        }
    }

    #[test]
    fn a_driver_is_keyed_by_its_toolchain_and_sources() {
        let pinned = key(Identity::pinned());
        assert!(pinned.starts_with("375b1431b7d89d1c-"), "{pinned}");
        assert_eq!(pinned.len(), 33);
    }
}
