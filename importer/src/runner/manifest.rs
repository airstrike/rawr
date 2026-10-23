//! Canonical pre-link runner manifest and generated files.

use crate::bridge::BuildRequestKey;
use sha2::{Digest as _, Sha256};

/// Source-location-independent native inventory metadata.
pub mod coverage;

const SCHEMA: u32 = 7;
const WIRE_VERSION: u32 = 2;
const RUNTIME_VERSION: u32 = 5;

/// One normalized generated file.
pub struct File {
    path: Box<str>,
    bytes: Box<[u8]>,
    digest: [u8; 32],
}

/// One ordered runtime-table entry sealed before linking.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Entry {
    identity: [u8; 32],
    signature: [u8; 32],
    arguments: u32,
}

/// Exact generated evidence compiled into the runner.
pub struct GeneratedManifest {
    build: BuildRequestKey,
    files: Box<[FileRecord]>,
    entries: Box<[Entry]>,
    coverage: coverage::Inventory,
    source_map: Box<[u8]>,
    source_map_digest: [u8; 32],
    schema: u32,
    wire: u32,
    generator: u32,
    runtime: u32,
    profile: u32,
}

/// Opaque authentication of one exact manifest carrier embedded in a runner.
pub struct Carrier {
    identity: [u8; 32],
    entries: Box<[Entry]>,
    coverage: coverage::Inventory,
}

#[derive(Clone)]
pub(crate) struct FileRecord {
    pub(crate) path: Box<str>,
    pub(crate) bytes: usize,
    pub(crate) digest: [u8; 32],
}

impl File {
    pub(crate) fn new(path: impl Into<Box<str>>, bytes: Box<[u8]>, digest: [u8; 32]) -> Self {
        Self {
            path: path.into(),
            bytes,
            digest,
        }
    }

    /// Return the portable relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Borrow the exact normalized bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Borrow the exact content digest.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl Entry {
    pub(crate) const fn new(identity: [u8; 32], signature: [u8; 32], arguments: u32) -> Self {
        Self {
            identity,
            signature,
            arguments,
        }
    }

    /// Borrow the stable semantic operation identity.
    #[must_use]
    pub const fn identity(&self) -> &[u8; 32] {
        &self.identity
    }

    /// Borrow the exact generated-call signature digest.
    #[must_use]
    pub const fn signature(&self) -> &[u8; 32] {
        &self.signature
    }

    /// Return the exact positional argument count accepted by the thunk.
    #[must_use]
    pub const fn arguments(&self) -> u32 {
        self.arguments
    }
}

