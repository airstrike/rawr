//! Fixed-path Cargo build transform from generated to built manifest.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod cached;

#[cfg(all(test, feature = "test-fixtures"))]
pub(crate) use cached::fixture_build;

impl<B> super::Manifest<super::Generated, B> {
    pub(crate) fn replay_hot(
        &mut self,
        live: &super::Manifest<super::Built, B>,
    ) -> Result<crate::node::read_set::Prepared, crate::node::Error> {
        cached::replay_hot(self, live)
    }

    /// Build once under the sole Cargo owner or revalidate a complete warm hit.
    pub fn build(
        self,
        cargo: &crate::cargo::Owner,
        cache: &crate::cache::Writer,
    ) -> Result<super::Manifest<super::Built, B>, Error> {
        self.build_with_owner(Some(cargo), cache)
    }

    /// Reuse a complete local/shared artifact before acquiring the sole Cargo
    /// owner. A true miss acquires that owner only for the compile transform.
    pub fn build_on_demand(
        self,
        cache: &crate::cache::Writer,
    ) -> Result<super::Manifest<super::Built, B>, Error> {
        self.build_with_owner(None, cache)
    }

    fn build_with_owner(
        self,
        cargo: Option<&crate::cargo::Owner>,
        cache: &crate::cache::Writer,
    ) -> Result<super::Manifest<super::Built, B>, Error> {
        if self.chain.is_some() || self.profile.node_chain().is_some() {
            return cached::build(self, cargo, cache);
        }
        let mut lease = cache.lease()?;
        let _writer = lease.try_write().map_err(|source| {
            if source.kind() == std::io::ErrorKind::WouldBlock {
                crate::cache::entry::Error::Busy
            } else {
                crate::cache::entry::Error::Io {
                    operation: "acquire runner cache writer lease",
                    path: cache.root().join(".writer.lock"),
                    source,
                }
            }
        })?;
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

        let acquired = if cargo.is_none() {
            Some(crate::cargo::Owner::acquire()?)
        } else {
            None
        };
        let entry = crate::cache::entry::prepare(cache, key, incoming)?;
        let result = self.cold(&entry);
        drop(acquired);
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
        let executable = executable(&entry);
        let output = entry.join("target");
        let artifact = super::artifact::BuiltArtifactProof::seal(
            &self.generated,
            &executable,
            &output,
            &[],
            &crate::cargo::Limits::standard(),
        )?;
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
        for file in &self.files {
            let path = entry.join(file.path());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| Error::Io {
                    operation: "create generated runner source directory",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&path, file.bytes()).map_err(|source| Error::Io {
                operation: "write generated runner file",
                path,
                source,
            })?;
        }
        let carrier = self.generated.carrier()?;
        let carrier_path = entry.join("rawr-generated-manifest.carrier");
        fs::write(&carrier_path, &carrier).map_err(|source| Error::Io {
            operation: "write generated manifest carrier",
            path: carrier_path,
            source,
        })?;
        let target = entry.join("target");
        let mut command = Command::new(cargo());
        command
            .args(["build", "--offline", "--locked", "-j1", "--manifest-path"])
            .arg(entry.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target);
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
            &executable(entry),
            &target,
            &[],
            &crate::cargo::Limits::standard(),
        )?;
        Ok(artifact)
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

fn executable(entry: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "rawr-generated-runner.exe"
    } else {
        "rawr-generated-runner"
    };
    entry.join("target/debug").join(name)
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
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
    #[error(transparent)]
    Manifest(#[from] super::manifest::Error),
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
