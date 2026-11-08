//! A bundle's generated sources as the manifests of their parts.
//!
//! A runner bundle keeps evidence of the sources Cargo built it from. Each
//! generation of them repeats most of the last one, and of the facade, so the
//! bundle keeps one index, `generated/sources.parts`, naming each file's
//! path, length, digest and the manifest of its parts.

use std::path::Path;

use crate::node;

/// The index's name in a bundle's `generated` directory.
pub(crate) const FILE: &str = "sources.parts";
const MAGIC: &[u8; 8] = b"RAWRSRC1";

/// One generated file the index names.
pub(crate) struct Entry<'a> {
    pub(crate) path: &'a str,
    pub(crate) length: u64,
    pub(crate) digest: [u8; 32],
    pub(crate) manifest: &'a [u8],
}

/// Keep each file's parts in `directory` and return the index naming them.
/// The caller holds the parts ([`super::Hold`]) until the index is in its
/// bundle.
pub(crate) fn write<'a>(
    directory: &Path,
    files: impl ExactSizeIterator<Item = (&'a str, &'a [u8], [u8; 32])>,
) -> Result<Box<[u8]>, node::Error> {
    let mut index = Vec::new();
    index.extend_from_slice(MAGIC);
    index.extend_from_slice(&(files.len() as u64).to_le_bytes());
    for (path, bytes, digest) in files {
        let manifest = super::write(directory, bytes)?;
        let path_length =
            u32::try_from(path.len()).map_err(|_| node::Error::Codec("source path is too long"))?;
        index.extend_from_slice(&path_length.to_le_bytes());
        index.extend_from_slice(path.as_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&digest);
        index.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
        index.extend_from_slice(&manifest);
    }
    Ok(index.into_boxed_slice())
}

/// Every entry of a well-formed index, in order, each manifest checked
/// against its file's length.
pub(crate) fn entries(index: &[u8]) -> Result<Vec<Entry<'_>>, node::Error> {
    let mut reader = Reader { bytes: index };
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(node::Error::Codec("generated sources index magic changed"));
    }
    let count = usize::try_from(reader.scalar()?)
        .map_err(|_| node::Error::Codec("generated sources index count is invalid"))?;
    // Every entry takes at least its fixed fields.
    if count > index.len() / 52 {
        return Err(node::Error::Codec(
            "generated sources index count is invalid",
        ));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let path_length = u32::from_le_bytes(
            reader
                .take(4)?
                .try_into()
                .map_err(|_| node::Error::Codec("generated sources index is truncated"))?,
        );
        let path = std::str::from_utf8(reader.take(path_length as usize)?)
            .map_err(|_| node::Error::Codec("generated source path is not UTF-8"))?;
        let length = reader.scalar()?;
        let digest = reader
            .take(32)?
            .try_into()
            .map_err(|_| node::Error::Codec("generated sources index is truncated"))?;
        let manifest_length = usize::try_from(reader.scalar()?)
            .map_err(|_| node::Error::Codec("generated sources index is truncated"))?;
        let manifest = reader.take(manifest_length)?;
        if super::check(manifest)? as u64 != length {
            return Err(node::Error::Codec(
                "generated source manifest differs from its length",
            ));
        }
        entries.push(Entry {
            path,
            length,
            digest,
            manifest,
        });
    }
    if !reader.bytes.is_empty() {
        return Err(node::Error::Codec(
            "generated sources index has trailing bytes",
        ));
    }
    Ok(entries)
}

/// Each manifest an index names; one that does not parse names none.
pub(crate) fn manifests(index: &[u8]) -> impl Iterator<Item = &[u8]> {
    entries(index)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.manifest)
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], node::Error> {
        if length > self.bytes.len() {
            return Err(node::Error::Codec("generated sources index is truncated"));
        }
        let (taken, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        Ok(taken)
    }

    fn scalar(&mut self) -> Result<u64, node::Error> {
        self.take(8)?
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(|_| node::Error::Codec("generated sources index is truncated"))
    }
}