impl GeneratedManifest {
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn new(
        build: BuildRequestKey,
        files: Box<[FileRecord]>,
        entries: Box<[Entry]>,
        source_map: Box<[u8]>,
        source_map_digest: [u8; 32],
    ) -> Self {
        Self::with_coverage(
            build,
            files,
            entries,
            coverage::Inventory::empty(),
            source_map,
            source_map_digest,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_coverage(
        build: BuildRequestKey,
        files: Box<[FileRecord]>,
        entries: Box<[Entry]>,
        coverage: coverage::Inventory,
        source_map: Box<[u8]>,
        source_map_digest: [u8; 32],
    ) -> Self {
        Self {
            build,
            files,
            entries,
            coverage,
            source_map,
            source_map_digest,
            schema: SCHEMA,
            wire: WIRE_VERSION,
            generator: crate::bridge::generator_version(),
            runtime: RUNTIME_VERSION,
            profile: crate::profile::version(),
        }
    }

    /// Return the exact cache lookup key.
    #[must_use]
    pub const fn build_request_key(&self) -> BuildRequestKey {
        self.build
    }

    /// Iterate exact ordered table entries.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Borrow the canonical source map.
    #[must_use]
    pub fn source_map(&self) -> &[u8] {
        &self.source_map
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.generated-manifest/v2");
        digest.update(self.build.as_bytes());
        for file in &self.files {
            digest.update(
                u64::try_from(file.path.len())
                    .unwrap_or(u64::MAX)
                    .to_le_bytes(),
            );
            digest.update(file.path.as_bytes());
            digest.update(u64::try_from(file.bytes).unwrap_or(u64::MAX).to_le_bytes());
            digest.update(file.digest);
        }
        for entry in &self.entries {
            digest.update(entry.identity);
            digest.update(entry.signature);
            digest.update(entry.arguments.to_le_bytes());
        }
        digest.update(self.coverage.digest());
        digest.update(self.schema.to_le_bytes());
        digest.update(self.wire.to_le_bytes());
        digest.update(self.generator.to_le_bytes());
        digest.update(self.runtime.to_le_bytes());
        digest.update(self.profile.to_le_bytes());
        digest.finalize().into()
    }

    /// Encode the exact derived bytes compiled into the runner.
    #[doc(hidden)]
    pub fn carrier(&self) -> Result<Box<[u8]>, Error> {
        let mut bytes = Vec::new();
        let estimated = self
            .files
            .iter()
            .try_fold(8_usize + 32 + 8, |size, file| {
                size.checked_add(8 + 8 + 32)?.checked_add(file.path.len())
            })
            .and_then(|size| size.checked_add(8))
            .and_then(|size| size.checked_add(self.entries.len().checked_mul(68)?))
            .and_then(|size| size.checked_add(8 + self.coverage.bytes().len()))
            .and_then(|size| size.checked_add(8 + 5 * 4))
            .filter(|size| *size <= crate::protocol::DIAGNOSTIC_BYTES)
            .ok_or(Error::Carrier)?;
        bytes
            .try_reserve_exact(estimated)
            .map_err(|_| Error::Carrier)?;
        bytes.extend_from_slice(b"RAWRGM01");
        bytes.extend_from_slice(self.build.as_bytes());
        push_length(&mut bytes, self.files.len())?;
        for file in &self.files {
            push_length(&mut bytes, file.path.len())?;
            bytes.extend_from_slice(file.path.as_bytes());
            push_length(&mut bytes, file.bytes)?;
            bytes.extend_from_slice(&file.digest);
        }
        push_length(&mut bytes, self.entries.len())?;
        for entry in &self.entries {
            bytes.extend_from_slice(&entry.identity);
            bytes.extend_from_slice(&entry.signature);
            bytes.extend_from_slice(&entry.arguments.to_le_bytes());
        }
        push_length(&mut bytes, self.coverage.bytes().len())?;
        bytes.extend_from_slice(self.coverage.bytes());
        push_length(&mut bytes, 0)?;
        bytes.extend_from_slice(&self.schema.to_le_bytes());
        bytes.extend_from_slice(&self.wire.to_le_bytes());
        bytes.extend_from_slice(&self.generator.to_le_bytes());
        bytes.extend_from_slice(&self.runtime.to_le_bytes());
        bytes.extend_from_slice(&self.profile.to_le_bytes());
        if bytes.len() != estimated {
            return Err(Error::Carrier);
        }
        Ok(bytes.into_boxed_slice())
    }

    /// Validate every manifest/file/table invariant before cache publication.
    pub fn validate(&self, files: &[File]) -> Result<(), Error> {
        if self.schema != SCHEMA
            || self.wire != WIRE_VERSION
            || self.generator != crate::bridge::generator_version()
            || self.runtime != RUNTIME_VERSION
            || self.profile != crate::profile::version()
        {
            return Err(Error::Schema);
        }
        if self.files.len() != files.len() {
            return Err(Error::FileCardinality);
        }
        for (expected, actual) in self.files.iter().zip(files) {
            if expected.path != actual.path
                || expected.bytes != actual.bytes.len()
                || expected.digest != actual.digest
            {
                return Err(Error::FileDrift);
            }
        }
        if self
            .files
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(Error::FileOrder);
        }
        if self.entries.iter().enumerate().any(|(index, entry)| {
            self.entries[index + 1..]
                .iter()
                .any(|later| later.identity == entry.identity)
        }) {
            return Err(Error::EntryOrder);
        }
        self.coverage.validate(&self.entries)?;
        let source_map_digest: [u8; 32] = Sha256::digest(&self.source_map).into();
        if source_map_digest != self.source_map_digest {
            return Err(Error::SourceMap);
        }
        Ok(())
    }
}

/// The schema, wire, generator, runtime and profile versions that end a
/// carrier, in that order.
#[doc(hidden)]
#[must_use]
pub const fn carrier_versions() -> [u32; 5] {
    [
        SCHEMA,
        WIRE_VERSION,
        crate::bridge::generator_version(),
        RUNTIME_VERSION,
        crate::profile::version(),
    ]
}

pub(crate) fn carrier_identity(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.generated-carrier/v2");
    digest.update(bytes);
    digest.finalize().into()
}

impl Carrier {
    /// Authenticate the bounded canonical manifest carrier compiled into a runner.
    #[doc(hidden)]
    pub fn transform(mut bytes: &[u8]) -> Result<Self, Error> {
        let original = bytes;
        if bytes.len() > crate::protocol::DIAGNOSTIC_BYTES || take(&mut bytes, 8)? != b"RAWRGM01" {
            return Err(Error::Carrier);
        }
        let _build = take(&mut bytes, 32)?;
        let files = take_length(&mut bytes)?;
        if files > crate::protocol::DIAGNOSTIC_BYTES {
            return Err(Error::Carrier);
        }
        for _ in 0..files {
            let path = take_length(&mut bytes)?;
            let _ = take(&mut bytes, path)?;
            let _file_bytes = take_length(&mut bytes)?;
            let _digest = take(&mut bytes, 32)?;
        }
        let entry_count = take_length(&mut bytes)?;
        if entry_count > crate::protocol::DIAGNOSTIC_BYTES / 68 {
            return Err(Error::Carrier);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(entry_count)
            .map_err(|_| Error::Carrier)?;
        for _ in 0..entry_count {
            let identity = take(&mut bytes, 32)?
                .try_into()
                .map_err(|_| Error::Carrier)?;
            let signature = take(&mut bytes, 32)?
                .try_into()
                .map_err(|_| Error::Carrier)?;
            let arguments = u32::from_le_bytes(
                take(&mut bytes, 4)?
                    .try_into()
                    .map_err(|_| Error::Carrier)?,
            );
            entries.push(Entry::new(identity, signature, arguments));
        }
        let coverage_length = take_length(&mut bytes)?;
        let coverage = coverage::Inventory::decode(take(&mut bytes, coverage_length)?, &entries)?;
        let source_map = take_length(&mut bytes)?;
        let _ = take(&mut bytes, source_map)?;
        let schema = take_u32(&mut bytes)?;
        let wire = take_u32(&mut bytes)?;
        let generator = take_u32(&mut bytes)?;
        let runtime = take_u32(&mut bytes)?;
        let profile = take_u32(&mut bytes)?;
        if !bytes.is_empty()
            || schema != SCHEMA
            || wire != WIRE_VERSION
            || generator != crate::bridge::generator_version()
            || runtime != RUNTIME_VERSION
            || profile != crate::profile::version()
            || entries.iter().enumerate().any(|(index, entry)| {
                entries[index + 1..]
                    .iter()
                    .any(|later| later.identity == entry.identity)
            })
        {
            return Err(Error::Carrier);
        }
        Ok(Self {
            identity: carrier_identity(original),
            entries: entries.into_boxed_slice(),
            coverage,
        })
    }

    /// Borrow the exact manifest-carrier identity.
    #[doc(hidden)]
    #[must_use]
    pub const fn identity(&self) -> &[u8; 32] {
        &self.identity
    }

    /// Match compiler-derived use facts to exact generated metadata only.
    /// A successful match does not establish live-process or admission authority.
    #[doc(hidden)]
    pub fn cover<'c, 'u, F, M>(
        &'c self,
        used: coverage::Use<'u>,
        functions: F,
        mappers: M,
    ) -> Option<coverage::Matched<'c>>
    where
        F: ExactSizeIterator<Item = coverage::Function<'u>> + Clone,
        M: ExactSizeIterator<Item = coverage::Mapper<'u>> + Clone,
    {
        coverage::cover(self, used, functions, mappers)
    }

    /// Borrow the canonical inventory digest; this is metadata, not authority.
    #[doc(hidden)]
    #[must_use]
    pub const fn coverage_digest(&self) -> &[u8; 32] {
        self.coverage.digest()
    }

    /// Return owned carrier metadata items and bytes for caller accounting.
    #[doc(hidden)]
    #[must_use]
    pub fn retained_usage(&self) -> (usize, usize) {
        let (items, bytes) = self.coverage.retained_usage();
        (
            items + self.entries.len(),
            std::mem::size_of::<Self>() + self.entries.len() * std::mem::size_of::<Entry>() + bytes
                - std::mem::size_of::<coverage::Inventory>(),
        )
    }

    /// Prove that the manifest contains one exact generated adapter row.
    #[doc(hidden)]
    #[must_use]
    pub fn contains(&self, identity: &[u8; 32], signature: &[u8; 32], arguments: u32) -> bool {
        self.entries.iter().any(|entry| {
            entry.identity() == identity
                && entry.signature() == signature
                && entry.arguments() == arguments
        })
    }
}

/// Deterministic generated-manifest drift.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("generated manifest schema or component version changed")]
    Schema,
    #[error("generated file is missing or extra")]
    FileCardinality,
    #[error("generated file path, length, or digest changed")]
    FileDrift,
    #[error("generated file order is not canonical")]
    FileOrder,
    #[error("generated table entry is duplicate, colliding, or reordered")]
    EntryOrder,
    #[error("generated source map digest changed")]
    SourceMap,
    #[error("generated manifest carrier exceeded its deterministic bound")]
    Carrier,
}

fn push_length(bytes: &mut Vec<u8>, value: usize) -> Result<(), Error> {
    let value = u64::try_from(value).map_err(|_| Error::Carrier)?;
    bytes.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], Error> {
    if length > bytes.len() {
        return Err(Error::Carrier);
    }
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    Ok(value)
}

fn take_length(bytes: &mut &[u8]) -> Result<usize, Error> {
    let value = u64::from_le_bytes(take(bytes, 8)?.try_into().map_err(|_| Error::Carrier)?);
    usize::try_from(value).map_err(|_| Error::Carrier)
}

fn take_u32(bytes: &mut &[u8]) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        take(bytes, 4)?.try_into().map_err(|_| Error::Carrier)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{Carrier, RUNTIME_VERSION, SCHEMA, WIRE_VERSION};

