//! Independently hashed canonical executable identity.

use std::fs;
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::super::Error;

const PATH_BYTES: usize = 16 * 1024;

/// Canonical path, file-system identity, length, and SHA-256 bytes.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Identity {
    pub path: Box<[u8]>,
    pub digest: [u8; 32],
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}

impl Identity {
    pub fn read(path: &Path) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(path).map_err(Error::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::Executable);
        }
        let path = fs::canonicalize(path).map_err(Error::Io)?;
        let path_bytes = path.as_os_str().as_bytes();
        if path_bytes.is_empty() || path_bytes.len() > PATH_BYTES {
            return Err(Error::Executable);
        }
        let mut file = fs::File::open(&path).map_err(Error::Io)?;
        let sealed = file.metadata().map_err(Error::Io)?;
        if !sealed.is_file()
            || sealed.dev() != metadata.dev()
            || sealed.ino() != metadata.ino()
            || sealed.len() != metadata.len()
        {
            return Err(Error::Executable);
        }
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut bytes = 0_u64;
        loop {
            let read = file.read(&mut buffer).map_err(Error::Io)?;
            if read == 0 {
                break;
            }
            bytes = bytes.checked_add(read as u64).ok_or(Error::Executable)?;
            digest.update(&buffer[..read]);
        }
        if bytes != sealed.len() {
            return Err(Error::Executable);
        }
        Ok(Self {
            path: path_bytes.into(),
            digest: digest.finalize().into(),
            device: sealed.dev(),
            inode: sealed.ino(),
            size: sealed.len(),
        })
    }
}
