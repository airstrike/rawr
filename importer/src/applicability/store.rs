//! Crates' intrinsic inventory entries, each under its own crate's key.
//!
//! An entry is published atomically, under a key its crate's native input
//! witness sets before any nightly build. A reader holds a lease on the
//! publication it read; a busy or unreadable entry is a miss. The witness
//! is the project's view of the crate, so a store keyed by it is the
//! project's. A crate whose whole closure is immutable has a machine-wide
//! key instead (`shared_key`), and its entry lives in rawr's home.

use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::inventory::Entry;
use crate::extract::engine::home;

const ENTRY: &str = "entry.json";

/// The shared key of one crate's entry: the extraction protocol and pinned
/// compiler, the crate's name and its native input witness, which covers
/// its sources, configuration, build-script outputs and dependencies.
#[must_use]
pub fn key(crate_name: &str, witness: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rawr.import.intrinsic-entry-key/v1");
    hash.update(crate::extract::protocol::SCHEMA.to_le_bytes());
    hash.update(crate::cargo::toolchain::pinned_commit().as_bytes());
    hash.update(crate_name.as_bytes());
    hash.update([0]);
    hash.update(witness);
    hash.finalize().into()
}

/// The machine-wide key of a crate whose closure is immutable: its
/// `extract::shared` key names its sources by checksum or commit, its
/// features, profile, target and dependencies, and the extraction toolchain.
#[must_use]
pub fn shared_key(crate_name: &str, shared: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rawr.import.intrinsic-entry-shared-key/v1");
    hash.update(crate::extract::protocol::SCHEMA.to_le_bytes());
    hash.update(crate::cargo::toolchain::pinned_commit().as_bytes());
    hash.update(crate_name.as_bytes());
    hash.update([0]);
    hash.update(shared.as_bytes());
    hash.finalize().into()
}

fn name(key: &[u8; 32]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A shared entry and the lease on the publication it was read from.
pub struct Held {
    entry: Entry,
    _lease: home::Lease,
}

impl Held {
    pub const fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn into_entry(self) -> Entry {
        self.entry
    }
}

/// Read the entry `key` names, when one is published and decodes as it.
#[must_use]
pub fn read(directory: &Path, key: &[u8; 32]) -> Option<Held> {
    let name = name(key);
    let lease = home::hold(directory, &name)?;
    let bytes = std::fs::read(directory.join(&name).join(ENTRY)).ok()?;
    let entry = Entry::decode(&bytes, key).ok()?;
    Some(Held {
        entry,
        _lease: lease,
    })
}

/// Publish one entry under its key. A reader holding the key makes this a
/// skipped publication, never a wait.
pub fn publish(directory: &Path, entry: &Entry) -> std::io::Result<()> {
    let bytes = entry
        .encode()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    home::publish(directory, &name(entry.key()), &[(ENTRY, &bytes)])
}
