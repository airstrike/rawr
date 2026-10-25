//! The stored API as content-defined parts.
//!
//! A captured API is about 231 MB for Gallery, and an edit to one crate changes
//! a small region of it. The bytes are cut where their content says (a gear
//! rolling hash, so a boundary depends only on the bytes just before it and an
//! insertion moves no later boundary), each part is kept once under
//! `<target>/rawr/api-parts/<sha256>`, frozen, and the store keeps only the
//! manifest naming the parts in order. An edit writes the parts it changed;
//! every other part is already there. A part is verified by its digest when
//! read, so a missing or altered part refuses the API, never misreads it.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node;

const MAGIC: &[u8; 8] = b"RAWRAPP1";
const HEADER_BYTES: usize = 24;
const ENTRY_BYTES: usize = 40;
/// Parts are 16 KiB to 256 KiB, about 80 KiB on average.
const MIN_PART: usize = 16 * 1024;
const MAX_PART: usize = 256 * 1024;
const BOUNDARY: u64 = (1 << 16) - 1;

/// The project's kept API parts.
pub(crate) fn directory(context: &crate::context::Context) -> PathBuf {
    context.rawr().join("api-parts")
}

/// Keep each part of `bytes` not already kept and return their manifest.
pub(crate) fn write(directory: &Path, bytes: &[u8]) -> Result<Box<[u8]>, node::Error> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| node::Error::Io {
            operation,
            path,
            source,
        }
    };
    fs::create_dir_all(directory).map_err(io("create API parts directory", directory))?;
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
            let mut file = fs::File::create(&staging).map_err(io("write API part", &staging))?;
            file.write_all(part)
                .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o444)))
                .map_err(io("write API part", &staging))?;
            drop(file);
            fs::rename(&staging, &path).map_err(io("publish API part", &path))?;
        }
        manifest.extend_from_slice(&(part.len() as u64).to_le_bytes());
        manifest.extend_from_slice(&digest);
        count += 1;
    }
    manifest[16..24].copy_from_slice(&count.to_le_bytes());
    Ok(manifest.into_boxed_slice())
}

/// The total length a well-formed manifest names.
pub(crate) fn check(manifest: &[u8], limit: usize) -> Result<usize, node::Error> {
    let scalar = |start: usize| {
        manifest
            .get(start..start + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(node::Error::Codec("API parts manifest is truncated"))
    };
    if manifest.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(node::Error::Codec("API parts manifest magic changed"));
    }
    let total = scalar(8)?;
    let count = scalar(16)?;
    if count
        .checked_mul(ENTRY_BYTES)
        .and_then(|entries| entries.checked_add(HEADER_BYTES))
        != Some(manifest.len())
    {
        return Err(node::Error::Codec("API parts manifest length is invalid"));
    }
    if total > limit {
        return Err(node::Error::Limit {
            resource: "cached API graph bytes",
            limit,
            actual: total,
        });
    }
    let mut sum = 0_usize;
    for entry in manifest[HEADER_BYTES..].as_chunks::<ENTRY_BYTES>().0 {
        let length = usize::try_from(u64::from_le_bytes(entry[..8].try_into().unwrap_or([0; 8])))
            .ok()
            .filter(|length| (1..=MAX_PART).contains(length))
            .ok_or(node::Error::Codec("API part length is invalid"))?;
        sum = sum
            .checked_add(length)
            .ok_or(node::Error::Codec("API parts total is invalid"))?;
    }
    if sum != total {
        return Err(node::Error::Codec("API parts do not sum to their total"));
    }
    Ok(total)
}

/// Read back the bytes a checked manifest names, each part verified.
pub(crate) fn read(
    directory: &Path,
    manifest: &[u8],
    limit: usize,
) -> Result<Box<[u8]>, node::Error> {
    let total = check(manifest, limit)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| node::Error::Allocation)?;
    for entry in manifest[HEADER_BYTES..].as_chunks::<ENTRY_BYTES>().0 {
        let length = u64::from_le_bytes(entry[..8].try_into().unwrap_or([0; 8])) as usize;
        let digest: [u8; 32] = entry[8..].try_into().unwrap_or([0; 32]);
        let path = directory.join(name(&digest));
        let part = fs::read(&path).map_err(|source| node::Error::Io {
            operation: "read API part",
            path: path.clone(),
            source,
        })?;
        if part.len() != length || <[u8; 32]>::from(Sha256::digest(&part)) != digest {
            return Err(node::Error::Corrupt {
                path,
                reason: "API part differs from its manifest",
            });
        }
        bytes.extend_from_slice(&part);
    }
    Ok(bytes.into_boxed_slice())
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
mod tests {
    use std::fs;

    fn bytes(seed: u64, length: usize) -> Vec<u8> {
        let mut state = seed;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    fn parts(directory: &std::path::Path) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
        let mut parts = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                (path, modified)
            })
            .collect::<Vec<_>>();
        parts.sort();
        parts
    }

    #[test]
    fn an_edit_writes_only_the_parts_it_changed_and_every_part_is_verified() {
        let directory = std::env::temp_dir().join(format!("rawr-api-parts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let limit = usize::MAX;
        let original = bytes(7, 4 * 1024 * 1024);
        let manifest = super::write(&directory, &original).unwrap();
        assert_eq!(
            &*super::read(&directory, &manifest, limit).unwrap(),
            &original[..]
        );
        let before = parts(&directory);
        assert!(before.len() > 8, "the bytes are cut into parts");

        // An insertion in the middle moves every later byte.
        let mut edited = original.clone();
        edited.splice(2_000_000..2_000_010, *b"an edit that grows the API");
        let edited_manifest = super::write(&directory, &edited).unwrap();
        assert_eq!(
            &*super::read(&directory, &edited_manifest, limit).unwrap(),
            &edited[..]
        );
        let after = parts(&directory);
        let added = after.iter().filter(|part| !before.contains(part)).count();
        assert!(
            added <= 2,
            "only the edited region's parts are new: {added}"
        );
        assert!(
            before.iter().all(|part| after.contains(part)),
            "kept parts are untouched"
        );
        assert_eq!(
            super::write(&directory, &original).unwrap(),
            manifest,
            "the manifest is a function of the bytes"
        );

        // A missing or altered part refuses the API rather than misreading it.
        let (victim, _) = &after[after.len() / 2];
        let mut permissions = fs::metadata(victim).unwrap().permissions();
        // Owner write is enough to alter it; nothing else needs access.
        let mode = std::os::unix::fs::PermissionsExt::mode(&permissions) | 0o200;
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, mode);
        fs::set_permissions(victim, permissions).unwrap();
        fs::write(victim, b"altered").unwrap();
        assert!(
            super::read(&directory, &manifest, limit).is_err()
                || super::read(&directory, &edited_manifest, limit).is_err()
        );
        assert!(super::read(&directory, &manifest, original.len() - 1).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
