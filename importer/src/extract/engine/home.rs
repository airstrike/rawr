//! Extraction tables shared per machine, in bindings under rawr's home.
//!
//! A stable guard beside each entry protects its table, metadata and
//! publication generation together. Replacing a directory never replaces its
//! guard. Every lock attempt is nonblocking: a busy cache is a miss or a
//! skipped publication/eviction, never a reason to delay a run (D608.7).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Where shared tables live, under rawr's home.
pub(crate) const DIRECTORY: &str = "bindings";
const TABLE: &str = "table.json";
const GENERATION: &str = ".generation";

/// A hold on the exact publication whose files a reader consumes.
pub(crate) struct Lease {
    _lock: File,
    generation: String,
}

impl Lease {
    /// The publication observed while acquiring this lease.
    pub(crate) fn generation(&self) -> &str {
        &self.generation
    }
}

fn guard(bindings: &Path, key: &str) -> io::Result<File> {
    // Unlinking a guard would let a new opener lock a different inode while
    // a previous reader still holds this one.
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(bindings.join(format!(".lock-{key}")))
}

fn lease(bindings: &Path, key: &str) -> Option<Lease> {
    let lock = guard(bindings, key).ok()?;
    lock.try_lock_shared().ok()?;
    let generation = fs::read_to_string(bindings.join(key).join(GENERATION)).ok()?;
    if generation.is_empty() {
        return None;
    }
    touch(bindings, key);
    Some(Lease {
        _lock: lock,
        generation,
    })
}

/// Read a table while retaining its publication lease. Its caller can read
/// the metadata under the same lease without mixing publications.
pub(crate) fn read(bindings: &Path, key: &str) -> Option<(Lease, Vec<u8>)> {
    let lease = lease(bindings, key)?;
    let bytes = fs::read(bindings.join(key).join(TABLE)).ok()?;
    Some((lease, bytes))
}

/// Hold the current publication of `key` for a reader of its own files.
pub(crate) fn hold(bindings: &Path, key: &str) -> Option<Lease> {
    lease(bindings, key)
}

/// The current publication, read under its guard without loading the table.
pub(crate) fn generation(bindings: &Path, key: &str) -> Option<String> {
    Some(lease(bindings, key)?.generation)
}

/// Mark a use while holding the key's guard, without walking the cache.
fn touch(bindings: &Path, key: &str) {
    let _ = File::open(bindings.join(key))
        .and_then(|directory| directory.set_modified(SystemTime::now()));
}

/// Publish all files as one generation. A reader of this key makes the
/// publication return WouldBlock; the existing entry remains unchanged.
pub(crate) fn publish(bindings: &Path, key: &str, files: &[(&str, &[u8])]) -> io::Result<()> {
    fs::create_dir_all(bindings)?;
    let lock = guard(bindings, key)?;
    lock.try_lock().map_err(io::Error::from)?;
    let id = unique();
    let staging = bindings.join(format!(".staging-{key}.{id}"));
    let written = (|| -> io::Result<()> {
        fs::create_dir(&staging)?;
        fs::write(staging.join(GENERATION), &id)?;
        for (name, bytes) in files {
            let mut file = io::BufWriter::new(File::create(staging.join(name))?);
            file.write_all(bytes)?;
            file.flush()?;
        }
        replace(
            &staging,
            &bindings.join(key),
            &bindings.join(format!(".retired-{key}.{id}")),
        )
    })();
    if written.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    written
}

/// The exclusive key guard stays held across both renames and rollback.
fn replace(staging: &Path, entry: &Path, retired: &Path) -> io::Result<()> {
    let replaced = match fs::rename(entry, retired) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if let Err(error) = fs::rename(staging, entry) {
        if replaced {
            fs::rename(retired, entry)?;
        }
        return Err(error);
    }
    if replaced {
        let _ = fs::remove_dir_all(retired);
    }
    Ok(())
}

/// Remove an entry only when no reader holds its stable guard.
pub(crate) fn evict(bindings: &Path, key: &str) {
    let Ok(lock) = guard(bindings, key) else {
        return;
    };
    if lock.try_lock().is_ok() {
        let _ = fs::remove_dir_all(bindings.join(key));
    }
}

