//! The recorded input files of the bundles an owner or an earlier process
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
use super::Replays;

/// The records an owner holds, newest first, by wire digest.
#[derive(Default)]
pub(in crate::cargo::resolution) struct Held(Vec<([u8; 32], Arc<Recorded>)>);

/// Records an owner holds: the current bundle and the few a return to
/// earlier sources may find.
const HELD_RECORDS: usize = 8;

/// Records kept on disk for later processes.
const KEPT_RECORDS: usize = 16;

/// The directory under `<target>/rawr` the records are kept in.
const DIRECTORY: &str = "recorded";

/// The record of the owner whose wire is `wire`, held in `replays` or
/// kept. A kept record is read with the lock released.
pub(super) fn find(replays: &Mutex<Replays>, rawr: &Path, wire: &[u8]) -> Option<Arc<Recorded>> {
    let key = key(wire);
    if let Some(held) = Replays::lock(replays).recorded.get(&key) {
        return Some(held);
    }
    let recorded = Arc::new(Recorded::decode(&fs::read(path(rawr, &key)).ok()?)?);
    Replays::lock(replays).recorded.hold(key, &recorded);
    Some(recorded)
}

/// Hold in `replays`, and keep, the record `of` gives for the owner whose
/// wire is `wire`, unless one is held already. The record is written with
/// the lock released.
pub(super) fn keep(
    replays: &Mutex<Replays>,
    rawr: &Path,
    wire: &[u8],
    of: impl FnOnce() -> Recorded,
) {
    let key = key(wire);
    if Replays::lock(replays).recorded.get(&key).is_some() {
        return;
    }
    let recorded = Arc::new(of());
    Replays::lock(replays).recorded.hold(key, &recorded);
    // A record not written only costs a later process a replay.
    let _ = write(rawr, &key, &recorded);
}

impl Held {
    fn get(&self, key: &[u8; 32]) -> Option<Arc<Recorded>> {
        self.0
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, recorded)| Arc::clone(recorded))
    }

    fn hold(&mut self, key: [u8; 32], recorded: &Arc<Recorded>) {
        self.0.retain(|(known, _)| *known != key);
        self.0.insert(0, (key, Arc::clone(recorded)));
        self.0.truncate(HELD_RECORDS);
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
