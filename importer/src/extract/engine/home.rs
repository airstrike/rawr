//! Extraction tables shared per machine, in `bindings` under rawr's home
//! (D608.4). Each entry is a directory named by its crate's key, holding
//! `table.json` and a `.lock` that readers lease.
//!
//! An entry appears whole or not at all: it is written beside the others and
//! renamed into place. A reader leases an entry while it reads it, and marks
//! its use by touching the entry; nothing walks the directory to record a
//! use. Removal by last use renames an idle entry aside first, and skips one
//! a reader leases. Nothing here waits for a lock or reads free disk
//! (D608.7).

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where shared tables live, under rawr's home.
pub(crate) const DIRECTORY: &str = "bindings";
const TABLE: &str = "table.json";
const LOCK: &str = ".lock";

/// A reader's hold on an entry: removal skips it until this drops.
pub(crate) struct Lease {
    _lock: File,
}

/// The table kept for `key`, leased, when an entry is published. An entry
/// being removed, or a key never published, reads as absent.
pub(crate) fn read(bindings: &Path, key: &str) -> Option<(Lease, Vec<u8>)> {
    let entry = bindings.join(key);
    let lock = File::open(entry.join(LOCK)).ok()?;
    lock.try_lock_shared().ok()?;
    let bytes = fs::read(entry.join(TABLE)).ok()?;
    // The last use, which removal orders entries by.
    let _ = File::open(&entry).and_then(|directory| directory.set_modified(SystemTime::now()));
    Some((Lease { _lock: lock }, bytes))
}

/// Publish `files` as `key`'s entry, replacing any entry before it. The
/// first file is the table. A failed write leaves the entry before it as it
/// was.
pub(crate) fn publish(bindings: &Path, key: &str, files: &[(&str, &[u8])]) -> io::Result<()> {
    fs::create_dir_all(bindings)?;
    let id = unique();
    let staging = bindings.join(format!(".staging-{key}-{id}"));
    let written = (|| -> io::Result<()> {
        fs::create_dir(&staging)?;
        for (name, bytes) in files {
            let mut file = io::BufWriter::new(File::create(staging.join(name))?);
            file.write_all(bytes)?;
            file.flush()?;
        }
        File::create(staging.join(LOCK))?;
        let entry = bindings.join(key);
        let retired = bindings.join(format!(".retired-{key}-{id}"));
        let replaced = fs::rename(&entry, &retired).is_ok();
        let published = fs::rename(&staging, &entry);
        if replaced {
            remove_unleased(&retired);
        }
        published
    })();
    if written.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    written
}

/// Remove `key`'s entry now, unless a reader leases it.
pub(crate) fn evict(bindings: &Path, key: &str) {
    let retired = bindings.join(format!(".retired-{key}-{}", unique()));
    if fs::rename(bindings.join(key), &retired).is_ok() {
        remove_unleased(&retired);
    }
}

/// Remove entries unused for `idle`, skipping any a reader leases, and the
/// leftovers of interrupted writes. Best effort.
pub(crate) fn prune(bindings: &Path, idle: Duration) {
    let Ok(entries) = fs::read_dir(bindings) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let age = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .unwrap_or_default();
        if name.starts_with(".retired-") {
            remove_unleased(&path);
            continue;
        }
        if name.starts_with(".staging-") {
            // A write that died; one still writing is younger than this.
            if age > Duration::from_secs(60 * 60) {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }
        if name.starts_with('.') || age <= idle {
            continue;
        }
        let Ok(lock) = File::open(path.join(LOCK)) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let retired = bindings.join(format!(".retired-{name}-{}", unique()));
        let renamed = fs::rename(&path, &retired).is_ok();
        drop(lock);
        if renamed {
            let _ = fs::remove_dir_all(&retired);
        }
    }
}

/// Remove a retired entry no reader leases; a leased one waits for a later
/// removal.
fn remove_unleased(retired: &Path) {
    let leased = File::open(retired.join(LOCK)).is_ok_and(|lock| lock.try_lock().is_err());
    if !leased {
        let _ = fs::remove_dir_all(retired);
    }
}

/// The bindings directory under rawr's home.
pub(crate) fn directory(home: &Path) -> PathBuf {
    home.join(DIRECTORY)
}

fn unique() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}
