//! Immutable Cargo execution bundles owned by discoverable node attempts.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use sha2::Digest as _;

use super::accounting::{self, Allocation, Charge, Class, Identity, LeaseKey, Record};
use crate::node::{Digest, Error, ExecutionKey, Kind};

pub(crate) mod buffer;
mod corruption;
mod directory;
pub(crate) mod geometry;
#[cfg(test)]
pub(crate) mod release_tests;
pub use corruption::Corruption;
use geometry::View;
pub(crate) use geometry::{check, Drift, Measured};

pub(crate) const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub(crate) const MAX_MEMBERS: u64 = 262_144;
pub(super) const MAX_BUNDLES: usize = 256;
const MAX_ORPHANS: usize = 8;
const BLOCK_BYTES: u64 = 4 * 1024;
const IDENTIFIER_BYTES: usize = 32;
const LEASE_BYTES: &[u8] = b"RAWRBNDL-LEASE-v1\n";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BundleId([u8; IDENTIFIER_BYTES]);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BundleRef {
    execution: ExecutionKey,
    id: BundleId,
}

/// Historical bundle reference and seal bytes; no current or native authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BundleCommitment {
    reference: BundleRef,
    seal_bytes: usize,
    seal_digest: [u8; 32],
}

impl BundleCommitment {
    pub(crate) const fn new(
        reference: BundleRef,
        seal_bytes: usize,
        seal_digest: [u8; 32],
    ) -> Self {
        Self {
            reference,
            seal_bytes,
            seal_digest,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Budget {
    charge: Charge,
}

pub(crate) struct Attempt {
    store: PathBuf,
    reference: BundleRef,
    budget: Budget,
    armed: bool,
}

pub(crate) struct Pending {
    store: PathBuf,
    reference: BundleRef,
    seal: Box<[u8]>,
    charge: Charge,
    /// Logical geometry fixed at seal, committed by the schema-5 record.
    geometry: [u8; 32],
    armed: bool,
    graph_budget: Option<crate::node::GraphBudget>,
}

/// Process-local shared lease preventing whole-bundle eviction while a runner
/// manifest or process can still dereference its fixed executable path.
pub(crate) struct Guard {
    _lease: File,
}

pub(crate) struct Eviction {
    root: PathBuf,
    _lease: File,
}

/// Exclusive, measured authority to retire one exact registered bundle.
/// This never grants permission to consume its damaged members.
pub(super) struct Repair {
    eviction: Eviction,
    allocation: Allocation,
    actual: Charge,
}

impl Repair {
    pub(super) fn allocation(&self) -> Allocation {
        self.allocation
    }

    pub(super) fn root(&self) -> &Path {
        &self.eviction.root
    }

    pub(super) fn validate(&self, root: &Path, allocation: Allocation) -> Result<Charge, Error> {
        if root != self.root() || allocation != self.allocation {
            return Err(Error::State(
                "bundle retirement changed its exclusive allocation",
            ));
        }
        validate_top_level(root, State::Published)?;
        validate_lease(root)?;
        let opened = self
            .eviction
            ._lease
            .metadata()
            .map_err(|source| Error::Io {
                operation: "inspect held bundle retirement lease",
                path: root.join("lease"),
                source,
            })?;
        let current = fs::symlink_metadata(root.join("lease")).map_err(|source| Error::Io {
            operation: "inspect current bundle retirement lease",
            path: root.join("lease"),
            source,
        })?;
        if !opened.is_file()
            || !current.is_file()
            || hard_linked(&opened)
            || hard_linked(&current)
            || !same_file(&opened, &current)
        {
            return Err(Error::CleanupRequired(
                "bundle retirement lease identity changed",
            ));
        }
        // The bundle is being deleted, so a change since the retirement
        // measurement only moves the excess: it is charged from the larger.
        let actual = super::recovery::tree_charge(root, u64::MAX)?;
        Ok(Charge {
            bytes: self
                .actual
                .bytes
                .max(actual.bytes)
                .saturating_sub(allocation.charge.bytes),
            allocations: 0,
            members: self
                .actual
                .members
                .max(actual.members)
                .saturating_sub(allocation.charge.members),
        })
    }

    pub(super) fn retire(
        self,
        publication: &super::lease::Publication<'_>,
        store: &Path,
        budget: &mut super::recovery::Budget,
    ) -> Result<(), Error> {
        let admitted = accounting::admit_repair_control(
            publication,
            store,
            super::control::Plan::retirement(self.root()),
            &self,
            budget,
        )?;
        thaw_root(self.root())?;
        let temporary =
            super::entry::stage_admitted_retirement(admitted, publication, store, self.root())?;
        remove_bundle(&temporary)
    }
}

#[derive(Clone, Copy)]
enum State {
    Unsealed,
    Sealed,
    Published,
}

impl BundleId {
    pub(crate) const fn from_bytes(bytes: [u8; IDENTIFIER_BYTES]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(self) -> [u8; IDENTIFIER_BYTES] {
        self.0
    }

    fn directory(self) -> String {
        hex(&self.0)
    }
}

impl BundleRef {
    pub(crate) const fn new(execution: ExecutionKey, id: BundleId) -> Self {
        Self { execution, id }
    }

    pub(crate) const fn execution(self) -> ExecutionKey {
        self.execution
    }

    pub(crate) const fn id(self) -> BundleId {
        self.id
    }

    pub(crate) fn relative(self) -> PathBuf {
        PathBuf::from("executions")
            .join(self.execution.directory())
            .join(self.id.directory())
    }

    pub(crate) fn relative_budgeted(
        self,
        budget: &crate::node::GraphBudget,
    ) -> Result<PathBuf, Error> {
        budget.decode(std::mem::size_of::<PathBuf>() + 140 + 128)?;
        let mut relative = PathBuf::new();
        relative
            .try_reserve_exact(140)
            .map_err(|_| Error::Allocation)?;
        relative.push("executions");
        relative.push(self.execution.directory());
        relative.push(self.id.directory());
        Ok(relative)
    }
}

impl Budget {
    pub(crate) fn bounded(node: Charge) -> Result<(Charge, Self), Error> {
        let bundle = Charge {
            bytes: MAX_BYTES,
            allocations: 1,
            members: MAX_MEMBERS,
        };
        Ok((node.checked_add(bundle)?, Self { charge: bundle }))
    }

    fn admits(self, actual: Charge) -> bool {
        actual.bytes <= self.charge.bytes
            && actual.allocations <= self.charge.allocations
            && actual.members <= self.charge.members
    }
}

impl Attempt {
    pub(crate) fn allocate(
        store: &Path,
        execution: ExecutionKey,
        budget: Budget,
    ) -> Result<Self, Error> {
        if accounting::bundle_count(store)? >= MAX_BUNDLES || count_bundles(store)? >= MAX_BUNDLES {
            return Err(Error::Limit {
                resource: "typed store immutable bundles",
                limit: MAX_BUNDLES,
                actual: MAX_BUNDLES.saturating_add(1),
            });
        }
        let relative = PathBuf::from("executions").join(execution.directory());
        let parent = super::entry::existing_directory(store, &relative)?.ok_or(Error::State(
            "runner bundle execution directory was not admitted before allocation",
        ))?;
        let mut collision = 0usize;
        let (id, root) = loop {
            if collision == MAX_ORPHANS {
                return Err(Error::CleanupRequired(
                    "runner bundle identifier collisions exceeded the bounded retry",
                ));
            }
            let id = random_id()?;
            let root = parent.join(id.directory());
            match fs::create_dir(&root) {
                Ok(()) => break (id, root),
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    collision = collision.saturating_add(1);
                }
                Err(source) => {
                    return Err(Error::Io {
                        operation: "allocate runner Cargo bundle",
                        path: root,
                        source,
                    });
                }
            }
        };
        let reference = BundleRef::new(execution, id);
        if let Err(error) = initialize(&root, &parent) {
            let _ = fs::remove_dir_all(&root);
            return Err(error);
        }
        Ok(Self {
            store: store.to_path_buf(),
            reference,
            budget,
            armed: true,
        })
    }

    pub(crate) const fn reference(&self) -> BundleRef {
        self.reference
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.store.join(self.reference.relative())
    }

    pub(crate) fn root_budgeted(
        &self,
        budget: &crate::node::GraphBudget,
    ) -> Result<PathBuf, Error> {
        path(
            &self.store,
            &self.reference.relative_budgeted(budget)?,
            budget,
        )
    }

    pub(crate) fn generated(&self) -> PathBuf {
        self.root().join("generated")
    }

    pub(crate) fn build(&self) -> PathBuf {
        self.root().join("build")
    }

    pub(crate) fn target(&self) -> PathBuf {
        self.root().join("target")
    }

    pub(crate) fn seal(self, bytes: Box<[u8]>) -> Result<Pending, Error> {
        self.seal_with_budget(bytes, None)
    }

    pub(crate) fn seal_budgeted(
        self,
        bytes: Box<[u8]>,
        budget: &crate::node::GraphBudget,
    ) -> Result<Pending, Error> {
        self.seal_with_budget(bytes, Some(budget))
    }

    fn seal_with_budget(
        mut self,
        bytes: Box<[u8]>,
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<Pending, Error> {
        let root = match budget {
            Some(budget) => self.root_budgeted(budget)?,
            None => self.root(),
        };
        validate_top_level_with_budget(&root, State::Unsealed, budget)?;
        let path = match budget {
            Some(budget) => path(&root, Path::new("seal"), budget)?,
            None => root.join("seal"),
        };
        if let Some(budget) = budget {
            budget.paths(1)?;
            super::path::input(&path, budget)?;
        }
        write_new(&path, &bytes)?;
        let Measured { charge, geometry } = survey_observed_members(
            &root,
            View::Pending,
            budget,
            false,
            |path| Error::Corrupt {
                path,
                reason: "runner Cargo bundle member is writable",
            },
            Error::Shape,
            |_, _| Ok(()),
        )?;
        if charge.bytes > MAX_BYTES || charge.members > MAX_MEMBERS || !self.budget.admits(charge) {
            return Err(Error::Limit {
                resource: "typed store runner bundle",
                limit: usize::try_from(self.budget.charge.bytes.min(MAX_BYTES))
                    .unwrap_or(usize::MAX),
                actual: usize::try_from(charge.bytes).unwrap_or(usize::MAX),
            });
        }
        if let Some(budget) = budget {
            budget.decode(self.store.as_os_str().as_encoded_bytes().len())?;
        }
        self.armed = false;
        Ok(Pending {
            store: self.store.clone(),
            reference: self.reference,
            seal: bytes,
            charge,
            geometry,
            armed: true,
            graph_budget: budget.cloned(),
        })
    }
}

fn initialize(root: &Path, parent: &Path) -> Result<(), Error> {
    for directory in ["generated", "build", "target", "declared"] {
        let path = root.join(directory);
        let created = fs::create_dir(&path);
        created.map_err(|source| Error::Io {
            operation: "create runner Cargo bundle partition",
            path,
            source,
        })?;
    }
    write_new(&root.join("lease"), LEASE_BYTES)?;
    super::entry::sync_directory(root)?;
    super::entry::sync_directory(parent)
}

pub(crate) fn prepare_execution(
    publication: &super::lease::Publication<'_>,
    store: &Path,
    execution: ExecutionKey,
) -> Result<(), Error> {
    super::entry::ensure_admitted_directory(
        publication,
        store,
        &PathBuf::from("executions").join(execution.directory()),
    )
    .map(|_| ())
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.armed {
            let root = self.root();
            discard(&root);
        }
    }
}

impl Pending {
    pub(crate) fn validate_unpublished(
        &self,
        reference: BundleRef,
        expected_seal_length: usize,
        expected_seal_digest: [u8; 32],
    ) -> Result<PathBuf, Error> {
        if self.reference != reference {
            return Err(Error::Codec(
                "pending runner bundle reference changed before publication",
            ));
        }
        let root = self.store.join(self.reference.relative());
        validate_top_level(&root, State::Sealed)?;
        validate_lease(&root)?;
        let seal_path = root.join("seal");
        let seal = super::entry::read_file(&seal_path, crate::node::MAX_CONTENT_BYTES)?;
        if seal.len() != expected_seal_length
            || <[u8; 32]>::from(sha2::Sha256::digest(&seal)) != expected_seal_digest
        {
            return Err(Error::Corrupt {
                path: seal_path,
                reason: "pending runner Cargo bundle seal changed",
            });
        }
        self.check_pending(
            &root,
            survey(&root, View::Pending, None, false, Error::Shape)?,
        )?;
        Ok(root)
    }

    pub(crate) fn validate_unpublished_budgeted(
        &self,
        reference: BundleRef,
        expected_seal_length: usize,
        expected_seal_digest: [u8; 32],
        budget: &crate::node::GraphBudget,
    ) -> Result<PathBuf, Error> {
        self.validate_unpublished_scoped(
            reference,
            expected_seal_length,
            expected_seal_digest,
            budget,
            crate::node::read_set::Scope::Complete,
        )
    }

    /// Authenticate pending ownership and controls on every replay. Physical
    /// allocation is remeasured only at a complete checkpoint.
    pub(crate) fn validate_unpublished_scoped(
        &self,
        reference: BundleRef,
        expected_seal_length: usize,
        expected_seal_digest: [u8; 32],
        budget: &crate::node::GraphBudget,
        scope: crate::node::read_set::Scope,
    ) -> Result<PathBuf, Error> {
        if self.reference != reference {
            return Err(Error::Codec(
                "pending runner bundle reference changed before publication",
            ));
        }
        let root = super::entry::existing_directory_budgeted(
            &self.store,
            &self.reference.relative_budgeted(budget)?,
            budget,
        )?
        .ok_or(Error::State("pending runner bundle is missing"))?;
        validate_top_level_with_budget(&root, State::Sealed, Some(budget))?;
        validate_lease_with_budget(&root, Some(budget))?;
        let seal_path = path(&root, Path::new("seal"), budget)?;
        let seal =
            super::entry::read_file_budgeted(&seal_path, crate::node::MAX_CONTENT_BYTES, budget)?;
        budget.hash(seal.len())?;
        if seal.len() != expected_seal_length
            || <[u8; 32]>::from(sha2::Sha256::digest(&seal)) != expected_seal_digest
        {
            return Err(Error::Corrupt {
                path: seal_path,
                reason: "pending runner Cargo bundle seal changed",
            });
        }
        if matches!(scope, crate::node::read_set::Scope::Complete) {
            self.check_pending(
                &root,
                survey(&root, View::Pending, Some(budget), false, Error::Shape)?,
            )?;
        }
        Ok(root)
    }

    /// Geometry rule at the pending seal recheck: identical counts and geometry, and a
    /// measured charge no greater than the charge fixed at seal.
    fn check_pending(&self, root: &Path, measured: Measured) -> Result<(), Error> {
        pending_drift(root, check(self.charge, Some(self.geometry), measured))
    }

    #[cfg(test)]
    pub(crate) fn complete_replay(
        &self,
        commitment: BundleCommitment,
        budget: &crate::node::GraphBudget,
    ) -> Result<(PathBuf, Completion<'static>), Error> {
        let root = self.validate_unpublished_scoped(
            commitment.reference,
            commitment.seal_bytes,
            commitment.seal_digest,
            budget,
            crate::node::read_set::Scope::Consumer,
        )?;
        let exact_root = path(&root, Path::new(""), budget)?;
        Ok((
            root,
            Completion {
                root: exact_root,
                graph: budget.clone(),
                source: CompletionSource::Pending {
                    store: path(&self.store, Path::new(""), budget)?,
                    charge: self.charge,
                    geometry: self.geometry,
                    commitment,
                },
            },
        ))
    }

    #[cfg(test)]
    pub(crate) fn publish(
        &mut self,
        kind: Kind,
        schema: u32,
        key: Digest,
        reservation: LeaseKey,
        publisher: Digest,
    ) -> Result<Record, Error> {
        self.publish_observed(kind, schema, key, reservation, publisher)
    }

    pub(crate) fn publish_observed(
        &mut self,
        kind: Kind,
        schema: u32,
        key: Digest,
        reservation: LeaseKey,
        publisher: Digest,
    ) -> Result<Record, Error> {
        let budget = self.graph_budget.as_ref();
        let root = match budget {
            Some(budget) => path(
                &self.store,
                &self.reference.relative_budgeted(budget)?,
                budget,
            )?,
            None => self.store.join(self.reference.relative()),
        };
        validate_top_level_with_budget(&root, State::Sealed, budget)?;
        validate_lease_with_budget(&root, budget)?;
        let seal_path = match budget {
            Some(budget) => path(&root, Path::new("seal"), budget)?,
            None => root.join("seal"),
        };
        if let Some(budget) = budget {
            budget.paths(1)?;
            budget.hash(self.seal.len())?;
            super::path::input(&seal_path, budget)?;
        }
        let metadata = fs::metadata(&seal_path).map_err(|source| Error::Io {
            operation: "inspect runner bundle seal",
            path: seal_path.clone(),
            source,
        })?;
        let identity = Identity {
            class: Class::Bundle,
            kind,
            schema,
            key,
            reservation,
            publisher,
        };
        if let Some(budget) = budget {
            budget.decode(accounting::BUNDLE_RECORD_BYTES)?;
        }
        let (bytes, record) =
            Record::seal_bundle(identity, &self.seal, &metadata, self.charge, self.geometry)?;
        if let Some(budget) = budget {
            budget.paths(4)?;
        }
        write_atomic_control_with_budget(&root, "allocation", &bytes, budget)?;
        freeze_with_budget(&root, budget)?;
        validate_top_level_with_budget(&root, State::Published, budget)?;
        let actual = survey(&root, View::Published, budget, true, Error::Shape)?;
        if let Some(budget) = budget {
            budget.paths(1)?;
            budget.hash(self.seal.len())?;
            super::path::input(&seal_path, budget)?;
        }
        let seal_metadata = fs::metadata(&seal_path).map_err(|source| Error::Io {
            operation: "inspect published runner bundle seal",
            path: seal_path,
            source,
        })?;
        record.validate_bundle(kind, schema, key, &self.seal, &seal_metadata, actual)?;
        Ok(record)
    }

    pub(crate) fn retain(mut self) {
        self.armed = false;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.armed {
            let root = self.store.join(self.reference.relative());
            discard(&root);
        }
    }
}

pub(crate) fn validate_complete(
    store: &Path,
    reference: BundleRef,
    kind: Kind,
    schema: u32,
    key: Digest,
    expected_seal_length: usize,
    expected_seal_digest: [u8; 32],
    budget: &crate::node::GraphBudget,
) -> Result<PathBuf, Error> {
    let commitment = BundleCommitment::new(reference, expected_seal_length, expected_seal_digest);
    let (root, seal_path, seal) = read_commitment_with(store, &commitment, budget, |root, _| {
        Corruption::observed(
            root,
            store,
            commitment,
            kind,
            schema,
            key,
            corruption::Fact::Seal,
            budget,
        )
        .map(Error::from)
        .unwrap_or_else(|error| error)
    })?;
    let measured = survey_observed_members(
        &root,
        View::Published,
        Some(budget),
        true,
        |_| {
            Corruption::observed(
                &root,
                store,
                commitment,
                kind,
                schema,
                key,
                corruption::Fact::Mode,
                budget,
            )
            .map(Error::from)
            .unwrap_or_else(|error| error)
        },
        |_| {
            Corruption::observed(
                &root,
                store,
                commitment,
                kind,
                schema,
                key,
                corruption::Fact::Geometry,
                budget,
            )
            .map(Error::from)
            .unwrap_or_else(|error| error)
        },
        |_, _| Ok(()),
    )?;
    let record = Record::read_bundle_budgeted(&root, budget).map_err(|error| match error {
        Error::Io { .. }
        | Error::Shape(_)
        | Error::OutsideRoot(_)
        | Error::Limit { .. }
        | Error::Allocation => error,
        _ => Error::Corrupt {
            path: root.join("allocation"),
            reason: "runner Cargo bundle allocation decoding failed",
        },
    })?;
    if let Err(drift) = check(record.charge(), record.sealed_geometry(), measured) {
        return Err(Error::from(Corruption::observed(
            &root,
            store,
            commitment,
            kind,
            schema,
            key,
            corruption::Fact::drift(drift),
            budget,
        )?));
    }
    budget.paths(1)?;
    super::path::input(&seal_path, budget)?;
    let metadata = fs::metadata(&seal_path).map_err(|source| Error::Io {
        operation: "inspect runner Cargo bundle seal",
        path: seal_path,
        source,
    })?;
    budget.hash(seal.len())?;
    record
        .validate_bundle(kind, schema, key, &seal, &metadata, measured)
        .map_err(|_| Error::Corrupt {
            path: root.join("allocation"),
            reason: "runner Cargo bundle allocation seal changed",
        })?;
    accounting::registered_with_budget(store, &record, Some(budget))?;
    Ok(root)
}

/// A single-use obligation to check the whole allocation at inventory completion.
/// Construction is limited to authenticated published or pending bundle controls.
pub(crate) struct Completion<'store> {
    root: PathBuf,
    graph: crate::node::GraphBudget,
    source: CompletionSource<'store>,
}

enum CompletionSource<'store> {
    Published {
        store: &'store Path,
        commitment: BundleCommitment,
        kind: Kind,
        schema: u32,
        key: Digest,
    },
    Pending {
        store: PathBuf,
        charge: Charge,
        geometry: [u8; 32],
        commitment: BundleCommitment,
    },
}

impl<'store> Completion<'store> {
    pub(crate) fn authenticate(
        store: &'store Path,
        commitment: BundleCommitment,
        kind: Kind,
        schema: u32,
        key: Digest,
        budget: &crate::node::GraphBudget,
    ) -> Result<(PathBuf, Self), Error> {
        let root = validate_consumer_commitment(store, &commitment, kind, schema, key, budget)?;
        let exact_root = path(&root, Path::new(""), budget)?;
        Ok((
            root,
            Self {
                root: exact_root,
                graph: budget.clone(),
                source: CompletionSource::Published {
                    store,
                    commitment,
                    kind,
                    schema,
                    key,
                },
            },
        ))
    }

