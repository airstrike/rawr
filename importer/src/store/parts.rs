//! Stored values as content-defined parts.
//!
//! A large stored value changes in a few regions from one edit to the next:
//! the API, the generic contract, the facade, the overlay and the runner's
//! generated sources. Their bytes are cut where their content says (a gear
//! rolling hash, so a boundary depends only on the bytes just before it and an
//! insertion moves no later boundary), each part is kept once under
//! `<target>/rawr/store.parts/<sha256>`, frozen, and the store keeps only the
//! manifest naming the parts in order. A new value writes the parts it
//! changed; every other part is already there. A part is verified by its
//! digest when read, so a missing or altered part refuses the value, never
//! misreads it. A part no stored manifest names goes when the store retires
//! the last value that named it ([`collect`]).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node;

pub(crate) mod sources;

const MAGIC: &[u8; 8] = b"RAWRAPP1";
const HEADER_BYTES: usize = 24;
const ENTRY_BYTES: usize = 40;
/// Parts are 16 KiB to 256 KiB, about 80 KiB on average.
const MIN_PART: usize = 16 * 1024;
const MAX_PART: usize = 256 * 1024;
const BOUNDARY: u64 = (1 << 16) - 1;
/// Held shared by a writer whose manifest the store has not published yet,
/// and exclusively by [`collect`].
const LOCK: &str = "lock";
/// The kinds whose content payloads are manifests.
const KINDS: [node::Kind; 4] = [
    node::Kind::Api,
    node::Kind::Generic,
    node::Kind::Facade,
    node::Kind::Overlay,
];

/// The parts the store at `store` keeps, beside it.
pub(crate) fn directory(store: &Path) -> PathBuf {
    store.with_file_name(format!(
        "{}.parts",
        store
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("store")
    ))
}

/// Keep each part of `bytes` not already kept and return their manifest.
pub(crate) fn write(directory: &Path, bytes: &[u8]) -> Result<Box<[u8]>, node::Error> {
    fs::create_dir_all(directory).map_err(io("create the parts directory", directory))?;
    let mut manifest = Vec::new();
    manifest
        .try_reserve_exact(HEADER_BYTES + ENTRY_BYTES * (bytes.len() / MIN_PART + 1))
        .map_err(|_| node::Error::Allocation)?;
    manifest.extend_from_slice(MAGIC);
    manifest.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    manifest.extend_from_slice(&[0; 8]);
    let mut count = 0_u64;
    let mut rest = bytes;
    while !rest.is_empty() {
        let (part, after) = rest.split_at(boundary(rest));
        rest = after;
        let digest: [u8; 32] = Sha256::digest(part).into();
        let path = directory.join(name(&digest));
        if fs::symlink_metadata(&path).is_err() {
            static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let staging = directory.join(format!(
                ".{}.{}.{}",
                name(&digest),
                std::process::id(),
                STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let mut file = fs::File::create(&staging).map_err(io("write a part", &staging))?;
            file.write_all(part)
                .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o444)))
                .map_err(io("write a part", &staging))?;
            drop(file);
            fs::rename(&staging, &path).map_err(io("publish a part", &path))?;
        }
        manifest.extend_from_slice(&(part.len() as u64).to_le_bytes());
        manifest.extend_from_slice(&digest);
        count += 1;
    }
    manifest[16..24].copy_from_slice(&count.to_le_bytes());
    Ok(manifest.into_boxed_slice())
}

/// The total length a well-formed manifest names.
pub(crate) fn check(manifest: &[u8]) -> Result<usize, node::Error> {
    let scalar = |start: usize| {
        manifest
            .get(start..start + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(node::Error::Codec("parts manifest is truncated"))
    };
    if manifest.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(node::Error::Codec("parts manifest magic changed"));
    }
    let total = scalar(8)?;
    let count = scalar(16)?;
    if count
        .checked_mul(ENTRY_BYTES)
        .and_then(|entries| entries.checked_add(HEADER_BYTES))
        != Some(manifest.len())
    {
        return Err(node::Error::Codec("parts manifest length is invalid"));
    }
    let mut sum = 0_usize;
    for (length, _) in entries(manifest) {
        let length = usize::try_from(length)
            .ok()
            .filter(|length| (1..=MAX_PART).contains(length))
            .ok_or(node::Error::Codec("part length is invalid"))?;
        sum = sum
            .checked_add(length)
            .ok_or(node::Error::Codec("parts total is invalid"))?;
    }
    if sum != total {
        return Err(node::Error::Codec("parts do not sum to their total"));
    }
    Ok(total)
}

/// Read back the bytes a manifest names, each part verified.
pub(crate) fn read(directory: &Path, manifest: &[u8]) -> Result<Box<[u8]>, node::Error> {
    let total = check(manifest)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| node::Error::Allocation)?;
    for (length, digest) in entries(manifest) {
        let path = directory.join(name(&digest));
        let part = fs::read(&path).map_err(|source| node::Error::Io {
            operation: "read a part",
            path: path.clone(),
            source,
        })?;
        if part.len() as u64 != length || <[u8; 32]>::from(Sha256::digest(&part)) != digest {
            return Err(node::Error::Corrupt {
                path,
                reason: "part differs from its manifest",
            });
        }
        bytes.extend_from_slice(&part);
    }
    Ok(bytes.into_boxed_slice())
}

/// Whether every part a checked manifest names is kept.
pub(crate) fn present(directory: &Path, manifest: &[u8]) -> Result<(), node::Error> {
    check(manifest)?;
    for (_, digest) in entries(manifest) {
        let path = directory.join(name(&digest));
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(node::Error::Corrupt {
                path,
                reason: "a part its manifest names is missing",
            });
        }
    }
    Ok(())
}

