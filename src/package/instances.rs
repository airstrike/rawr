//! The instances native code calls marked generic functions at (D866,
//! D869), as the last check of the generated crate recorded them
//! (`rawr_import::record`). `rawr check` keeps the record beside the
//! project's other rawr files ([`store`]), where the shadow crate's support
//! table ([`super::island::support`]) and the resolver, which compiles each
//! instance, both read it, so they never disagree. The record names the
//! build of rawr that kept it, and only that build reads it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rawr_import::record::{Instance, Record};

use super::graph::Graph;

/// One instance the support table holds: the path the generic function's
/// call names, its instance entry's path from the crate root, and its type
/// arguments, written from the crate root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) function: String,
    pub(crate) types: Vec<String>,
}

/// Where the project at `root` keeps its record.
fn location(root: &Path) -> PathBuf {
    rawr_import::location::rawr_directory_here(root).join("instances.json")
}

/// The build of rawr that keeps a record: the digest of its sources
/// ([`rawr_import::host::DIGEST`]), in hex. The record came from the shadow
/// crate this build generated, so another build reads it as none and
/// records its own, as a fresh project does (#1601). The runner this build
/// makes is built from the host it wrote, whose stamp gives the runner the
/// same digest, so it reads the record its check kept (#1606).
fn build() -> String {
    rawr_import::host::DIGEST
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A kept record's bytes: the build's line, then the record.
fn encode(build: &str, record: &Record) -> io::Result<Vec<u8>> {
    let mut bytes = format!("{build}\n").into_bytes();
    serde_json::to_writer(&mut bytes, record).map_err(io::Error::other)?;
    Ok(bytes)
}

/// The record in `bytes` if `build` kept it, or none.
fn decode(build: &str, bytes: &[u8]) -> Record {
    bytes
        .strip_prefix(build.as_bytes())
        .and_then(|rest| rest.strip_prefix(b"\n"))
        .and_then(|record| serde_json::from_slice(record).ok())
        .unwrap_or_default()
}

/// Keep `record` as the project at `root`'s; whether it differs from the
/// one kept before, so the shadow crate's table must be written again.
///
/// # Errors
///
/// The record can't be written.
pub fn store(root: &Path, record: &Record) -> io::Result<bool> {
    let path = location(root);
    let bytes = encode(&build(), record)?;
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
        .map(|bytes| decode(&build(), &bytes))
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
    /// the shadow crate has an instance entry for, among `generic`, each
    /// entry's path by its call's.
    pub(crate) fn instance_entries(
        &self,
        generic: &std::collections::BTreeMap<String, String>,
    ) -> Vec<Entry> {
        self.native_instances()
            .into_iter()
            .filter_map(|instance| {
                Some(Entry {
                    function: generic.get(&instance.path)?.clone(),
                    path: instance.path,
                    types: instance.types,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rawr_import::record::{Instance, Record};

    #[test]
    fn a_record_another_build_kept_is_read_as_none() {
        let record = Record::new(vec![Instance {
            path: "widget::row_group".into(),
            name: "(app::Message,)".into(),
            types: vec!["crate::Message".into()],
        }]);
        let bytes = super::encode("aa", &record).expect("the record encodes");
        assert_eq!(super::decode("aa", &bytes), record);
        assert_eq!(super::decode("ab", &bytes), Record::default());
        assert_eq!(super::decode("a", &bytes), Record::default());
        // A record kept before records named their build is another's.
        let unnamed = serde_json::to_vec(&record).expect("the record encodes");
        assert_eq!(super::decode("aa", &unnamed), Record::default());
    }
}