    /// Every member, including allocation root and controls, contributes to the
    /// same charge calculation that supplies inventory observations.
    pub(crate) fn observe(
        self,
        root: &Path,
        budget: &crate::node::GraphBudget,
        member: impl FnMut(&Path, &fs::Metadata) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.root != root {
            return Err(Error::Codec("complete allocation root differs"));
        }
        if !self.graph.shares_with(budget) {
            return Err(Error::Codec("complete allocation graph differs"));
        }
        match self.source {
            CompletionSource::Published {
                store,
                commitment,
                kind,
                schema,
                key,
            } => {
                let (current, seal_path, seal) =
                    read_commitment_with(store, &commitment, budget, |root, _| {
                        Corruption::observed(
                            root,
                            store,
                            commitment,
                            kind,
                            schema,
                            key,
                            corruption::Fact::Seal,
                            budget,
                        )
                        .map(Error::from)
                        .unwrap_or_else(|error| error)
                    })?;
                if current != root {
                    return Err(Error::Codec("complete bundle root changed"));
                }
                let measured = survey_observed_members(
                    root,
                    View::Published,
                    Some(budget),
                    true,
                    |_| {
                        Corruption::observed(
                            root,
                            store,
                            commitment,
                            kind,
                            schema,
                            key,
                            corruption::Fact::Mode,
                            budget,
                        )
                        .map(Error::from)
                        .unwrap_or_else(|error| error)
                    },
                    |_| {
                        Corruption::observed(
                            root,
                            store,
                            commitment,
                            kind,
                            schema,
                            key,
                            corruption::Fact::Geometry,
                            budget,
                        )
                        .map(Error::from)
                        .unwrap_or_else(|error| error)
                    },
                    member,
                )?;
                let record = Record::read_bundle_budgeted(root, budget)?;
                if let Err(drift) = check(record.charge(), record.sealed_geometry(), measured) {
                    return Err(Error::from(Corruption::observed(
                        root,
                        store,
                        commitment,
                        kind,
                        schema,
                        key,
                        corruption::Fact::drift(drift),
                        budget,
                    )?));
                }
                budget.paths(1)?;
                super::path::input(&seal_path, budget)?;
                let metadata = fs::metadata(&seal_path).map_err(|source| Error::Io {
                    operation: "inspect complete bundle seal",
                    path: seal_path,
                    source,
                })?;
                budget.hash(seal.len())?;
                record
                    .validate_bundle(kind, schema, key, &seal, &metadata, measured)
                    .map_err(|_| Error::Corrupt {
                        path: root.join("allocation"),
                        reason: "runner Cargo bundle allocation seal changed",
                    })?;
                accounting::registered_with_budget(store, &record, Some(budget))?;
                Ok(())
            }
            CompletionSource::Pending {
                store,
                charge,
                geometry,
                commitment,
            } => {
                let current = super::entry::existing_directory_budgeted(
                    &store,
                    &commitment.reference.relative_budgeted(budget)?,
                    budget,
                )?
                .ok_or(Error::State("pending complete bundle is missing"))?;
                if current != root {
                    return Err(Error::Codec("pending complete root differs"));
                }
                validate_top_level_with_budget(root, State::Sealed, Some(budget))?;
                validate_lease_with_budget(root, Some(budget))?;
                let seal_path = path(root, Path::new("seal"), budget)?;
                let seal = super::entry::read_file_budgeted(
                    &seal_path,
                    crate::node::MAX_CONTENT_BYTES,
                    budget,
                )?;
                budget.hash(seal.len())?;
                if seal.len() != commitment.seal_bytes
                    || <[u8; 32]>::from(sha2::Sha256::digest(&seal)) != commitment.seal_digest
                {
                    return Err(Error::Corrupt {
                        path: seal_path,
                        reason: "pending complete seal changed",
                    });
                }
                let measured = survey_observed_members(
                    root,
                    View::Pending,
                    Some(budget),
                    false,
                    Error::Shape,
                    Error::Shape,
                    member,
                )?;
                pending_drift(root, check(charge, Some(geometry), measured))
            }
        }
    }
}

/// Authenticate historical controls, seal, allocation record and registry only.
/// Does not walk payload membership, measure current charge or replay inputs.
pub(crate) fn validate_commitment(
    store: &Path,
    commitment: &BundleCommitment,
    kind: Kind,
    schema: u32,
    key: Digest,
    budget: &crate::node::GraphBudget,
) -> Result<(), Error> {
    validate_commitment_with(store, commitment, kind, schema, key, budget, |_, path| {
        Error::Corrupt {
            path,
            reason: "runner Cargo bundle seal changed",
        }
    })
    .map(|_| ())
}

/// Current candidate admission retains positive seal-mismatch provenance while
/// leaving complete physical accounting to the named complete checkpoints.
pub(crate) fn validate_consumer_commitment(
    store: &Path,
    commitment: &BundleCommitment,
    kind: Kind,
    schema: u32,
    key: Digest,
    budget: &crate::node::GraphBudget,
) -> Result<PathBuf, Error> {
    validate_commitment_with(store, commitment, kind, schema, key, budget, |root, _| {
        Corruption::observed(
            root,
            store,
            *commitment,
            kind,
            schema,
            key,
            corruption::Fact::Seal,
            budget,
        )
        .map(Error::from)
        .unwrap_or_else(|error| error)
    })
}

fn validate_commitment_with(
    store: &Path,
    commitment: &BundleCommitment,
    kind: Kind,
    schema: u32,
    key: Digest,
    budget: &crate::node::GraphBudget,
    mismatch: impl FnMut(&Path, PathBuf) -> Error,
) -> Result<PathBuf, Error> {
    let (root, seal_path, seal) = read_commitment_with(store, commitment, budget, mismatch)?;
    let record = Record::read_bundle_budgeted(&root, budget)?;
    budget.paths(1)?;
    super::path::input(&seal_path, budget)?;
    let metadata = fs::metadata(&seal_path).map_err(|source| Error::Io {
        operation: "inspect retained bundle seal",
        path: seal_path,
        source,
    })?;
    budget.hash(seal.len())?;
    record.validate_bundle_commitment(kind, schema, key, &seal, &metadata)?;
    accounting::registered_with_budget(store, &record, Some(budget))?;
    Ok(root)
}

#[cfg(test)]
fn read_commitment(
    store: &Path,
    commitment: &BundleCommitment,
    budget: &crate::node::GraphBudget,
) -> Result<(PathBuf, PathBuf, Box<[u8]>), Error> {
    read_commitment_with(store, commitment, budget, |_, path| Error::Corrupt {
        path,
        reason: "runner Cargo bundle seal changed",
    })
}

fn read_commitment_with(
    store: &Path,
    commitment: &BundleCommitment,
    budget: &crate::node::GraphBudget,
    mut mismatch: impl FnMut(&Path, PathBuf) -> Error,
) -> Result<(PathBuf, PathBuf, Box<[u8]>), Error> {
    let relative = commitment.reference.relative_budgeted(budget)?;
    let root =
        super::entry::existing_directory_budgeted(store, &relative, budget)?.ok_or_else(|| {
            Error::Corrupt {
                path: store.join(&relative),
                reason: "runner Cargo bundle is missing",
            }
        })?;
    validate_top_level_with_budget(&root, State::Published, Some(budget))?;
    validate_lease_with_budget(&root, Some(budget))?;
    let seal_path = path(&root, Path::new("seal"), budget)?;
    let seal = super::entry::read_file_budgeted_with(
        &seal_path,
        crate::node::MAX_CONTENT_BYTES,
        budget,
        |_| mismatch(&root, seal_path.clone()),
    )?;
    budget.hash(seal.len())?;
    if seal.len() != commitment.seal_bytes
        || <[u8; 32]>::from(sha2::Sha256::digest(&seal)) != commitment.seal_digest
    {
        return Err(mismatch(&root, seal_path));
    }
    Ok((root, seal_path, seal))
}

fn pending_drift(root: &Path, result: Result<(), Drift>) -> Result<(), Error> {
    match result {
        Ok(()) => Ok(()),
        Err(Drift::Geometry) => Err(Error::Corrupt {
            path: root.to_path_buf(),
            reason: "pending runner Cargo bundle geometry changed",
        }),
        Err(Drift::Count | Drift::Growth) => Err(Error::Corrupt {
            path: root.to_path_buf(),
            reason: "pending runner Cargo bundle charge changed",
        }),
    }
}

pub(crate) fn pin(store: &Path, reference: BundleRef) -> Result<Guard, Error> {
    pin_with_budget(store, reference, None)
}

pub(crate) fn pin_budgeted(
    store: &Path,
    reference: BundleRef,
    budget: &crate::node::GraphBudget,
) -> Result<Guard, Error> {
    pin_with_budget(store, reference, Some(budget))
}

fn pin_with_budget(
    store: &Path,
    reference: BundleRef,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<Guard, Error> {
    let root = match budget {
        Some(budget) => path(store, &reference.relative_budgeted(budget)?, budget)?,
        None => store.join(reference.relative()),
    };
    validate_top_level_with_budget(&root, State::Published, budget)?;
    validate_lease_with_budget(&root, budget)?;
    let path = match budget {
        Some(budget) => path(&root, Path::new("lease"), budget)?,
        None => root.join("lease"),
    };
    if let Some(budget) = budget {
        budget.paths(1)?;
        super::path::input(&path, budget)?;
    }
    let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
        operation: "inspect runner bundle lease",
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path));
    }
    if let Some(budget) = budget {
        budget.paths(1)?;
        super::path::input(&path, budget)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|source| Error::Io {
            operation: "open runner bundle lease",
            path: path.clone(),
            source,
        })?;
    lock_pinned_with_budget(root, path, file, budget)
}