/// Remove unused entries and abandoned writes, skipping all busy keys.
pub(crate) fn prune(bindings: &Path, idle: Duration) {
    let Ok(entries) = fs::read_dir(bindings) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let temporary = name.starts_with(".retired-") || name.starts_with(".staging-");
        if name.starts_with('.') && !temporary {
            continue;
        }
        let key = if temporary {
            // The name carries the key even if a writer died before its
            // first file. Older temporary names ended with pid-nanoseconds.
            let Some(key) = temporary_key(&name) else {
                continue;
            };
            key.to_owned()
        } else {
            name
        };
        let Ok(lock) = guard(bindings, &key) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        // Inspect current metadata after taking the guard. A replacement
        // or use since the directory scan must be considered anew.
        let age = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .unwrap_or_default();
        let expired = if temporary {
            age > Duration::from_secs(60 * 60)
        } else {
            age > idle
        };
        if expired {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn temporary_key(name: &str) -> Option<&str> {
    let suffix = name
        .strip_prefix(".staging-")
        .or_else(|| name.strip_prefix(".retired-"))?;
    suffix
        .rsplit_once('.')
        .map(|(key, _)| key)
        .or_else(|| suffix.rsplitn(3, '-').nth(2))
}

/// The bindings directory under rawr's home.
pub(crate) fn directory(home: &Path) -> PathBuf {
    home.join(DIRECTORY)
}

fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{
        evict, generation, prune, publish, read, replace, unique, File, PathBuf, SystemTime,
    };
    use super::{fs, io, Duration, TABLE};

    fn scratch() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tests")
            .join(format!("bindings-home-{}", unique()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_reader_keeps_table_metadata_and_generation_together() {
        let root = scratch();
        publish(
            &root,
            "crate",
            &[(TABLE, b"old"), ("meta.json", b"old metadata")],
        )
        .unwrap();
        let (lease, bytes) = read(&root, "crate").unwrap();
        let old = lease.generation().to_owned();
        let error = publish(
            &root,
            "crate",
            &[(TABLE, b"new"), ("meta.json", b"new metadata")],
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        evict(&root, "crate");
        assert_eq!(bytes, b"old");
        assert_eq!(
            fs::read(root.join("crate/meta.json")).unwrap(),
            b"old metadata"
        );
        assert_eq!(generation(&root, "crate").as_deref(), Some(old.as_str()));
        drop(lease);
        publish(
            &root,
            "crate",
            &[(TABLE, b"new"), ("meta.json", b"new metadata")],
        )
        .unwrap();
        let (lease, bytes) = read(&root, "crate").unwrap();
        assert_ne!(lease.generation(), old);
        assert_eq!(bytes, b"new");
        assert_eq!(
            fs::read(root.join("crate/meta.json")).unwrap(),
            b"new metadata"
        );
        drop(lease);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pruning_and_a_writer_never_remove_a_readers_entry() {
        let root = scratch();
        publish(&root, "crate", &[(TABLE, b"old")]).unwrap();
        let (lease, _) = read(&root, "crate").unwrap();
        let old = SystemTime::now() - Duration::from_secs(90);
        File::open(root.join("crate"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        prune(&root, Duration::from_secs(1));
        assert!(root.join("crate/table.json").is_file());
        drop(lease);
        prune(&root, Duration::from_secs(1));
        assert!(!root.join("crate").exists());
        assert!(root.join(".lock-crate").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_replacement_restores_the_previous_entry() {
        let root = scratch();
        let entry = root.join("crate");
        fs::create_dir(&entry).unwrap();
        fs::write(entry.join(TABLE), "old").unwrap();
        let error = replace(&root.join("missing"), &entry, &root.join("retired")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(entry.join(TABLE)).unwrap(), b"old");
        assert!(!root.join("retired").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_abandoned_write_without_payload_is_reclaimed() {
        let root = scratch();
        let staging = root.join(".staging-crate.1-2-3");
        fs::create_dir(&staging).unwrap();
        File::open(&staging)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(7200))
            .unwrap();
        prune(&root, Duration::from_secs(86400));
        assert!(!staging.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