/// Whether an allocation of `class` and `kind` names parts, so retiring it
/// can leave some unnamed.
pub(super) fn named_by(class: super::accounting::Class, kind: node::Kind) -> bool {
    match class {
        super::accounting::Class::Content => KINDS.contains(&kind),
        super::accounting::Class::Bundle => kind == node::Kind::Runner,
        super::accounting::Class::Action => false,
    }
}

/// A shared hold on the parts: while it is held, [`collect`] deletes none, so
/// a writer can keep parts before the store holds the manifest that names
/// them.
pub(crate) struct Hold {
    _file: fs::File,
}

impl Hold {
    pub(crate) fn take(directory: &Path) -> Result<Self, node::Error> {
        let file = lock_file(directory)?;
        file.lock_shared()
            .map_err(io("hold the parts", &directory.join(LOCK)))?;
        Ok(Self { _file: file })
    }
}

/// Delete every part no manifest in the store at `store` names: neither a
/// content payload of a parted kind nor a bundle's [`sources`] index. Runs
/// under the store's publication lease, which every content publication
/// holds; a writer holding a [`Hold`] defers it to the next retirement. A
/// manifest that does not parse names nothing.
pub(super) fn collect(
    _publication: &super::lease::Publication<'_>,
    store: &Path,
) -> Result<(), node::Error> {
    let directory = directory(store);
    if !directory.is_dir() {
        return Ok(());
    }
    let file = lock_file(&directory)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(()),
        Err(fs::TryLockError::Error(source)) => {
            return Err(io("collect the parts", &directory.join(LOCK))(source));
        }
    }
    let named = named(store)?;
    let entries = fs::read_dir(&directory).map_err(io("list the parts", &directory))?;
    for entry in entries {
        let entry = entry.map_err(io("list the parts", &directory))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        // A staging file is a dead writer's: no writer runs while this
        // holds the lock and the publication lease.
        let unnamed = match digest(file_name) {
            Some(digest) => !named.contains(&digest),
            None => file_name.starts_with('.'),
        };
        if unnamed {
            let path = entry.path();
            fs::remove_file(&path).map_err(io("retire a part", &path))?;
        }
    }
    Ok(())
}

/// Every part a manifest in the store names.
pub(super) fn named(store: &Path) -> Result<BTreeSet<[u8; 32]>, node::Error> {
    let mut named = BTreeSet::new();
    let mut name = |manifest: &[u8]| {
        if check(manifest).is_ok() {
            named.extend(entries(manifest).map(|(_, digest)| digest));
        }
    };
    for kind in KINDS {
        let kind = store.join("content").join(kind.directory());
        for schema in children(&kind)? {
            for content in children(&schema)? {
                if let Ok(manifest) = fs::read(content.join("payload")) {
                    name(&manifest);
                }
            }
        }
    }
    // Registered bundles and attempts still building alike.
    for execution in children(&store.join("executions"))? {
        for bundle in children(&execution)? {
            if let Ok(index) = fs::read(bundle.join("generated").join(sources::FILE)) {
                for manifest in sources::manifests(&index) {
                    name(manifest);
                }
            }
        }
    }
    Ok(named)
}

fn children(directory: &Path) -> Result<Vec<PathBuf>, node::Error> {
    match fs::read_dir(directory) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(io("list the store for its parts", directory))
            })
            .collect(),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(io("list the store for its parts", directory)(source)),
    }
}

fn lock_file(directory: &Path) -> Result<fs::File, node::Error> {
    fs::create_dir_all(directory).map_err(io("create the parts directory", directory))?;
    let path = directory.join(LOCK);
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(io("open the parts lock", &path))
}

/// Each part's length and digest, in order, of a checked manifest.
fn entries(manifest: &[u8]) -> impl Iterator<Item = (u64, [u8; 32])> + '_ {
    manifest
        .get(HEADER_BYTES..)
        .unwrap_or_default()
        .as_chunks::<ENTRY_BYTES>()
        .0
        .iter()
        .map(|entry| {
            (
                u64::from_le_bytes(entry[..8].try_into().unwrap_or([0; 8])),
                entry[8..].try_into().unwrap_or([0; 32]),
            )
        })
}

fn io(operation: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> node::Error {
    let path = path.to_path_buf();
    move |source| node::Error::Io {
        operation,
        path,
        source,
    }
}

/// The length of the first part of `bytes`.
fn boundary(bytes: &[u8]) -> usize {
    if bytes.len() <= MIN_PART {
        return bytes.len();
    }
    let end = bytes.len().min(MAX_PART);
    let mut hash = 0_u64;
    for (offset, byte) in bytes[MIN_PART..end].iter().enumerate() {
        hash = (hash << 1).wrapping_add(GEAR[usize::from(*byte)]);
        if hash & BOUNDARY == 0 {
            return MIN_PART + offset + 1;
        }
    }
    end
}

fn name(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut name = String::with_capacity(64);
    for byte in digest {
        let _ = write!(name, "{byte:02x}");
    }
    name
}

/// The digest a part's file name spells, if it is one.
fn digest(name: &str) -> Option<[u8; 32]> {
    let bytes = name.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let mut digest = [0; 32];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        digest[index] = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(digest)
}

/// Fixed pseudo-random values per byte (SplitMix64), part of the format.
const GEAR: [u64; 256] = {
    let mut table = [0; 256];
    let mut state = 0x5241_5752_4150_4931_u64;
    let mut index = 0;
    while index < 256 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        table[index] = value ^ (value >> 31);
        index += 1;
    }
    table
};

#[cfg(test)]
mod tests;
