//! Content-owned authoritative Cargo build products.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node::{self, Parent};
use crate::{context::Context, metrics};

const MAGIC: &[u8; 8] = b"RAWRBLD2";
const SCHEMA: u32 = 2;
const PRODUCT_HEADER_BYTES: usize = 124;
const BUNDLE_MAGIC: &[u8; 8] = b"RAWRBTRE";
const BUNDLE_SCHEMA: u32 = 1;
const BUNDLE_HEADER_BYTES: usize = 16;
const ENTRY_HEADER_BYTES: usize = 49;
const TARGET: u8 = 1;
const PROTOCOL: u8 = 2;

/// One Cargo build-product capture, restoration or revalidation account.
/// The file-count and path-byte accounting targets a bundle crosses while it is
/// captured, validated or restored are recorded here and published under
/// `Account::Build`; the work continues and nothing refuses. Physical bounds
/// (the store's member and byte ceilings, the node content ceiling, `depth`,
/// checked arithmetic, `try_reserve`) keep refusing.
pub(crate) struct BuildAccount {
    id: u64,
    exceeded: crate::exceedance::Record,
}

impl BuildAccount {
    pub(crate) fn new() -> Self {
        Self {
            id: crate::exceedance::next_id(),
            exceeded: crate::exceedance::Record::new(),
        }
    }

    /// The axes of this account that crossed their targets so far.
    #[cfg(test)]
    pub(crate) const fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded
    }

    /// Record one measured amount against its accounting target. A crossing
    /// is published with a truthful high water and the caller continues.
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .exceeded
                .exceeded(resource, target as u64, actual as u64, None)
        {
            crate::exceedance::publish(crate::exceedance::Account::Build, self.id, &self.exceeded);
        }
    }
}

impl Default for BuildAccount {
    fn default() -> Self {
        Self::new()
    }
}

/// Small content node referring to one immutable authoritative Cargo bundle.
pub(crate) struct Product {
    canonical: Box<[u8]>,
    raw: Range<usize>,
    reference: crate::store::bundle::BundleRef,
    seal_bytes: usize,
    seal_digest: [u8; 32],
}

/// Resolve or reuse one authoritative Cargo bundle under the current unit-graph
/// parent, restoring its exact files before semantic parsing.
pub(crate) fn cached(
    context: &Context,
    parent: &node::Validated<super::UnitGraph>,
    authority: &[u8],
    target: &Path,
    protocol: &Path,
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<Product>, node::Error> {
    let limits = context.limits();
    let parent = Parent::validated(parent);
    let recipe = context.discover_with_parents::<Product>(
        authority,
        product_byte_limit(&limits)?,
        &[parent],
    )?;
    let store = context.open_store()?;
    let validation_store = store.clone();
    let product = node::Evaluator::new(&store, sink).discover_bundled_budgeted(
        &recipe,
        move |candidate, budget| candidate.revalidate(&validation_store, &limits, budget),
        |attempt| Product::capture(execute()?, target, protocol, &limits, attempt),
    )?;
    let _pin = crate::store::bundle::pin(store.root(), product.value().reference)?;
    product.value().restore_budgeted(
        store.root(),
        target,
        protocol,
        &limits,
        store.graph_budget(),
    )?;
    Ok(product)
}

impl Product {
    fn capture(
        raw: Box<[u8]>,
        target: &Path,
        protocol: &Path,
        limits: &super::super::Limits,
        attempt: crate::store::bundle::Attempt,
    ) -> Result<node::Bundled<Self>, node::Error> {
        // The node content bound is the physical store ceiling; the JSON
        // accounting target is recorded by the Cargo JSON epoch that parses
        // this document, never here.
        if raw.len() > node::MAX_CONTENT_BYTES {
            return Err(node::Error::Limit {
                resource: "cached authoritative build JSON bytes",
                limit: node::MAX_CONTENT_BYTES,
                actual: raw.len(),
            });
        }
        let reference = attempt.reference();
        // One build account per capture records every accounting target
        // the bundle crosses; nothing here refuses on one.
        let mut account = BuildAccount::new();
        let seal = capture_bundle(
            target,
            protocol,
            &attempt.target(),
            &attempt.build(),
            limits,
            &mut account,
        )?;
        let product = Self::from_bundle(raw, reference, &seal, limits, &mut account)?;
        let pending = attempt.seal(seal)?;
        node::Bundled::validate(product, pending, |product, pending| {
            product.revalidate_pending(pending, limits)
        })
    }

    fn from_bundle(
        raw: Box<[u8]>,
        reference: crate::store::bundle::BundleRef,
        seal: &[u8],
        limits: &super::super::Limits,
        account: &mut BuildAccount,
    ) -> Result<Self, node::Error> {
        validate_manifest(seal, limits, account)?;
        let mut writer = Writer::new(product_byte_limit(limits)?);
        writer.append(MAGIC)?;
        writer.append(&SCHEMA.to_le_bytes())?;
        writer.append(&reference.execution().as_bytes())?;
        writer.append(&reference.id().as_bytes())?;
        writer.append(&(raw.len() as u64).to_le_bytes())?;
        writer.append(&(seal.len() as u64).to_le_bytes())?;
        writer.append(&<[u8; 32]>::from(Sha256::digest(seal)))?;
        let raw_start = writer.len();
        writer.append(&raw)?;
        let raw_end = writer.len();
        let canonical = writer.finish();
        let product = Self::decode_canonical(canonical, limits)?;
        if product.raw != (raw_start..raw_end) || product.reference != reference {
            return Err(node::Error::State(
                "authoritative build bundle product changed during capture",
            ));
        }
        Ok(product)
    }

    fn decode_canonical(
        canonical: Box<[u8]>,
        limits: &super::super::Limits,
    ) -> Result<Self, node::Error> {
        let decoded = validate_product(&canonical, limits)?;
        Ok(Self {
            canonical,
            raw: decoded.raw,
            reference: decoded.reference,
            seal_bytes: decoded.seal_bytes,
            seal_digest: decoded.seal_digest,
        })
    }

    fn revalidate(
        &self,
        store: &crate::store::Store,
        limits: &super::super::Limits,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        let key = node::content_digest::<Self>(&self.canonical);
        let root = crate::store::bundle::validate_complete(
            store.root(),
            self.reference,
            node::Kind::NativeProduct,
            SCHEMA,
            key,
            self.seal_bytes,
            self.seal_digest,
            budget,
        )?;
        validate_bundle_root_with_budget(
            &root,
            limits,
            true,
            Some(budget),
            &mut BuildAccount::new(),
        )?;
        Ok(self.canonical.clone())
    }

    fn revalidate_pending(
        &self,
        pending: &crate::store::bundle::Pending,
        limits: &super::super::Limits,
    ) -> Result<Box<[u8]>, node::Error> {
        let root =
            pending.validate_unpublished(self.reference, self.seal_bytes, self.seal_digest)?;
        validate_bundle_root(&root, limits, false, &mut BuildAccount::new())?;
        Ok(self.canonical.clone())
    }

    #[cfg(test)]
    fn restore(
        &self,
        store: &Path,
        target: &Path,
        protocol: &Path,
        limits: &super::super::Limits,
    ) -> Result<(), node::Error> {
        self.restore_budgeted(store, target, protocol, limits, &node::GraphBudget::new())
    }

    fn restore_budgeted(
        &self,
        store: &Path,
        target: &Path,
        protocol: &Path,
        limits: &super::super::Limits,
        budget: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        let decoded = validate_product(&self.canonical, limits)?;
        if decoded.raw != self.raw
            || decoded.reference != self.reference
            || decoded.seal_bytes != self.seal_bytes
            || decoded.seal_digest != self.seal_digest
        {
            return Err(node::Error::Codec(
                "cached authoritative build product fields changed",
            ));
        }
        let root = store.join(self.reference.relative());
        budget.paths(1)?;
        budget.hash(self.seal_bytes)?;
        let seal = super::super::read_bounded(
            "cached authoritative build bundle seal",
            &root.join("seal"),
            node::MAX_CONTENT_BYTES,
        )?;
        if seal.len() != self.seal_bytes
            || <[u8; 32]>::from(Sha256::digest(&seal)) != self.seal_digest
        {
            return Err(node::Error::Corrupt {
                path: root.join("seal"),
                reason: "authoritative Cargo bundle seal changed before restoration",
            });
        }
        let mut reader = ManifestReader::new(&seal, node::MAX_CONTENT_BYTES)?;
        reader.header()?;
        let count = reader.count;
        // One build account per restoration records the restored tree's
        // accounting targets; the tree is still compared and restored in full.
        let mut account = BuildAccount::new();
        let target_current = current_files_with_budget(target, limits, Some(budget), &mut account)?;
        let protocol_current =
            current_files_with_budget(protocol, limits, Some(budget), &mut account)?;
        let target_empty = target_current.is_empty();
        let protocol_empty = protocol_current.is_empty();
        let mut target_expected = BTreeSet::new();
        let mut protocol_expected = BTreeSet::new();
        for _ in 0..count {
            let entry = reader.entry()?;
            let (source_root, destination_root, current, empty, expected) = match entry.namespace {
                TARGET => (
                    root.join("target"),
                    target,
                    &target_current,
                    target_empty,
                    &mut target_expected,
                ),
                PROTOCOL => (
                    root.join("build"),
                    protocol,
                    &protocol_current,
                    protocol_empty,
                    &mut protocol_expected,
                ),
                _ => {
                    return Err(node::Error::Codec(
                        "cached authoritative build namespace changed",
                    ));
                }
            };
            if !expected.insert(entry.path.clone()) {
                return Err(node::Error::Codec(
                    "cached authoritative build path is duplicated",
                ));
            }
            let path = destination_root.join(&entry.path);
            if current.contains(&entry.path) {
                validate_file_with_mode(
                    &path,
                    entry.mode,
                    entry.length,
                    entry.digest,
                    Some(budget),
                )?;
            } else if empty {
                materialize_file(
                    &source_root.join(&entry.path),
                    &path,
                    entry.mode,
                    entry.length,
                    entry.digest,
                    Some(budget),
                )?;
            } else {
                return Err(node::Error::Corrupt {
                    path,
                    reason: "cached authoritative build tree is incomplete",
                });
            }
        }
        reader.finish()?;
        if (!target_empty && target_expected != target_current)
            || (!protocol_empty && protocol_expected != protocol_current)
        {
            return Err(node::Error::Codec(
                "cached authoritative build tree has extra files",
            ));
        }
        Ok(())
    }

    pub(crate) fn raw(&self) -> &[u8] {
        &self.canonical[self.raw.clone()]
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::NativeProduct;
    const SCHEMA: u32 = SCHEMA;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.canonical.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build product bytes",
                limit,
                actual: self.canonical.len(),
            });
        }
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build product bytes",
                limit,
                actual: bytes.len(),
            });
        }
        Self::decode_canonical(
            bytes.to_vec().into_boxed_slice(),
            &super::super::Limits::standard(),
        )
    }
}

