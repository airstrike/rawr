//! Fixed-path Cargo build transform from generated to built manifest.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod cached;

pub(crate) use cached::EXECUTION_SCHEMA;

#[cfg(all(test, feature = "test-fixtures", feature = "rustc-driver"))]
pub(crate) use cached::fixture_build;

impl<B> super::Manifest<super::Generated, B> {
    pub(crate) fn replay_hot(
        &mut self,
        live: &super::Manifest<super::Built, B>,
    ) -> Result<crate::node::read_set::Prepared, crate::node::Error> {
        cached::replay_hot(self, live)
    }

    /// Reuse a complete local or shared artifact, or build one on the run
    /// owner's Cargo `lane`.
    pub fn build(
        self,
        lane: &mut crate::cargo::Lane,
        cache: &crate::cache::Writer,
    ) -> Result<super::Manifest<super::Built, B>, Error> {
        // A launch with its runner built, or found built, is starting it.
        self.build_on(lane, cache)
            .inspect(|_| crate::progress::stage(4))
    }

    fn build_on(
        self,
        lane: &mut crate::cargo::Lane,
        cache: &crate::cache::Writer,
    ) -> Result<super::Manifest<super::Built, B>, Error> {
        if self.chain.is_some() || self.profile.node_chain().is_some() {
            return cached::build(self, lane, cache);
        }
        let _writer = cache.lease()?;
        let key = self.generated.build_request_key();
        let entry = crate::cache::entry::path(cache.root(), key);
        if crate::cache::seal::exists(&entry) {
            return match self.warm(entry) {
                Ok(built) => {
                    crate::metrics::record_node(
                        crate::metrics::NodeKind::Runner,
                        crate::metrics::NodeEvent::ValidatedHit,
                    );
                    Ok(built)
                }
                Err(error) => {
                    crate::metrics::record_node(
                        crate::metrics::NodeKind::Runner,
                        crate::metrics::NodeEvent::RejectedHit,
                    );
                    Err(error)
                }
            };
        }
        let incoming = self
            .files
            .iter()
            .try_fold(0_u64, |bytes, file| {
                bytes.checked_add(file.bytes().len() as u64)
            })
            .ok_or(Error::Cache(crate::cache::entry::Error::Quota))?;
        crate::metrics::record_node(
            crate::metrics::NodeKind::Runner,
            crate::metrics::NodeEvent::ExecutedMiss,
        );

        let entry = crate::cache::entry::prepare(cache, key, incoming)?;
        let result = self.cold(&entry);
        if result.is_ok() {
            crate::metrics::record_node(
                crate::metrics::NodeKind::Runner,
                crate::metrics::NodeEvent::Published,
            );
        }
        if result.is_err() {
            let _ = fs::remove_dir_all(&entry);
        }
        result
    }

    fn warm(self, entry: PathBuf) -> Result<super::Manifest<super::Built, B>, Error> {
        validate_carrier(&entry, &self.generated)?;
        let executable = executable(&entry, &self.files);
        let output = entry.join("target");
        let artifact =
            super::artifact::BuiltArtifactProof::seal(&self.generated, &executable, &output, &[])?;
        crate::cache::seal::validate(&entry, &self.generated.digest(), &artifact.digest())?;
        let pin = crate::cache::pins::Pin::acquire(&entry)?;
        Ok(self.built(
            super::Native::Legacy {
                artifact,
                _pin: pin,
            },
            None,
        ))
    }

    fn cold(self, entry: &Path) -> Result<super::Manifest<super::Built, B>, Error> {
        let artifact = self.compile_entry(entry)?;
        crate::cache::seal::publish(entry, &self.generated.digest(), &artifact.digest())?;
        let pin = crate::cache::pins::Pin::acquire(entry)?;
        Ok(self.built(
            super::Native::Legacy {
                artifact,
                _pin: pin,
            },
            None,
        ))
    }

