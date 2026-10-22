//! Per-crate extraction reuse. A selected crate's protocol body (every
//! sidecar field after the handshake) is a function of the crate's metadata
//! and what Rawr asks of it: it names no file paths, and its crate hash (SVH)
//! folds in every upstream crate's. A body is kept once per key under the
//! project's `<target>/rawr/extraction`, frozen and digest-checked; an
//! unchanged crate's sidecar is its fresh handshake around the kept body.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{ExternalCrate, Invocation, Root, Sysroot, PROTOCOL_SCHEMA};

pub(super) struct Cache {
    directory: PathBuf,
    driver: [u8; 32],
    used: BTreeSet<String>,
}

/// The handshake parts extraction derives: the sysroot crates it named and
/// the external crates it reached, by identity; their artifacts are this
/// session's loaded paths.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    digest: String,
    sysroot: Vec<Sysroot>,
    external: Vec<(String, String)>,
}

pub(super) struct Reused {
    pub(super) sysroot: Vec<Sysroot>,
    pub(super) external: Vec<ExternalCrate>,
    pub(super) body: Vec<u8>,
}

impl Cache {
    pub(super) fn open(directory: &Path) -> Result<Self, String> {
        // The extraction code is the driver's own; a rebuilt driver keys anew.
        let executable =
            std::env::current_exe().map_err(|error| format!("driver executable: {error}"))?;
        let driver = fs::read(&executable)
            .map_err(|error| format!("failed to hash the driver executable: {error}"))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            driver: Sha256::digest(driver).into(),
            used: BTreeSet::new(),
        })
    }

    /// The crate's semantic invocation, its verified metadata digest, and its
    /// SVH and `StableCrateId`, never the nonce, correlation or paths.
    pub(super) fn key(
        &self,
        invocation: &Invocation,
        crate_hash: &str,
        stable_crate_id: &str,
    ) -> String {
        let mut digest = Sha256::new();
        let mut frame = |bytes: &[u8]| {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        };
        frame(b"rawr.extract.reuse/v1");
        frame(&self.driver);
        frame(&PROTOCOL_SCHEMA.to_le_bytes());
        let Root::Selected { aliases } = &invocation.root;
        let package = &invocation.package;
        let artifact = &invocation.artifact;
        for value in [
            &invocation.compiler_commit,
            &invocation.host,
            &invocation.selected_target,
            &package.cargo_id,
            &package.name,
            &package.version,
            &package.source,
            &invocation.unit_key,
            &invocation.target,
            &invocation.target_kind,
            &invocation.crate_name,
            &invocation.edition,
            &invocation.profile,
            &artifact.digest,
            &artifact.profile,
            &artifact.probe_alias,
        ] {
            frame(value.as_bytes());
        }
        frame(&artifact.length.to_le_bytes());
        frame(crate_hash.as_bytes());
        frame(stable_crate_id.as_bytes());
        for list in [
            aliases,
            &invocation.dependencies,
            &invocation.target_kinds,
            &invocation.crate_types,
            &invocation.features,
        ] {
            frame(&(list.len() as u64).to_le_bytes());
            for value in list {
                frame(value.as_bytes());
            }
        }
        frame(&(invocation.demand.len() as u64).to_le_bytes());
        for path in &invocation.demand {
            frame(&(path.len() as u64).to_le_bytes());
            for segment in path {
                frame(segment.as_bytes());
            }
        }
        super::hex(&digest.finalize()).unwrap_or_default()
    }

    /// The kept body for `key` whose digest holds and whose external crates are
    /// all loaded in this session; anything else is a miss.
    pub(super) fn load(&mut self, key: &str, loaded: &[ExternalCrate]) -> Option<Reused> {
        self.used.insert(key.to_owned());
        let mut bytes = fs::read(self.directory.join(key)).ok()?;
        let split = bytes.iter().rposition(|byte| *byte == b'\n')?;
        let header: Header = serde_json::from_slice(&bytes[split + 1..]).ok()?;
        bytes.truncate(split);
        if super::hex(&Sha256::digest(&bytes)).ok()? != header.digest {
            return None;
        }
        let mut external = Vec::with_capacity(header.external.len());
        for (name, id) in &header.external {
            external.push(
                loaded
                    .iter()
                    .find(|crate_| &crate_.crate_name == name && &crate_.stable_crate_id == id)?
                    .clone(),
            );
        }
        Some(Reused {
            sysroot: header.sysroot,
            external,
            body: bytes,
        })
    }

    /// Keep a freshly extracted body, frozen. Failure only costs a later miss.
    pub(super) fn store(
        &self,
        key: &str,
        sysroot: &[Sysroot],
        external: &[ExternalCrate],
        body: &[u8],
    ) {
        let header = Header {
            digest: super::hex(&Sha256::digest(body)).unwrap_or_default(),
            sysroot: sysroot.to_vec(),
            external: external
                .iter()
                .map(|crate_| (crate_.crate_name.clone(), crate_.stable_crate_id.clone()))
                .collect(),
        };
        let temporary = self
            .directory
            .join(format!(".{key}.tmp-{}", std::process::id()));
        let written = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(body)?;
            file.write_all(b"\n")?;
            serde_json::to_writer(&mut file, &header)?;
            let mut permissions = file.metadata()?.permissions();
            permissions.set_readonly(true);
            file.set_permissions(permissions)?;
            fs::rename(&temporary, self.directory.join(key))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }

    /// Retire bodies this run did not ask for, so the directory holds one
    /// probe's worth.
    pub(super) fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.len() == 64
                && name
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                && !self.used.contains(name)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