struct DecodedProduct {
    raw: Range<usize>,
    reference: crate::store::bundle::BundleRef,
    seal_bytes: usize,
    seal_digest: [u8; 32],
}

struct CaptureState<'capture> {
    entries: u32,
    members: usize,
    path_bytes: usize,
    limits: &'capture super::super::Limits,
    account: &'capture mut BuildAccount,
}

impl<'capture> CaptureState<'capture> {
    fn new(limits: &'capture super::super::Limits, account: &'capture mut BuildAccount) -> Self {
        Self {
            entries: 0,
            members: 0,
            path_bytes: 0,
            limits,
            account,
        }
    }

    /// The file count and path bytes are accounting targets recorded on
    /// the build account; the file is still captured. Checked-add overflow
    /// stays an arithmetic failure and the store's member ceiling stays
    /// physical (`retain_member`).
    fn retain_path(&mut self, path: &Path) -> Result<(), node::Error> {
        self.entries = self.entries.checked_add(1).ok_or(node::Error::Allocation)?;
        self.account.exceeded(
            "cached authoritative build files",
            self.limits.items,
            usize::try_from(self.entries).unwrap_or(usize::MAX),
        );
        self.path_bytes = self
            .path_bytes
            .checked_add(path.as_os_str().len())
            .ok_or(node::Error::Allocation)?;
        self.account.exceeded(
            "cached authoritative build path bytes",
            self.limits.input_proof_bytes,
            self.path_bytes,
        );
        Ok(())
    }

    fn retain_member(&mut self) -> Result<(), node::Error> {
        self.members = self.members.checked_add(1).ok_or(node::Error::Allocation)?;
        let limit = bundle_member_limit();
        if self.members > limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build filesystem members",
                limit,
                actual: self.members,
            });
        }
        Ok(())
    }
}

fn capture_bundle(
    target: &Path,
    protocol: &Path,
    bundle_target: &Path,
    bundle_protocol: &Path,
    limits: &super::super::Limits,
    account: &mut BuildAccount,
) -> Result<Box<[u8]>, node::Error> {
    if !current_files(bundle_target, limits, account)?.is_empty()
        || !current_files(bundle_protocol, limits, account)?.is_empty()
    {
        return Err(node::Error::State(
            "authoritative Cargo bundle partitions were not empty",
        ));
    }
    let mut writer = Writer::new(node::MAX_CONTENT_BYTES);
    writer.append(BUNDLE_MAGIC)?;
    writer.append(&BUNDLE_SCHEMA.to_le_bytes())?;
    let count_offset = writer.len();
    writer.append(&0_u32.to_le_bytes())?;
    let mut state = CaptureState::new(limits, account);
    capture_root(TARGET, target, bundle_target, &mut writer, &mut state)?;
    capture_root(PROTOCOL, protocol, bundle_protocol, &mut writer, &mut state)?;
    writer.replace_u32(count_offset, state.entries)?;
    let bytes = writer.finish();
    validate_manifest(&bytes, limits, account)?;
    Ok(bytes)
}

