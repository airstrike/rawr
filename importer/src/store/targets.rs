//! Each build target's current generation.
//!
//! A project builds a runner for each target it runs: its program, its
//! tests, another binary. The pin ledger holds one generation per role, the
//! one bound last, so binding one target's generation would leave another
//! target's unrooted. Binding a target's generation as Live and LastGood
//! also records it here, in place of that target's earlier record, and a
//! recorded generation is a root for eviction: switching between targets
//! reuses each one's runner, and binding one retires only what that target
//! superseded (#1407). A project keeps the records of the [`KEPT`] targets
//! bound most recently; binding another drops the one bound longest ago.
//!
//! The records live beside the store (`store.targets`), outside its
//! inventory: one directory per project, one file per target, each holding
//! the order it was bound in and the references of its generation.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::accounting::Reference;
use crate::node::{Digest, Error};

const DIRECTORY: &str = "targets";
const MAGIC: &[u8; 8] = b"RAWRTRGT";
const SCHEMA: u16 = 1;
const HEADER_BYTES: usize = 24;
/// How many targets' generations a project keeps.
const KEPT: usize = 8;

/// One build target of a project: what a run, a test run or another binary
/// builds a runner for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Target([u8; 32]);

impl Target {
    /// The target of a caller that names none: every such bind is one
    /// target's.
    pub const UNNAMED: Self = Self([0; 32]);

    /// The target `name` identifies, such as a program's path and how it is
    /// built.
    #[must_use]
    pub fn named(name: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.store.target/v1");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name);
        Self(digest.finalize().into())
    }

    fn file_name(self) -> String {
        Digest::from_bytes(self.0).directory()
    }
}

/// Record `references` as `target`'s generation in `project`, bound in
/// `order`, in place of the target's earlier record, then drop the records
/// beyond the project's [`KEPT`] bound most recently. A record that already
/// holds these references is left as it is. The caller holds the project lease.
pub(super) fn record(
    store: &Path,
    project: Digest,
    target: Target,
    references: &[Reference],
    order: u64,
) -> Result<(), Error> {
    let directory = project_directory(store, project);
    fs::create_dir_all(&directory).map_err(io("create the target records", &directory))?;
    let path = directory.join(target.file_name());
    // A bind of the generation the target already records, as a run of an
    // unchanged project makes, writes nothing: the record roots it already.
    // Its order stays the one it was first bound in.
    if fs::read(&path)
        .ok()
        .and_then(|recorded| decode(&recorded))
        .is_some_and(|(_, recorded)| recorded == references)
    {
        return Ok(());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(HEADER_BYTES + references.len() * Reference::encoded_bytes())
        .map_err(|_| Error::Allocation)?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(
        &u32::try_from(references.len())
            .map_err(|_| Error::Allocation)?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&order.to_le_bytes());
    for reference in references {
        reference.encode(&mut bytes);
    }
    let staged = directory.join(format!(".{}-{}", target.file_name(), std::process::id()));
    fs::write(&staged, &bytes).map_err(io("write a target record", &staged))?;
    fs::rename(&staged, &path).map_err(io("publish a target record", &path))?;
    let mut records = records(&directory);
    records.sort_unstable_by_key(|(order, _, _)| std::cmp::Reverse(*order));
    for (_, path, _) in records.iter().skip(KEPT) {
        fs::remove_file(path).map_err(io("drop a target record", path))?;
    }
    Ok(())
}

/// Every reference a recorded target generation of any project holds.
pub(super) fn rooted(store: &Path) -> Vec<Reference> {
    let Ok(projects) = fs::read_dir(directory(store)) else {
        return Vec::new();
    };
    projects
        .flatten()
        .flat_map(|project| records(&project.path()))
        .flat_map(|(_, _, references)| references)
        .collect()
}

/// Drop `project`'s records, as collecting the project releases its roots.
pub(super) fn release(store: &Path, project: Digest) {
    let _ = fs::remove_dir_all(project_directory(store, project));
}

/// The records in a project's `directory`: each one's order, path and
/// references. A record that does not decode holds nothing.
fn records(directory: &Path) -> Vec<(u64, PathBuf, Vec<Reference>)> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let path = entry.path();
            let bytes = fs::read(&path).ok()?;
            let (order, references) = decode(&bytes)?;
            Some((order, path, references))
        })
        .collect()
}

fn decode(bytes: &[u8]) -> Option<(u64, Vec<Reference>)> {
    if bytes.get(..8)? != MAGIC || bytes.get(8..10)? != SCHEMA.to_le_bytes() {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?)).ok()?;
    let order = u64::from_le_bytes(bytes.get(16..24)?.try_into().ok()?);
    if bytes.len() != HEADER_BYTES + count.checked_mul(Reference::encoded_bytes())? {
        return None;
    }
    let references = (0..count)
        .map(|index| Reference::decode(bytes, HEADER_BYTES + index * Reference::encoded_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some((order, references))
}

fn directory(store: &Path) -> PathBuf {
    store.with_file_name(format!(
        "{}.{DIRECTORY}",
        store
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("store")
    ))
}

pub(super) fn project_directory(store: &Path, project: Digest) -> PathBuf {
    directory(store).join(project.directory())
}

fn io(operation: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io {
        operation,
        path,
        source,
    }
}