#[cfg(test)]
fn lock_pinned(root: PathBuf, path: PathBuf, file: File) -> Result<Guard, Error> {
    lock_pinned_with_budget(root, path, file, None)
}

fn lock_pinned_with_budget(
    root: PathBuf,
    path: PathBuf,
    file: File,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<Guard, Error> {
    #[cfg(unix)]
    {
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockShared).map_err(|source| {
            Error::Io {
                operation: "pin immutable runner bundle",
                path: path.clone(),
                source: std::io::Error::from_raw_os_error(source.raw_os_error()),
            }
        })?;
    }
    #[cfg(not(unix))]
    {
        return Err(Error::Io {
            operation: "pin immutable runner bundle",
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "runner bundle leases require Unix advisory locks",
            ),
        });
    }
    if let Some(budget) = budget {
        budget.paths(2)?;
        super::path::input(&path, budget)?;
    }
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened pinned runner bundle lease",
        path: path.clone(),
        source,
    })?;
    let current = fs::metadata(&path).map_err(|source| Error::Io {
        operation: "reinspect pinned runner bundle lease",
        path: path.clone(),
        source,
    })?;
    if hard_linked(&opened) || !same_file(&opened, &current) {
        return Err(Error::Shape(path));
    }
    validate_top_level_with_budget(&root, State::Published, budget)?;
    validate_lease_with_budget(&root, budget)?;
    Ok(Guard { _lease: file })
}

pub(super) fn prepare_eviction(
    store: &Path,
    allocation: Allocation,
) -> Result<Option<Eviction>, Error> {
    let root = locate_allocation(store, allocation)?.ok_or(Error::CleanupRequired(
        "a registered immutable bundle is missing during eviction",
    ))?;
    prepare_eviction_at(root, allocation)
}

fn prepare_eviction_at(root: PathBuf, allocation: Allocation) -> Result<Option<Eviction>, Error> {
    let path = root.join("lease");
    let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
        operation: "inspect runner bundle eviction lease",
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path));
    }
    let file = File::open(&path).map_err(|source| Error::Io {
        operation: "open runner bundle eviction lease",
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(source) if source == rustix::io::Errno::WOULDBLOCK => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "probe runner bundle eviction lease",
                    path,
                    source: std::io::Error::from_raw_os_error(source.raw_os_error()),
                });
            }
        }
    }
    #[cfg(not(unix))]
    {
        return Err(Error::Io {
            operation: "probe runner bundle eviction lease",
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "runner bundle eviction requires Unix advisory locks",
            ),
        });
    }
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened runner bundle eviction lease",
        path: path.clone(),
        source,
    })?;
    let current = fs::metadata(&path).map_err(|source| Error::Io {
        operation: "reinspect runner bundle eviction lease",
        path: path.clone(),
        source,
    })?;
    if hard_linked(&opened) || !same_file(&opened, &current) {
        return Err(Error::Shape(path));
    }
    validate_top_level(&root, State::Published)?;
    let lease = super::entry::read_file(&root.join("lease"), LEASE_BYTES.len())?;
    if lease.as_ref() != LEASE_BYTES {
        return Err(Error::Corrupt {
            path: root.join("lease"),
            reason: "runner Cargo bundle lease identity changed",
        });
    }
    if Record::read_bundle(&root)?.allocation() != allocation {
        return Err(Error::Corrupt {
            path: root.join("allocation"),
            reason: "runner bundle allocation changed during eviction",
        });
    }
    Ok(Some(Eviction { root, _lease: file }))
}

pub(super) fn prepare_recovery_eviction(
    store: &Path,
    allocation: Allocation,
) -> Result<Option<Eviction>, Error> {
    let Some(root) = locate_allocation(store, allocation)? else {
        return Ok(None);
    };
    let Some(eviction) = prepare_eviction_at(root, allocation)? else {
        return Err(Error::CleanupRequired(
            "a runner process still pins an interrupted bundle eviction",
        ));
    };
    Ok(Some(eviction))
}

impl Eviction {
    pub(super) fn matches_reference(&self, store: &Path, reference: BundleRef) -> bool {
        self.root
            .strip_prefix(store)
            .is_ok_and(|relative| relative == reference.relative())
    }

    pub(super) fn into_repair(
        self,
        allocation: Allocation,
        budget: &mut super::recovery::Budget,
    ) -> Result<Repair, Error> {
        validate_top_level(&self.root, State::Published)?;
        validate_lease(&self.root)?;
        if Record::read_bundle(&self.root)?.allocation() != allocation {
            return Err(Error::CleanupRequired(
                "bundle retirement allocation changed",
            ));
        }
        let mut actual = super::recovery::tree_charge(&self.root, u64::MAX)?;
        actual.allocations = 1;
        budget.bundle()?;
        budget.deletion(actual)?;
        Ok(Repair {
            eviction: self,
            allocation,
            actual,
        })
    }

    pub(crate) fn retire(
        self,
        publication: &super::lease::Publication<'_>,
        store: &Path,
    ) -> Result<(), Error> {
        let admitted = accounting::admit_control(
            publication,
            store,
            super::control::Plan::retirement(&self.root),
        )?;
        thaw_root(&self.root)?;
        let temporary =
            super::entry::stage_admitted_retirement(admitted, publication, store, &self.root)?;
        remove_bundle(&temporary)
    }

    #[cfg(test)]
    pub(super) fn interrupt_after_root_thaw(
        self,
        publication: &super::lease::Publication<'_>,
        store: &Path,
    ) -> Result<PathBuf, Error> {
        let admitted = accounting::admit_control(
            publication,
            store,
            super::control::Plan::retirement(&self.root),
        )?;
        admitted.consume(publication, store, "retirement", &self.root, 0)?;
        thaw_root(&self.root)?;
        Ok(self.root)
    }
}

fn locate_allocation(store: &Path, allocation: Allocation) -> Result<Option<PathBuf>, Error> {
    if allocation.class != Class::Bundle {
        return Err(Error::State(
            "bundle eviction requires one registered immutable bundle",
        ));
    }
    let mut matched = None;
    for path in bundle_paths(store)? {
        let record = match Record::read_bundle(&path) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.allocation() == allocation && matched.replace(path).is_some() {
            return Err(Error::CleanupRequired(
                "an immutable bundle allocation resolved ambiguously during eviction",
            ));
        }
    }
    Ok(matched)
}

