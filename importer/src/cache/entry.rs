//! Fixed unsealed and sealed cache-entry operations.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn path(root: &Path, key: crate::bridge::BuildRequestKey) -> PathBuf {
    root.join(super::key::directory(key))
}

pub(crate) fn prepare(
    writer: &super::Writer,
    key: crate::bridge::BuildRequestKey,
    incoming: u64,
) -> Result<PathBuf, Error> {
    let entry = path(writer.root(), key);
    if entry.exists() {
        if super::seal::exists(&entry) {
            return Err(Error::AlreadySealed);
        }
        fs::remove_dir_all(&entry).map_err(|source| Error::Io {
            operation: "clean failed runner cache entry",
            path: entry.clone(),
            source,
        })?;
    }
    super::eviction::fit(writer.root(), incoming)?;
    fs::create_dir(&entry).map_err(|source| Error::Io {
        operation: "create fixed runner cache entry",
        path: entry.clone(),
        source,
    })?;
    Ok(entry)
}

pub(crate) fn directories(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut entries = Vec::new();
    for child in fs::read_dir(root).map_err(|source| Error::Io {
        operation: "inspect runner cache",
        path: root.to_path_buf(),
        source,
    })? {
        let child = child.map_err(|source| Error::Io {
            operation: "read runner cache entry",
            path: root.to_path_buf(),
            source,
        })?;
        if child
            .file_type()
            .map_err(|source| Error::Io {
                operation: "inspect runner cache entry type",
                path: child.path(),
                source,
            })?
            .is_dir()
        {
            entries.push(child.path());
        }
    }
    Ok(entries)
}

pub(crate) fn bytes(root: &Path) -> Result<u64, Error> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(&directory).map_err(|source| Error::Io {
            operation: "measure runner cache entry",
            path: directory.clone(),
            source,
        })? {
            let child = child.map_err(|source| Error::Io {
                operation: "read runner cache entry size",
                path: directory.clone(),
                source,
            })?;
            let ty = child.file_type().map_err(|source| Error::Io {
                operation: "inspect runner cache entry shape",
                path: child.path(),
                source,
            })?;
            if ty.is_symlink() {
                return Err(Error::Symlink(child.path()));
            }
            if ty.is_dir() {
                pending.push(child.path());
            } else if ty.is_file() {
                total = total
                    .checked_add(
                        child
                            .metadata()
                            .map_err(|source| Error::Io {
                                operation: "inspect runner cache file length",
                                path: child.path(),
                                source,
                            })?
                            .len(),
                    )
                    .ok_or(Error::Quota)?;
            } else {
                return Err(Error::Shape(child.path()));
            }
        }
    }
    Ok(total)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cache I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("runner cache path is a symlink: {0}")]
    Symlink(PathBuf),
    #[error("runner cache path has an unsupported shape: {0}")]
    Shape(PathBuf),
    #[error("runner cache quota arithmetic overflowed")]
    Quota,
    #[error("runner cache is full and every evictable entry is pinned")]
    PinnedFull,
    #[error("runner cache entry is already sealed")]
    AlreadySealed,
    #[error("runner cache seal does not match its manifest and artifact")]
    SealDrift,
    #[error("runner cache writer lease is busy")]
    Busy,
}
