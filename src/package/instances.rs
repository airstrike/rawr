//! The instances native code calls marked generic functions at (D866,
//! D869), as the last check of the generated crate recorded them
//! (`rawr_import::record`). `rawr check` keeps the record beside the
//! project's other rawr files ([`store`]), where the shadow crate's support
//! table ([`super::island::support`]) and the resolver, which compiles each
//! instance, both read it, so they never disagree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rawr_import::record::{Instance, Record};

use super::graph::Graph;

/// One instance the support table holds: the generic function's path from
/// the crate root and its type arguments, written from the crate root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) types: Vec<String>,
}

/// Where the project at `root` keeps its record.
fn location(root: &Path) -> PathBuf {
    rawr_import::location::rawr_directory_here(root).join("instances.json")
}

/// Keep `record` as the project at `root`'s; whether it differs from the
/// one kept before, so the shadow crate's table must be written again.
///
/// # Errors
///
/// The record can't be written.
pub fn store(root: &Path, record: &Record) -> io::Result<bool> {
    let path = location(root);
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    if fs::read(&path).is_ok_and(|kept| kept == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, bytes)?;
    Ok(true)
}

/// The record the project at `root` keeps, or none.
#[must_use]
pub fn kept(root: &Path) -> Record {
    fs::read(location(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The name rawr gives the instance of a generic function whose types'
/// tuple is named `name`, as `type_name` names it: FNV-1a of the name, as
/// the shadow crate's `keyed` hashes it at run time.
#[must_use]
pub(crate) fn key(name: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A short digest of `text`, as a profile identity names the record it
/// was checked with.
#[must_use]
pub fn digest(text: &str) -> String {
    key(text)
}

impl Graph {
    /// The instances the project's native code calls marked generic
    /// functions at.
    pub(crate) fn native_instances(&self) -> Vec<Instance> {
        kept(self.entry_package().root()).instances().to_vec()
    }

    /// The support table's entries: each recorded instance of a function
    /// the shadow crate has an instance entry for, among `generic`.
    pub(crate) fn instance_entries(
        &self,
        generic: &std::collections::BTreeSet<String>,
    ) -> Vec<Entry> {
        self.native_instances()
            .into_iter()
            .filter(|instance| generic.contains(&instance.path))
            .map(|instance| Entry {
                path: instance.path,
                types: instance.types,
            })
            .collect()
    }
}
