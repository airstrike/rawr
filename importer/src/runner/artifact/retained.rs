//! Complete, fixed-path runtime data retained by a shared runner bundle.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node;
use crate::store::{self, bundle};

#[cfg(feature = "rustc-driver")]
mod batch;

#[cfg(feature = "rustc-driver")]
pub(crate) use batch::observe as observe_hashes;

const MAGIC: &[u8; 8] = b"RAWRDATA";
const SCHEMA: u32 = 1;
const RESOLUTION_SCHEMA: u32 = 3;
const HEADER: usize = 92;
const RECORD: usize = 49;
const ROOTS: [&[u8]; 4] = [b"build", b"declared", b"generated", b"target"];

/// Persistent inventory; only current replay can promote it to native authority.
pub(crate) struct Stored {
    canonical: Box<[u8]>,
    digest: [u8; 32],
    reference: bundle::BundleRef,
}

/// Positive mismatch reached while comparing one bounded owned inventory.
/// Construction is confined to the replay's actual member comparisons.
pub(crate) struct Mismatch {
    commitment: bundle::BundleCommitment,
}

impl Mismatch {
    pub(crate) fn commitment(self) -> bundle::BundleCommitment {
        self.commitment
    }
}

/// Supervisor controls independently validated outside the data inventory.
pub(crate) struct Control {
    pub schema: u32,
    pub key: node::Digest,
    pub seal_bytes: usize,
    pub seal_digest: [u8; 32],
}

/// Current retained data together with the original shared work authority.
pub(crate) struct Proof {
    stored: Stored,
    store: store::Store,
    control: Control,
}

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Pending,
    Frozen,
}

/// Exact error provenance for one resolution validation invocation.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy)]
pub(crate) enum Origin<'a> {
    Unpublished(Mode),
    Published(&'a store::Store, node::Kind, u32, node::Digest),
}

#[cfg(feature = "rustc-driver")]
impl Origin<'_> {
    pub(crate) fn mode(self) -> Mode {
        match self {
            Self::Unpublished(mode) => mode,
            Self::Published(..) => Mode::Frozen,
        }
    }

    fn mismatch(
        self,
        evidence: Mismatch,
        root: &Path,
        path: &Path,
        budget: &node::GraphBudget,
    ) -> node::Error {
        match self {
            Self::Unpublished(_) => corrupt(path),
            Self::Published(store, kind, schema, key) => {
                bundle::Corruption::member(evidence, root, store.root(), kind, schema, key, budget)
                    .map(node::Error::from)
                    .unwrap_or_else(|error| error)
            }
        }
    }
}

struct Record {
    path: Box<[u8]>,
    kind: u8,
    mode: u32,
    bytes: u64,
    digest: [u8; 32],
}

/// Borrowed expected facts; replay never constructs a second owned inventory.
struct Expected<'a> {
    path: &'a [u8],
    kind: u8,
    mode: u32,
    bytes: u64,
    digest: [u8; 32],
}

impl Stored {
    pub(crate) fn capture(
        root: &Path,
        reference: bundle::BundleRef,
        mode: Mode,
        budget: &node::GraphBudget,
    ) -> Result<Self, node::Error> {
        Self::capture_with(root, reference, mode, budget, SCHEMA, |_| true)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn capture_resolution(
        root: &Path,
        reference: bundle::BundleRef,
        mode: Mode,
        budget: &node::GraphBudget,
        scope: &crate::cargo::resolution::content::Scope,
    ) -> Result<Self, node::Error> {
        if scope.root() != root || !scope.shares_with(budget) {
            return Err(node::Error::State(
                "resolution content scope belongs to another bundle or graph",
            ));
        }
        Self::capture_classified(
            root,
            reference,
            mode,
            budget,
            RESOLUTION_SCHEMA,
            |path| scope.requires_content(path),
            |path| {
                if scope.requires_membership(path) {
                    3
                } else if scope.requires_directory(path) {
                    4
                } else {
                    0
                }
            },
        )
    }

    fn capture_with(
        root: &Path,
        reference: bundle::BundleRef,
        mode: Mode,
        budget: &node::GraphBudget,
        schema: u32,
        retains: impl Fn(&Path) -> bool,
    ) -> Result<Self, node::Error> {
        Self::capture_classified(root, reference, mode, budget, schema, retains, |_| {
            if schema == RESOLUTION_SCHEMA {
                3
            } else {
                0
            }
        })
    }

    fn capture_classified(
        root: &Path,
        reference: bundle::BundleRef,
        mode: Mode,
        budget: &node::GraphBudget,
        schema: u32,
        retains: impl Fn(&Path) -> bool,
        directory_kind: impl Fn(&Path) -> u8,
    ) -> Result<Self, node::Error> {
        let canonical = directory(root, budget)?;
        if canonical != root || !root.ends_with(reference.relative_budgeted(budget)?) {
            return Err(node::Error::OutsideRoot(root.to_path_buf()));
        }
        let mut pending = bundle::buffer::Buffer::new();
        for name in ROOTS.into_iter().rev() {
            let name =
                std::str::from_utf8(name).map_err(|_| node::Error::Codec("retained root"))?;
            push_path(&mut pending, root, Path::new(name), budget)?;
        }
        let mut records = bundle::buffer::Buffer::new();
        let mut length = HEADER
            .checked_add(root.as_os_str().as_encoded_bytes().len())
            .ok_or(node::Error::Allocation)?;
        let mut file_bytes = 0_u64;
        while let Some(path) = pending.pop() {
            budget.paths(1)?;
            store::path::input(&path, budget)?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|source| io("inspect retained member", &path, source))?;
            if metadata.file_type().is_symlink()
                || (!metadata.is_dir() && (!metadata.is_file() || bundle::hard_linked(&metadata)))
            {
                return Err(node::Error::Shape(path));
            }
            let expected_mode = bundle::frozen_mode(&metadata, metadata.is_dir());
            if matches!(mode, Mode::Frozen) && permissions(&metadata) != expected_mode {
                return Err(corrupt(&path));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| node::Error::OutsideRoot(path.clone()))?
                .as_os_str()
                .as_encoded_bytes();
            length = length
                .checked_add(RECORD)
                .and_then(|size| size.checked_add(relative.len()))
                .ok_or(node::Error::Allocation)?;
            bounded(length)?;
            budget.decode(relative.len())?;
            let relative = relative.to_vec().into_boxed_slice();
            let (kind, bytes, digest) = if metadata.is_dir() {
                let remaining = (bundle::MAX_MEMBERS as usize)
                    .saturating_sub(records.len() + pending.len() + 1);
                for child in bundle::paths(&path, remaining, budget)?.into_iter().rev() {
                    pending.push(child, budget)?;
                }
                (directory_kind(&path), 0, [0; 32])
            } else {
                file_bytes = file_bytes
                    .checked_add(metadata.len())
                    .ok_or(node::Error::Allocation)?;
                if file_bytes > bundle::MAX_BYTES {
                    return Err(node::Error::Limit {
                        resource: "runner retained data bytes",
                        limit: bundle::MAX_BYTES as usize,
                        actual: usize::try_from(file_bytes).unwrap_or(usize::MAX),
                    });
                }
                if retains(&path) {
                    (
                        1,
                        metadata.len(),
                        hash(&path, &metadata, bundle::MAX_BYTES as usize, budget)?,
                    )
                } else {
                    (2, metadata.len(), [0; 32])
                }
            };
            records.push(
                Record {
                    path: relative,
                    kind,
                    mode: expected_mode,
                    bytes,
                    digest,
                },
                budget,
            )?;
            if records.len() > bundle::MAX_MEMBERS as usize {
                return Err(node::Error::Limit {
                    resource: "runner retained members",
                    limit: bundle::MAX_MEMBERS as usize,
                    actual: records.len(),
                });
            }
        }
        let mut records = records.into_vec();
        records.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        budget.decode(length)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&schema.to_le_bytes());
        bytes.extend_from_slice(&reference.execution().as_bytes());
        bytes.extend_from_slice(&reference.id().as_bytes());
        bytes.extend_from_slice(&(length as u64).to_le_bytes());
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        let root = root.as_os_str().as_encoded_bytes();
        bytes.extend_from_slice(&(root.len() as u32).to_le_bytes());
        bytes.extend_from_slice(root);
        for record in records {
            bytes.extend_from_slice(&(record.path.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&record.path);
            bytes.push(record.kind);
            bytes.extend_from_slice(&record.mode.to_le_bytes());
            bytes.extend_from_slice(&record.bytes.to_le_bytes());
            bytes.extend_from_slice(&record.digest);
        }
        debug_assert_eq!(bytes.len(), length);
        budget.hash(bytes.len())?;
        Ok(Self {
            digest: Sha256::digest(&bytes).into(),
            canonical: bytes.into_boxed_slice(),
            reference,
        })
    }

