//! Sealed identity of the separately supplied benchmark window title.

use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::Error;

const FILE_NAME: &[u8] = b".phase9-window-title";
const PATH_BYTES: usize = 16 * 1024;
const TITLE_BYTES: usize = 16 * 1024;

/// Closed role of the only authenticated runtime title path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    ProjectWindowTitle,
}

/// Canonical regular-file identity bound into every launch evidence record.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Identity {
    role: Role,
    path: Box<[u8]>,
    bytes: Box<[u8]>,
    digest: [u8; 32],
    size: u64,
    mode: u32,
    device: u64,
    inode: u64,
    owner: u32,
}

impl Identity {
    /// Read and seal one canonical, owner-only, bounded ASCII title file.
    pub fn read(path: &Path) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(path).map_err(Error::Io)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Title("file type"));
        }
        let canonical = fs::canonicalize(path).map_err(Error::Io)?;
        if canonical != path || !canonical.is_absolute() {
            return Err(Error::Title("canonical path"));
        }
        let path = canonical.as_os_str().as_bytes();
        if path.is_empty()
            || path.len() > PATH_BYTES
            || canonical
                .file_name()
                .is_none_or(|name| name.as_bytes() != FILE_NAME)
        {
            return Err(Error::Title("path role"));
        }
        let mode = metadata.permissions().mode() & 0o777;
        let owner = metadata.uid();
        if mode != 0o600 || owner != nix::unistd::Uid::effective().as_raw() {
            return Err(Error::Title("mode or owner"));
        }
        let bytes = fs::read(&canonical).map_err(Error::Io)?;
        if bytes.is_empty()
            || bytes.len() > TITLE_BYTES
            || metadata.len() != bytes.len() as u64
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            return Err(Error::Title("bytes"));
        }
        let identity = Self {
            role: Role::ProjectWindowTitle,
            path: path.into(),
            digest: digest(&bytes),
            size: metadata.len(),
            mode,
            device: metadata.dev(),
            inode: metadata.ino(),
            owner,
            bytes: bytes.into_boxed_slice(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.role != Role::ProjectWindowTitle
            || self.path.is_empty()
            || self.path.len() > PATH_BYTES
            || !self.path.starts_with(b"/")
            || !self.path.ends_with(FILE_NAME)
            || self.bytes.is_empty()
            || self.bytes.len() > TITLE_BYTES
            || self.size != self.bytes.len() as u64
            || self.mode != 0o600
            || self.owner != nix::unistd::Uid::effective().as_raw()
            || self.device == 0
            || self.inode == 0
            || !self
                .bytes
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
            || self.digest != digest(&self.bytes)
        {
            return Err(Error::Title("identity"));
        }
        Ok(())
    }

    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn path(&self) -> &[u8] {
        &self.path
    }

    #[cfg(test)]
    pub(super) fn testing() -> Self {
        let bytes = Box::from(&b"Baseline Title"[..]);
        Self {
            role: Role::ProjectWindowTitle,
            path: Box::from(&b"/tmp/project/.phase9-window-title"[..]),
            digest: digest(&bytes),
            size: bytes.len() as u64,
            mode: 0o600,
            device: 1,
            inode: 1,
            owner: nix::unistd::Uid::effective().as_raw(),
            bytes,
        }
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.window-title/v1");
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_identity_retains_every_bounded_title_fact() {
        let identity = super::Identity::testing();
        identity
            .validate()
            .expect("fixture title identity validates");
        assert_eq!(identity.bytes(), b"Baseline Title");
        assert!(identity.path().ends_with(super::FILE_NAME));
    }
}
