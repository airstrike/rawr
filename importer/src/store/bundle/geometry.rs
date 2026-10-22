//! Sealed logical geometry of one runner bundle.
//!
//! A schema-5 bundle record commits to this digest at seal. Every later
//! complete physical check recomputes it in the same measurement walk and
//! requires it to be identical. Only a file's allocated blocks may fall after
//! seal; any logical, membership, type, mode or link change is drift.
//!
//! Canonical traversal: depth-first preorder from the bundle root with
//! siblings in byte order of their names (`read_sorted`), so the commitment
//! depends only on the tree and never on directory enumeration order. The
//! `allocation` control cannot exist when the pending seal is measured, so it
//! is committed after the traversal with its fixed schema-5 length.
//!
//! Each member contributes its relative path, type, frozen mode, link facts
//! and exact unrounded logical length. A directory's logical geometry is its
//! committed member set: its filesystem-reported size and link count are
//! derived from that set and are not stable across filesystems, so both are
//! recorded as zero. A regular file must be singly linked; symlinks, hard
//! links and special files are rejected by the walk before reaching here.

use std::fs;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::super::accounting::{Charge, BUNDLE_RECORD_BYTES};
use crate::node::Error;

const DOMAIN: &[u8] = b"RAWRBNDL-GEOMETRY-v1\0";
const CONTROL: &str = "allocation";
const DIRECTORY: u8 = 1;
const FILE: u8 = 2;
const CONTROL_MODE: u32 = 0o444;

/// Which modes a walk observes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum View {
    /// Sealed but not yet frozen: modes are the frozen modes the publication
    /// will apply, and the allocation control is the fixed schema-5 record.
    Pending,
    /// Published and frozen: exact permission bits.
    Published,
    /// Published with only its root thawed for whole-allocation retirement.
    Retiring,
}

/// One complete measurement walk: the charge and the geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Measured {
    pub(crate) charge: Charge,
    pub(crate) geometry: [u8; 32],
}

/// Why a complete physical check rejected a sealed bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Drift {
    /// Allocation or member count differs.
    Count,
    /// Per-member logical geometry differs.
    Geometry,
    /// The measured charge exceeds the recorded charge.
    Growth,
}

/// The geometry rule. `sealed` is `None` only for a schema-4 bundle record, which
/// is judged by the labelled legacy rule: identical counts and a measured
/// charge no greater than the recorded charge. The caller has already proven
/// record and allocation identity, and the walk's type and link checks.
pub(crate) fn check(
    recorded: Charge,
    sealed: Option<[u8; 32]>,
    measured: Measured,
) -> Result<(), Drift> {
    let actual = measured.charge;
    if actual.allocations != recorded.allocations || actual.members != recorded.members {
        return Err(Drift::Count);
    }
    if let Some(sealed) = sealed
        && sealed != measured.geometry
    {
        return Err(Drift::Geometry);
    }
    if actual.bytes > recorded.bytes {
        return Err(Drift::Growth);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Member {
    kind: u8,
    mode: u32,
    links: u64,
    length: u64,
}

pub(super) struct Accumulator {
    hasher: Sha256,
    view: View,
    members: u64,
    control: Option<Member>,
}

impl Accumulator {
    pub(super) fn new(view: View) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        Self {
            hasher,
            view,
            members: 0,
            control: None,
        }
    }

    /// Commit one member and return the bytes hashed for the caller's account.
    pub(super) fn member(
        &mut self,
        root: &Path,
        path: &Path,
        metadata: &fs::Metadata,
    ) -> Result<usize, Error> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::OutsideRoot(path.to_path_buf()))?;
        let directory = metadata.is_dir();
        let mode = match self.view {
            View::Pending => super::frozen_mode(metadata, directory),
            View::Retiring if relative.as_os_str().is_empty() => {
                super::frozen_mode(metadata, directory)
            }
            View::Published | View::Retiring => exact_mode(metadata, directory),
        };
        let member = Member {
            kind: if directory { DIRECTORY } else { FILE },
            mode,
            links: if directory { 0 } else { links(metadata) },
            length: if directory { 0 } else { metadata.len() },
        };
        self.members = self.members.checked_add(1).ok_or(Error::Allocation)?;
        if relative == Path::new(CONTROL) && self.view != View::Pending {
            if self.control.replace(member).is_some() {
                return Err(Error::Shape(path.to_path_buf()));
            }
            return Ok(0);
        }
        Ok(write(
            &mut self.hasher,
            relative.as_os_str().as_encoded_bytes(),
            member,
        ))
    }

    pub(super) fn finish(mut self) -> Result<[u8; 32], Error> {
        let control = match self.view {
            View::Pending => {
                self.members = self.members.checked_add(1).ok_or(Error::Allocation)?;
                Some(Member {
                    kind: FILE,
                    mode: CONTROL_MODE,
                    links: 1,
                    length: BUNDLE_RECORD_BYTES as u64,
                })
            }
            View::Published | View::Retiring => self.control,
        };
        match control {
            Some(control) => {
                self.hasher.update([1]);
                write(&mut self.hasher, CONTROL.as_bytes(), control);
            }
            None => self.hasher.update([0]),
        }
        self.hasher.update(self.members.to_le_bytes());
        Ok(self.hasher.finalize().into())
    }
}

