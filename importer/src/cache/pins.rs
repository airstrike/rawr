//! Cross-process cache pins represented by owned marker files.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static PIN_ID: AtomicU64 = AtomicU64::new(1);

pub struct Pin {
    path: PathBuf,
}

impl Pin {
    pub(crate) fn acquire(entry: &Path) -> Result<Self, super::entry::Error> {
        let path = entry.join(format!(
            ".pin-{}-{}",
            std::process::id(),
            PIN_ID.fetch_add(1, Ordering::Relaxed)
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| super::entry::Error::Io {
                operation: "create runner cache pin",
                path: path.clone(),
                source,
            })?;
        Ok(Self { path })
    }
}

impl Drop for Pin {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn present(entry: &Path) -> Result<bool, super::entry::Error> {
    for child in fs::read_dir(entry).map_err(|source| super::entry::Error::Io {
        operation: "inspect runner cache pins",
        path: entry.to_path_buf(),
        source,
    })? {
        let child = child.map_err(|source| super::entry::Error::Io {
            operation: "read runner cache pin",
            path: entry.to_path_buf(),
            source,
        })?;
        if child.file_name().to_string_lossy().starts_with(".pin-") {
            return Ok(true);
        }
    }
    Ok(false)
}
