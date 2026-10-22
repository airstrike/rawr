//! Explicit fixture-root file writes with cumulative limits before effects.

use std::fs;
use std::path::{Path, PathBuf};

use super::{paths, Failure};

/// Limits apply cumulatively to actual writes and path-plus-content bytes.
#[derive(Clone, Copy)]
pub(crate) struct FileLimits {
    pub(crate) count: usize,
    pub(crate) bytes: usize,
}

/// An exact fixture-root-relative regular-file result of one scripted child.
pub(crate) struct File {
    path: PathBuf,
    bytes: Vec<u8>,
}

pub(super) struct Writes {
    limits: FileLimits,
    used: FileLimits,
}

impl File {
    /// Creation/replacement is restricted to this explicit relative file.
    pub(crate) fn new(path: PathBuf, bytes: Vec<u8>) -> Self {
        Self { path, bytes }
    }
}

impl Writes {
    pub(super) fn new(limits: FileLimits) -> Self {
        Self {
            limits,
            used: FileLimits { count: 0, bytes: 0 },
        }
    }

    pub(super) fn apply(&mut self, root: &Path, files: &[File]) -> Result<(), Failure> {
        let count = self
            .used
            .count
            .checked_add(files.len())
            .ok_or(Failure::FileLimit)?;
        let mut bytes = self.used.bytes;
        for file in files {
            bytes = bytes
                .checked_add(file.path.as_os_str().len())
                .and_then(|value| value.checked_add(file.bytes.len()))
                .ok_or(Failure::FileLimit)?;
        }
        if count > self.limits.count || bytes > self.limits.bytes {
            return Err(Failure::FileLimit);
        }
        // Validate the entire response before its first filesystem mutation.
        for (index, file) in files.iter().enumerate() {
            if !paths::relative(&file.path) {
                return Err(Failure::File("invalid relative path"));
            }
            if files[..index]
                .iter()
                .any(|earlier| earlier.path == file.path)
            {
                return Err(Failure::File("duplicate response path"));
            }
            check(root, &file.path)?;
        }
        self.used = FileLimits { count, bytes };
        for file in files {
            let path = root.join(&file.path);
            let parent = path.parent().ok_or(Failure::File("file parent absent"))?;
            fs::create_dir_all(parent).map_err(|_| Failure::Io)?;
            fs::write(path, &file.bytes).map_err(|_| Failure::Io)?;
        }
        Ok(())
    }
}

fn check(root: &Path, relative: &Path) -> Result<(), Failure> {
    let mut path = root.to_path_buf();
    let count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if index + 1 == count && metadata.is_file() => {}
            Ok(metadata) if index + 1 != count && metadata.is_dir() => {}
            Ok(_) => {
                return Err(Failure::File(
                    "existing path has wrong type or is a symlink",
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Failure::Io),
        }
    }
    Ok(())
}