    fn compile_entry(&self, entry: &Path) -> Result<super::artifact::BuiltArtifactProof, Error> {
        self.write_sources(entry)?;
        let target = entry.join("target");
        let mut command = Command::new(cargo());
        command.args(["build", "--offline", "--locked"]);
        match shared_build_directory() {
            // One Cargo build at a time holds a shared build directory, so
            // Cargo's own job count applies.
            Some(build) => {
                let value = toml_edit::Value::from(build.to_string_lossy().as_ref());
                command
                    .arg("--config")
                    .arg(format!("build.build-dir={value}"));
            }
            None => {
                command.arg("-j1");
            }
        }
        command
            .arg("--manifest-path")
            .arg(entry.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            // The program's stdin is the program's (D500.1).
            .stdin(std::process::Stdio::null());
        let mut child = command.spawn().map_err(|source| Error::Io {
            operation: "launch generated runner build",
            path: entry.join("Cargo.toml"),
            source,
        })?;
        crate::metrics::record_process(crate::metrics::ProcessKind::CargoRunner);
        let status = child.wait().map_err(|source| Error::Io {
            operation: "wait for generated runner build",
            path: entry.join("Cargo.toml"),
            source,
        })?;
        if !status.success() {
            return Err(Error::Build(status.code()));
        }
        let artifact = super::artifact::BuiltArtifactProof::seal(
            &self.generated,
            &executable(entry, &self.files),
            &target,
            &[],
        )?;
        Ok(artifact)
    }

    /// Write the runner's sources into `directory` as Cargo builds them:
    /// rawr's crate named at the host rather than at the Cargo view its
    /// resolution read, each member rebased on that manifest, the lock
    /// settled with rawr's own dependency graph, which the harness locked
    /// rawr's crate without (#1914), and the manifest carrier beside them.
    pub fn write_sources(&self, directory: &Path) -> Result<(), Error> {
        use crate::bridge::generate;
        // The runner compiles rawr's crate from the host, not from the
        // Cargo view its resolution read, and so does each of its members.
        let runner = self
            .files
            .iter()
            .find(|file| file.path() == "Cargo.toml")
            .and_then(|file| std::str::from_utf8(file.bytes()).ok())
            .map(|text| crate::host::relocated(text).unwrap_or_else(|| text.to_owned()));
        let parsed = runner
            .as_deref()
            .and_then(|runner| runner.parse::<toml_edit::DocumentMut>().ok());
        for file in &self.files {
            let path = directory.join(file.path());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| Error::Io {
                    operation: "create generated runner source directory",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let relocated = match file.path() {
                "Cargo.toml" => runner.clone(),
                path if generate::is_member_manifest(path) => parsed.as_ref().and_then(|runner| {
                    let mut member = std::str::from_utf8(file.bytes())
                        .ok()?
                        .parse::<toml_edit::DocumentMut>()
                        .ok()?;
                    generate::member::rebase(runner, &mut member)?;
                    Some(member.to_string())
                }),
                _ => None,
            };
            let bytes = relocated.as_deref().map_or(file.bytes(), str::as_bytes);
            fs::write(&path, bytes).map_err(|source| Error::Io {
                operation: "write generated runner file",
                path,
                source,
            })?;
        }
        let carrier = self.generated.carrier()?;
        let carrier_path = directory.join("rawr-generated-manifest.carrier");
        fs::write(&carrier_path, &carrier).map_err(|source| Error::Io {
            operation: "write generated manifest carrier",
            path: carrier_path,
            source,
        })?;
        settle_lock(directory)
    }

    fn built(
        self,
        native: super::Native,
        read_set: Option<crate::node::read_set::Prepared>,
    ) -> super::Manifest<super::Built, B> {
        super::Manifest {
            request: self.request,
            native: self.native,
            profile: self.profile,
            revision: self.revision,
            generated: self.generated,
            files: self.files,
            chain: self.chain,
            overlay: self.overlay,
            phase: super::Built { native, read_set },
        }
    }
}

/// The runner's executable that Cargo's build of `entry` leaves, by the
/// name its generated manifest in `files` gives it.
fn executable(entry: &Path, files: &[super::manifest::File]) -> PathBuf {
    let manifest = files
        .iter()
        .find(|file| file.path() == "Cargo.toml")
        .and_then(|file| std::str::from_utf8(file.bytes()).ok())
        .unwrap_or_default();
    let name = crate::bridge::generate::member::executable(manifest);
    entry.join("target/debug").join(name)
}

/// Settle the lock of the runner in `directory` to its manifest: Cargo adds
/// what the lock lacks, rawr's crate graph among it, and changes no package
/// the lock already pins.
fn settle_lock(directory: &Path) -> Result<(), Error> {
    let manifest = directory.join("Cargo.toml");
    let output = Command::new(cargo())
        .args(["update", "--workspace", "--offline", "--quiet"])
        .arg("--manifest-path")
        .arg(&manifest)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|source| Error::Io {
            operation: "settle the generated runner's lockfile",
            path: manifest.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::Lock(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    // Cargo settled with every patch; the runner builds without the ones it
    // left unused (`runner::patches`).
    let lock = directory.join("Cargo.lock");
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|source| Error::Io {
            operation: "read the settled generated runner",
            path: path.to_path_buf(),
            source,
        })
    };
    if let Some((pruned_manifest, pruned_lock)) =
        super::patches::pruned(&read(&manifest)?, &read(&lock)?)
    {
        for (path, text) in [(&manifest, pruned_manifest), (&lock, pruned_lock)] {
            fs::write(path, text).map_err(|source| Error::Io {
                operation: "write the settled generated runner",
                path: path.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

/// The Cargo build directory every fixed-path runner build in a test process
/// shares, so `rawr` and its dependencies compile once per test suite rather
/// than once per test.
#[cfg(feature = "test-fixtures")]
static SHARED_BUILD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Share `directory` as the build directory of every fixed-path runner build
/// in this process. The first call wins.
#[cfg(feature = "test-fixtures")]
#[doc(hidden)]
pub fn share_build_directory(directory: PathBuf) {
    let _ = SHARED_BUILD.set(directory);
}

#[cfg(feature = "test-fixtures")]
fn shared_build_directory() -> Option<&'static Path> {
    SHARED_BUILD.get().map(PathBuf::as_path)
}

#[cfg(not(feature = "test-fixtures"))]
const fn shared_build_directory() -> Option<&'static Path> {
    None
}

/// Generated runner build or cache failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Import(#[from] crate::Error),
    #[error(transparent)]
    Node(#[from] crate::node::Error),
    #[error(transparent)]
    Cache(#[from] crate::cache::entry::Error),
    #[error(transparent)]
    Artifact(#[from] super::artifact::Error),
    #[error("generated runner I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("generated runner Cargo build failed with status {0:?}")]
    Build(Option<i32>),
    #[error("cargo could not settle the generated runner's lockfile: {0}")]
    Lock(Box<str>),
    #[error(transparent)]
    Manifest(#[from] super::manifest::Error),
}

impl Error {
    /// Whether a cache entry this build read went stale before it was used
    /// (`crate::Error::is_stale`), so the run prepares again (#613).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match self {
            Self::Import(error) => error.is_stale(),
            Self::Node(error) => error.is_stale(),
            _ => false,
        }
    }
}

fn validate_carrier(
    entry: &Path,
    manifest: &super::manifest::GeneratedManifest,
) -> Result<(), Error> {
    let path = entry.join("rawr-generated-manifest.carrier");
    let actual = fs::read(&path).map_err(|source| Error::Io {
        operation: "read generated manifest carrier",
        path,
        source,
    })?;
    if actual.as_slice() != manifest.carrier()?.as_ref() {
        return Err(Error::Manifest(super::manifest::Error::Carrier));
    }
    Ok(())
}
