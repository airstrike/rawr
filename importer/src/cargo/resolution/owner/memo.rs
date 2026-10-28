//! A shared resolution remembered in the project's Rawr directory (D609).
//!
//! A run that every selected crate's shared table answers still asks Cargo
//! how the project resolves and replays the tables: about 3 s for iced in a
//! debug build, every run. The resolution is a function of the request
//! (whose identity covers the manifests, the lock, the config, the target
//! and the demand), the pinned toolchain, the files its proof names, and
//! the shared entries it read. So the next run with all of those unchanged
//! takes the same answer back: it reads those files again, and an entry's
//! table by its length and modification time, which publication changes.
//! Anything else runs the resolution again.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::bindings::{Script, Settled};
use crate::input::proof::{Entry, Proof, Role};

const SCHEMA: u32 = 1;
/// Kept beside the harness copies a shared resolution leaves.
const FILE: &str = "shared/remembered.json";

/// One file a shared resolution read, as its proof names it: a harness copy
/// or one of the request's inputs, with its length and digest, or none when
/// it was not there.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct File {
    harness: bool,
    path: String,
    read: Option<(usize, String)>,
}

impl File {
    pub(super) fn new(role: Role, path: &str, read: Option<(usize, [u8; 32])>) -> Self {
        Self {
            harness: role == Role::Harness,
            path: path.to_owned(),
            read: read.map(|(bytes, digest)| (bytes, crate::extract::engine::hex(&digest))),
        }
    }

    fn entry(&self) -> Option<Entry> {
        let role = if self.harness {
            Role::Harness
        } else {
            Role::Authority
        };
        Some(match &self.read {
            Some((bytes, digest)) => Entry::File {
                role,
                package_id: None,
                path: self.path.as_str().into(),
                bytes: *bytes,
                digest: unhex(digest)?,
            },
            None => Entry::Missing {
                role,
                package_id: None,
                path: self.path.as_str().into(),
            },
        })
    }

    fn current(&self) -> bool {
        match (fs::read(&self.path), &self.read) {
            (Ok(bytes), Some((length, digest))) => {
                bytes.len() == *length
                    && crate::extract::engine::hex(&Sha256::digest(&bytes)) == *digest
            }
            (Err(_), None) => true,
            _ => false,
        }
    }
}

/// One shared entry the resolution read, and its table file as it was.
#[derive(Deserialize, Serialize)]
struct Used {
    key: String,
    crate_name: String,
    scripts: BTreeMap<String, Vec<Script>>,
    table: (u64, u128),
}

/// A shared resolution, as the next run takes it back.
#[derive(Deserialize, Serialize)]
pub(super) struct Memo {
    schema: u32,
    request: String,
    attestation: String,
    identity: String,
    native: String,
    files: Vec<File>,
    entries: Vec<Used>,
}

impl Memo {
    /// The identity of the sidecar document the tables replayed.
    pub(super) fn identity(&self) -> Option<[u8; 32]> {
        unhex(&self.identity)
    }

    /// The proof of the files the resolution read, as it gave it.
    pub(super) fn proof(&self) -> Option<Proof> {
        let entries = self
            .files
            .iter()
            .map(File::entry)
            .collect::<Option<Vec<_>>>()?;
        Some(Proof::shared(
            entries.into_boxed_slice(),
            unhex(&self.native)?,
        ))
    }

    /// The tables the resolution read, settled once the runner builds.
    pub(super) fn settled(&self) -> Vec<Settled> {
        self.entries
            .iter()
            .map(|used| Settled::Read {
                key: used.key.clone(),
                crate_name: used.crate_name.clone(),
                scripts: used.scripts.clone(),
            })
            .collect()
    }
}

/// What a shared resolution read, to remember for the next run.
pub(super) struct Resolution<'a> {
    pub(super) request: &'a [u8; 32],
    pub(super) attestation: &'a [u8; 32],
    pub(super) identity: &'a [u8; 32],
    pub(super) native: &'a [u8; 32],
    pub(super) files: &'a [File],
    pub(super) settled: &'a [Settled],
}