    pub(crate) fn decode(
        bytes: &[u8],
        root: &Path,
        reference: bundle::BundleRef,
        budget: &node::GraphBudget,
    ) -> Result<Self, node::Error> {
        Self::decode_with(bytes, root, reference, budget, SCHEMA)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn decode_resolution(
        bytes: &[u8],
        root: &Path,
        reference: bundle::BundleRef,
        budget: &node::GraphBudget,
    ) -> Result<Self, node::Error> {
        Self::decode_with(bytes, root, reference, budget, RESOLUTION_SCHEMA)
    }

    fn decode_with(
        bytes: &[u8],
        root: &Path,
        reference: bundle::BundleRef,
        budget: &node::GraphBudget,
        schema: u32,
    ) -> Result<Self, node::Error> {
        bounded(bytes.len())?;
        if bytes.len() < HEADER
            || bytes.get(..8) != Some(MAGIC)
            || number::<4>(bytes, 8)? != schema.to_le_bytes()
            || number::<32>(bytes, 12)? != reference.execution().as_bytes()
            || number::<32>(bytes, 44)? != reference.id().as_bytes()
            || u64::from_le_bytes(number(bytes, 76)?) != bytes.len() as u64
        {
            return Err(node::Error::Codec("retained inventory header changed"));
        }
        let count = u32::from_le_bytes(number(bytes, 84)?) as usize;
        let root_length = u32::from_le_bytes(number(bytes, 88)?) as usize;
        let mut offset = HEADER
            .checked_add(root_length)
            .ok_or(node::Error::Allocation)?;
        if count < ROOTS.len()
            || count > bundle::MAX_MEMBERS as usize
            || count > bytes.len().saturating_sub(offset) / (RECORD + 1)
            || bytes.get(HEADER..offset) != Some(root.as_os_str().as_encoded_bytes())
        {
            return Err(node::Error::Codec(
                "retained inventory count or path changed",
            ));
        }
        // A flat, complete path/type stream is also the exact child set of every directory.
        budget.decode(
            count
                .checked_mul(std::mem::size_of::<(&[u8], u8)>())
                .ok_or(node::Error::Allocation)?,
        )?;
        let mut paths: Vec<(&[u8], u8)> = Vec::new();
        paths
            .try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        for _ in 0..count {
            let size = u32::from_le_bytes(number(bytes, offset)?) as usize;
            offset = offset.checked_add(4).ok_or(node::Error::Allocation)?;
            let end = offset.checked_add(size).ok_or(node::Error::Allocation)?;
            let path = bytes
                .get(offset..end)
                .ok_or(node::Error::Codec("retained path truncated"))?;
            let mut components = path.split(|byte| *byte == b'/');
            if !components.next().is_some_and(|part| ROOTS.contains(&part))
                || path.contains(&0)
                || path
                    .split(|byte| *byte == b'/')
                    .any(|part| part.is_empty() || part == b"." || part == b"..")
                || paths.last().is_some_and(|(prior, _)| *prior >= path)
            {
                return Err(node::Error::Codec("retained paths are not canonical"));
            }
            let kind = *bytes
                .get(end)
                .ok_or(node::Error::Codec("retained type truncated"))?;
            let mode = u32::from_le_bytes(number(bytes, end + 1)?);
            let length = u64::from_le_bytes(number(bytes, end + 5)?);
            let digest = number::<32>(bytes, end + 13)?;
            if kind > if schema == RESOLUTION_SCHEMA { 4 } else { 1 }
                || !valid_mode(mode, is_directory(kind))
                || (is_directory(kind) && (length != 0 || digest != [0; 32]))
                || (kind == 2 && digest != [0; 32])
            {
                return Err(node::Error::Codec("retained member record changed"));
            }
            if let Some(index) = path.iter().rposition(|byte| *byte == b'/') {
                let parent = &path[..index];
                if !paths
                    .binary_search_by(|(candidate, _)| candidate.cmp(&parent))
                    .ok()
                    .is_some_and(|index| {
                        is_directory(paths[index].1)
                            && (schema != RESOLUTION_SCHEMA
                                || !matches!(kind, 1 | 3 | 4)
                                || paths[index].1 != 0)
                    })
                {
                    return Err(node::Error::Codec("retained parent missing"));
                }
            } else if !is_directory(kind) {
                return Err(node::Error::Codec("retained data root is not a directory"));
            }
            paths.push((path, kind));
            offset = end.checked_add(RECORD - 4).ok_or(node::Error::Allocation)?;
        }
        if offset != bytes.len()
            || ROOTS
                .iter()
                .any(|root| paths.binary_search_by(|(path, _)| path.cmp(root)).is_err())
        {
            return Err(node::Error::Codec(
                "retained root set or record length changed",
            ));
        }
        budget.decode(bytes.len())?;
        budget.hash(bytes.len())?;
        Ok(Self {
            canonical: bytes.into(),
            digest: Sha256::digest(bytes).into(),
            reference,
        })
    }

    pub(crate) fn replay(
        &self,
        root: &Path,
        mode: Mode,
        budget: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        self.replay_with(root, mode, budget, |_, path| corrupt(path))
    }

    pub(crate) fn replay_published(
        &self,
        root: &Path,
        store: &store::Store,
        kind: node::Kind,
        schema: u32,
        key: node::Digest,
        budget: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        self.replay_with(root, Mode::Frozen, budget, |evidence, _| {
            bundle::Corruption::member(evidence, root, store.root(), kind, schema, key, budget)
                .map(node::Error::from)
                .unwrap_or_else(|error| error)
        })
    }

    fn replay_with(
        &self,
        root: &Path,
        mode: Mode,
        budget: &node::GraphBudget,
        mismatch: impl Fn(Mismatch, &Path) -> node::Error,
    ) -> Result<(), node::Error> {
        self.replay_content(
            root,
            mode,
            budget,
            node::read_set::Scope::Complete,
            None,
            mismatch,
            |path, metadata, _| hash(path, metadata, bundle::MAX_BYTES as usize, budget).map(Some),
        )
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn replay_resolution_content_from(
        &self,
        root: &Path,
        origin: Origin<'_>,
        budget: &node::GraphBudget,
        content: impl FnMut(&Path, &fs::Metadata, [u8; 32]) -> Result<[u8; 32], node::Error>,
    ) -> Result<(), node::Error> {
        self.replay_resolution_content_scoped_from(
            root,
            origin,
            budget,
            node::read_set::Scope::Complete,
            content,
        )
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn replay_resolution_content_scoped_from(
        &self,
        root: &Path,
        origin: Origin<'_>,
        budget: &node::GraphBudget,
        scope: node::read_set::Scope,
        content: impl FnMut(&Path, &fs::Metadata, [u8; 32]) -> Result<[u8; 32], node::Error>,
    ) -> Result<(), node::Error> {
        self.replay_resolution_content_completing(root, origin, budget, scope, None, content)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn replay_resolution_content_completing(
        &self,
        root: &Path,
        origin: Origin<'_>,
        budget: &node::GraphBudget,
        scope: node::read_set::Scope,
        completion: Option<bundle::Completion<'_>>,
        mut content: impl FnMut(&Path, &fs::Metadata, [u8; 32]) -> Result<[u8; 32], node::Error>,
    ) -> Result<(), node::Error> {
        if u32::from_le_bytes(number(&self.canonical, 8)?) != RESOLUTION_SCHEMA {
            return Err(node::Error::Codec(
                "joint replay requires resolution inventory",
            ));
        }
        self.replay_content(
            root,
            origin.mode(),
            budget,
            scope,
            completion,
            |evidence, path| origin.mismatch(evidence, root, path, budget),
            |path, metadata, expected| content(path, metadata, expected).map(Some),
        )
    }

    /// Complete geometry and the declared/generated control trees. Bulk build
    /// and target content remains an obligation, not a current replay proof.
    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn resolution_controls(
        &self,
        root: &Path,
        origin: Origin<'_>,
        budget: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        self.resolution_controls_scoped(root, origin, budget, node::read_set::Scope::Complete)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn resolution_controls_scoped(
        &self,
        root: &Path,
        origin: Origin<'_>,
        budget: &node::GraphBudget,
        scope: node::read_set::Scope,
    ) -> Result<(), node::Error> {
        if u32::from_le_bytes(number(&self.canonical, 8)?) != RESOLUTION_SCHEMA {
            return Err(node::Error::Codec("geometry requires resolution inventory"));
        }
        self.replay_content(
            root,
            origin.mode(),
            budget,
            scope,
            None,
            |evidence, path| origin.mismatch(evidence, root, path, budget),
            |path, metadata, _| {
                let relative = path.strip_prefix(root).map_err(|_| corrupt(path))?;
                let control = matches!(relative.components().next(),
                    Some(std::path::Component::Normal(name)) if name == "declared" || name == "generated");
                if control {
                    hash(path, metadata, bundle::MAX_BYTES as usize, budget).map(Some)
                } else {
                    Ok(None)
                }
            },
        )
    }

    /// Authenticate the exact owned control bytes before the caller interprets
    /// them. This grants no freshness to a later filesystem read.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn require_resolution_bytes(
        &self,
        root: &Path,
        origin: Origin<'_>,
        path: &Path,
        bytes: &[u8],
        budget: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        let (expected_length, expected) = self.resolution_commitment(root, path)?;
        budget.hash(bytes.len())?;
        let actual: [u8; 32] = Sha256::digest(bytes).into();
        if bytes.len() as u64 != expected_length || actual != expected {
            // These owned bytes reject interpretation, but their reader
            // may have raced a path replacement. Only the inventory's
            // safe current read can establish candidate-local corruption.
            self.replay_resolution_content_from(root, origin, budget, |path, metadata, _| {
                hash(path, metadata, bundle::MAX_BYTES as usize, budget)
            })?;
            return Err(node::Error::Codec(
                "early control bytes changed during read",
            ));
        }
        Ok(())
    }

    /// The sealed length and SHA-256 committed for one early control file.
    /// This is historical bundle content, never a current filesystem claim.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn resolution_commitment(
        &self,
        root: &Path,
        path: &Path,
    ) -> Result<(u64, [u8; 32]), node::Error> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| corrupt(path))?
            .as_os_str()
            .as_encoded_bytes();
        let count = u32::from_le_bytes(number(&self.canonical, 84)?) as usize;
        let root_length = u32::from_le_bytes(number(&self.canonical, 88)?) as usize;
        let mut offset = HEADER
            .checked_add(root_length)
            .ok_or(node::Error::Allocation)?;
        for _ in 0..count {
            let length = u32::from_le_bytes(number(&self.canonical, offset)?) as usize;
            offset = offset.checked_add(4).ok_or(node::Error::Allocation)?;
            let end = offset.checked_add(length).ok_or(node::Error::Allocation)?;
            let member = self
                .canonical
                .get(offset..end)
                .ok_or(node::Error::Codec("retained path truncated"))?;
            if member == relative {
                if self.canonical.get(end) != Some(&1) {
                    return Err(node::Error::Codec("early control lacks content commitment"));
                }
                return Ok((
                    u64::from_le_bytes(number(&self.canonical, end + 5)?),
                    number(&self.canonical, end + 13)?,
                ));
            }
            offset = end.checked_add(RECORD - 4).ok_or(node::Error::Allocation)?;
        }
        Err(node::Error::Codec("early control absent from inventory"))
    }

    fn replay_content(
        &self,
        root: &Path,
        mode: Mode,
        budget: &node::GraphBudget,
        scope: node::read_set::Scope,
        completion: Option<bundle::Completion<'_>>,
        mismatch: impl Fn(Mismatch, &Path) -> node::Error,
        mut content: impl FnMut(&Path, &fs::Metadata, [u8; 32]) -> Result<Option<[u8; 32]>, node::Error>,
    ) -> Result<(), node::Error> {
        let count = u32::from_le_bytes(number(&self.canonical, 84)?) as usize;
        let root_length = u32::from_le_bytes(number(&self.canonical, 88)?) as usize;
        let mut offset = HEADER
            .checked_add(root_length)
            .ok_or(node::Error::Allocation)?;
        if self.canonical.get(HEADER..offset) != Some(root.as_os_str().as_encoded_bytes())
            || directory(root, budget)? != root
            || !root.ends_with(self.reference.relative_budgeted(budget)?)
        {
            return Err(corrupt(root));
        }
        budget.decode(
            count
                .checked_mul(std::mem::size_of::<Expected<'_>>())
                .ok_or(node::Error::Allocation)?,
        )?;
        let mut expected = Vec::new();
        expected
            .try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        for _ in 0..count {
            let size = u32::from_le_bytes(number(&self.canonical, offset)?) as usize;
            offset = offset.checked_add(4).ok_or(node::Error::Allocation)?;
            let end = offset.checked_add(size).ok_or(node::Error::Allocation)?;
            let path = self
                .canonical
                .get(offset..end)
                .ok_or(node::Error::Codec("retained path truncated"))?;
            expected.push(Expected {
                path,
                kind: *self
                    .canonical
                    .get(end)
                    .ok_or(node::Error::Codec("retained type truncated"))?,
                mode: u32::from_le_bytes(number(&self.canonical, end + 1)?),
                bytes: u64::from_le_bytes(number(&self.canonical, end + 5)?),
                digest: number(&self.canonical, end + 13)?,
            });
            offset = end.checked_add(RECORD - 4).ok_or(node::Error::Allocation)?;
        }
        if offset != self.canonical.len() {
            return Err(corrupt(root));
        }
        if scope == node::read_set::Scope::Consumer {
            if completion.is_some() {
                return Err(node::Error::Codec("complete obligation in consumer replay"));
            }
            if u32::from_le_bytes(number(&self.canonical, 8)?) != RESOLUTION_SCHEMA {
                return Err(node::Error::Codec(
                    "consumer replay requires resolution inventory",
                ));
            }
            return self.replay_consumer(root, mode, budget, &expected, mismatch, content);
        }
        // Mark each expected member once: counting alone could accept a repeated
        // directory entry in place of a missing member during enumeration.
        budget.decode(count)?;
        let mut seen = Vec::new();
        seen.try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        seen.resize(count, 0_u8);
        let mut visited = 0_usize;
        let mut file_bytes = 0_u64;
        let mut member = |path: &Path, metadata: &fs::Metadata| {
            // Root and allocation controls are measured by the shared walker,
            // but only payload trees have retained inventory records.
            let payload = path
                .strip_prefix(root)
                .ok()
                .and_then(|path| path.components().next());
            if !matches!(payload, Some(std::path::Component::Normal(name))
                if ROOTS.contains(&name.as_encoded_bytes()))
            {
                return Ok(());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| node::Error::OutsideRoot(path.to_path_buf()))?
                .as_os_str()
                .as_encoded_bytes();
            let index = expected
                .binary_search_by(|entry| entry.path.cmp(relative))
                .map_err(|_| corrupt(path))?;
            if seen[index] != 0 {
                return Err(corrupt(path));
            }
            seen[index] = 1;
            visited += 1;
            let entry = &expected[index];
            if metadata.file_type().is_symlink()
                || (!metadata.is_dir() && (!metadata.is_file() || bundle::hard_linked(metadata)))
                || is_directory(entry.kind) != metadata.is_dir()
                || bundle::frozen_mode(metadata, metadata.is_dir()) != entry.mode
                || (matches!(mode, Mode::Frozen) && permissions(metadata) != entry.mode)
            {
                return Err(corrupt(path));
            }
            if !metadata.is_dir() {
                file_bytes = file_bytes
                    .checked_add(metadata.len())
                    .ok_or(node::Error::Allocation)?;
                if file_bytes > bundle::MAX_BYTES {
                    return Err(node::Error::Limit {
                        resource: "runner retained data bytes",
                        limit: bundle::MAX_BYTES as usize,
                        actual: usize::try_from(file_bytes).unwrap_or(usize::MAX),
                    });
                }
                if metadata.len() != entry.bytes
                    || (entry.kind == 1
                        && content(path, metadata, entry.digest)?
                            .is_some_and(|actual| actual != entry.digest))
                {
                    return Err(mismatch(
                        Mismatch {
                            commitment: bundle::BundleCommitment::new(
                                self.reference,
                                self.canonical.len(),
                                self.digest,
                            ),
                        },
                        path,
                    ));
                }
            }
            Ok(())
        };
        if let Some(completion) = completion {
            if scope != node::read_set::Scope::Complete {
                return Err(node::Error::Codec("complete obligation in consumer replay"));
            }
            completion.observe(root, budget, &mut member)?;
        } else {
            let mut pending = bundle::buffer::Buffer::new();
            for name in ROOTS.into_iter().rev() {
                let name =
                    std::str::from_utf8(name).map_err(|_| node::Error::Codec("retained root"))?;
                push_path(&mut pending, root, Path::new(name), budget)?;
            }
            let mut walked = 0_usize;
            while let Some(path) = pending.pop() {
                walked += 1;
                budget.paths(1)?;
                store::path::input(&path, budget)?;
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|source| io("inspect retained member", &path, source))?;
                member(&path, &metadata)?;
                if metadata.is_dir() {
                    let remaining =
                        (bundle::MAX_MEMBERS as usize).saturating_sub(walked + pending.len());
                    for child in bundle::paths(&path, remaining, budget)?.into_iter().rev() {
                        pending.push(child, budget)?;
                    }
                }
            }
        }
        if visited != count {
            return Err(mismatch(
                Mismatch {
                    commitment: bundle::BundleCommitment::new(
                        self.reference,
                        self.canonical.len(),
                        self.digest,
                    ),
                },
                root,
            ));
        }
        Ok(())
    }

    fn replay_consumer(
        &self,
        root: &Path,
        mode: Mode,
        budget: &node::GraphBudget,
        expected: &[Expected<'_>],
        mismatch: impl Fn(Mismatch, &Path) -> node::Error,
        mut content: impl FnMut(&Path, &fs::Metadata, [u8; 32]) -> Result<Option<[u8; 32]>, node::Error>,
    ) -> Result<(), node::Error> {
        for entry in expected
            .iter()
            .filter(|entry| matches!(entry.kind, 1 | 3 | 4))
        {
            #[cfg(unix)]
            let relative = {
                use std::os::unix::ffi::OsStrExt as _;
                Path::new(std::ffi::OsStr::from_bytes(entry.path))
            };
            #[cfg(not(unix))]
            let relative = Path::new(
                std::str::from_utf8(entry.path)
                    .map_err(|_| node::Error::Codec("retained path encoding"))?,
            );
            let path = bundle::path(root, relative, budget)?;
            budget.paths(1)?;
            store::path::input(&path, budget)?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|source| io("inspect retained consumer", &path, source))?;
            if metadata.file_type().is_symlink()
                || is_directory(entry.kind) != metadata.is_dir()
                || (!metadata.is_dir() && (!metadata.is_file() || bundle::hard_linked(&metadata)))
                || bundle::frozen_mode(&metadata, metadata.is_dir()) != entry.mode
                || (matches!(mode, Mode::Frozen) && permissions(&metadata) != entry.mode)
            {
                return Err(corrupt(&path));
            }
            if metadata.is_dir() {
                if directory(&path, budget)? != path {
                    return Err(corrupt(&path));
                }
                if entry.kind == 3 {
                    let children = bundle::paths(&path, bundle::MAX_MEMBERS as usize, budget)?;
                    let start = expected.partition_point(|child| {
                        child
                            .path
                            .iter()
                            .cmp(entry.path.iter().chain(std::iter::once(&b'/')))
                            .is_lt()
                    });
                    let mut wanted = expected[start..]
                        .iter()
                        .take_while(|child| {
                            child.path.starts_with(entry.path)
                                && child.path.get(entry.path.len()) == Some(&b'/')
                        })
                        .filter(|child| {
                            child
                                .path
                                .iter()
                                .rposition(|byte| *byte == b'/')
                                .is_some_and(|index| &child.path[..index] == entry.path)
                        });
                    for child in children {
                        let relative = child
                            .strip_prefix(root)
                            .map_err(|_| corrupt(&child))?
                            .as_os_str()
                            .as_encoded_bytes();
                        if wanted.next().is_none_or(|wanted| wanted.path != relative) {
                            return Err(corrupt(&path));
                        }
                    }
                    if wanted.next().is_some() {
                        return Err(corrupt(&path));
                    }
                }
            } else if metadata.len() != entry.bytes
                || content(&path, &metadata, entry.digest)?
                    .is_some_and(|actual| actual != entry.digest)
            {
                return Err(mismatch(
                    Mismatch {
                        commitment: bundle::BundleCommitment::new(
                            self.reference,
                            self.canonical.len(),
                            self.digest,
                        ),
                    },
                    &path,
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.canonical
    }
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn into_bytes(self) -> Box<[u8]> {
        self.canonical
    }
    pub(crate) const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub(crate) fn native_identity(&self, executable: [u8; 32]) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.runner.native-artifact/v1");
        digest.update(executable);
        digest.update(self.digest);
        digest.update(self.reference.execution().as_bytes());
        digest.update(self.reference.id().as_bytes());
        digest.finalize().into()
    }
}

impl Proof {
    pub(crate) fn validate(
        stored: Stored,
        store: store::Store,
        control: Control,
    ) -> Result<Self, node::Error> {
        let proof = Self {
            stored,
            store,
            control,
        };
        proof.revalidate()?;
        Ok(proof)
    }

    pub(crate) fn revalidate(&self) -> Result<(), node::Error> {
        let root = bundle::validate_complete(
            self.store.root(),
            self.stored.reference,
            node::Kind::Runner,
            self.control.schema,
            self.control.key,
            self.control.seal_bytes,
            self.control.seal_digest,
            self.store.graph_budget(),
        )?;
        self.stored.replay_published(
            &root,
            &self.store,
            node::Kind::Runner,
            self.control.schema,
            self.control.key,
            self.store.graph_budget(),
        )
    }

    pub(crate) fn budget(&self) -> &node::GraphBudget {
        self.store.graph_budget()
    }
    pub(crate) fn native_identity(&self, executable: [u8; 32]) -> [u8; 32] {
        self.stored.native_identity(executable)
    }
}

fn is_directory(kind: u8) -> bool {
    matches!(kind, 0 | 3 | 4)
}

pub(crate) fn directory(path: &Path, budget: &node::GraphBudget) -> Result<PathBuf, node::Error> {
    let anchor = path
        .ancestors()
        .last()
        .ok_or_else(|| node::Error::Shape(path.to_path_buf()))?;
    let relative = path
        .strip_prefix(anchor)
        .map_err(|_| node::Error::OutsideRoot(path.to_path_buf()))?;
    store::existing_directory_budgeted(anchor, relative, budget)?.ok_or_else(|| corrupt(path))
}

pub(crate) fn hash(
    path: &Path,
    metadata: &fs::Metadata,
    limit: usize,
    budget: &node::GraphBudget,
) -> Result<[u8; 32], node::Error> {
    let length = usize::try_from(metadata.len()).map_err(|_| node::Error::Allocation)?;
    if length > limit {
        return Err(node::Error::Limit {
            resource: "runner artifact file bytes",
            limit,
            actual: length,
        });
    }
    // The digest is a pure function of the file bytes: one computation per
    // physical identity serves every later check. `metadata` is the caller's
    // current no-follow observation of `path` under a parent it has proved
    // canonical, so an identity hit needs neither the bytes nor a re-check.
    let identity = crate::file_digest::Identity::of(metadata);
    if let Some(digest) = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256)
        && !bundle::hard_linked(metadata)
    {
        return Ok(digest);
    }
    budget.hash(length.checked_add(1).ok_or(node::Error::Allocation)?)?;
    let parent = path
        .parent()
        .ok_or_else(|| node::Error::Shape(path.to_path_buf()))?;
    if directory(parent, budget)? != parent {
        return Err(node::Error::OutsideRoot(path.to_path_buf()));
    }
    budget.paths(1)?;
    store::path::input(path, budget)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut source = options
        .open(path)
        .map_err(|source| io("open retained file", path, source))?;
    budget.paths(1)?;
    let opened = source
        .metadata()
        .map_err(|source| io("inspect opened retained file", path, source))?;
    if !opened.is_file()
        || bundle::hard_linked(&opened)
        || opened.len() != metadata.len()
        || permissions(&opened) != permissions(metadata)
        || !same_file(metadata, &opened)
    {
        return Err(corrupt(path));
    }
    let opened_identity = crate::file_digest::Identity::of(&opened);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining = length;
    while remaining != 0 {
        let size = remaining.min(buffer.len());
        source
            .read_exact(&mut buffer[..size])
            .map_err(|source| io("hash retained file", path, source))?;
        digest.update(&buffer[..size]);
        remaining -= size;
    }
    if source
        .read(&mut buffer[..1])
        .map_err(|source| io("check retained file end", path, source))?
        != 0
    {
        return Err(corrupt(path));
    }
    budget.paths(1)?;
    store::path::input(path, budget)?;
    let after =
        fs::symlink_metadata(path).map_err(|source| io("reinspect retained file", path, source))?;
    if !same_file(metadata, &after)
        || after.len() != metadata.len()
        || permissions(&after) != permissions(metadata)
        || after.file_type().is_symlink()
        || bundle::hard_linked(&after)
    {
        return Err(corrupt(path));
    }
    let digest: [u8; 32] = digest.finalize().into();
    if crate::file_digest::Identity::of(&after) == opened_identity {
        crate::file_digest::insert(opened_identity, crate::file_digest::Domain::Sha256, digest);
    }
    Ok(digest)
}

fn push_path(
    paths: &mut bundle::buffer::Buffer<PathBuf>,
    parent: &Path,
    name: &Path,
    budget: &node::GraphBudget,
) -> Result<(), node::Error> {
    let length = parent
        .as_os_str()
        .as_encoded_bytes()
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(name.as_os_str().as_encoded_bytes().len()))
        .ok_or(node::Error::Allocation)?;
    budget.decode(length)?;
    let mut path = PathBuf::new();
    path.try_reserve_exact(length)
        .map_err(|_| node::Error::Allocation)?;
    path.push(parent);
    path.push(name);
    paths.push(path, budget)
}

fn bounded(length: usize) -> Result<(), node::Error> {
    if length > node::MAX_CONTENT_BYTES {
        Err(node::Error::Limit {
            resource: "runner retained inventory bytes",
            limit: node::MAX_CONTENT_BYTES,
            actual: length,
        })
    } else {
        Ok(())
    }
}

fn number<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], node::Error> {
    bytes
        .get(offset..offset.saturating_add(N))
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(node::Error::Codec("retained inventory truncated"))
}

fn corrupt(path: &Path) -> node::Error {
    node::Error::Corrupt {
        path: path.to_path_buf(),
        reason: "runner retained data changed",
    }
}
fn io(operation: &'static str, path: &Path, source: std::io::Error) -> node::Error {
    node::Error::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(unix)]
pub(crate) fn permissions(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o7777
}
#[cfg(not(unix))]
pub(crate) fn permissions(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}
#[cfg(unix)]
fn valid_mode(mode: u32, directory: bool) -> bool {
    mode == 0o555 || (!directory && mode == 0o444)
}
#[cfg(not(unix))]
fn valid_mode(mode: u32, _directory: bool) -> bool {
    mode == 1
}
#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev() && left.ino() == right.ino()
}
#[cfg(not(unix))]
fn same_file(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
        reference: bundle::BundleRef,
    }

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "rawr-retained-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&base).unwrap();
            let base = fs::canonicalize(base).unwrap();
            let reference = bundle::BundleRef::new(
                node::ExecutionKey::from_bytes([1; 32]),
                bundle::BundleId::from_bytes([2; 32]),
            );
            let root = base.join(reference.relative());
            for name in [
                "build/opaque/out/empty",
                "target/debug",
                "generated",
                "declared",
            ] {
                fs::create_dir_all(root.join(name)).unwrap();
            }
            fs::write(root.join("build/opaque/out/runtime.asset"), b"runtime data").unwrap();
            fs::write(root.join("target/debug/runner"), b"exe!").unwrap();
            fs::write(root.join("generated/source"), b"source").unwrap();
            Self {
                base,
                root,
                reference,
            }
        }

        fn capture(&self) -> Stored {
            Stored::capture(
                &self.root,
                self.reference,
                Mode::Pending,
                &node::GraphBudget::new(),
            )
            .unwrap()
        }
        fn replay(&self, stored: &Stored, mode: Mode) -> Result<(), node::Error> {
            stored.replay(&self.root, mode, &node::GraphBudget::new())
        }

        #[cfg(unix)]
        fn freeze(&self) {
            use std::os::unix::fs::PermissionsExt as _;
            fn visit(path: &Path) {
                let meta = fs::symlink_metadata(path).unwrap();
                if meta.is_dir() {
                    for child in fs::read_dir(path).unwrap() {
                        visit(&child.unwrap().path());
                    }
                }
                fs::set_permissions(
                    path,
                    fs::Permissions::from_mode(bundle::frozen_mode(&meta, meta.is_dir())),
                )
                .unwrap();
            }
            visit(&self.root);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fn thaw(path: &Path) {
                    if let Ok(meta) = fs::symlink_metadata(path)
                        && meta.is_dir()
                        && !meta.file_type().is_symlink()
                    {
                        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
                        for child in fs::read_dir(path).unwrap() {
                            thaw(&child.unwrap().path());
                        }
                    }
                }
                thaw(&self.base);
            }
            fs::remove_dir_all(&self.base).unwrap();
        }
    }

    #[cfg(feature = "rustc-driver")]
    fn scoped_fixture(fixture: &Fixture) -> Stored {
        let scratch = fixture.root.join("target/debug/scratch");
        Stored::capture_classified(
            &fixture.root,
            fixture.reference,
            Mode::Pending,
            &node::GraphBudget::new(),
            RESOLUTION_SCHEMA,
            |path| !path.starts_with(&scratch),
            |path| {
                if path.starts_with(&scratch) {
                    0
                } else if path.starts_with(fixture.root.join("target")) {
                    4
                } else {
                    3
                }
            },
        )
        .unwrap()
    }

    #[cfg(feature = "rustc-driver")]
    fn consumer(fixture: &Fixture, stored: &Stored) -> Result<(), node::Error> {
        let budget = node::GraphBudget::new();
        stored.replay_resolution_content_scoped_from(
            &fixture.root,
            Origin::Unpublished(Mode::Pending),
            &budget,
            node::read_set::Scope::Consumer,
            |path, metadata, _| hash(path, metadata, bundle::MAX_BYTES as usize, &budget),
        )
    }

    #[test]
    #[cfg(feature = "rustc-driver")]
    fn resolution_consumer_defers_only_irrelevant_physical_drift_until_complete() {
        let fixture = Fixture::new();
        let scratch = fixture.root.join("target/debug/scratch");
        fs::create_dir(&scratch).unwrap();
        fs::write(scratch.join("temporary"), b"before").unwrap();
        let stored = scoped_fixture(&fixture);
        let stored = Stored::decode_resolution(
            stored.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new(),
        )
        .unwrap();
        let mut old = stored.bytes().to_vec();
        old[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert!(Stored::decode_resolution(
            &old,
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new()
        )
        .is_err());
        consumer(&fixture, &stored).unwrap();
        fs::write(scratch.join("temporary"), b"different length").unwrap();
        consumer(&fixture, &stored).unwrap();
        assert!(fixture.replay(&stored, Mode::Pending).is_err());
        fs::remove_dir_all(&scratch).unwrap();
        consumer(&fixture, &stored).unwrap();
        assert!(fixture.replay(&stored, Mode::Pending).is_err());
        fs::create_dir(&scratch).unwrap();
        fs::write(scratch.join("temporary"), b"before").unwrap();
        fixture.replay(&stored, Mode::Pending).unwrap();
        fs::write(fixture.root.join("generated/added"), b"new control").unwrap();
        assert!(consumer(&fixture, &stored).is_err());
        fs::remove_file(fixture.root.join("generated/added")).unwrap();
        fs::remove_file(fixture.root.join("generated/source")).unwrap();
        assert!(consumer(&fixture, &stored).is_err());
        fs::create_dir(fixture.root.join("generated/source")).unwrap();
        assert!(consumer(&fixture, &stored).is_err());
    }

    #[test]
    #[cfg(all(feature = "rustc-driver", unix))]
    fn resolution_consumer_rejects_relevant_alias_and_uncharged_parent_kind() {
        let fixture = Fixture::new();
        let stored = scoped_fixture(&fixture);
        let relevant = fixture.root.join("target/debug/runner");
        fs::rename(&relevant, fixture.root.join("target/debug/renamed")).unwrap();
        std::os::unix::fs::symlink("renamed", &relevant).unwrap();
        assert!(consumer(&fixture, &stored).is_err());
        fs::remove_file(&relevant).unwrap();
        fs::rename(fixture.root.join("target/debug/renamed"), &relevant).unwrap();
        consumer(&fixture, &stored).unwrap();
        let invalid = Stored::capture_classified(
            &fixture.root,
            fixture.reference,
            Mode::Pending,
            &node::GraphBudget::new(),
            RESOLUTION_SCHEMA,
            |_| true,
            |_| 0,
        )
        .unwrap();
        assert!(Stored::decode_resolution(
            invalid.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new()
        )
        .is_err());
    }

    #[test]
    #[cfg(feature = "rustc-driver")]
    fn resolution_non_content_members_keep_structural_checks_and_cannot_be_native() {
        let fixture = Fixture::new();
        let scratch = fixture.root.join("build/opaque/scratch");
        fs::write(&scratch, b"one").unwrap();
        let native = fixture.capture();
        let selected = Stored::capture_with(
            &fixture.root,
            fixture.reference,
            Mode::Pending,
            &node::GraphBudget::new(),
            RESOLUTION_SCHEMA,
            |path| path != scratch,
        )
        .unwrap();
        assert!(Stored::decode(
            selected.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new(),
        )
        .is_err());
        assert!(Stored::decode_resolution(
            native.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new(),
        )
        .is_err());
        let selected = Stored::decode_resolution(
            selected.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new(),
        )
        .unwrap();
        fs::write(&scratch, b"two").unwrap();
        fixture.replay(&selected, Mode::Pending).unwrap();
        assert!(fixture.replay(&native, Mode::Pending).is_err());
        fs::write(&scratch, b"longer").unwrap();
        assert!(fixture.replay(&selected, Mode::Pending).is_err());
        fs::write(&scratch, b"one").unwrap();
        fs::write(
            fixture.root.join("build/opaque/out/runtime.asset"),
            b"changed data",
        )
        .unwrap();
        assert!(fixture.replay(&selected, Mode::Pending).is_err());
        fs::write(
            fixture.root.join("build/opaque/out/runtime.asset"),
            b"runtime data",
        )
        .unwrap();
        fixture.replay(&selected, Mode::Pending).unwrap();
        fs::remove_file(&scratch).unwrap();
        assert!(fixture.replay(&selected, Mode::Pending).is_err());
    }

    #[test]
    #[cfg(feature = "rustc-driver")]
    fn geometry_does_not_authenticate_changed_control_bytes() {
        let fixture = Fixture::new();
        let stored = Stored::capture_with(
            &fixture.root,
            fixture.reference,
            Mode::Pending,
            &node::GraphBudget::new(),
            RESOLUTION_SCHEMA,
            |_| true,
        )
        .unwrap();
        let path = fixture.root.join("build/opaque/out/runtime.asset");
        fs::write(&path, b"changed data").unwrap();
        let graph = node::GraphBudget::new();
        let origin = Origin::Unpublished(Mode::Pending);
        stored
            .resolution_controls(&fixture.root, origin, &graph)
            .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(stored
            .require_resolution_bytes(&fixture.root, origin, &path, &bytes, &graph)
            .is_err());
        assert!(stored.replay(&fixture.root, Mode::Pending, &graph).is_err());
        fs::write(&path, b"runtime data").unwrap();
        let bytes = fs::read(&path).unwrap();
        stored
            .require_resolution_bytes(&fixture.root, origin, &path, &bytes, &graph)
            .unwrap();
        // A graph at its hashed target still authenticates the control
        // bytes and records the axis at exactly target + the bytes hashed.
        let spent = node::GraphBudget::new();
        spent
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        stored
            .require_resolution_bytes(&fixture.root, origin, &path, &bytes, &spent)
            .unwrap();
        let axis = spent
            .exceedances()
            .axis(node::HASHED_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(axis.target, node::MAX_HASHED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.high_water,
            node::MAX_HASHED_BYTES_PER_GRAPH + bytes.len() as u64
        );
    }

    /// Give every regular file under `root` a new status-change time, so its
    /// physical identity, and therefore its memoized digest, changes.
    fn touch_files(root: &Path) {
        for child in fs::read_dir(root).unwrap() {
            let path = child.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                touch_files(&path);
            } else if metadata.is_file() {
                fs::set_permissions(&path, metadata.permissions()).unwrap();
            }
        }
    }

    #[test]
    fn replay_reuses_unchanged_digests_and_rereads_changed_files_without_reencoding() {
        let fixture = Fixture::new();
        let stored = fixture.capture();
        let file_work = [b"runtime data".as_slice(), b"exe!", b"source"]
            .into_iter()
            .map(|bytes| bytes.len() + 1)
            .sum::<usize>();
        let limit = usize::try_from(node::MAX_HASHED_BYTES_PER_GRAPH).unwrap();
        // The capture already computed every member digest; an
        // unchanged replay reuses them and reads nothing.
        let reused = node::GraphBudget::new();
        stored
            .replay(&fixture.root, Mode::Pending, &reused)
            .unwrap();
        assert_eq!(reused.census_usage().unwrap().2, 0);
        touch_files(&fixture.root);
        let exact = node::GraphBudget::new();
        exact.hash(limit - file_work).unwrap();
        stored.replay(&fixture.root, Mode::Pending, &exact).unwrap();

        // The exact floor fills the hashed target with no exceedance;
        // one byte less remaining still replays and records the axis at
        // exactly target + 1, proving the replay hashes exactly `file_work`.
        assert!(!exact.exceedances().is_exceeded());
        assert_eq!(exact.census_usage().unwrap().2, limit as u64);

        touch_files(&fixture.root);
        let floor = node::GraphBudget::new();
        floor.hash(limit - file_work + 1).unwrap();
        stored.replay(&fixture.root, Mode::Pending, &floor).unwrap();
        let axis = floor
            .exceedances()
            .axis("typed store hashed candidate bytes per graph")
            .copied()
            .unwrap();
        assert_eq!(axis.target, limit as u64);
        assert_eq!(axis.first_requested, limit as u64 + 1);
        assert_eq!(axis.high_water, limit as u64 + 1);
    }

    #[test]
    fn replay_matches_members_when_tree_order_differs_from_flat_path_order() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("build/opaque/out.before"), b"sibling").unwrap();
        fs::write(fixture.root.join("build/opaque/out/child"), b"nested").unwrap();
        let stored = fixture.capture();
        fixture.replay(&stored, Mode::Pending).unwrap();
        fs::remove_file(fixture.root.join("build/opaque/out.before")).unwrap();
        assert!(fixture.replay(&stored, Mode::Pending).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn fixed_paths_empty_directories_and_opaque_cargo_bytes_survive_freeze() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("build/.fingerprint")).unwrap();
        fs::write(fixture.root.join("build/.fingerprint/lock"), b"opaque").unwrap();
        let stored = fixture.capture();
        let decoded = Stored::decode(
            stored.bytes(),
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new(),
        )
        .unwrap();
        assert_eq!(decoded.digest(), stored.digest());
        assert_eq!(
            decoded.native_identity([3; 32]),
            stored.native_identity([3; 32])
        );
        assert!(stored
            .bytes()
            .windows(b"build/opaque/out/empty".len())
            .any(|bytes| bytes == b"build/opaque/out/empty"));
        fixture.freeze();
        fixture.replay(&decoded, Mode::Frozen).unwrap();
        assert_eq!(
            fs::canonicalize(fixture.root.join("build/opaque/out/runtime.asset")).unwrap(),
            fixture.root.join("build/opaque/out/runtime.asset")
        );
    }

    #[test]
    #[cfg(unix)]
    fn same_length_out_dir_corruption_restoring_mode_changes_native_identity() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = Fixture::new();
        let stored = fixture.capture();
        fixture.freeze();
        fixture.replay(&stored, Mode::Frozen).unwrap();
        let path = fixture.root.join("build/opaque/out/runtime.asset");
        let before = fs::metadata(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&path, b"changed data").unwrap();
        fs::set_permissions(&path, before.permissions()).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), before.len());
        assert!(fixture.replay(&stored, Mode::Frozen).is_err());
        let current = fixture.capture();
        assert_ne!(stored.digest(), current.digest());
        assert_ne!(
            stored.native_identity([3; 32]),
            current.native_identity([3; 32])
        );
        assert_eq!(
            fs::read(fixture.root.join("target/debug/runner")).unwrap(),
            b"exe!"
        );
    }

    #[test]
    fn full_membership_rejects_omission_addition_rename_and_type_replacement() {
        for change in 0..7 {
            let fixture = Fixture::new();
            let stored = fixture.capture();
            let asset = fixture.root.join("build/opaque/out/runtime.asset");
            match change {
                0 => fs::remove_file(asset).unwrap(),
                1 => fs::write(fixture.root.join("declared/new"), b"added").unwrap(),
                2 => fs::create_dir(fixture.root.join("declared/empty")).unwrap(),
                3 => fs::remove_dir(fixture.root.join("build/opaque/out/empty")).unwrap(),
                4 => {
                    fs::rename(asset, fixture.root.join("build/opaque/out/renamed.asset")).unwrap()
                }
                5 => {
                    fs::remove_file(asset).unwrap();
                    fs::write(
                        fixture.root.join("build/opaque/out/replace.asset"),
                        b"runtime data",
                    )
                    .unwrap();
                }
                6 => {
                    fs::remove_file(&asset).unwrap();
                    fs::create_dir(asset).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                fixture.replay(&stored, Mode::Pending).is_err(),
                "mutation {change}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn modes_symlinks_and_hardlinks_cannot_replay() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};
        for change in 0..5 {
            let fixture = Fixture::new();
            let stored = fixture.capture();
            fixture.freeze();
            let asset = fixture.root.join("build/opaque/out/runtime.asset");
            let parent = asset.parent().unwrap();
            fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
            match change {
                0 => fs::set_permissions(&asset, fs::Permissions::from_mode(0o555)).unwrap(),
                1 => fs::set_permissions(&asset, fs::Permissions::from_mode(0o400)).unwrap(),
                2 => {
                    fs::remove_file(&asset).unwrap();
                    symlink(fixture.root.join("target/debug/runner"), &asset).unwrap();
                }
                3 => {
                    fs::remove_file(&asset).unwrap();
                    fs::hard_link(fixture.root.join("target/debug/runner"), &asset).unwrap();
                }
                4 => fs::set_permissions(
                    fixture.root.join("build/opaque/out/empty"),
                    fs::Permissions::from_mode(0o500),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
            assert!(
                fixture.replay(&stored, Mode::Frozen).is_err(),
                "mutation {change}"
            );
        }
    }

    #[test]
    fn decoder_rejects_counts_paths_roots_and_truncation_before_allocation() {
        let fixture = Fixture::new();
        let stored = fixture.capture();
        for change in 0..6 {
            let mut bytes = stored.bytes().to_vec();
            match change {
                0 => bytes[84..88].copy_from_slice(&u32::MAX.to_le_bytes()),
                1 => bytes[88..92].copy_from_slice(&u32::MAX.to_le_bytes()),
                2 => {
                    bytes.pop();
                }
                3 => bytes[8..12].copy_from_slice(&0_u32.to_le_bytes()),
                4 => bytes[12] ^= 1,
                5 => bytes[92] ^= 1,
                _ => unreachable!(),
            }
            let budget = node::GraphBudget::new();
            budget
                .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                .unwrap();
            assert!(matches!(
                Stored::decode(&bytes, &fixture.root, fixture.reference, &budget),
                Err(node::Error::Codec(_))
            ));
        }
        let mut bytes = stored.bytes().to_vec();
        let index = bytes
            .windows(b"build/opaque/out".len())
            .position(|bytes| bytes == b"build/opaque/out")
            .unwrap();
        bytes[index + 6] = b'/';
        assert!(Stored::decode(
            &bytes,
            &fixture.root,
            fixture.reference,
            &node::GraphBudget::new()
        )
        .is_err());
        assert!(Stored::decode(
            stored.bytes(),
            &fixture.base,
            fixture.reference,
            &node::GraphBudget::new()
        )
        .is_err());
    }

    #[test]
    fn bounded_hash_precharges_open_and_eof_probe_and_keeps_artifact_cap_separate() {
        let fixture = Fixture::new();
        let executable = fixture.root.join("target/debug/runner");
        let mut metadata = fs::metadata(&executable).unwrap();
        // The four content bytes plus the EOF probe byte are charged
        // before the open. Five remaining fills the target exactly; four
        // remaining still hashes and records the axis at exactly target + 1.
        for remaining in [4, 5] {
            // A fresh status change gives each iteration a new identity, so
            // the memoized first digest cannot satisfy it.
            fs::set_permissions(&executable, metadata.permissions()).unwrap();
            metadata = fs::metadata(&executable).unwrap();
            let budget = node::GraphBudget::new();
            budget
                .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize - remaining)
                .unwrap();
            hash(&executable, &metadata, 4, &budget).unwrap();
            let exceeded = budget.exceedances();
            assert_eq!(exceeded.is_exceeded(), remaining == 4);
            if remaining == 4 {
                let axis = exceeded.axis(node::HASHED_RESOURCE).unwrap();
                assert_eq!(axis.first_requested, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
                assert_eq!(axis.high_water, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
            } else {
                assert_eq!(
                    budget.census_usage().unwrap().2,
                    node::MAX_HASHED_BYTES_PER_GRAPH
                );
            }
        }
        // A spent graph still precharges the five bytes before the open; the
        // missing file then fails for its own physical reason. A fresh
        // identity, since a memoized digest skips the precharge.
        fs::set_permissions(&executable, metadata.permissions()).unwrap();
        metadata = fs::metadata(&executable).unwrap();
        let budget = node::GraphBudget::new();
        budget
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        assert!(matches!(
            hash(&fixture.root.join("missing"), &metadata, 4, &budget),
            Err(node::Error::Io { .. })
        ));
        assert_eq!(
            budget
                .exceedances()
                .axis(node::HASHED_RESOURCE)
                .unwrap()
                .high_water,
            node::MAX_HASHED_BYTES_PER_GRAPH + 5
        );
        let budget = node::GraphBudget::new();
        budget.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        assert!(matches!(
            hash(&fixture.root.join("missing"), &metadata, 4, &budget),
            Err(node::Error::Io { .. })
        ));
        assert!(budget
            .exceedances()
            .axis(node::PATHS_RESOURCE)
            .is_some_and(|axis| axis.high_water > node::MAX_PATH_CHECKS_PER_GRAPH as u64));
        assert!(super::super::hash(executable.clone(), 4, Some(&node::GraphBudget::new())).is_ok());
        assert!(matches!(
            super::super::hash(executable, 3, Some(&node::GraphBudget::new())),
            Err(super::super::Error::Limit)
        ));
        // The twelve-byte runtime asset is independent of the four-byte executable allowance.
        fixture.capture();
    }

    #[test]
    fn an_unchanged_member_is_hashed_once_and_a_changed_one_again() {
        let fixture = Fixture::new();
        let executable = fixture.root.join("target/debug/runner");
        fs::set_permissions(
            &executable,
            fs::metadata(&executable).unwrap().permissions(),
        )
        .unwrap();
        let metadata = fs::metadata(&executable).unwrap();
        let first = node::GraphBudget::new();
        let digest = hash(&executable, &metadata, 4, &first).unwrap();
        assert_eq!(
            first.census_usage().unwrap().2,
            5,
            "the first check reads the file"
        );
        let second = node::GraphBudget::new();
        assert_eq!(hash(&executable, &metadata, 4, &second).unwrap(), digest);
        assert_eq!(
            second.census_usage().unwrap().2,
            0,
            "a later check does no hashing"
        );
        fs::set_permissions(&executable, metadata.permissions()).unwrap();
        let changed = fs::metadata(&executable).unwrap();
        let third = node::GraphBudget::new();
        assert_eq!(hash(&executable, &changed, 4, &third).unwrap(), digest);
        assert_eq!(
            third.census_usage().unwrap().2,
            5,
            "a new identity is hashed again"
        );
    }

    #[test]
    fn retained_asset_uses_its_own_bound_while_executable_sealing_records_its_target() {
        let fixture = Fixture::new();
        let manifest = crate::runner::manifest::GeneratedManifest::new(
            crate::bridge::BuildRequestKey::fixture([1; 32]),
            Box::default(),
            Box::default(),
            Box::default(),
            [0; 32],
        );
        let mut limits = crate::cargo::Limits::standard();
        limits.json_bytes = 4;
        let budget = node::GraphBudget::new();
        let executable = fixture.root.join("target/debug/runner");
        let artifact = super::super::BuiltArtifactProof::seal_budgeted(
            &manifest,
            &executable,
            &fixture.root.join("target"),
            &[],
            &limits,
            &budget,
        )
        .unwrap();
        artifact.revalidate_budgeted(&budget).unwrap();
        Stored::capture(&fixture.root, fixture.reference, Mode::Pending, &budget).unwrap();
        // The JSON target is accounting for artifact sealing; one
        // byte over it is recorded and the executable still seals, bound only
        // by the physical capture bound.
        fs::write(&executable, b"five!").unwrap();
        let sealed = super::super::BuiltArtifactProof::seal_budgeted(
            &manifest,
            &executable,
            &fixture.root.join("target"),
            &[],
            &limits,
            &budget,
        )
        .expect("the JSON target records and admits");
        assert_eq!(sealed.executable.bytes, 5);
        assert_eq!(sealed.byte_limit, crate::process::CAPTURE_BYTES);
        sealed.revalidate_budgeted(&budget).unwrap();
    }
}