    fn carrier(entries: &[([u8; 32], [u8; 32], u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RAWRGM01");
        bytes.extend_from_slice(&[3; 32]);
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        for (identity, signature, arguments) in entries {
            bytes.extend_from_slice(identity);
            bytes.extend_from_slice(signature);
            bytes.extend_from_slice(&arguments.to_le_bytes());
        }
        let coverage = super::coverage::Inventory::empty();
        bytes.extend_from_slice(&(coverage.bytes().len() as u64).to_le_bytes());
        bytes.extend_from_slice(coverage.bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&SCHEMA.to_le_bytes());
        bytes.extend_from_slice(&WIRE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&crate::bridge::generator_version().to_le_bytes());
        bytes.extend_from_slice(&RUNTIME_VERSION.to_le_bytes());
        bytes.extend_from_slice(&crate::profile::version().to_le_bytes());
        bytes
    }

    #[test]
    fn generated_carrier_authenticates_only_exact_rows_and_rejects_duplicate_or_tamper() {
        let bytes = carrier(&[([5; 32], [7; 32], 2)]);
        let proof = Carrier::transform(&bytes).expect("canonical carrier authenticates");
        assert!(proof.contains(&[5; 32], &[7; 32], 2));
        assert!(!proof.contains(&[6; 32], &[7; 32], 2));
        assert!(!proof.contains(&[5; 32], &[8; 32], 2));
        assert!(!proof.contains(&[5; 32], &[7; 32], 3));

        let duplicate = carrier(&[([5; 32], [7; 32], 2), ([5; 32], [8; 32], 3)]);
        assert!(Carrier::transform(&duplicate).is_err());

        let mut tampered = bytes;
        *tampered.last_mut().expect("carrier is nonempty") ^= 1;
        assert!(Carrier::transform(&tampered).is_err());
    }

    #[test]
    fn prior_manifest_schema_and_inventory_versions_fail_closed() {
        let mut bytes = carrier(&[]);
        let schema = bytes.len() - 20;
        bytes[schema..schema + 4].copy_from_slice(&6_u32.to_le_bytes());
        assert!(Carrier::transform(&bytes).is_err());

        let mut bytes = carrier(&[]);
        // Header, build, file count, entry count, inventory byte count.
        let inventory = 8 + 32 + 8 + 8 + 8;
        bytes[inventory..inventory + 4].copy_from_slice(&0_u32.to_le_bytes());
        assert!(Carrier::transform(&bytes).is_err());
    }
}