fn capture_root(
    namespace: u8,
    source_root: &Path,
    destination_root: &Path,
    writer: &mut Writer,
    state: &mut CaptureState<'_>,
) -> Result<(), node::Error> {
    let source_root = fs::canonicalize(source_root).map_err(|source| node::Error::Io {
        operation: "canonicalize authoritative build root",
        path: source_root.to_path_buf(),
        source,
    })?;
    let destination_root =
        fs::canonicalize(destination_root).map_err(|source| node::Error::Io {
            operation: "canonicalize authoritative bundle partition",
            path: destination_root.to_path_buf(),
            source,
        })?;
    let mut pending = vec![source_root.clone()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let depth = directory
            .strip_prefix(&source_root)
            .map_err(|_| node::Error::OutsideRoot(directory.clone()))?
            .components()
            .count();
        if depth > state.limits.depth {
            return Err(node::Error::Limit {
                resource: "cached authoritative build tree depth",
                limit: state.limits.depth,
                actual: depth,
            });
        }
        let entries = fs::read_dir(&directory).map_err(|source| node::Error::Io {
            operation: "read authoritative build directory",
            path: directory.clone(),
            source,
        })?;
        let mut children = Vec::new();
        for entry in entries {
            state.retain_member()?;
            children
                .try_reserve(1)
                .map_err(|_| node::Error::Allocation)?;
            children.push(
                entry
                    .map_err(|source| node::Error::Io {
                        operation: "read authoritative build directory entry",
                        path: directory.clone(),
                        source,
                    })?
                    .path(),
            );
        }
        children.sort_unstable();
        for path in children {
            let metadata = fs::symlink_metadata(&path).map_err(|source| node::Error::Io {
                operation: "inspect authoritative build entry",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(node::Error::Shape(path));
            }
            if metadata.is_dir() {
                pending
                    .try_reserve(1)
                    .map_err(|_| node::Error::Allocation)?;
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(node::Error::Shape(path));
            }
            files.try_reserve(1).map_err(|_| node::Error::Allocation)?;
            files.push(path);
        }
    }
    files.sort_unstable();
    for path in files {
        let metadata = fs::symlink_metadata(&path).map_err(|source| node::Error::Io {
            operation: "reinspect authoritative build file",
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(node::Error::Shape(path));
        }
        let relative = path
            .strip_prefix(&source_root)
            .map_err(|_| node::Error::OutsideRoot(path.clone()))?;
        validate_relative(relative)?;
        state.retain_path(relative)?;
        let evidence = Evidence::capture(&path, bundle_byte_limit())?;
        let destination = destination_root.join(relative);
        clone_file_exact(
            &path,
            &destination,
            evidence.mode,
            evidence.length,
            evidence.digest,
            None,
        )?;
        writer.entry(
            namespace,
            evidence.mode,
            relative,
            evidence.length,
            evidence.digest,
        )?;
    }
    Ok(())
}

fn validate_product(
    bytes: &[u8],
    limits: &super::super::Limits,
) -> Result<DecodedProduct, node::Error> {
    let limit = product_byte_limit(limits)?;
    if bytes.len() > limit || bytes.len() < PRODUCT_HEADER_BYTES {
        return Err(node::Error::Limit {
            resource: "cached authoritative build product bytes",
            limit,
            actual: bytes.len(),
        });
    }
    if bytes.get(..8) != Some(MAGIC) || u32_at(bytes, 8)? != SCHEMA {
        return Err(node::Error::Codec(
            "cached authoritative build product schema changed",
        ));
    }
    let reference = crate::store::bundle::BundleRef::new(
        node::ExecutionKey::from_bytes(array_at(bytes, 12)?),
        crate::store::bundle::BundleId::from_bytes(array_at(bytes, 44)?),
    );
    let raw_length = usize::try_from(u64_at(bytes, 76)?)
        .map_err(|_| node::Error::Codec("cached authoritative build JSON length is invalid"))?;
    if raw_length > node::MAX_CONTENT_BYTES {
        return Err(node::Error::Limit {
            resource: "cached authoritative build JSON bytes",
            limit: node::MAX_CONTENT_BYTES,
            actual: raw_length,
        });
    }
    let seal_bytes = usize::try_from(u64_at(bytes, 84)?)
        .map_err(|_| node::Error::Codec("authoritative build bundle seal length is invalid"))?;
    if !(BUNDLE_HEADER_BYTES..=node::MAX_CONTENT_BYTES).contains(&seal_bytes) {
        return Err(node::Error::Limit {
            resource: "authoritative build bundle seal bytes",
            limit: node::MAX_CONTENT_BYTES,
            actual: seal_bytes,
        });
    }
    let seal_digest = array_at(bytes, 92)?;
    let raw_end = PRODUCT_HEADER_BYTES
        .checked_add(raw_length)
        .ok_or(node::Error::Allocation)?;
    if raw_end != bytes.len() {
        return Err(node::Error::Codec(
            "cached authoritative build product has trailing or truncated bytes",
        ));
    }
    Ok(DecodedProduct {
        raw: PRODUCT_HEADER_BYTES..raw_end,
        reference,
        seal_bytes,
        seal_digest,
    })
}

fn validate_manifest(
    bytes: &[u8],
    limits: &super::super::Limits,
    account: &mut BuildAccount,
) -> Result<(), node::Error> {
    let mut reader = ManifestReader::new(bytes, node::MAX_CONTENT_BYTES)?;
    reader.header()?;
    let count = usize::try_from(reader.count).unwrap_or(usize::MAX);
    // The store's member ceiling is physical and refuses; the item target is
    // an accounting target recorded on the build account.
    let members = bundle_member_limit();
    if count > members {
        return Err(node::Error::Limit {
            resource: "cached authoritative build filesystem members",
            limit: members,
            actual: count,
        });
    }
    account.exceeded("cached authoritative build files", limits.items, count);
    let mut previous: Option<(u8, PathBuf)> = None;
    let mut path_bytes = 0_usize;
    for _ in 0..reader.count {
        let entry = reader.entry()?;
        validate_relative(&entry.path)?;
        let depth = entry.path.components().count();
        if depth > limits.depth {
            return Err(node::Error::Limit {
                resource: "cached authoritative build tree depth",
                limit: limits.depth,
                actual: depth,
            });
        }
        path_bytes = path_bytes
            .checked_add(entry.path.as_os_str().len())
            .ok_or(node::Error::Allocation)?;
        account.exceeded(
            "cached authoritative build path bytes",
            limits.input_proof_bytes,
            path_bytes,
        );
        let identity = (entry.namespace, entry.path.clone());
        if previous
            .as_ref()
            .is_some_and(|previous| previous >= &identity)
        {
            return Err(node::Error::Codec(
                "cached authoritative build files are not canonical",
            ));
        }
        previous = Some(identity);
    }
    reader.finish()?;
    Ok(())
}

fn validate_bundle_root(
    root: &Path,
    limits: &super::super::Limits,
    frozen: bool,
    account: &mut BuildAccount,
) -> Result<(), node::Error> {
    validate_bundle_root_with_budget(root, limits, frozen, None, account)
}

fn validate_bundle_root_with_budget(
    root: &Path,
    limits: &super::super::Limits,
    frozen: bool,
    budget: Option<&node::GraphBudget>,
    account: &mut BuildAccount,
) -> Result<(), node::Error> {
    if let Some(budget) = budget {
        budget.paths(2)?;
    }
    validate_empty_directory(
        &root.join("generated"),
        "authoritative Cargo bundle generated partition changed",
    )?;
    validate_empty_directory(
        &root.join("declared"),
        "authoritative Cargo bundle declared partition changed",
    )?;
    let seal = super::super::read_bounded(
        "cached authoritative build bundle seal",
        &root.join("seal"),
        node::MAX_CONTENT_BYTES,
    )?;
    validate_manifest(&seal, limits, account)?;
    let target = root.join("target");
    let protocol = root.join("build");
    let target_current = current_files_with_budget(&target, limits, budget, account)?;
    let protocol_current = current_files_with_budget(&protocol, limits, budget, account)?;
    let mut target_expected = BTreeSet::new();
    let mut protocol_expected = BTreeSet::new();
    let mut reader = ManifestReader::new(&seal, node::MAX_CONTENT_BYTES)?;
    reader.header()?;
    for _ in 0..reader.count {
        let entry = reader.entry()?;
        let (namespace_root, expected) = match entry.namespace {
            TARGET => (&target, &mut target_expected),
            PROTOCOL => (&protocol, &mut protocol_expected),
            _ => {
                return Err(node::Error::Codec(
                    "cached authoritative build namespace changed",
                ));
            }
        };
        if !expected.insert(entry.path.clone()) {
            return Err(node::Error::Codec(
                "cached authoritative build path is duplicated",
            ));
        }
        validate_bundle_file(
            &namespace_root.join(entry.path),
            entry.mode,
            entry.length,
            entry.digest,
            frozen,
            budget,
        )?;
    }
    reader.finish()?;
    if target_expected != target_current || protocol_expected != protocol_current {
        return Err(node::Error::Codec(
            "authoritative Cargo bundle tree has missing or extra files",
        ));
    }
    Ok(())
}

fn current_files(
    root: &Path,
    limits: &super::super::Limits,
    account: &mut BuildAccount,
) -> Result<BTreeSet<PathBuf>, node::Error> {
    current_files_with_budget(root, limits, None, account)
}

fn current_files_with_budget(
    root: &Path,
    limits: &super::super::Limits,
    budget: Option<&node::GraphBudget>,
    account: &mut BuildAccount,
) -> Result<BTreeSet<PathBuf>, node::Error> {
    let mut files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    let mut path_bytes = 0_usize;
    let mut members = 0usize;
    while let Some(directory) = pending.pop() {
        if let Some(budget) = budget {
            budget.paths(1)?;
        }
        let depth = directory
            .strip_prefix(root)
            .map_err(|_| node::Error::OutsideRoot(directory.clone()))?
            .components()
            .count();
        if depth > limits.depth {
            return Err(node::Error::Limit {
                resource: "restored authoritative build tree depth",
                limit: limits.depth,
                actual: depth,
            });
        }
        let entries = fs::read_dir(&directory).map_err(|source| node::Error::Io {
            operation: "read restored authoritative build directory",
            path: directory.clone(),
            source,
        })?;
        let mut children = Vec::new();
        for entry in entries {
            members = members.checked_add(1).ok_or(node::Error::Allocation)?;
            let limit = bundle_member_limit();
            if members > limit {
                return Err(node::Error::Limit {
                    resource: "restored authoritative build filesystem members",
                    limit,
                    actual: members,
                });
            }
            children
                .try_reserve(1)
                .map_err(|_| node::Error::Allocation)?;
            children.push(
                entry
                    .map_err(|source| node::Error::Io {
                        operation: "read restored authoritative build entry",
                        path: directory.clone(),
                        source,
                    })?
                    .path(),
            );
        }
        children.sort_unstable();
        for path in children {
            if let Some(budget) = budget {
                budget.paths(1)?;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|source| node::Error::Io {
                operation: "inspect restored authoritative build entry",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(node::Error::Shape(path));
            }
            if metadata.is_dir() {
                pending
                    .try_reserve(1)
                    .map_err(|_| node::Error::Allocation)?;
                pending.push(path);
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| node::Error::OutsideRoot(path.clone()))?
                    .to_path_buf();
                validate_relative(&relative)?;
                // Path bytes and file count are accounting targets
                // recorded on the build account; the walk continues. A
                // repeated relative path is a shape failure, not a target.
                path_bytes = path_bytes
                    .checked_add(relative.as_os_str().len())
                    .ok_or(node::Error::Allocation)?;
                account.exceeded(
                    "restored authoritative build path bytes",
                    limits.input_proof_bytes,
                    path_bytes,
                );
                if !files.insert(relative) {
                    return Err(node::Error::Codec(
                        "restored authoritative build files are not distinct",
                    ));
                }
                account.exceeded(
                    "restored authoritative build files",
                    limits.items,
                    files.len(),
                );
            } else {
                return Err(node::Error::Shape(path));
            }
        }
    }
    Ok(files)
}

fn materialize_file(
    source: &Path,
    destination: &Path,
    mode: u32,
    length: u64,
    digest: [u8; 32],
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    clone_file_exact(source, destination, mode, length, digest, budget)
}

fn clone_file_exact(
    source: &Path,
    destination: &Path,
    expected_mode: u32,
    expected_length: u64,
    expected_digest: [u8; 32],
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    if let Some(budget) = budget {
        budget.paths(2)?;
    }
    let parent = destination
        .parent()
        .ok_or_else(|| node::Error::Shape(destination.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| node::Error::Io {
        operation: "create restored authoritative build directory",
        path: parent.to_path_buf(),
        source,
    })?;
    clone_file(source, destination).map_err(|source| node::Error::Io {
        operation: "clone authoritative Cargo bundle file",
        path: destination.to_path_buf(),
        source,
    })?;
    let evidence = Evidence::capture_with_budget(destination, bundle_byte_limit(), budget)?;
    if evidence.length != expected_length || evidence.digest != expected_digest {
        return Err(node::Error::Corrupt {
            path: destination.to_path_buf(),
            reason: "cloned authoritative build file changed",
        });
    }
    let file = OpenOptions::new()
        .read(true)
        .open(destination)
        .map_err(|source| node::Error::Io {
            operation: "open cloned authoritative build file",
            path: destination.to_path_buf(),
            source,
        })?;
    set_mode(&file, expected_mode, destination)?;
    file.sync_all().map_err(|source| node::Error::Io {
        operation: "sync restored authoritative build file",
        path: destination.to_path_buf(),
        source,
    })?;
    let metadata = fs::symlink_metadata(destination).map_err(|source| node::Error::Io {
        operation: "inspect cloned authoritative build file mode",
        path: destination.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || mode(&metadata) != expected_mode
    {
        return Err(node::Error::Shape(destination.to_path_buf()));
    }
    Ok(())
}

fn validate_bundle_file(
    path: &Path,
    original_mode: u32,
    expected_length: u64,
    expected_digest: [u8; 32],
    frozen: bool,
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    let expected_mode = if frozen {
        frozen_mode(original_mode)
    } else {
        original_mode
    };
    validate_file_with_mode(
        path,
        expected_mode,
        expected_length,
        expected_digest,
        budget,
    )
}

fn validate_file_with_mode(
    path: &Path,
    expected_mode: u32,
    expected_length: u64,
    expected_digest: [u8; 32],
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
        operation: "inspect restored authoritative build file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || mode(&metadata) != expected_mode
        || metadata.len() != expected_length
    {
        return Err(node::Error::Shape(path.to_path_buf()));
    }
    let evidence = Evidence::capture_with_budget(path, bundle_byte_limit(), budget)?;
    if evidence.length != expected_length || evidence.digest != expected_digest {
        return Err(node::Error::Corrupt {
            path: path.to_path_buf(),
            reason: "restored authoritative build file changed",
        });
    }
    Ok(())
}

struct Evidence {
    mode: u32,
    length: u64,
    digest: [u8; 32],
}

fn bundle_byte_limit() -> usize {
    usize::try_from(crate::store::bundle::MAX_BYTES).unwrap_or(usize::MAX)
}

/// The product's node content bound: the physical store ceiling, never derived
/// from the configurable JSON accounting target.
fn product_byte_limit(limits: &super::super::Limits) -> Result<usize, node::Error> {
    let _ = limits;
    Ok(node::MAX_CONTENT_BYTES)
}

fn bundle_member_limit() -> usize {
    usize::try_from(crate::store::bundle::MAX_MEMBERS).unwrap_or(usize::MAX)
}

impl Evidence {
    fn capture(path: &Path, limit: usize) -> Result<Self, node::Error> {
        Self::capture_with_budget(path, limit, None)
    }

    fn capture_with_budget(
        path: &Path,
        limit: usize,
        budget: Option<&node::GraphBudget>,
    ) -> Result<Self, node::Error> {
        let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
            operation: "inspect authoritative build file",
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(node::Error::Shape(path.to_path_buf()));
        }
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length > limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build file bytes",
                limit,
                actual: length,
            });
        }
        if let Some(budget) = budget {
            budget.hash(length)?;
        }
        let mut file = File::open(path).map_err(|source| node::Error::Io {
            operation: "open authoritative build file",
            path: path.to_path_buf(),
            source,
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut actual = 0usize;
        loop {
            let read = file.read(&mut buffer).map_err(|source| node::Error::Io {
                operation: "hash authoritative build file",
                path: path.to_path_buf(),
                source,
            })?;
            if read == 0 {
                break;
            }
            actual = actual.checked_add(read).ok_or(node::Error::Allocation)?;
            if actual > limit {
                return Err(node::Error::Limit {
                    resource: "cached authoritative build file bytes",
                    limit,
                    actual,
                });
            }
            hasher.update(&buffer[..read]);
        }
        if actual != length {
            return Err(node::Error::Codec(
                "authoritative build file length changed while hashing",
            ));
        }
        Ok(Self {
            mode: mode(&metadata),
            length: metadata.len(),
            digest: hasher.finalize().into(),
        })
    }
}

fn validate_relative(path: &Path) -> Result<(), node::Error> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(node::Error::Shape(path.to_path_buf()));
    }
    if path.to_str().is_none() {
        return Err(node::Error::Codec(
            "cached authoritative build path is not UTF-8",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
fn mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn set_mode(file: &fs::File, mode: u32, path: &Path) -> Result<(), node::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|source| node::Error::Io {
            operation: "set restored authoritative build mode",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(unix)]
const fn frozen_mode(mode: u32) -> u32 {
    if mode & 0o111 != 0 {
        0o555
    } else {
        0o444
    }
}

#[cfg(not(unix))]
const fn frozen_mode(_mode: u32) -> u32 {
    1
}

fn clone_file(source: &Path, destination: &Path) -> std::io::Result<u64> {
    fs::copy(source, destination)
}

fn validate_empty_directory(path: &Path, reason: &'static str) -> Result<(), node::Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
        operation: "inspect authoritative Cargo bundle partition",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(node::Error::Shape(path.to_path_buf()));
    }
    let mut entries = fs::read_dir(path).map_err(|source| node::Error::Io {
        operation: "read authoritative Cargo bundle partition",
        path: path.to_path_buf(),
        source,
    })?;
    match entries.next() {
        None => Ok(()),
        Some(Ok(entry)) => Err(node::Error::Corrupt {
            path: entry.path(),
            reason,
        }),
        Some(Err(source)) => Err(node::Error::Io {
            operation: "read authoritative Cargo bundle partition entry",
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(not(unix))]
fn set_mode(file: &fs::File, mode: u32, path: &Path) -> Result<(), node::Error> {
    let mut permissions = file
        .metadata()
        .map_err(|source| node::Error::Io {
            operation: "inspect restored authoritative build mode",
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    permissions.set_readonly(mode != 0);
    file.set_permissions(permissions)
        .map_err(|source| node::Error::Io {
            operation: "set restored authoritative build mode",
            path: path.to_path_buf(),
            source,
        })
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), node::Error> {
        let next = self.bytes.len().saturating_add(bytes.len());
        if next > self.limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build product bytes",
                limit: self.limit,
                actual: next,
            });
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(|_| node::Error::Allocation)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn replace_u32(&mut self, offset: usize, value: u32) -> Result<(), node::Error> {
        let target =
            self.bytes
                .get_mut(offset..offset.saturating_add(4))
                .ok_or(node::Error::Codec(
                    "cached authoritative build count offset changed",
                ))?;
        target.copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn entry(
        &mut self,
        namespace: u8,
        mode: u32,
        path: &Path,
        length: u64,
        digest: [u8; 32],
    ) -> Result<(), node::Error> {
        let path = path
            .to_str()
            .ok_or(node::Error::Codec(
                "cached authoritative build path is not UTF-8",
            ))?
            .as_bytes();
        let path_length = u32::try_from(path.len())
            .map_err(|_| node::Error::Codec("cached authoritative build path length is invalid"))?;
        self.append(&[namespace])?;
        self.append(&mode.to_le_bytes())?;
        self.append(&path_length.to_le_bytes())?;
        self.append(&length.to_le_bytes())?;
        self.append(&digest)?;
        self.append(path)
    }

    fn finish(self) -> Box<[u8]> {
        self.bytes.into_boxed_slice()
    }
}

struct ManifestReader<'bytes> {
    bytes: &'bytes [u8],
    offset: usize,
    count: u32,
}

struct Entry {
    namespace: u8,
    mode: u32,
    path: PathBuf,
    length: u64,
    digest: [u8; 32],
}

impl<'bytes> ManifestReader<'bytes> {
    fn new(bytes: &'bytes [u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached authoritative build product bytes",
                limit,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            bytes,
            offset: 0,
            count: 0,
        })
    }

    fn take(&mut self, length: usize) -> Result<&'bytes [u8], node::Error> {
        let end = self.offset.saturating_add(length);
        let bytes = self.bytes.get(self.offset..end).ok_or(node::Error::Codec(
            "cached authoritative build product is truncated",
        ))?;
        self.offset = end;
        Ok(bytes)
    }

    fn header(&mut self) -> Result<(), node::Error> {
        if self.take(8)? != BUNDLE_MAGIC || self.u32()? != BUNDLE_SCHEMA {
            return Err(node::Error::Codec(
                "cached authoritative build bundle schema changed",
            ));
        }
        self.count = self.u32()?;
        if self.offset != BUNDLE_HEADER_BYTES {
            return Err(node::Error::Codec(
                "cached authoritative build bundle header changed",
            ));
        }
        Ok(())
    }

    fn entry(&mut self) -> Result<Entry, node::Error> {
        if self.bytes.len().saturating_sub(self.offset) < ENTRY_HEADER_BYTES {
            return Err(node::Error::Codec(
                "cached authoritative build entry is truncated",
            ));
        }
        let namespace = self.take(1)?[0];
        if ![TARGET, PROTOCOL].contains(&namespace) {
            return Err(node::Error::Codec(
                "cached authoritative build namespace changed",
            ));
        }
        let mode = self.u32()?;
        #[cfg(unix)]
        if mode & !0o777 != 0 {
            return Err(node::Error::Codec(
                "cached authoritative build mode is invalid",
            ));
        }
        #[cfg(not(unix))]
        if mode > 1 {
            return Err(node::Error::Codec(
                "cached authoritative build mode is invalid",
            ));
        }
        let path_length = usize::try_from(self.u32()?)
            .map_err(|_| node::Error::Codec("cached authoritative build path length is invalid"))?;
        let length = self.u64()?;
        let digest = self
            .take(32)?
            .try_into()
            .map_err(|_| node::Error::Codec("cached authoritative build digest is invalid"))?;
        let path = std::str::from_utf8(self.take(path_length)?)
            .map_err(|_| node::Error::Codec("cached authoritative build path is not UTF-8"))?;
        Ok(Entry {
            namespace,
            mode,
            path: PathBuf::from(path),
            length,
            digest,
        })
    }

    fn u32(&mut self) -> Result<u32, node::Error> {
        self.take(4)?
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(|_| node::Error::Codec("cached authoritative build u32 is invalid"))
    }

    fn u64(&mut self) -> Result<u64, node::Error> {
        self.take(8)?
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(|_| node::Error::Codec("cached authoritative build u64 is invalid"))
    }

    fn finish(self) -> Result<(), node::Error> {
        if self.offset != self.bytes.len() {
            return Err(node::Error::Codec(
                "cached authoritative build product has trailing bytes",
            ));
        }
        Ok(())
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, node::Error> {
    bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(node::Error::Codec(
            "cached authoritative build u32 is truncated",
        ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, node::Error> {
    bytes
        .get(offset..offset.saturating_add(8))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(node::Error::Codec(
            "cached authoritative build u64 is truncated",
        ))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], node::Error> {
    bytes
        .get(offset..offset.saturating_add(N))
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(node::Error::Codec(
            "cached authoritative build fixed field is truncated",
        ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::node::Value as _;

    use super::{Product, PRODUCT_HEADER_BYTES, SCHEMA};

    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn authoritative_build_product_round_trips_and_restores_exact_trees() {
        let root = fixture("round-trip");
        let target = root.join("target");
        let protocol = root.join("protocol");
        fs::create_dir_all(target.join("debug/deps")).expect("target tree creates");
        fs::create_dir_all(&protocol).expect("protocol tree creates");
        fs::write(target.join("debug/deps/libfixture.rlib"), b"artifact").expect("artifact writes");
        fs::write(protocol.join("sidecar.json"), b"{\"ready\":true}\n").expect("sidecar writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(
                target.join("debug/deps/libfixture.rlib"),
                fs::Permissions::from_mode(0o640),
            )
            .expect("artifact mode sets");
        }
        let limits = super::super::super::Limits::standard();
        let (store, product) = fixture_product(
            &root,
            &target,
            &protocol,
            b"{\"compiler-artifact\":true}\n",
            &limits,
        );
        let bundle_root = store.join(product.reference.relative());
        let mut account = super::BuildAccount::new();
        super::validate_bundle_root(&bundle_root, &limits, false, &mut account)
            .expect("captured bundle validates");
        fs::write(bundle_root.join("target/extra"), b"extra").expect("extra bundle file writes");
        assert!(super::validate_bundle_root(&bundle_root, &limits, false, &mut account).is_err());
        assert!(!account.exceedances().is_exceeded());
        fs::remove_file(bundle_root.join("target/extra")).expect("extra bundle file retires");
        let encoded = product
            .encode(crate::node::MAX_CONTENT_BYTES)
            .expect("product encodes");
        assert_eq!(
            encoded.len(),
            PRODUCT_HEADER_BYTES + b"{\"compiler-artifact\":true}\n".len()
        );
        let replayed =
            Product::decode(&encoded, crate::node::MAX_CONTENT_BYTES).expect("product decodes");
        assert_eq!(replayed.raw(), b"{\"compiler-artifact\":true}\n");

        fs::remove_dir_all(&target).expect("target retires");
        fs::remove_dir_all(&protocol).expect("protocol retires");
        fs::create_dir(&target).expect("empty target recreates");
        fs::create_dir(&protocol).expect("empty protocol recreates");
        replayed
            .restore(&store, &target, &protocol, &limits)
            .expect("product restores");
        assert_eq!(
            fs::read(target.join("debug/deps/libfixture.rlib")).expect("artifact reads"),
            b"artifact"
        );
        assert_eq!(
            fs::read(protocol.join("sidecar.json")).expect("sidecar reads"),
            b"{\"ready\":true}\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            assert_eq!(
                fs::metadata(target.join("debug/deps/libfixture.rlib"))
                    .expect("artifact inspects")
                    .mode()
                    & 0o777,
                0o640
            );
        }
        fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn authoritative_build_product_rejects_tampering_and_noncanonical_bytes() {
        let root = fixture("tamper");
        let target = root.join("target");
        let protocol = root.join("protocol");
        fs::create_dir(&target).expect("target creates");
        fs::create_dir(&protocol).expect("protocol creates");
        fs::write(target.join("artifact"), b"admitted").expect("artifact writes");
        let limits = super::super::super::Limits::standard();
        let (store, product) = fixture_product(&root, &target, &protocol, b"{}\n", &limits);
        fs::write(target.join("artifact"), b"changed").expect("artifact changes");
        assert!(product
            .restore(&store, &target, &protocol, &limits)
            .is_err());
        fs::remove_dir_all(&target).expect("tampered target retires");
        fs::remove_dir_all(&protocol).expect("protocol retires");
        fs::create_dir(&target).expect("empty target recreates");
        fs::create_dir(&protocol).expect("empty protocol recreates");
        fs::write(
            store
                .join(product.reference.relative())
                .join("target/artifact"),
            b"changed",
        )
        .expect("bundle artifact changes");
        assert!(product
            .restore(&store, &target, &protocol, &limits)
            .is_err());

        let mut trailing = product
            .encode(crate::node::MAX_CONTENT_BYTES)
            .expect("product encodes")
            .into_vec();
        trailing.push(0);
        assert!(Product::decode(&trailing, crate::node::MAX_CONTENT_BYTES).is_err());
        let mut schema = product
            .encode(crate::node::MAX_CONTENT_BYTES)
            .expect("product encodes")
            .into_vec();
        schema[8..12].copy_from_slice(&SCHEMA.saturating_add(1).to_le_bytes());
        assert!(Product::decode(&schema, crate::node::MAX_CONTENT_BYTES).is_err());
        fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn cached_authoritative_build_restores_a_warm_tree_without_execution() {
        let root = fixture("cached");
        let cargo_home = root.join("cargo-home");
        let project = root.join("project");
        let dependency = root.join("dependency");
        fs::create_dir(&cargo_home).expect("Cargo home creates");
        fs::create_dir_all(project.join("src")).expect("project creates");
        fs::create_dir_all(dependency.join("src")).expect("dependency creates");
        let manifest = project.join("Cargo.toml");
        let dependency_manifest = dependency.join("Cargo.toml");
        let lock = project.join("Cargo.lock");
        fs::write(&manifest, b"[package]\nname='root'\nversion='0.0.0'\n")
            .expect("manifest writes");
        fs::write(
            &dependency_manifest,
            b"[package]\nname='dependency'\nversion='0.0.0'\n",
        )
        .expect("dependency manifest writes");
        fs::write(&lock, b"version = 4\n").expect("lock writes");
        fs::write(project.join("src/lib.rs"), b"pub fn root() {}\n").expect("source writes");
        fs::write(dependency.join("src/lib.rs"), b"pub fn dependency() {}\n")
            .expect("dependency source writes");
        let environment = super::super::super::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home.to_str().expect("Cargo home is Unicode"),
        )])
        .expect("environment seals");
        let limits = super::super::super::Limits::standard();
        let context = crate::context::Context::capture(
            &project,
            environment,
            super::super::super::toolchain::HOST,
            &limits,
        )
        .expect("context seals");
        // `project/target` holds the project's store.
        let target = project.join("cargo-target");
        let protocol = project.join("protocol");
        let metadata_raw = serde_json::to_vec(&serde_json::json!({
            "packages": [
                {"manifest_path": manifest.to_str().expect("manifest is Unicode")},
                {"manifest_path": dependency_manifest.to_str().expect("dependency is Unicode")}
            ]
        }))
        .expect("metadata serializes")
        .into_boxed_slice();
        let metadata = super::super::cached_locked(
            &context,
            b"build-test-metadata/v1",
            &manifest,
            &lock,
            &target,
            &crate::metrics::Noop,
            || Ok(metadata_raw.clone()),
        )
        .expect("metadata parent validates");
        let unit_raw = br#"{"version":1,"units":[],"roots":[0]}"#
            .to_vec()
            .into_boxed_slice();
        let unit = super::super::cached_unit_graph(
            &context,
            &metadata,
            b"build-test-unit/v1",
            &crate::metrics::Noop,
            || Ok(unit_raw.clone()),
        )
        .expect("unit parent validates");
        fs::create_dir(&target).expect("target creates");
        fs::create_dir(&protocol).expect("protocol creates");
        let executions = AtomicUsize::new(0);
        let cold = super::cached(
            &context,
            &unit,
            b"build-test-product/v1",
            &target,
            &protocol,
            &crate::metrics::Noop,
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                fs::create_dir_all(target.join("debug/deps")).expect("artifact parent creates");
                fs::write(target.join("debug/deps/libroot.rlib"), b"artifact")
                    .expect("artifact writes");
                fs::write(protocol.join("sidecar.json"), b"sidecar").expect("sidecar writes");
                Ok(b"{\"build\":true}\n".as_slice().into())
            },
        )
        .expect("cold build executes and seals");
        assert_eq!(cold.value().raw(), b"{\"build\":true}\n");
        drop(cold);
        fs::remove_dir_all(&target).expect("cold target retires");
        fs::remove_dir_all(&protocol).expect("cold protocol retires");
        fs::create_dir(&target).expect("empty target recreates");
        fs::create_dir(&protocol).expect("empty protocol recreates");
        let warm = super::cached(
            &context,
            &unit,
            b"build-test-product/v1",
            &target,
            &protocol,
            &crate::metrics::Noop,
            || -> crate::Result<Box<[u8]>> {
                executions.fetch_add(1, Ordering::Relaxed);
                panic!("a validated warm build product must not execute")
            },
        )
        .expect("warm build validates and restores");
        assert_eq!(warm.value().raw(), b"{\"build\":true}\n");
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(
            fs::read(target.join("debug/deps/libroot.rlib")).expect("restored artifact reads"),
            b"artifact"
        );
        assert_eq!(
            fs::read(protocol.join("sidecar.json")).expect("restored sidecar reads"),
            b"sidecar"
        );
        drop(warm);
        context
            .open_store()
            .expect("fixture store opens")
            .retire_fixture()
            .expect("fixture store retires");
        fs::remove_dir_all(root).expect("fixture retires");
    }

    fn fixture(label: &str) -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time follows the epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rawr-build-product-{label}-{now}-{id}"));
        fs::create_dir(&root).expect("fixture root creates");
        canonical(&root)
    }

    fn fixture_product(
        root: &Path,
        target: &Path,
        protocol: &Path,
        raw: &[u8],
        limits: &super::super::super::Limits,
    ) -> (PathBuf, Product) {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed).to_le_bytes();
        let mut execution = [0_u8; 32];
        execution[..8].copy_from_slice(&id);
        execution[8..16].copy_from_slice(&id);
        let mut bundle = [0_u8; 32];
        bundle[16..24].copy_from_slice(&id);
        bundle[24..].copy_from_slice(&id);
        let reference = crate::store::bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes(execution),
            crate::store::bundle::BundleId::from_bytes(bundle),
        );
        let store = root.join("store");
        let bundle_root = store.join(reference.relative());
        for directory in ["generated", "build", "target", "declared"] {
            fs::create_dir_all(bundle_root.join(directory)).expect("bundle partition creates");
        }
        let mut account = super::BuildAccount::new();
        let seal = super::capture_bundle(
            target,
            protocol,
            &bundle_root.join("target"),
            &bundle_root.join("build"),
            limits,
            &mut account,
        )
        .expect("bundle captures");
        fs::write(bundle_root.join("seal"), &seal).expect("bundle seal writes");
        let product = Product::from_bundle(raw.into(), reference, &seal, limits, &mut account)
            .expect("bundle product captures");
        assert!(
            !account.exceedances().is_exceeded(),
            "the fixture bundle sits under every standard accounting target"
        );
        (store, product)
    }

    /// The bundle's file count and path bytes are accounting targets on
    /// the build account at capture, seal validation and tree restoration:
    /// exact figures record nothing, one under captures the identical seal and
    /// records only that axis with its truthful high water, and nothing
    /// refuses. The store's member ceiling and `depth` stay physical.
    #[test]
    fn bundle_file_and_path_targets_record_without_refusal() {
        let root = fixture("targets");
        let target = root.join("target");
        let protocol = root.join("protocol");
        fs::create_dir_all(target.join("debug/deps")).expect("target tree creates");
        fs::create_dir_all(&protocol).expect("protocol tree creates");
        fs::write(target.join("debug/deps/libfixture.rlib"), b"artifact").expect("artifact writes");
        fs::write(protocol.join("sidecar.json"), b"{\"ready\":true}\n").expect("sidecar writes");
        const TARGET_PATH: &str = "debug/deps/libfixture.rlib";
        const PROTOCOL_PATH: &str = "sidecar.json";
        let path_bytes = TARGET_PATH.len() + PROTOCOL_PATH.len();
        let bundles = AtomicUsize::new(0);
        let exercise = |adjust: &dyn Fn(&mut super::super::super::Limits)| {
            let mut limits = super::super::super::Limits::standard();
            adjust(&mut limits);
            let bundle = root.join(format!(
                "bundle-{}",
                bundles.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(bundle.join("target")).expect("bundle target partition creates");
            fs::create_dir_all(bundle.join("build")).expect("bundle build partition creates");
            let mut account = super::BuildAccount::new();
            let seal = super::capture_bundle(
                &target,
                &protocol,
                &bundle.join("target"),
                &bundle.join("build"),
                &limits,
                &mut account,
            )
            .expect("the bundle captures under every accounting target");
            super::validate_manifest(&seal, &limits, &mut account)
                .expect("the seal validates under every accounting target");
            let restored = super::current_files_with_budget(&target, &limits, None, &mut account)
                .expect("the restored tree walks under every accounting target");
            assert_eq!(restored.len(), 1);
            (seal, account.exceedances())
        };
        let (standard, record) = exercise(&|_| {});
        assert!(!record.is_exceeded());
        let exact = |limits: &mut super::super::super::Limits| {
            limits.items = 2;
            limits.input_proof_bytes = path_bytes;
        };
        let (seal, record) = exercise(&exact);
        assert_eq!(seal, standard);
        assert!(!record.is_exceeded());
        type Adjust<'a> = &'a dyn Fn(&mut super::super::super::Limits);
        type Exceeded<'a> = &'a [(&'a str, usize, usize)];
        let cases: [(Adjust, Exceeded); 4] = [
            (
                &|limits| {
                    exact(limits);
                    limits.items = 1;
                },
                &[("cached authoritative build files", 1, 2)][..],
            ),
            (
                &|limits| {
                    exact(limits);
                    limits.items = 0;
                },
                &[
                    ("cached authoritative build files", 0, 2),
                    ("restored authoritative build files", 0, 1),
                ][..],
            ),
            (
                &|limits| {
                    exact(limits);
                    limits.input_proof_bytes = path_bytes - 1;
                },
                &[(
                    "cached authoritative build path bytes",
                    path_bytes - 1,
                    path_bytes,
                )][..],
            ),
            (
                &|limits| {
                    exact(limits);
                    limits.input_proof_bytes = TARGET_PATH.len() - 1;
                },
                &[
                    (
                        "cached authoritative build path bytes",
                        TARGET_PATH.len() - 1,
                        path_bytes,
                    ),
                    (
                        "restored authoritative build path bytes",
                        TARGET_PATH.len() - 1,
                        TARGET_PATH.len(),
                    ),
                ][..],
            ),
        ];
        for (adjust, expected) in cases {
            let (seal, record) = exercise(adjust);
            assert_eq!(seal, standard, "the seal is byte-identical across targets");
            let mut names = record.axes().map(|axis| axis.name).collect::<Vec<_>>();
            names.sort_unstable();
            let mut expected_names = expected
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<Vec<_>>();
            expected_names.sort_unstable();
            assert_eq!(names, expected_names);
            for (name, target, high_water) in expected {
                let axis = record.axis(name).expect("the lowered axis is recorded");
                assert_eq!(
                    (axis.target, axis.high_water),
                    (*target as u64, *high_water as u64)
                );
                assert!(axis.first_requested > axis.target);
                assert!(axis.first_requested <= axis.high_water);
            }
        }
        fs::remove_dir_all(root).expect("fixture retires");
    }

    fn canonical(path: &Path) -> PathBuf {
        fs::canonicalize(path).expect("fixture path canonicalizes")
    }
}
