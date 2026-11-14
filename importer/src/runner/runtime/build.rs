//! The cargo build directory the runtimes of one base key share (#1983).
//!
//! A runtime entry's key names the host it was compiled from, so a new
//! build of rawr asks for a new entry. Compiled in a directory of its own,
//! each new entry would compile every crate from scratch. Instead, every
//! entry whose build agrees on the base key (the toolchain, the lockfile,
//! the runtime's manifest with its features and opt-level, the hashing
//! seeds) compiles in one build directory, `runtime-builds/<base>` in the
//! runtime home. Its sources are a stable copy of the host's (`source`), so
//! cargo's own fingerprints decide what an edit makes dirty. An entry keeps
//! only copies of the libraries a runner links, never links to the build
//! directory's, which the next build overwrites.
//!
//! A build directory goes once no entry names it, under the lock that every
//! build of it holds, so nothing is removed by age.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest as _, Sha256};

use super::{native, source, Toolchain};
use crate::node;

/// What the base key covers is named apart from an entry's key.
const DOMAIN: &[u8] = b"rawr.runtime.build/v1";
/// In the runtime home: one build directory per base key.
const BUILDS: &str = "runtime-builds";
/// In an entry: the base key of the build directory that compiled it.
const BASE: &str = "base";

/// The build directory of one base key, with what a build there writes.
pub(super) struct Build {
    directory: PathBuf,
    key: String,
    source: Source,
    wrapper: String,
    lock: Vec<u8>,
}

/// Where a build reads rawr's sources.
enum Source {
    /// A written host, copied to the build directory's stable path first.
    Host(PathBuf),
    /// A development checkout, which names itself and is read in place.
    Checkout(PathBuf),
}