fn write(hasher: &mut Sha256, relative: &[u8], member: Member) -> usize {
    hasher.update((relative.len() as u64).to_le_bytes());
    hasher.update(relative);
    hasher.update([member.kind]);
    hasher.update(member.mode.to_le_bytes());
    hasher.update(member.links.to_le_bytes());
    hasher.update(member.length.to_le_bytes());
    relative.len() + 8 + 1 + 4 + 8 + 8
}

#[cfg(unix)]
fn exact_mode(metadata: &fs::Metadata, _directory: bool) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o7777
}

#[cfg(not(unix))]
fn exact_mode(metadata: &fs::Metadata, directory: bool) -> u32 {
    super::frozen_mode(metadata, directory)
}

#[cfg(unix)]
fn links(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.nlink()
}

#[cfg(not(unix))]
fn links(_metadata: &fs::Metadata) -> u64 {
    1
}

/// Test-only measurement seam: injects the allocated length a filesystem
/// reports for one exact inode, modelling APFS speculative preallocation at
/// seal and its later release without depending on filesystem timing.
#[cfg(test)]
pub(crate) mod seam {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    thread_local! {
        static ALLOCATED: RefCell<BTreeMap<(u64, u64), u64>> = const { RefCell::new(BTreeMap::new()) };
        static LEGACY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Emulate a legacy (schema-4) binary on this thread: it writes schema-4 bundle
    /// records and admits them as authority.
    pub(crate) fn legacy_writer(enabled: bool) {
        LEGACY.with(|legacy| legacy.set(enabled));
    }

    pub(crate) fn is_legacy_writer() -> bool {
        LEGACY.with(std::cell::Cell::get)
    }

    #[cfg(unix)]
    fn key(metadata: &fs::Metadata) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt as _;
        (metadata.dev(), metadata.ino())
    }

    #[cfg(not(unix))]
    fn key(_metadata: &fs::Metadata) -> (u64, u64) {
        (0, 0)
    }

    /// Report `bytes` allocated for this file until cleared.
    pub(crate) fn set(path: &Path, bytes: u64) {
        let metadata = fs::symlink_metadata(path).expect("the seam target exists");
        ALLOCATED.with(|map| map.borrow_mut().insert(key(&metadata), bytes));
    }

    pub(crate) fn clear_all() {
        ALLOCATED.with(|map| map.borrow_mut().clear());
    }

    pub(crate) fn allocated(metadata: &fs::Metadata, measured: u64) -> u64 {
        ALLOCATED.with(|map| {
            map.borrow()
                .get(&key(metadata))
                .copied()
                .unwrap_or(measured)
        })
    }
}