pub(super) fn recover_planned(
    root: &Path,
    reservation: LeaseKey,
    kind: Kind,
    schema: u32,
    key: Digest,
    registered: Option<Allocation>,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let LeaseKey::Execution(execution) = reservation else {
        return Err(Error::State(
            "planned runner bundle recovery requires one execution reservation",
        ));
    };
    let mut matched = None;
    let mut incomplete = None;
    let relative = PathBuf::from("executions").join(execution.directory());
    let Some(execution_root) = super::entry::existing_directory(root, &relative)? else {
        return match registered {
            None => Ok(()),
            Some(_) => Err(Error::CleanupRequired(
                "a registered runner bundle execution directory is missing",
            )),
        };
    };
    for entry in read_sorted(&execution_root)? {
        validate_identifier(&entry.file_name())?;
        let bundle = entry.path();
        let metadata = fs::symlink_metadata(&bundle).map_err(|source| Error::Io {
            operation: "inspect planned runner bundle recovery candidate",
            path: bundle.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Shape(bundle));
        }
        let record = match Record::read_bundle(&bundle) {
            Ok(record) => record,
            Err(_) => {
                if incomplete.replace(bundle).is_some() {
                    return Err(Error::CleanupRequired(
                        "a planned runner bundle has ambiguous incomplete publications",
                    ));
                }
                continue;
            }
        };
        let allocation = record.allocation();
        if allocation.class == Class::Bundle
            && allocation.kind == kind
            && allocation.schema == schema
            && allocation.key == key
            && matched.replace((bundle, record)).is_some()
        {
            return Err(Error::CleanupRequired(
                "a planned runner bundle resolved ambiguously",
            ));
        }
    }
    if let Some(incomplete) = incomplete {
        if registered.is_some() || matched.is_some() {
            return Err(Error::CleanupRequired(
                "a planned runner bundle mixes complete and incomplete publications",
            ));
        }
        let charge = measure(&incomplete, false)?;
        budget.bundle()?;
        budget.deletion(charge)?;
        remove_bundle(&incomplete)?;
    }
    match (matched, registered) {
        (Some((bundle, record)), Some(expected)) => {
            if record.allocation() != expected || record.reservation() == reservation {
                return Err(Error::CleanupRequired(
                    "a registered runner bundle conflicts with recovery",
                ));
            }
            let _ = bundle;
            Ok(())
        }
        (Some((bundle, record)), None) if record.reservation() == reservation => {
            budget.bundle()?;
            let allocation = record.allocation();
            budget.deletion(allocation.charge)?;
            remove_bundle(&bundle)
        }
        (None, None) => Ok(()),
        _ => Err(Error::CleanupRequired(
            "a planned runner bundle changed during recovery",
        )),
    }
}

pub(super) fn recover_orphans(
    store: &Path,
    execution: ExecutionKey,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let relative = PathBuf::from("executions").join(execution.directory());
    let Some(directory) = super::entry::existing_directory(store, &relative)? else {
        return Ok(());
    };
    let mut removed = Vec::new();
    for entry in read_sorted(&directory)? {
        validate_identifier(&entry.file_name())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect runner bundle orphan",
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Shape(path));
        }
        if published_shape(&path)? {
            continue;
        }
        // A spent pass bound defers the rest to a later pass, never refuses.
        if removed.len() == MAX_ORPHANS || !budget.bundle_fits() {
            break;
        }
        let Some(charge) = budget.dead_tree(&path)? else {
            break;
        };
        budget.bundle()?;
        budget.deletion(charge)?;
        removed.push(path);
    }
    for path in removed {
        remove_bundle(&path)?;
    }
    Ok(())
}

/// Recover globally dead unsealed attempts before reserving new space. The
/// execution guard is the liveness proof and is acquired before publication.
pub(super) fn recover_dead(
    store: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<bool, Error> {
    let mut executions =
        BTreeMap::<ExecutionKey, (super::lease::ExecutionGuard, Vec<PathBuf>)>::new();
    let mut count = 0usize;
    let referenced = referenced_bundles(store, budget)?;
    for path in bundle_paths(store)? {
        if published_shape(&path)? && referenced.contains(&Record::read_bundle(&path)?.allocation())
        {
            continue;
        }
        // The pass bound defers the rest to the next pass, never refuses.
        if count == MAX_ORPHANS {
            break;
        }
        let execution = execution_from_bundle(&path)?;
        if let Some((_, paths)) = executions.get_mut(&execution) {
            count += 1;
            paths.push(path);
            continue;
        }
        let Some(guard) = super::lease::try_execution(store, execution)? else {
            continue;
        };
        count += 1;
        executions.insert(execution, (guard, vec![path]));
    }
    let recovered = !executions.is_empty();
    for (execution, (_guard, paths)) in executions {
        super::lease::with_publication(store, |publication| {
            accounting::recover_key(publication, store, LeaseKey::Execution(execution), budget)?;
            let referenced = referenced_bundles(store, budget)?;
            for path in &paths {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                        return Err(Error::Shape(path.clone()));
                    }
                    Ok(_) => {}
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(source) => {
                        return Err(Error::Io {
                            operation: "reinspect dead runner bundle",
                            path: path.clone(),
                            source,
                        });
                    }
                }
                if published_shape(path)? {
                    // Published but never committed: the ledger is the only
                    // authority, so an unreferenced, unpinned bundle is garbage.
                    let record = Record::read_bundle(path)?;
                    let allocation = record.allocation();
                    if referenced.contains(&allocation) {
                        continue;
                    }
                    if !budget.bundle_fits() || !budget.fits(allocation.charge) {
                        return Ok(());
                    }
                    let Some(_eviction) = prepare_eviction_at(path.clone(), allocation)? else {
                        continue;
                    };
                    budget.bundle()?;
                    budget.deletion(allocation.charge)?;
                    remove_bundle(path)?;
                    continue;
                }
                let Some(charge) = budget.dead_tree(path)?.filter(|_| budget.bundle_fits()) else {
                    return Ok(());
                };
                budget.bundle()?;
                budget.deletion(charge)?;
                remove_bundle(path)?;
            }
            Ok(())
        })?;
    }
    Ok(recovered)
}

/// Exact bundle allocations the ledger registers or journals for eviction.
fn referenced_bundles(
    store: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<Vec<Allocation>, Error> {
    Ok(accounting::registrations_recovery(store, budget)?
        .iter()
        .chain(accounting::pending_eviction_recovery(store, budget)?.iter())
        .map(|registered| registered.allocation())
        .filter(|allocation| allocation.class == Class::Bundle)
        .collect())
}

fn published_shape(path: &Path) -> Result<bool, Error> {
    let mut published = true;
    for name in ["seal", "allocation"] {
        match fs::symlink_metadata(path.join(name)) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || hard_linked(&metadata) =>
            {
                return Err(Error::Shape(path.join(name)));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => published = false,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect runner bundle publication state",
                    path: path.join(name),
                    source,
                });
            }
        }
    }
    if published {
        validate_top_level(path, State::Published)?;
        Record::read_bundle(path)?;
    }
    Ok(published)
}

fn execution_from_bundle(path: &Path) -> Result<ExecutionKey, Error> {
    let name = path
        .parent()
        .and_then(Path::file_name)
        .ok_or(Error::State("runner bundle has no execution directory"))?;
    decode_identifier(name)
        .map(ExecutionKey::from_bytes)
        .ok_or(Error::CleanupRequired(
            "runner bundle execution identity changed",
        ))
}

pub(crate) fn count_bundles(root: &Path) -> Result<usize, Error> {
    let mut count = 0usize;
    for _ in bundle_paths(root)? {
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn bundle_paths(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let executions = root.join("executions");
    let mut bundles = Vec::new();
    for execution in read_sorted(&executions)? {
        validate_identifier(&execution.file_name())?;
        let execution_path = execution.path();
        let metadata = fs::symlink_metadata(&execution_path).map_err(|source| Error::Io {
            operation: "inspect runner bundle execution directory",
            path: execution_path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Shape(execution_path));
        }
        let remaining = MAX_BUNDLES.saturating_sub(bundles.len()).saturating_add(1);
        for bundle in read_sorted_bounded(&execution_path, remaining)? {
            validate_identifier(&bundle.file_name())?;
            let path = bundle.path();
            bundles.push(path);
            if bundles.len() > MAX_BUNDLES {
                return Err(Error::Limit {
                    resource: "typed store immutable bundles",
                    limit: MAX_BUNDLES,
                    actual: bundles.len(),
                });
            }
        }
    }
    Ok(bundles)
}

fn validate_top_level(root: &Path, state: State) -> Result<(), Error> {
    validate_top_level_with_budget(root, state, None)
}

fn validate_top_level_with_budget(
    root: &Path,
    state: State,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    let mut names = match budget {
        Some(budget) => names(root, 8, budget)?,
        None => read_sorted(root)?
            .into_iter()
            .map(|entry| entry.file_name())
            .collect(),
    };
    names.sort_unstable();
    let expected: &[&str] = match state {
        State::Unsealed => &["build", "declared", "generated", "lease", "target"],
        State::Sealed => &["build", "declared", "generated", "lease", "seal", "target"],
        State::Published => &[
            "allocation",
            "build",
            "declared",
            "generated",
            "lease",
            "seal",
            "target",
        ],
    };
    if names.len() != expected.len()
        || names
            .iter()
            .zip(expected.iter())
            .any(|(actual, expected)| actual != *expected)
    {
        for (name, reason) in [
            (
                "allocation",
                "published runner Cargo bundle allocation is missing",
            ),
            ("build", "runner Cargo bundle build directory is missing"),
            (
                "declared",
                "runner Cargo bundle declared directory is missing",
            ),
            (
                "generated",
                "runner Cargo bundle generated directory is missing",
            ),
            ("lease", "runner Cargo bundle lease is missing"),
            ("seal", "sealed runner Cargo bundle seal is missing"),
            ("target", "runner Cargo bundle target directory is missing"),
        ] {
            let required = !matches!(
                (state, name),
                (State::Unsealed, "allocation" | "seal") | (State::Sealed, "allocation")
            );
            if required && !names.iter().any(|actual| actual == name) {
                return Err(Error::Corrupt {
                    path: root.to_path_buf(),
                    reason,
                });
            }
        }
        let reason = match (state, names.len().cmp(&expected.len())) {
            (State::Unsealed, std::cmp::Ordering::Less) => {
                "unsealed runner Cargo bundle top-level member is missing"
            }
            (State::Unsealed, std::cmp::Ordering::Greater) => {
                "unsealed runner Cargo bundle has an undeclared top-level member"
            }
            (State::Unsealed, std::cmp::Ordering::Equal) => {
                "unsealed runner Cargo bundle top-level name changed"
            }
            (State::Sealed, std::cmp::Ordering::Less) => {
                "sealed runner Cargo bundle top-level member is missing"
            }
            (State::Sealed, std::cmp::Ordering::Greater) => {
                "sealed runner Cargo bundle has an undeclared top-level member"
            }
            (State::Sealed, std::cmp::Ordering::Equal) => {
                "sealed runner Cargo bundle top-level name changed"
            }
            (State::Published, std::cmp::Ordering::Less) => {
                "published runner Cargo bundle top-level member is missing"
            }
            (State::Published, std::cmp::Ordering::Greater) => {
                "published runner Cargo bundle has an undeclared top-level member"
            }
            (State::Published, std::cmp::Ordering::Equal) => {
                "published runner Cargo bundle top-level name changed"
            }
        };
        return Err(Error::Corrupt {
            path: root.to_path_buf(),
            reason,
        });
    }
    Ok(())
}

fn validate_lease(root: &Path) -> Result<(), Error> {
    validate_lease_with_budget(root, None)
}

fn validate_lease_with_budget(
    root: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    let path = match budget {
        Some(budget) => path(root, Path::new("lease"), budget)?,
        None => root.join("lease"),
    };
    let bytes = match budget {
        Some(budget) => super::entry::read_file_budgeted(&path, LEASE_BYTES.len(), budget)?,
        None => super::entry::read_file(&path, LEASE_BYTES.len())?,
    };
    if bytes.as_ref() != LEASE_BYTES {
        return Err(Error::Corrupt {
            path,
            reason: "runner Cargo bundle lease identity changed",
        });
    }
    Ok(())
}

fn measure(root: &Path, future_allocation: bool) -> Result<Charge, Error> {
    measure_with_budget(root, future_allocation, None, false)
}

fn measure_with_budget(
    root: &Path,
    future_allocation: bool,
    budget: Option<&crate::node::GraphBudget>,
    require_frozen: bool,
) -> Result<Charge, Error> {
    measure_observed(root, future_allocation, budget, require_frozen, |path| {
        Error::Corrupt {
            path,
            reason: "runner Cargo bundle member is writable",
        }
    })
}

fn measure_observed(
    root: &Path,
    future_allocation: bool,
    budget: Option<&crate::node::GraphBudget>,
    require_frozen: bool,
    writable_member: impl Fn(PathBuf) -> Error,
) -> Result<Charge, Error> {
    measure_observed_members(
        root,
        future_allocation,
        budget,
        require_frozen,
        writable_member,
        |_, _| Ok(()),
    )
}

fn measure_observed_members(
    root: &Path,
    future_allocation: bool,
    budget: Option<&crate::node::GraphBudget>,
    require_frozen: bool,
    writable_member: impl Fn(PathBuf) -> Error,
    member: impl FnMut(&Path, &fs::Metadata) -> Result<(), Error>,
) -> Result<Charge, Error> {
    let view = if future_allocation {
        View::Pending
    } else {
        View::Published
    };
    survey_observed_members(
        root,
        view,
        budget,
        require_frozen,
        writable_member,
        Error::Shape,
        member,
    )
    .map(|measured| measured.charge)
}

fn survey(
    root: &Path,
    view: View,
    budget: Option<&crate::node::GraphBudget>,
    require_frozen: bool,
    misshapen_member: impl Fn(PathBuf) -> Error,
) -> Result<Measured, Error> {
    survey_observed_members(
        root,
        view,
        budget,
        require_frozen,
        |path| Error::Corrupt {
            path,
            reason: "runner Cargo bundle member is writable",
        },
        misshapen_member,
        |_, _| Ok(()),
    )
}

/// One charged walk supplies the charge and the geometry together.
fn survey_observed_members(
    root: &Path,
    view: View,
    budget: Option<&crate::node::GraphBudget>,
    require_frozen: bool,
    writable_member: impl Fn(PathBuf) -> Error,
    misshapen_member: impl Fn(PathBuf) -> Error,
    mut member: impl FnMut(&Path, &fs::Metadata) -> Result<(), Error>,
) -> Result<Measured, Error> {
    let future_allocation = view == View::Pending;
    let mut geometry = geometry::Accumulator::new(view);
    let mut bytes = if future_allocation { BLOCK_BYTES } else { 0 };
    let mut members = if future_allocation { 1_u64 } else { 0 };
    let result = (|| {
        let mut pending = traversal(root, budget)?;
        while let Some(path) = pending.pop() {
            if let Some(budget) = budget {
                budget.paths(1)?;
                super::path::input(&path, budget)?;
            }
            members = members.checked_add(1).ok_or(Error::Allocation)?;
            if members > MAX_MEMBERS {
                return Err(Error::Limit {
                    resource: "runner bundle filesystem members",
                    limit: usize::try_from(MAX_MEMBERS).unwrap_or(usize::MAX),
                    actual: usize::try_from(members).unwrap_or(usize::MAX),
                });
            }
            let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                operation: "inspect runner bundle member",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(misshapen_member(path));
            }
            if require_frozen && writable(&metadata) {
                return Err(writable_member(path));
            }
            let charged = if metadata.is_dir() {
                let remaining = MAX_MEMBERS
                    .saturating_sub(members)
                    .saturating_sub(pending.len() as u64);
                for child in paths_with_budget(
                    &path,
                    usize::try_from(remaining).map_err(|_| Error::Allocation)?,
                    budget,
                )?
                .into_iter()
                .rev()
                {
                    push(&mut pending, child, budget)?;
                }
                BLOCK_BYTES
            } else if metadata.is_file() && !hard_linked(&metadata) {
                round_block(metadata.len())?.max(round_block(allocated_bytes(&metadata)?)?)
            } else {
                return Err(misshapen_member(path));
            };
            member(&path, &metadata)?;
            let hashed = geometry.member(root, &path, &metadata)?;
            if let Some(budget) = budget {
                budget.hash(hashed)?;
            }
            bytes = bytes.checked_add(charged).ok_or(Error::Allocation)?;
            if bytes > MAX_BYTES {
                return Err(Error::Limit {
                    resource: "runner bundle physical bytes",
                    limit: usize::try_from(MAX_BYTES).unwrap_or(usize::MAX),
                    actual: usize::try_from(bytes).unwrap_or(usize::MAX),
                });
            }
        }
        Ok(Charge {
            bytes,
            allocations: 1,
            members,
        })
    })();
    result.and_then(|charge| {
        Ok(Measured {
            charge,
            geometry: geometry.finish()?,
        })
    })
}

pub(crate) fn path(
    parent: &Path,
    relative: &Path,
    budget: &crate::node::GraphBudget,
) -> Result<PathBuf, Error> {
    let length = parent
        .as_os_str()
        .as_encoded_bytes()
        .len()
        .checked_add(relative.as_os_str().as_encoded_bytes().len())
        .and_then(|size| size.checked_add(1))
        .ok_or(Error::Allocation)?;
    budget.decode(
        std::mem::size_of::<PathBuf>()
            .checked_add(length)
            .ok_or(Error::Allocation)?,
    )?;
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    owned.push(parent);
    owned.push(relative);
    Ok(owned)
}

#[cfg(test)]
fn freeze(root: &Path) -> Result<(), Error> {
    freeze_with_budget(root, None)
}

fn freeze_with_budget(root: &Path, budget: Option<&crate::node::GraphBudget>) -> Result<(), Error> {
    let mut directories = buffer::Buffer::new();
    let mut pending = traversal(root, budget)?;
    while let Some(path) = pending.pop() {
        if let Some(budget) = budget {
            budget.paths(1)?;
            super::path::input(&path, budget)?;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect runner bundle before freezing",
            path: path.clone(),
            source,
        })?;
        if metadata.is_dir() {
            for child in paths_with_budget(&path, MAX_MEMBERS as usize, budget)? {
                push(&mut pending, child, budget)?;
            }
            push(&mut directories, path, budget)?;
        } else if metadata.is_file()
            && !metadata.file_type().is_symlink()
            && !hard_linked(&metadata)
        {
            if let Some(budget) = budget {
                budget.paths(1)?;
                super::path::input(&path, budget)?;
            }
            set_frozen(&path, &metadata, false)?;
        } else {
            return Err(Error::Shape(path));
        }
    }
    let mut directories = directories.into_vec();
    directories.sort_unstable_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        if let Some(budget) = budget {
            budget.paths(1)?;
            super::path::input(&directory, budget)?;
        }
        let metadata = fs::symlink_metadata(&directory).map_err(|source| Error::Io {
            operation: "inspect runner bundle directory before freezing",
            path: directory.clone(),
            source,
        })?;
        if let Some(budget) = budget {
            budget.paths(2)?;
            super::path::input(&directory, budget)?;
            super::path::input(&directory, budget)?;
        }
        set_frozen(&directory, &metadata, true)?;
        // One full sync of the root orders publication after the frozen tree.
        // An interior directory's lost entry or mode after a crash fails the
        // bundle's validation and is rebuilt, so it is not synced.
        if directory == root {
            super::entry::sync_directory(&directory)?;
        }
    }
    Ok(())
}

