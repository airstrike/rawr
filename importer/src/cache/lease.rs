//! Cross-process writer exclusion for cache mutation.

use std::fs::{File, OpenOptions};
use std::path::Path;

pub(crate) fn open(root: &Path) -> Result<fd_lock::RwLock<File>, super::entry::Error> {
    let path = root.join(".writer.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| super::entry::Error::Io {
            operation: "open runner cache writer lease",
            path,
            source,
        })?;
    Ok(fd_lock::RwLock::new(file))
}