impl Build {
    /// The build of the runtime of rawr at `root`, as `dependency` names it,
    /// for `toolchain` in `environment`. `host` is the digest of the sources
    /// at `root`: zero for a development checkout, whose path then joins the
    /// base key and whose files are read where they are.
    pub(super) fn plan(
        root: &Path,
        dependency: &dyn toml_edit::TableLike,
        toolchain: &Toolchain,
        environment: &crate::cargo::config::Environment,
        host: &[u8; 32],
    ) -> Result<Self, node::Error> {
        let level = super::opt_level(environment)
            .map_err(|reason| node::Error::Transform(reason.into_boxed_str()))?;
        let lock = fs::read(root.join("Cargo.lock")).map_err(|source| node::Error::Io {
            operation: "read the lockfile of rawr's runtime",
            path: root.join("Cargo.lock"),
            source,
        })?;
        let checkout = *host == [0; 32];
        let mut key = Sha256::new();
        for part in [
            DOMAIN,
            if checkout {
                root.as_os_str().as_encoded_bytes()
            } else {
                b""
            },
            &Sha256::digest(&lock),
            toolchain.version.as_bytes(),
            // The manifest with no path: the build directory names its own.
            super::wrapper_manifest(Path::new(""), dependency, &level).as_bytes(),
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
        let key = super::hex(&key.finalize());
        let directory = builds()?.join(&key[..16]);
        let source = if checkout {
            Source::Checkout(root.to_path_buf())
        } else {
            Source::Host(root.to_path_buf())
        };
        let compiled = match &source {
            Source::Host(_) => directory.join("source"),
            Source::Checkout(root) => root.clone(),
        };
        Ok(Self {
            wrapper: super::wrapper_manifest(&compiled, dependency, &level),
            directory,
            key,
            source,
            lock,
        })
    }

    /// The key of the entry this build compiles for the host `host`.
    pub(super) fn entry_key(&self, host: &[u8; 32]) -> String {
        let mut key = Sha256::new();
        for part in [super::DOMAIN, self.key.as_bytes(), host] {
            key.update((part.len() as u64).to_le_bytes());
            key.update(part);
        }
        super::hex(&key.finalize())
    }

    /// Whether this build reads a development checkout in place, where cargo
    /// decides at every build what is fresh.
    pub(super) fn reads_checkout(&self) -> bool {
        matches!(self.source, Source::Checkout(_))
    }

    /// Compile the runtime with the project's toolchain and publish copies
    /// of its libraries in `entry/lib`, stamped with `key`. The caller holds
    /// the entry's exclusive lock; this holds the build directory's for as
    /// long as it writes there.
    pub(super) fn compile(
        &self,
        entry: &Path,
        key: &str,
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
        // Named before the build directory is locked, so a prune that lists
        // the entries from here on keeps it.
        fs::write(entry.join(BASE), &self.key)
            .map_err(io("record the build of rawr's runtime", entry.join(BASE)))?;
        let _lock = self.lock()?;
        crate::progress::note(
            "runtime-compile",
            "rawr: compiling rawr's runtime for this toolchain; later projects reuse it",
        );
        if let Source::Host(host) = &self.source {
            source::mirror(host, &self.directory.join("source")).map_err(io(
                "copy rawr's sources for its runtime",
                self.directory.join("source"),
            ))?;
        }
        let package = self.directory.join("runtime");
        fs::create_dir_all(&package).map_err(io("write rawr's runtime", package.clone()))?;
        super::write_changed(&package.join("Cargo.toml"), self.wrapper.as_bytes())?;
        super::write_changed(&package.join("lib.rs"), b"")?;
        // The runtime resolves from the lockfile rawr was released with. Cargo
        // drops what the runtime doesn't need and changes no version.
        let source = package.join("source.lock");
        if fs::read(&source).ok().as_deref() != Some(self.lock.as_slice()) {
            fs::write(package.join("Cargo.lock"), &self.lock).map_err(io(
                "write the lockfile of rawr's runtime",
                package.join("Cargo.lock"),
            ))?;
            fs::write(&source, &self.lock).map_err(io("record rawr's lockfile", source.clone()))?;
        }
        let messages = self.cargo(&package, toolchain, environment)?;
        publish(entry, key, &messages)
    }

    /// The build directory, locked for this build. A prune that retired it
    /// between its creation and the lock took the lock file with it, and the
    /// second attempt makes the directory anew.
    fn lock(&self) -> Result<File, node::Error> {
        let path = self.directory.join(".lock");
        for _ in 0..2 {
            let Ok(lock) = fs::create_dir_all(&self.directory).and_then(|()| {
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(&path)
            }) else {
                continue;
            };
            lock.lock().map_err(|source| node::Error::Io {
                operation: "lock the build of rawr's runtime",
                path: path.clone(),
                source,
            })?;
            if path.exists() {
                return Ok(lock);
            }
        }
        Err(node::Error::Io {
            operation: "lock the build of rawr's runtime",
            path,
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })
    }

    /// Run cargo on the runtime's `package`, with its log in the build
    /// directory, and return its JSON messages.
    fn cargo(
        &self,
        package: &Path,
        toolchain: &Toolchain,
        environment: &crate::cargo::config::Environment,
    ) -> Result<Vec<u8>, node::Error> {
        let io = |operation: &'static str, path: PathBuf| {
            move |source| node::Error::Io {
                operation,
                path,
                source,
            }
        };
        let target = self.directory.join("target");
        let build = toml_edit::Value::from(target.to_string_lossy().as_ref()).to_string();
        let mut command = Command::new(&toolchain.cargo);
        command
            .args(["build", "--message-format=json-render-diagnostics"])
            .arg("--config")
            .arg(format!("build.build-dir={build}"))
            .arg("--manifest-path")
            .arg(package.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .current_dir(package)
            .env_clear()
            .envs(environment.entries().filter(|(key, _)| !overrides(key)))
            // An empty value overrides every other source of rustflags.
            .env("CARGO_ENCODED_RUSTFLAGS", "")
            .env("RUSTC", &toolchain.rustc)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The toolchain's Cargo runs without its rustup proxy, so the runtime's
        // build scripts need the library path the proxy would have set, as the
        // project's own build does.
        if let Some(loader) = environment.loader_path(&toolchain.rustc) {
            command.env(crate::cargo::config::LOADER_PATH, loader);
        }
        let mut child = command.spawn().map_err(io(
            "launch the build of rawr's runtime",
            toolchain.cargo.clone(),
        ))?;
        let stderr = child.stderr.take();
        let log = self.directory.join("build.log");
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
        Ok(messages)
    }

    /// This build's directory.
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }
}