fn traversal(
    root: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<buffer::Buffer<PathBuf>, Error> {
    let path = match budget {
        Some(budget) => {
            let length = root.as_os_str().as_encoded_bytes().len();
            budget.decode(length)?;
            let mut path = PathBuf::new();
            path.try_reserve_exact(length)
                .map_err(|_| Error::Allocation)?;
            path.push(root);
            path
        }
        None => root.to_path_buf(),
    };
    let mut pending = buffer::Buffer::new();
    push(&mut pending, path, budget)?;
    Ok(pending)
}

fn push(
    pending: &mut buffer::Buffer<PathBuf>,
    path: PathBuf,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    match budget {
        Some(budget) => pending.push(path, budget),
        None => pending.push_legacy(path),
    }
}

#[cfg(test)]
fn validate_frozen(root: &Path) -> Result<(), Error> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect immutable runner bundle",
            path: path.clone(),
            source,
        })?;
        if writable(&metadata) {
            return Err(Error::Corrupt {
                path,
                reason: "runner Cargo bundle member is writable",
            });
        }
        if metadata.is_dir() {
            for child in read_sorted(&path)? {
                pending.push(child.path());
            }
        } else if !metadata.is_file() || metadata.file_type().is_symlink() || hard_linked(&metadata)
        {
            return Err(Error::Shape(path));
        }
    }
    Ok(())
}

fn thaw_root(root: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(root).map_err(|source| Error::Io {
        operation: "inspect runner bundle root for atomic retirement",
        path: root.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::Shape(root.to_path_buf()));
    }
    set_writable(root, &metadata)
}

fn remove_bundle(path: &Path) -> Result<(), Error> {
    thaw(path)?;
    fs::remove_dir_all(path).map_err(|source| Error::Io {
        operation: "remove incomplete runner bundle",
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(parent) = path.parent() {
        super::entry::sync_directory(parent)?;
    }
    Ok(())
}

/// Best-effort removal of an abandoned bundle.
fn discard(path: &Path) {
    let _ = thaw(path);
    let _ = fs::remove_dir_all(path);
}

#[cfg(any(test, feature = "test-fixtures"))]
pub(super) fn retire_fixture_store(root: &Path) -> Result<(), Error> {
    thaw(root)?;
    fs::remove_dir_all(root).map_err(|source| Error::Io {
        operation: "retire typed store fixture",
        path: root.to_path_buf(),
        source,
    })?;
    if let Some(parent) = root.parent() {
        super::entry::sync_directory(parent)?;
    }
    Ok(())
}

fn thaw(root: &Path) -> Result<(), Error> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect runner bundle for cleanup",
                path: root.to_path_buf(),
                source,
            });
        }
    };
    set_writable(root, &metadata)?;
    if metadata.is_dir() {
        for child in read_sorted(root)? {
            thaw(&child.path())?;
        }
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| Error::Io {
            operation: "create runner bundle control file",
            path: path.to_path_buf(),
            source,
        })?;
    output.write_all(bytes).map_err(|source| Error::Io {
        operation: "write runner bundle control file",
        path: path.to_path_buf(),
        source,
    })?;
    output.sync_all().map_err(|source| Error::Io {
        operation: "sync runner bundle control file",
        path: path.to_path_buf(),
        source,
    })
}

fn write_atomic_control_with_budget(
    root: &Path,
    name: &'static str,
    bytes: &[u8],
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    let target = match budget {
        Some(budget) => path(root, Path::new(name), budget)?,
        None => root.join(name),
    };
    if let Some(budget) = budget {
        budget.decode(name.len().checked_add(8).ok_or(Error::Allocation)?)?;
    }
    let pending_name = format!("{name}.pending");
    let pending = match budget {
        Some(budget) => path(root, Path::new(&pending_name), budget)?,
        None => root.join(pending_name),
    };
    if let Some(budget) = budget {
        super::path::input(&pending, budget)?;
    }
    write_new(&pending, bytes)?;
    if let Some(budget) = budget {
        super::path::input(root, budget)?;
    }
    super::entry::sync_directory(root)?;
    if let Some(budget) = budget {
        super::path::input(&pending, budget)?;
        super::path::input(&target, budget)?;
    }
    fs::rename(&pending, &target).map_err(|source| Error::Io {
        operation: "publish runner bundle control file",
        path: target,
        source,
    })?;
    if let Some(budget) = budget {
        super::path::input(root, budget)?;
    }
    super::entry::sync_directory(root)
}

