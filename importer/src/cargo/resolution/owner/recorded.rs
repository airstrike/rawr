//! The recorded input files of the bundles this process or an earlier one
//! replayed, by each owner's wire (#1794).
//!
//! A lookup of a resolution compares a candidate's recorded files first: a
//! file whose current length or bytes differ proves the candidate
//! superseded by an edit, with no replay. A superseded candidate stays in
//! its bucket, so a save that puts those files back finds its harness
//! again. Each record is also kept under `<target>/rawr/recorded`, so a
//! later process tells superseded candidates apart as well. A record is a
//! hint: it only ever refuses a candidate, never admits one, so a lost,
//! stale or damaged record costs a replay or a build, never a wrong hit.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest as _, Sha256};

use super::super::watched::Recorded;

/// The records this process holds, newest first, by wire digest.
static HELD: Mutex<Vec<([u8; 32], Arc<Recorded>)>> = Mutex::new(Vec::new());

/// Records held in this process: the current bundle and the few a return
/// to earlier sources may find.
const HELD_RECORDS: usize = 8;

/// Records kept on disk for later processes.
const KEPT_RECORDS: usize = 16;

/// The directory under `<target>/rawr` the records are kept in.
const DIRECTORY: &str = "recorded";

/// The record of the owner whose wire is `wire`, held or kept.
pub(super) fn find(rawr: &Path, wire: &[u8]) -> Option<Arc<Recorded>> {
    let key = key(wire);
    if let Some(held) = held(&key) {
        return Some(held);
    }
    let recorded = Arc::new(Recorded::decode(&fs::read(path(rawr, &key)).ok()?)?);
    hold(key, &recorded);
    Some(recorded)
}

/// Hold, and keep, the record `of` gives for the owner whose wire is
/// `wire`, unless one is held already.
pub(super) fn keep(rawr: &Path, wire: &[u8], of: impl FnOnce() -> Recorded) {
    let key = key(wire);
    if held(&key).is_some() {
        return;
    }
    let recorded = Arc::new(of());
    hold(key, &recorded);
    // A record not written only costs a later process a replay.
    let _ = write(rawr, &key, &recorded);
}

fn held(key: &[u8; 32]) -> Option<Arc<Recorded>> {
    let held = HELD.lock().ok()?;
    held.iter()
        .find(|(known, _)| known == key)
        .map(|(_, recorded)| Arc::clone(recorded))
}

fn hold(key: [u8; 32], recorded: &Arc<Recorded>) {
    if let Ok(mut held) = HELD.lock() {
        held.retain(|(known, _)| *known != key);
        held.insert(0, (key, Arc::clone(recorded)));
        held.truncate(HELD_RECORDS);
    }
}

fn write(rawr: &Path, key: &[u8; 32], recorded: &Recorded) -> std::io::Result<()> {
    let directory = rawr.join(DIRECTORY);
    fs::create_dir_all(&directory)?;
    let path = path(rawr, key);
    let staging = path.with_extension(format!("{}.staging", std::process::id()));
    fs::write(&staging, recorded.encode())?;
    fs::rename(&staging, &path)?;
    prune(&directory)
}

/// Remove all but the newest kept records.
fn prune(directory: &Path) -> std::io::Result<()> {
    let mut records = fs::read_dir(directory)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    if records.len() <= KEPT_RECORDS {
        return Ok(());
    }
    records.sort_unstable_by(|left, right| right.cmp(left));
    for (_, path) in &records[KEPT_RECORDS..] {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn key(wire: &[u8]) -> [u8; 32] {
    let mut key = Sha256::new();
    key.update(b"rawr.resolution.recorded/v1");
    key.update(wire);
    key.finalize().into()
}

fn path(rawr: &Path, key: &[u8; 32]) -> PathBuf {
    rawr.join(DIRECTORY).join(crate::extract::engine::hex(key))
}