/// `runtime-builds` in the runtime home.
fn builds() -> Result<PathBuf, node::Error> {
    Ok(super::runtimes()?
        .parent()
        .map_or_else(|| PathBuf::from(BUILDS), |home| home.join(BUILDS)))
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

/// Copy the libraries cargo's `messages` name into a fresh `entry/lib`, so
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
        // A copy: the build directory overwrites its libraries at its next
        // build, which must never reach a runner linking this entry.
        fs::copy(path, staging.join(name))
            .map_err(io("stage a library of rawr's runtime", path.clone()))?;
    }
    let lib = entry.join("lib");
    let retired = entry.join(format!(".lib-old-{id}"));
    if lib.exists() {
        fs::rename(&lib, &retired).map_err(io("retire rawr's runtime", lib.clone()))?;
    }
    fs::rename(&staging, &lib).map_err(io("publish rawr's runtime", lib.clone()))?;
    let _ = fs::remove_dir_all(&retired);
    let native = native::Link::all_in(&crate::cargo::messages::Messages::parse(messages))
        .iter()
        .map(native::Link::record)
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

/// Best effort: remove the build directories under the runtime home whose
/// `runtimes` hold no entry that names them, other than `keep`. One being
/// built is locked and skipped; nothing here waits for a lock. Each goes by
/// being renamed aside under its lock first, so a partial removal is never
/// taken for a build directory and is finished by a later prune.
pub(super) fn prune(runtimes: &Path, keep: &[&Path]) {
    let Some(builds) = runtimes.parent().map(|home| home.join(BUILDS)) else {
        return;
    };
    let Ok(listed) = fs::read_dir(&builds) else {
        return;
    };
    let named = fs::read_dir(runtimes)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path().join(BASE)).ok())
        .filter_map(|key| key.get(..16).map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for build in listed.flatten() {
        let name = build.file_name().to_string_lossy().into_owned();
        let path = build.path();
        if name.starts_with(".retired-") {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        if name.starts_with('.')
            || named.contains(&name)
            || keep.contains(&path.as_path())
            || !build.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let Ok(lock) = File::open(path.join(".lock")) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let retired = builds.join(format!(".retired-{name}-{}", std::process::id()));
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

    #[test]
    fn a_build_directory_goes_once_no_entry_names_it() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-runtime-builds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runtimes = root.join("runtime");
        let builds = root.join(super::BUILDS);
        let named = "0123456789abcdef";
        for name in [
            named,
            "fedcba9876543210",
            "1111111111111111",
            "2222222222222222",
        ] {
            fs::create_dir_all(builds.join(name).join("target")).unwrap();
            fs::write(builds.join(name).join(".lock"), b"").unwrap();
        }
        fs::create_dir_all(builds.join(".retired-old-1/target")).unwrap();
        fs::create_dir_all(runtimes.join("entry")).unwrap();
        fs::write(
            runtimes.join("entry").join(super::BASE),
            format!("{named}{}", "0".repeat(48)),
        )
        .unwrap();
        // A build under way holds its lock.
        let building = fs::File::open(builds.join("1111111111111111/.lock")).unwrap();
        building.lock().unwrap();
        let kept = builds.join("2222222222222222");
        super::prune(&runtimes, &[kept.as_path()]);
        let mut left = fs::read_dir(&builds)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(
            left,
            ["0123456789abcdef", "1111111111111111", "2222222222222222"]
        );
        drop(building);
        fs::remove_dir_all(root).unwrap();
    }
}