/// Remember `resolution` under the project's Rawr directory. Best effort:
/// nothing remembered runs the resolution again.
pub(super) fn remember(rawr: &Path, bindings: &Path, resolution: &Resolution<'_>) {
    let mut entries = Vec::new();
    for settled in resolution.settled {
        let Settled::Read {
            key,
            crate_name,
            scripts,
        } = settled
        else {
            return;
        };
        let Some(table) = table(bindings, key) else {
            return;
        };
        entries.push(Used {
            key: key.clone(),
            crate_name: crate_name.clone(),
            scripts: scripts.clone(),
            table,
        });
    }
    let hex = crate::extract::engine::hex;
    let memo = Memo {
        schema: SCHEMA,
        request: hex(resolution.request),
        attestation: hex(resolution.attestation),
        identity: hex(resolution.identity),
        native: hex(resolution.native),
        files: resolution.files.to_vec(),
        entries,
    };
    let Ok(bytes) = serde_json::to_vec(&memo) else {
        return;
    };
    let path = rawr.join(FILE);
    let temporary = path.with_extension(format!("json.{}", std::process::id()));
    if fs::write(&temporary, bytes).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

/// The remembered resolution for `request` under `attestation`, when every
/// file it read and every entry it used is as it was. Each entry's use is
/// marked, as reading it would.
pub(super) fn recall(
    rawr: &Path,
    bindings: &Path,
    request: &[u8; 32],
    attestation: &[u8; 32],
) -> Option<Memo> {
    let memo = serde_json::from_slice::<Memo>(&fs::read(rawr.join(FILE)).ok()?).ok()?;
    let hex = crate::extract::engine::hex;
    if memo.schema != SCHEMA
        || memo.request != hex(request)
        || memo.attestation != hex(attestation)
        || memo.entries.is_empty()
        || !memo.files.iter().all(File::current)
        || !memo
            .entries
            .iter()
            .all(|used| table(bindings, &used.key) == Some(used.table))
    {
        return None;
    }
    for used in &memo.entries {
        crate::extract::engine::home::touch(bindings, &used.key);
    }
    Some(memo)
}

/// A shared entry's table file, by its length and modification time.
fn table(bindings: &Path, key: &str) -> Option<(u64, u128)> {
    let metadata = fs::metadata(crate::extract::engine::home::table(bindings, key)).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), modified.as_nanos()))
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut bytes = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{recall, remember, File, Resolution};
    use crate::bindings::Settled;
    use crate::input::proof::Role;

    fn scratch() -> PathBuf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-remembered-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rawr/shared")).unwrap();
        root
    }

    #[test]
    fn a_remembered_resolution_returns_until_a_file_or_an_entry_changes() {
        use sha2::{Digest as _, Sha256};
        let root = scratch();
        let rawr = root.join("rawr");
        let bindings = root.join("bindings");
        crate::extract::engine::home::publish(&bindings, "iced", &[("table.json", b"units")])
            .unwrap();
        let manifest = root.join("Cargo.toml");
        std::fs::write(&manifest, "[package]").unwrap();
        let lock = root.join("Cargo.lock");
        let read = |path: &PathBuf| {
            std::fs::read(path)
                .ok()
                .map(|bytes| (bytes.len(), Sha256::digest(&bytes).into()))
        };
        let files = [
            File::new(Role::Authority, manifest.to_str().unwrap(), read(&manifest)),
            File::new(Role::Authority, lock.to_str().unwrap(), read(&lock)),
        ];
        let settled = [Settled::Read {
            key: "iced".to_owned(),
            crate_name: "iced".to_owned(),
            scripts: std::collections::BTreeMap::new(),
        }];
        let request = [1; 32];
        let attestation = [2; 32];
        remember(
            &rawr,
            &bindings,
            &Resolution {
                request: &request,
                attestation: &attestation,
                identity: &[3; 32],
                native: &[4; 32],
                files: &files,
                settled: &settled,
            },
        );
        let memo = recall(&rawr, &bindings, &request, &attestation).unwrap();
        assert_eq!(memo.identity(), Some([3; 32]));
        assert!(memo.proof().is_some());
        assert_eq!(memo.settled().len(), 1);
        // Another request, or another toolchain, runs the resolution.
        assert!(recall(&rawr, &bindings, &[9; 32], &attestation).is_none());
        assert!(recall(&rawr, &bindings, &request, &[9; 32]).is_none());
        // A lock that appears is a change.
        std::fs::write(&lock, "# lock").unwrap();
        assert!(recall(&rawr, &bindings, &request, &attestation).is_none());
        std::fs::remove_file(&lock).unwrap();
        assert!(recall(&rawr, &bindings, &request, &attestation).is_some());
        // A republished entry is a change.
        crate::extract::engine::home::publish(&bindings, "iced", &[("table.json", b"other")])
            .unwrap();
        assert!(recall(&rawr, &bindings, &request, &attestation).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