fn random_id() -> Result<BundleId, Error> {
    let path = Path::new("/dev/urandom");
    let mut source = File::open(path).map_err(|source| Error::Io {
        operation: "open runner bundle entropy source",
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = [0_u8; IDENTIFIER_BYTES];
    source.read_exact(&mut bytes).map_err(|source| Error::Io {
        operation: "read runner bundle entropy",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(BundleId(bytes))
}

fn read_sorted(path: &Path) -> Result<Vec<fs::DirEntry>, Error> {
    read_sorted_bounded(path, usize::try_from(MAX_MEMBERS).unwrap_or(usize::MAX))
}

/// Observe complete native names through the reviewed accounting adapter.
pub(crate) fn names(
    path: &Path,
    limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Vec<std::ffi::OsString>, Error> {
    directory::names(path, limit, budget)
}

pub(crate) fn paths(
    path: &Path,
    limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Vec<PathBuf>, Error> {
    let names = names(path, limit, budget)?;
    budget.decode(
        names
            .len()
            .checked_mul(std::mem::size_of::<PathBuf>())
            .ok_or(Error::Allocation)?,
    )?;
    let mut paths = Vec::new();
    paths
        .try_reserve_exact(names.len())
        .map_err(|_| Error::Allocation)?;
    for name in names {
        let length = path
            .as_os_str()
            .as_encoded_bytes()
            .len()
            .checked_add(name.as_encoded_bytes().len())
            .and_then(|size| size.checked_add(1))
            .ok_or(Error::Allocation)?;
        budget.decode(length)?;
        let mut owned = PathBuf::new();
        owned
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        owned.push(path);
        owned.push(name);
        paths.push(owned);
    }
    Ok(paths)
}

fn paths_with_budget(
    path: &Path,
    limit: usize,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<Vec<PathBuf>, Error> {
    match budget {
        Some(budget) => paths(path, limit, budget),
        None => Ok(read_sorted_bounded(path, limit)?
            .into_iter()
            .map(|entry| entry.path())
            .collect()),
    }
}

/// Explicit legacy enumeration, with no shared-budget admission route.
fn read_sorted_bounded(path: &Path, limit: usize) -> Result<Vec<fs::DirEntry>, Error> {
    let mut directory = fs::read_dir(path).map_err(|source| Error::Io {
        operation: "read runner bundle directory",
        path: path.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    loop {
        let Some(entry) = directory.next() else {
            break;
        };
        if entries.len() == limit {
            return Err(Error::Limit {
                resource: "runner bundle directory members",
                limit,
                actual: entries.len().saturating_add(1),
            });
        }
        let entry = entry.map_err(|source| Error::Io {
            operation: "read runner bundle directory entry",
            path: path.to_path_buf(),
            source,
        })?;
        entries
            .try_reserve_exact(1)
            .map_err(|_| Error::Allocation)?;
        entries.push(entry);
    }
    entries.sort_unstable_by_key(|left| left.file_name());
    Ok(entries)
}

fn validate_identifier(name: &std::ffi::OsStr) -> Result<(), Error> {
    let bytes = name.as_encoded_bytes();
    if bytes.len() != IDENTIFIER_BYTES * 2
        || bytes
            .iter()
            .any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(Error::CleanupRequired(
            "runner bundle identifier shape changed",
        ));
    }
    Ok(())
}

fn decode_identifier(name: &std::ffi::OsStr) -> Option<[u8; IDENTIFIER_BYTES]> {
    let encoded = name.as_encoded_bytes();
    if encoded.len() != IDENTIFIER_BYTES * 2 {
        return None;
    }
    let mut decoded = [0_u8; IDENTIFIER_BYTES];
    for (index, pair) in encoded.as_chunks::<2>().0.iter().enumerate() {
        decoded[index] = decode_nibble(pair[0])?.checked_mul(16)? | decode_nibble(pair[1])?;
    }
    Some(decoded)
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn round_block(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK_BYTES - 1)
        .map(|bytes| bytes / BLOCK_BYTES * BLOCK_BYTES)
        .ok_or(Error::Allocation)
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    use std::os::unix::fs::MetadataExt as _;

    let measured = metadata
        .blocks()
        .checked_mul(512)
        .ok_or(Error::Allocation)?;
    #[cfg(test)]
    let measured = geometry::seam::allocated(metadata, measured);
    Ok(measured)
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    Ok(metadata.len())
}

#[cfg(unix)]
pub(crate) fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
}

#[cfg(not(unix))]
pub(crate) fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_frozen(path: &Path, metadata: &fs::Metadata, directory: bool) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = frozen_mode(metadata, directory);
    // A member written frozen keeps its status-change time, and with it the
    // digest memoized for its identity.
    if metadata.permissions().mode() & 0o7777 == mode {
        return Ok(());
    }

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| Error::Io {
        operation: "freeze runner bundle member",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
/// A frozen member's mode. Files are read-only. Directories stay writable
/// by their owner, so `cargo clean` (and any `rm -r`) can remove a bundle;
/// a member added to one is still caught, by the bundle's recorded listing.
pub(crate) fn frozen_mode(metadata: &fs::Metadata, directory: bool) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    if directory {
        0o755
    } else if metadata.mode() & 0o111 != 0 {
        0o555
    } else {
        0o444
    }
}

#[cfg(not(unix))]
pub(crate) fn frozen_mode(_metadata: &fs::Metadata, _directory: bool) -> u32 {
    1
}

#[cfg(not(unix))]
fn set_frozen(path: &Path, metadata: &fs::Metadata, _directory: bool) -> Result<(), Error> {
    let mut permissions = metadata.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|source| Error::Io {
        operation: "freeze runner bundle member",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn set_writable(path: &Path, metadata: &fs::Metadata) -> Result<(), Error> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let mode = if metadata.is_dir() {
        0o755
    } else {
        metadata.mode() | 0o200
    };

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| Error::Io {
        operation: "thaw runner bundle member for cleanup",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_writable(path: &Path, metadata: &fs::Metadata) -> Result<(), Error> {
    let mut permissions = metadata.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).map_err(|source| Error::Io {
        operation: "thaw runner bundle member for cleanup",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn writable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    // A frozen directory is 0o755 (see `frozen_mode`).
    let mode = metadata.mode() & 0o7777;
    if metadata.is_dir() {
        mode != 0o755 && mode & 0o022 != 0
    } else {
        mode & 0o222 != 0
    }
}

#[cfg(not(unix))]
fn writable(metadata: &fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sha2::Sha256;

    use super::*;

    static FIXTURE: AtomicU64 = AtomicU64::new(1);

    fn store(label: &str) -> (PathBuf, super::super::Store) {
        let root = std::env::temp_dir().join(format!(
            "rawr-runner-bundle-{label}-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("the bundle fixture root creates");
        let store =
            super::super::Store::open(&root, [0x31; 32]).expect("the bundle fixture store opens");
        let executions = [
            1, 2, 3, 4, 5, 6, 7, 8, 21, 22, 31, 32, 33, 40, 41, 42, 43, 44, 45, 46, 47, 48,
        ];
        super::super::lease::with_publication(store.root(), |publication| {
            for value in executions {
                prepare_execution(publication, store.root(), execution(value))?;
            }
            Ok(())
        })
        .expect("the bundle fixture execution parents are admitted");
        for value in executions {
            super::super::lease::with_execution(store.root(), execution(value), || Ok(()))
                .expect("the bundle fixture execution lock is admitted");
        }
        (root, store)
    }

    fn budget(bytes: u64, members: u64) -> Budget {
        Budget {
            charge: Charge {
                bytes,
                allocations: 1,
                members,
            },
        }
    }

    fn execution(value: u8) -> ExecutionKey {
        ExecutionKey::from_bytes([value; 32])
    }

    #[test]
    fn shared_pending_publication_cannot_reset_its_original_budget() {
        let (fixture, store) = store("publication-budget");
        let graph = crate::node::GraphBudget::new();
        let attempt =
            Attempt::allocate(store.root(), execution(1), budget(1024 * 1024, 64)).unwrap();
        let root = attempt.root();
        fs::write(attempt.target().join("runner"), b"inert").unwrap();
        let mut pending = attempt
            .seal_budgeted(Box::from(&b"seal"[..]), &graph)
            .unwrap();
        graph.exhaust_paths();
        assert!(!graph.exceedances().is_exceeded());
        // Publication keeps charging the one original account it was
        // sealed with. Its very first path check lands past the exhausted
        // target and is recorded there, proving the budget was never reset.
        pending
            .publish(
                Kind::Runner,
                3,
                Digest::from_bytes([1; 32]),
                LeaseKey::Execution(execution(1)),
                Digest::from_bytes([2; 32]),
            )
            .expect("an accounting axis never refuses publication");
        let axis = graph
            .exceedances()
            .axis(crate::node::PATHS_RESOURCE)
            .copied()
            .expect("publication path checks were charged to the original graph");
        assert_eq!(
            axis.first_requested,
            crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert!(axis.high_water >= axis.first_requested);
        assert!(root.exists());
        drop(pending);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn shared_freeze_precharges_metadata_and_permission_mutation() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("rawr-freeze-floor-{}", std::process::id()));
        fs::write(&root, b"inert").unwrap();
        for available in [1, 2] {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o644)).unwrap();
            let graph = crate::node::GraphBudget::new();
            graph
                .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - available)
                .unwrap();
            // The metadata probe and the permission mutation are each
            // charged one path check before they run. Two remaining fit
            // exactly; one remaining still freezes and records the exact total.
            freeze_with_budget(&root, Some(&graph)).expect("an accounting axis never refuses");
            assert!(!writable(&fs::metadata(&root).unwrap()));
            let paths = graph
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| axis.high_water);
            assert_eq!(
                paths,
                (available == 1).then_some(crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
            );
        }
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn shared_names_and_path_construction_precharge_before_owned_work() {
        let missing = Path::new("/definitely-missing-rawr-budget-fixture");
        let decode = crate::node::GraphBudget::new();
        decode
            .decode(crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        // The owned caller path bytes are still charged before the native
        // open; the exhausted target records that exact charge instead of
        // refusing, and the operation reports its own filesystem error.
        let missing_len = missing.as_os_str().as_encoded_bytes().len() as u64;
        let caller_bytes = std::mem::size_of::<PathBuf>() as u64 + 2 * missing_len + 1;
        assert!(matches!(
            names(missing, 4, &decode),
            Err(Error::Io { path, source, .. })
                if path == missing && source.kind() == std::io::ErrorKind::NotFound
        ));
        let decoded = decode
            .exceedances()
            .axis(crate::node::DECODED_RESOURCE)
            .copied()
            .expect("the caller path ownership was charged");
        assert_eq!(
            decoded.first_requested,
            crate::node::MAX_DECODED_BYTES_PER_GRAPH + caller_bytes
        );
        assert_eq!(decoded.high_water, decoded.first_requested);
        let joined_bytes = std::mem::size_of::<PathBuf>() as u64 + missing_len + 4 + 1;
        assert_eq!(
            path(missing, Path::new("name"), &decode).expect("an accounting axis never refuses"),
            missing.join("name")
        );
        assert_eq!(
            decode
                .exceedances()
                .axis(crate::node::DECODED_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_DECODED_BYTES_PER_GRAPH + caller_bytes + joined_bytes)
        );
        let paths = crate::node::GraphBudget::new();
        paths.paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        assert!(matches!(
            names(missing, 4, &paths),
            Err(Error::Io { path, source, .. })
                if path == missing && source.kind() == std::io::ErrorKind::NotFound
        ));
        assert_eq!(
            paths
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| (axis.first_requested, axis.high_water)),
            Some((
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1,
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            ))
        );
    }

    #[test]
    fn bundle_transforms_unsealed_to_validated_pending_to_frozen_publication() {
        let (fixture, store) = store("lifecycle");
        let attempt = Attempt::allocate(store.root(), execution(1), budget(1024 * 1024, 64))
            .expect("the bundle attempt allocates");
        let root = attempt.root();
        validate_top_level(&root, State::Unsealed).expect("the unsealed shape validates");
        fs::write(attempt.generated().join("Cargo.toml"), b"[package]\n")
            .expect("the generated input writes");
        fs::write(attempt.target().join("runner"), b"runner").expect("the runner output writes");
        let reference = attempt.reference();
        let seal = Box::<[u8]>::from(b"sealed runner bundle".as_slice());
        let seal_digest = Sha256::digest(&seal).into();
        let mut pending = attempt.seal(seal.clone()).expect("the bundle seals");
        pending
            .validate_unpublished(reference, seal.len(), seal_digest)
            .expect("the pending bundle revalidates");
        let key = Digest::from_bytes([0x42; 32]);
        let record = pending
            .publish(
                Kind::Runner,
                2,
                key,
                LeaseKey::Execution(execution(1)),
                Digest::from_bytes([0x51; 32]),
            )
            .expect("the bundle publishes");
        validate_top_level(&root, State::Published).expect("the published shape validates");
        validate_frozen(&root).expect("the published bundle is immutable");
        assert_eq!(
            record.charge(),
            measure(&root, false).expect("the bundle measures")
        );

        #[cfg(unix)]
        {
            let guard = pin(store.root(), reference).expect("the published bundle pins");
            let contender = File::open(root.join("lease")).expect("the contender lease opens");
            assert!(rustix::fs::flock(
                &contender,
                rustix::fs::FlockOperation::NonBlockingLockExclusive
            )
            .is_err());
            drop(guard);
            rustix::fs::flock(
                &contender,
                rustix::fs::FlockOperation::NonBlockingLockExclusive,
            )
            .expect("the bundle becomes exclusively lockable after its pin drops");
        }

        drop(pending);
        assert!(
            !root.exists(),
            "an unretained publication cleans up exactly"
        );
        fs::remove_dir_all(fixture).expect("the bundle fixture retires");
    }

    #[test]
    fn commitment_authenticates_controls_without_claiming_member_census() {
        let (fixture, store) = store("commitment");
        let attempt =
            Attempt::allocate(store.root(), execution(1), budget(1024 * 1024, 64)).unwrap();
        let root = attempt.root();
        fs::write(root.join("target/member"), b"original").unwrap();
        let reference = attempt.reference();
        let seal = Box::<[u8]>::from(b"historical seal".as_slice());
        let commitment = BundleCommitment::new(reference, seal.len(), Sha256::digest(&seal).into());
        let mut pending = attempt.seal(seal).unwrap();
        let key = Digest::from_bytes([0x42; 32]);
        let record = pending
            .publish(
                Kind::Runner,
                2,
                key,
                LeaseKey::Execution(execution(1)),
                Digest::from_bytes([0x51; 32]),
            )
            .unwrap();
        let graph = crate::node::GraphBudget::new();
        let (_, seal_path, bytes) = read_commitment(store.root(), &commitment, &graph).unwrap();
        record
            .validate_bundle_commitment(
                Kind::Runner,
                2,
                key,
                &bytes,
                &fs::metadata(&seal_path).unwrap(),
            )
            .unwrap();
        // Publication alone has not committed the allocation to its registry.
        assert!(
            validate_commitment(store.root(), &commitment, Kind::Runner, 2, key, &graph).is_err()
        );
        let wrong = BundleCommitment::new(reference, commitment.seal_bytes, [0; 32]);
        assert!(read_commitment(store.root(), &wrong, &graph).is_err());
        // A nested physical change is outside record-only authority. Full
        // validation must still reject its changed physical charge.
        let member = root.join("target/member");
        let metadata = fs::metadata(&member).unwrap();
        set_writable(&member, &metadata).unwrap();
        fs::write(&member, vec![7; 8192]).unwrap();
        fs::set_permissions(&member, metadata.permissions()).unwrap();
        let (_, seal_path, bytes) = read_commitment(store.root(), &commitment, &graph).unwrap();
        record
            .validate_bundle_commitment(
                Kind::Runner,
                2,
                key,
                &bytes,
                &fs::metadata(&seal_path).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            validate_complete(
                store.root(),
                reference,
                Kind::Runner,
                2,
                key,
                commitment.seal_bytes,
                commitment.seal_digest,
                &graph
            ),
            Err(Error::CleanupRequired(
                "typed store allocation is sealed but absent from its installation ledger"
            ))
        ));
        // An exhausted decoded-bytes target never refuses the commitment
        // read. The read still charges its exact ownership (measured on a fresh
        // account) to the original account and records the exact total.
        let probe = crate::node::GraphBudget::new();
        read_commitment(store.root(), &commitment, &probe).unwrap();
        let (probe_decoded, _, _) = probe.census_usage().unwrap();
        assert!(probe_decoded > 0);
        assert!(!probe.exceedances().is_exceeded());
        let spent = crate::node::GraphBudget::new();
        spent
            .decode(crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let (_, _, reread) = read_commitment(store.root(), &commitment, &spent)
            .expect("an accounting axis never refuses");
        assert_eq!(reread, bytes);
        assert_eq!(
            spent
                .exceedances()
                .axis(crate::node::DECODED_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_DECODED_BYTES_PER_GRAPH + probe_decoded)
        );
        drop(pending);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn retirement_rechecks_held_lease_inode_and_original_member_floor() {
        let (fixture, store) = store("retirement-lease");
        let attempt =
            Attempt::allocate(store.root(), execution(1), budget(1024 * 1024, 64)).unwrap();
        let root = attempt.root();
        let mut pending = attempt.seal(Box::from(b"seal".as_slice())).unwrap();
        let record = pending
            .publish(
                Kind::Runner,
                2,
                Digest::from_bytes([0x39; 32]),
                LeaseKey::Execution(execution(1)),
                Digest::from_bytes([0x61; 32]),
            )
            .unwrap();
        let allocation = record.allocation();
        let mut recovery = super::super::recovery::Budget::new();
        let eviction = prepare_eviction_at(root.clone(), allocation)
            .unwrap()
            .unwrap();
        let repair = eviction.into_repair(allocation, &mut recovery).unwrap();
        repair.validate(&root, allocation).unwrap();
        thaw_root(&root).unwrap();
        fs::rename(root.join("lease"), fixture.join("old-lease")).unwrap();
        fs::write(root.join("lease"), LEASE_BYTES).unwrap();
        freeze(&root).unwrap();
        assert!(matches!(
            repair.validate(&root, allocation),
            Err(Error::CleanupRequired(
                "bundle retirement lease identity changed"
            ))
        ));
        drop(repair);
        drop(pending);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn pending_bundle_rejects_late_output_and_drop_cleans_the_whole_attempt() {
        let (fixture, store) = store("pending-drift");
        let attempt = Attempt::allocate(store.root(), execution(2), budget(1024 * 1024, 64))
            .expect("the bundle attempt allocates");
        let root = attempt.root();
        let reference = attempt.reference();
        let seal = Box::<[u8]>::from(b"pending".as_slice());
        let digest = Sha256::digest(&seal).into();
        let pending = attempt
            .seal(seal.clone())
            .expect("the pending bundle seals");
        fs::write(root.join("target/late"), b"late").expect("the late output writes");
        let graph = crate::node::GraphBudget::new();
        pending
            .validate_unpublished_scoped(
                reference,
                seal.len(),
                digest,
                &graph,
                crate::node::read_set::Scope::Consumer,
            )
            .expect("intermediate commitment does not claim current physical accounting");
        assert!(matches!(
            pending.validate_unpublished_budgeted(reference, seal.len(), digest, &graph),
            Err(Error::Corrupt {
                reason: "pending runner Cargo bundle charge changed",
                ..
            })
        ));
        assert!(matches!(
            pending.validate_unpublished(reference, seal.len(), digest),
            Err(Error::Corrupt {
                reason: "pending runner Cargo bundle charge changed",
                ..
            })
        ));
        drop(pending);
        assert!(
            !root.exists(),
            "the rejected pending bundle cleans up exactly"
        );
        fs::remove_dir_all(fixture).expect("the bundle fixture retires");
    }

    #[test]
    fn partial_allocation_marker_is_recovered_as_one_uncommitted_bundle() {
        let (fixture, store) = store("partial-marker");
        let execution = execution(21);
        let attempt = Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))
            .expect("the partial-marker attempt allocates");
        let root = attempt.root();
        let pending = attempt
            .seal(Box::<[u8]>::from(b"pending".as_slice()))
            .expect("the partial-marker attempt seals");
        write_new(&root.join("allocation.pending"), b"partial allocation")
            .expect("the interrupted allocation marker exists");
        std::mem::forget(pending);

        recover_planned(
            store.root(),
            LeaseKey::Execution(execution),
            Kind::Runner,
            2,
            Digest::from_bytes([0x61; 32]),
            None,
            &mut crate::store::recovery::Budget::new(),
        )
        .expect("the exact incomplete publication recovers");
        assert!(!root.exists());
        fs::remove_dir_all(fixture).expect("the partial-marker fixture retires");
    }

    #[cfg(unix)]
    #[test]
    fn blocked_pin_revalidates_the_same_lease_path_after_lock_acquisition() {
        let (fixture, store) = store("pin-race");
        let execution = execution(22);
        let attempt = Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))
            .expect("the pin-race attempt allocates");
        let root = attempt.root();
        let mut pending = attempt
            .seal(Box::<[u8]>::from(b"pin-race".as_slice()))
            .expect("the pin-race attempt seals");
        pending
            .publish(
                Kind::Runner,
                2,
                Digest::from_bytes([0x62; 32]),
                LeaseKey::Execution(execution),
                Digest::from_bytes([0x63; 32]),
            )
            .expect("the pin-race bundle publishes");
        pending.retain();

        let path = root.join("lease");
        let exclusive = File::open(&path).expect("the exclusive lease opens");
        rustix::fs::flock(&exclusive, rustix::fs::FlockOperation::LockExclusive)
            .expect("the fixture holds the exclusive lease");
        let opened = File::open(&path).expect("the blocked pin opens the old lease inode");
        let worker_root = root.clone();
        let worker_path = path.clone();
        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            opened_tx.send(()).expect("the pin-race signal sends");
            lock_pinned(worker_root, worker_path, opened)
        });
        opened_rx.recv().expect("the pin-race signal arrives");
        let moved = root.with_file_name("retired-pin-race");
        fs::rename(&root, &moved).expect("the published bundle path is replaced while pin waits");
        drop(exclusive);
        assert!(worker.join().expect("the blocked pin joins").is_err());
        remove_bundle(&moved).expect("the moved pin-race bundle retires");
        fs::remove_dir_all(fixture).expect("the pin-race fixture retires");
    }

    #[test]
    fn abandoned_attempt_is_recovered_before_the_next_allocation() {
        let (fixture, store) = store("orphan");
        let attempt = Attempt::allocate(store.root(), execution(3), budget(1024 * 1024, 64))
            .expect("the first bundle attempt allocates");
        let abandoned = attempt.root();
        std::mem::forget(attempt);
        recover_orphans(
            store.root(),
            execution(3),
            &mut crate::store::recovery::Budget::new(),
        )
        .expect("the miss-side recovery retires the exact orphan");
        let successor = Attempt::allocate(store.root(), execution(3), budget(1024 * 1024, 64))
            .expect("the successor allocates after recovery");
        assert!(!abandoned.exists(), "the exact abandoned bundle is removed");
        assert_eq!(count_bundles(store.root()).expect("bundles count"), 1);
        drop(successor);
        fs::remove_dir_all(fixture).expect("the bundle fixture retires");
    }

    #[test]
    fn orphan_recovery_defers_the_ninth_bundle_to_a_later_pass() {
        let (fixture, store) = store("orphan-cap");
        let execution = execution(31);
        let mut abandoned = Vec::new();
        for _ in 0..=MAX_ORPHANS {
            let attempt = Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))
                .expect("the bounded orphan fixture allocates");
            abandoned.push(attempt.root());
            std::mem::forget(attempt);
        }

        for remaining in [1, 0] {
            recover_orphans(
                store.root(),
                execution,
                &mut crate::store::recovery::Budget::new(),
            )
            .expect("bounded orphan recovery never refuses");
            assert_eq!(
                abandoned.iter().filter(|path| path.exists()).count(),
                remaining
            );
        }
        fs::remove_dir_all(fixture).expect("the orphan-cap fixture retires");
    }

    #[test]
    fn globally_dead_attempt_requires_and_consumes_its_execution_lease() {
        let (fixture, store) = store("global-orphan");
        let execution = execution(32);
        let abandoned = super::super::lease::with_execution(store.root(), execution, || {
            let attempt = Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))?;
            let abandoned = attempt.root();
            std::mem::forget(attempt);
            Ok(abandoned)
        })
        .expect("the abandoned attempt releases its execution lease");

        assert!(
            recover_dead(store.root(), &mut crate::store::recovery::Budget::new(),)
                .expect("the globally dead attempt recovers")
        );
        assert!(!abandoned.exists());
        fs::remove_dir_all(fixture).expect("the global-orphan fixture retires");
    }

    #[test]
    fn live_attempt_is_skipped_until_its_execution_lease_retires() {
        let (fixture, store) = store("live-orphan");
        let execution = execution(33);
        let root = store.root().to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            super::super::lease::with_execution(&root, execution, || {
                let attempt = Attempt::allocate(&root, execution, budget(1024 * 1024, 64))?;
                let abandoned = attempt.root();
                std::mem::forget(attempt);
                ready_tx
                    .send(abandoned)
                    .map_err(|_| Error::State("the live-orphan fixture receiver disappeared"))?;
                release_rx
                    .recv()
                    .map_err(|_| Error::State("the live-orphan fixture sender disappeared"))?;
                Ok(())
            })
        });
        let abandoned = ready_rx.recv().expect("the live attempt reports its root");

        assert!(
            !recover_dead(store.root(), &mut crate::store::recovery::Budget::new(),)
                .expect("the live attempt is inspected without waiting")
        );
        assert!(abandoned.is_dir());
        release_tx
            .send(())
            .expect("the live attempt is allowed to retire");
        worker
            .join()
            .expect("the live-attempt worker joins")
            .expect("the live execution lease retires");

        assert!(
            recover_dead(store.root(), &mut crate::store::recovery::Budget::new(),)
                .expect("the retired attempt recovers")
        );
        assert!(!abandoned.exists());
        fs::remove_dir_all(fixture).expect("the live-orphan fixture retires");
    }

    #[test]
    fn global_dead_attempt_recovery_defers_the_ninth_to_a_later_pass() {
        let (fixture, store) = store("global-orphan-cap");
        let mut abandoned = Vec::new();
        for identity in 40..=40 + MAX_ORPHANS {
            let execution = execution(identity as u8);
            abandoned.push(
                super::super::lease::with_execution(store.root(), execution, || {
                    let attempt =
                        Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))?;
                    let root = attempt.root();
                    std::mem::forget(attempt);
                    Ok(root)
                })
                .expect("the dead-attempt fixture releases its execution lease"),
            );
        }

        for remaining in [1, 0] {
            recover_dead(store.root(), &mut crate::store::recovery::Budget::new())
                .expect("bounded dead-bundle recovery never refuses");
            assert_eq!(
                abandoned.iter().filter(|path| path.exists()).count(),
                remaining
            );
        }
        fs::remove_dir_all(fixture).expect("the global-orphan-cap fixture retires");
    }

    /// The owner reserved `execution`, ran `interrupt` under its execution
    /// lease and was killed; a later run then recovered its dead reservation.
    fn interrupted_owner(
        store: &super::super::Store,
        execution: ExecutionKey,
        interrupt: impl FnOnce(Attempt) -> Result<(), Error>,
    ) -> PathBuf {
        let key = LeaseKey::Execution(execution);
        super::super::lease::with_publication(store.root(), |publication| {
            prepare_execution(publication, store.root(), execution)?;
            accounting::reserve(
                publication,
                store.root(),
                key,
                budget(1024 * 1024, 64).charge,
            )
            .map(drop)
        })
        .expect("the interrupted owner reserves its execution");
        let abandoned = super::super::lease::with_execution(store.root(), execution, || {
            let attempt = Attempt::allocate(store.root(), execution, budget(1024 * 1024, 64))?;
            let root = attempt.root();
            interrupt(attempt)?;
            Ok(root)
        })
        .expect("the killed owner releases its execution lease");
        super::super::lease::with_publication(store.root(), |publication| {
            accounting::recover_key(
                publication,
                store.root(),
                key,
                &mut crate::store::recovery::Budget::new(),
            )
        })
        .expect("recovering the dead reservation tolerates its bundle remains");
        super::super::accounting::physical_usage(store.root())
            .expect("inventory charges the remains instead of refusing the store");
        abandoned
    }

    #[test]
    fn interrupted_unpublished_bundle_self_heals_after_its_reservation_is_recovered() {
        let (fixture, store) = store("interrupted-unpublished");
        let abandoned = interrupted_owner(&store, execution(24), |attempt| {
            fs::write(attempt.target().join("runner"), b"partial")
                .map_err(|_| Error::State("the partial output writes"))?;
            fs::hard_link(
                attempt.target().join("runner"),
                attempt.target().join("linked"),
            )
            .map_err(|_| Error::State("Cargo-style hard link writes"))?;
            std::mem::forget(attempt);
            Ok(())
        });
        assert!(abandoned.is_dir());
        assert!(
            recover_dead(store.root(), &mut crate::store::recovery::Budget::new(),)
                .expect("dead-bundle recovery removes the unpublished remains")
        );
        assert!(!abandoned.exists());
        super::super::accounting::physical_usage(store.root()).expect("the store stays usable");
        fs::remove_dir_all(fixture).expect("the interrupted-unpublished fixture retires");
    }

    #[test]
    fn interrupted_published_bundle_self_heals_once_unpinned() {
        let (fixture, store) = store("interrupted-published");
        let execution = execution(25);
        let abandoned = interrupted_owner(&store, execution, |attempt| {
            let mut pending = attempt.seal(Box::<[u8]>::from(b"interrupted".as_slice()))?;
            pending.publish(
                Kind::Runner,
                2,
                Digest::from_bytes([0x66; 32]),
                LeaseKey::Execution(execution),
                Digest::from_bytes([0x67; 32]),
            )?;
            std::mem::forget(pending); // killed between publication and ledger commit
            Ok(())
        });
        let reference = BundleRef::new(
            execution,
            BundleId(decode_identifier(abandoned.file_name().unwrap()).unwrap()),
        );
        let guard = pin(store.root(), reference).expect("a reader pins the remains");
        recover_dead(store.root(), &mut crate::store::recovery::Budget::new())
            .expect("a pinned unregistered bundle is skipped");
        assert!(abandoned.is_dir(), "a pinned bundle is never deleted");
        drop(guard);
        assert!(
            recover_dead(store.root(), &mut crate::store::recovery::Budget::new(),)
                .expect("dead-bundle recovery removes the unregistered publication")
        );
        assert!(!abandoned.exists());
        super::super::accounting::physical_usage(store.root()).expect("the store stays usable");
        fs::remove_dir_all(fixture).expect("the interrupted-published fixture retires");
    }

    #[cfg(unix)]
    #[test]
    fn bundle_measurement_rejects_symlinks_and_hard_links() {
        use std::os::unix::fs::symlink;

        let (fixture, store) = store("link-shape");
        let attempt = Attempt::allocate(store.root(), execution(4), budget(1024 * 1024, 64))
            .expect("the symlink attempt allocates");
        let root = attempt.root();
        symlink("outside", attempt.generated().join("link"))
            .expect("the synthetic symlink creates");
        assert!(matches!(
            attempt.seal(Box::from(&b"seal"[..])),
            Err(Error::Shape(_))
        ));
        assert!(!root.exists(), "the rejected symlink attempt cleans up");

        let attempt = Attempt::allocate(store.root(), execution(5), budget(1024 * 1024, 64))
            .expect("the hard-link attempt allocates");
        let root = attempt.root();
        let first = attempt.generated().join("first");
        fs::write(&first, b"shared inode").expect("the first hard link writes");
        fs::hard_link(&first, attempt.generated().join("second"))
            .expect("the second hard link creates");
        assert!(matches!(
            attempt.seal(Box::from(&b"seal"[..])),
            Err(Error::Shape(_))
        ));
        assert!(!root.exists(), "the rejected hard-link attempt cleans up");
        fs::remove_dir_all(fixture).expect("the bundle fixture retires");
    }

    #[test]
    fn frozen_bundle_validation_uses_the_single_charged_measurement_walk() {
        let root =
            std::env::temp_dir().join(format!("rawr-bundle-frozen-budget-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the frozen budget root exists");
        fs::write(root.join("payload"), b"bundle").expect("the frozen payload writes");
        freeze(&root).expect("the fixture freezes");

        let exact = crate::node::GraphBudget::new();
        exact
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 10)
            .expect("metadata, open/mount, raw reads, dot/member/exhaustion probes precharge");
        let expected = measure_with_budget(&root, false, Some(&exact), true)
            .expect("one charged frozen walk admits the exact path cap");
        assert!(!exact.exceedances().is_exceeded());

        // One operation less remaining still completes the same single
        // walk; the exceedance records exactly one path check past the target,
        // proving the walk is charged exactly ten operations.
        let floor = crate::node::GraphBudget::new();
        floor
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 9)
            .expect("the one-operation floor precharges");
        assert_eq!(
            measure_with_budget(&root, false, Some(&floor), true)
                .expect("an accounting axis never refuses"),
            expected
        );
        assert_eq!(
            floor
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| (axis.first_requested, axis.high_water)),
            Some((
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1,
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            ))
        );
        remove_bundle(&root).expect("the frozen budget fixture retires");
    }

    #[test]
    fn joint_measurement_supplies_each_member_without_a_second_path_walk() {
        let root = std::env::temp_dir().join(format!("rawr-joint-measure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("payload"), b"bundle").unwrap();
        freeze(&root).unwrap();
        let expected = measure_with_budget(&root, false, None, true).unwrap();
        let graph = crate::node::GraphBudget::new();
        graph
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 10)
            .unwrap();
        let mut observed = Vec::new();
        let actual = measure_observed_members(
            &root,
            false,
            Some(&graph),
            true,
            Error::Shape,
            |path, metadata| {
                observed.push((path.to_path_buf(), metadata.is_dir()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            observed,
            vec![(root.clone(), true), (root.join("payload"), false)]
        );
        // The callbacks share the exact ten-operation walk. The account
        // sits exactly at its target: nothing is recorded, and one more path
        // check is still charged while recording the exact cumulative total.
        assert!(!graph.exceedances().is_exceeded());
        graph.paths(1).expect("an accounting axis never refuses");
        assert_eq!(
            graph
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1),
            "callbacks share the exact ten-operation walk"
        );
        remove_bundle(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn pending_completion_rejects_late_ancestor_alias_without_content_handles() {
        let (fixture, store) = store("pending-completion-alias");
        let attempt =
            Attempt::allocate(store.root(), execution(2), budget(1024 * 1024, 64)).unwrap();
        let root = attempt.root();
        let seal = Box::<[u8]>::from(b"pending".as_slice());
        let commitment = BundleCommitment::new(
            attempt.reference(),
            seal.len(),
            Sha256::digest(&seal).into(),
        );
        let pending = attempt.seal(seal).unwrap();
        let graph = crate::node::GraphBudget::new();
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        let ancestor = root.parent().unwrap();
        let moved = ancestor.with_extension("detached");
        fs::rename(ancestor, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, ancestor).unwrap();
        let mut observed = 0;
        let result = completion.observe(&root, &graph, |_, _| {
            observed += 1;
            Ok(())
        });
        fs::remove_file(ancestor).unwrap();
        fs::rename(&moved, ancestor).unwrap();
        assert!(
            result.is_err(),
            "late intermediate alias must not discharge the obligation"
        );
        assert_eq!(
            observed, 0,
            "ancestor validation precedes the allocation walk"
        );
        drop(pending);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn pending_completion_rechecks_late_charge_and_seal_on_the_original_graph() {
        let (fixture, store) = store("pending-completion");
        let attempt =
            Attempt::allocate(store.root(), execution(2), budget(1024 * 1024, 64)).unwrap();
        let root = attempt.root();
        let seal = Box::<[u8]>::from(b"pending".as_slice());
        let commitment = BundleCommitment::new(
            attempt.reference(),
            seal.len(),
            Sha256::digest(&seal).into(),
        );
        let pending = attempt.seal(seal).unwrap();
        let graph = crate::node::GraphBudget::new();
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        let mut members = Vec::new();
        completion
            .observe(&root, &graph, |path, _| {
                members.push(path.strip_prefix(&root).unwrap().to_path_buf());
                Ok(())
            })
            .unwrap();
        assert!(members.contains(&PathBuf::new()));
        for control in ["lease", "seal", "build", "declared", "generated", "target"] {
            assert!(
                members.contains(&PathBuf::from(control)),
                "missing {control}"
            );
        }
        assert!(
            !members.contains(&PathBuf::from("allocation")),
            "pending charge reserves the future record"
        );
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        assert!(matches!(
            completion.observe(&root, &crate::node::GraphBudget::new(), |_, _| Ok(())),
            Err(Error::Codec("complete allocation graph differs"))
        ));
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        fs::write(root.join("target/late"), b"extra").unwrap();
        assert!(completion.observe(&root, &graph, |_, _| Ok(())).is_err());
        fs::remove_file(root.join("target/late")).unwrap();
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        fs::write(root.join("seal"), b"changed").unwrap();
        assert!(matches!(
            completion.observe(&root, &graph, |_, _| Ok(())),
            Err(Error::Corrupt {
                reason: "pending complete seal changed",
                ..
            })
        ));
        drop(pending);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn bundle_directory_collection_precharges_each_entry_and_bounds_its_growth() {
        let root = std::env::temp_dir().join(format!(
            "rawr-bundle-directory-precharge-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        for index in 0..4 {
            fs::write(root.join(index.to_string()), b"entry").unwrap();
        }
        let exact = crate::node::GraphBudget::new();
        exact
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 11)
            .unwrap();
        assert_eq!(names(&root, 4, &exact).unwrap().len(), 4);
        // The collection is charged exactly eleven path checks. The exact
        // account records nothing and still charges one more, recording the
        // exact cumulative total.
        assert!(!exact.exceedances().is_exceeded());
        exact.paths(1).expect("an accounting axis never refuses");
        assert_eq!(
            exact
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
        );

        let floor = crate::node::GraphBudget::new();
        floor
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 10)
            .unwrap();
        assert_eq!(
            names(&root, 4, &floor)
                .expect("an accounting axis never refuses")
                .len(),
            4
        );
        assert_eq!(
            floor
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
        );
        assert!(matches!(
            names(&root, 3, &crate::node::GraphBudget::new()),
            Err(Error::Limit {
                resource: "runner bundle directory members",
                limit: 3,
                actual: 4
            })
        ));

        fs::remove_dir_all(&root).unwrap();
        let blocked = crate::node::GraphBudget::new();
        blocked
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH)
            .unwrap();
        // An exhausted target still charges the failed open of a missing
        // directory (exactly one path check, recorded) and reports the
        // filesystem error rather than refusing.
        assert!(
            matches!(
                names(&root, 4, &blocked),
                Err(Error::Io { path, source, .. })
                    if path == root && source.kind() == std::io::ErrorKind::NotFound
            ),
            "an exhausted budget still opens a missing directory and reports it"
        );
        assert_eq!(
            blocked
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .map(|axis| (axis.first_requested, axis.high_water)),
            Some((
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1,
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            ))
        );
    }

    #[test]
    fn bundle_count_and_physical_byte_caps_fail_closed() {
        let (fixture, store) = store("hard-caps");
        let execution_root = store
            .root()
            .join("executions")
            .join(execution(6).directory());
        assert!(
            execution_root.is_dir(),
            "the synthetic execution root exists"
        );
        for index in 0..MAX_BUNDLES {
            let mut id = [0; IDENTIFIER_BYTES];
            id[IDENTIFIER_BYTES - std::mem::size_of::<usize>()..]
                .copy_from_slice(&index.to_be_bytes());
            fs::create_dir(execution_root.join(BundleId::from_bytes(id).directory()))
                .expect("the synthetic bundle slot creates");
        }
        assert!(matches!(
            Attempt::allocate(store.root(), execution(7), budget(MAX_BYTES, MAX_MEMBERS)),
            Err(Error::Limit {
                resource: "typed store immutable bundles",
                limit: MAX_BUNDLES,
                ..
            })
        ));
        fs::remove_dir_all(&execution_root).expect("the synthetic bundle slots retire");

        let attempt = Attempt::allocate(store.root(), execution(8), budget(MAX_BYTES, MAX_MEMBERS))
            .expect("the oversized attempt allocates");
        let root = attempt.root();
        File::create(attempt.target().join("oversized"))
            .and_then(|file| file.set_len(MAX_BYTES))
            .expect("the sparse oversized output sizes");
        assert!(matches!(
            attempt.seal(Box::from(&b"seal"[..])),
            Err(Error::Limit {
                resource: "runner bundle physical bytes",
                ..
            })
        ));
        assert!(!root.exists(), "the oversized attempt cleans up exactly");
        fs::remove_dir_all(fixture).expect("the hard-cap fixture retires");
    }
}
