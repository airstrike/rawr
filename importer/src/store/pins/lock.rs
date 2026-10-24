use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::node::{Digest, Error};

use super::{Owner, Role, Root};
use crate::store::accounting::{Charge, Reference};

const MAGIC: &[u8; 8] = b"RAWRPINR";
const SCHEMA: u16 = 1;
const HEADER_BYTES: usize = 120;
const MAX_ROOTS: usize = 1_024;
const MAX_EDGES: usize = 65_536;
const MAX_RECORD_BYTES: usize = HEADER_BYTES + super::MAX_EDGES * 96;
const MAX_TOTAL_BYTES: usize = MAX_ROOTS * HEADER_BYTES + MAX_EDGES * 96;

/// One process-owned shared OS lease over an immutable pin-root record.
///
/// The record is durable evidence of the exact closure. This value is the
/// liveness proof: dropping it releases the only fact that can keep the record
/// from miss-side stale-pin recovery and eviction.
pub(crate) struct Guard {
    _file: File,
}
impl Guard {}

/// Exact pre-mutation record state retained until a pin replacement commits.
pub(super) struct Acquisition {
    project: Digest,
    pinned: Root,
    existing: bool,
}

impl Acquisition {
    pub(super) fn inspect(store: &Path, project: Digest, pinned: Root) -> Result<Self, Error> {
        let relative = PathBuf::from("pins")
            .join(pinned.role.directory())
            .join(pinned.owner.directory());
        let existing = super::super::entry::existing_directory(store, &relative)?.is_some();
        if existing {
            let bytes = Record {
                project,
                root: pinned.clone(),
            }
            .encode()?;
            validate_record_file(&store.join(relative), &bytes)?;
        }
        Ok(Self {
            project,
            pinned,
            existing,
        })
    }

    pub(super) fn acquire(
        &self,
        publication: &crate::store::lease::Publication<'_>,
        store: &Path,
    ) -> Result<Guard, Error> {
        acquire(publication, store, self.project, &self.pinned)
    }

    pub(super) fn retain(
        &self,
        _publication: &crate::store::lease::Publication<'_>,
        store: &Path,
    ) -> Result<Guard, Error> {
        if !self.existing {
            return Err(Error::CleanupRequired(
                "a retained project pin record is missing",
            ));
        }
        let bytes = Record {
            project: self.project,
            root: self.pinned.clone(),
        }
        .encode()?;
        let path = store
            .join("pins")
            .join(self.pinned.role.directory())
            .join(self.pinned.owner.directory())
            .join("root");
        open_shared(&path, &bytes)
    }

    pub(super) fn rollback(
        &self,
        publication: &crate::store::lease::Publication<'_>,
        store: &Path,
    ) -> Result<(), Error> {
        if self.existing {
            return Ok(());
        }
        let relative = PathBuf::from("pins")
            .join(self.pinned.role.directory())
            .join(self.pinned.owner.directory());
        if super::super::entry::existing_directory(store, &relative)?.is_none() {
            return Ok(());
        }
        retire_released(publication, store, self.project, &self.pinned)
    }
}

/// One publication-locked view of every durable pin root.
///
/// Active records remain shared-locked by this value. Stale records remain
/// exclusively locked, so no process can revive one while an eviction pass
/// reasons about its closure.
pub(crate) struct Snapshot {
    protected: Box<[Reference]>,
    stale: Box<[Stale]>,
    active: Box<[Active]>,
}

pub(crate) struct Active {
    project: Digest,
    references: Box<[Reference]>,
    _file: Guard,
}

pub(crate) struct Stale {
    directory: PathBuf,
    project: Digest,
    role: Role,
    owner: Owner,
    references: Box<[Reference]>,
    charge: Charge,
    _file: Guard,
}

struct Record {
    project: Digest,
    root: Root,
}

struct Inventory {
    roots: usize,
    edges: usize,
    bytes: usize,
}

struct Located {
    directory: PathBuf,
    bytes: Box<[u8]>,
    record: Record,
}

pub(super) fn acquire(
    publication: &crate::store::lease::Publication<'_>,
    store: &Path,
    project: Digest,
    pinned: &Root,
) -> Result<Guard, Error> {
    let bytes = Record {
        project,
        root: pinned.clone(),
    }
    .encode()?;
    let relative = PathBuf::from("pins")
        .join(pinned.role.directory())
        .join(pinned.owner.directory());
    let target = store.join(&relative);
    match super::super::entry::existing_directory(store, &relative)? {
        Some(_) => {
            validate_record_file(&target, &bytes)?;
        }
        None => {
            Inventory::read(store)?.admit(pinned.references.len(), bytes.len())?;
            match super::super::entry::publish_directory(
                publication,
                store,
                &target,
                "root",
                &bytes,
            )? {
                super::super::entry::Publication::Published => {}
                super::super::entry::Publication::Existing => {
                    validate_record_file(&target, &bytes)?;
                }
            }
        }
    }
    super::transaction::checkpoint(super::transaction::Stage::Published(pinned.role))?;
    open_shared(&target.join("root"), &bytes)
}

pub(super) fn retire_released(
    publication: &crate::store::lease::Publication<'_>,
    store: &Path,
    project: Digest,
    released: &Root,
) -> Result<(), Error> {
    let bytes = Record {
        project,
        root: released.clone(),
    }
    .encode()?;
    let relative = PathBuf::from("pins")
        .join(released.role.directory())
        .join(released.owner.directory());
    let directory = super::super::entry::existing_directory(store, &relative)?.ok_or(
        Error::CleanupRequired("a released pin root disappeared before retirement"),
    )?;
    validate_record_file(&directory, &bytes)?;
    let lock = match open_snapshot(&directory.join("root"), &bytes)? {
        SnapshotLock::Stale(lock) => lock,
        SnapshotLock::Active(_) => {
            return Err(Error::CleanupRequired(
                "a released pin root retained an active OS owner",
            ));
        }
    };
    let temporary = super::super::entry::stage_retirement(publication, store, &directory)?;
    fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
        operation: "retire released pin root",
        path: temporary.clone(),
        source,
    })?;
    drop(lock);
    super::super::entry::sync_directory(
        temporary
            .parent()
            .ok_or(Error::State("released pin root has no temporary directory"))?,
    )
}

impl Record {
    fn encode(&self) -> Result<Box<[u8]>, Error> {
        let length = HEADER_BYTES
            .checked_add(
                self.root
                    .references
                    .len()
                    .checked_mul(Reference::encoded_bytes())
                    .ok_or(Error::Allocation)?,
            )
            .filter(|length| *length <= MAX_RECORD_BYTES)
            .ok_or(Error::Limit {
                resource: "typed store pin-root bytes",
                limit: MAX_RECORD_BYTES,
                actual: usize::MAX,
            })?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&SCHEMA.to_le_bytes());
        bytes.push(self.root.role as u8);
        bytes.extend_from_slice(&[0; 5]);
        bytes.extend_from_slice(&self.project.as_bytes());
        bytes.extend_from_slice(&self.root.owner.0.as_bytes());
        bytes.extend_from_slice(&self.root.proof.as_bytes());
        bytes.extend_from_slice(&(self.root.references.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        for reference in &self.root.references {
            reference.encode(&mut bytes);
        }
        debug_assert_eq!(bytes.len(), length);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_BYTES
            || &bytes[..8] != MAGIC
            || u16_at(bytes, 8)? != SCHEMA
            || bytes[11..16].iter().any(|byte| *byte != 0)
            || bytes[116..120].iter().any(|byte| *byte != 0)
        {
            return Err(Error::Codec("typed store pin-root header changed"));
        }
        let role =
            Role::from_code(bytes[10]).ok_or(Error::Codec("typed store pin-root role changed"))?;
        let project = Digest::from_bytes(array_at(bytes, 16)?);
        let owner = Owner(Digest::from_bytes(array_at(bytes, 48)?));
        let proof = Digest::from_bytes(array_at(bytes, 80)?);
        let count = usize::try_from(u32_at(bytes, 112)?)
            .map_err(|_| Error::Codec("typed store pin-root edge count is not addressable"))?;
        let length = HEADER_BYTES
            .checked_add(
                count
                    .checked_mul(Reference::encoded_bytes())
                    .ok_or(Error::Allocation)?,
            )
            .ok_or(Error::Allocation)?;
        if count > super::MAX_EDGES || length != bytes.len() || length > MAX_RECORD_BYTES {
            return Err(Error::Codec("typed store pin-root edge count changed"));
        }
        let mut references = Vec::new();
        references
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let mut offset = HEADER_BYTES;
        for _ in 0..count {
            references.push(Reference::decode(bytes, offset)?);
            offset += Reference::encoded_bytes();
        }
        if references.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Codec(
                "typed store pin-root references changed order",
            ));
        }
        let root = Root {
            role,
            owner,
            proof,
            references: references.into_boxed_slice(),
        };
        if owner != Owner::derive(project, role, proof, &root.references)
            || (Record {
                project,
                root: root.clone(),
            })
            .encode()?
            .as_ref()
                != bytes
        {
            return Err(Error::Codec("typed store pin-root is not canonical"));
        }
        Ok(Self { project, root })
    }
}

impl Inventory {
    fn read(store: &Path) -> Result<Self, Error> {
        Self::catalog(store).map(|(inventory, _)| inventory)
    }

    fn catalog(store: &Path) -> Result<(Self, Vec<Located>), Error> {
        let mut inventory = Self {
            roots: 0,
            edges: 0,
            bytes: 0,
        };
        let mut located = Vec::new();
        located
            .try_reserve_exact(MAX_ROOTS)
            .map_err(|_| Error::Allocation)?;
        for role in [Role::Live, Role::LastGood, Role::Prepared, Role::InFlight] {
            let relative = PathBuf::from("pins").join(role.directory());
            let directory =
                super::super::entry::existing_directory(store, &relative)?.ok_or_else(|| {
                    Error::Corrupt {
                        path: store.join(&relative),
                        reason: "typed store pin-role directory is missing",
                    }
                })?;
            let entries = fs::read_dir(&directory).map_err(|source| Error::Io {
                operation: "read typed store pin-role directory",
                path: directory.clone(),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    operation: "read typed store pin-root entry",
                    path: directory.clone(),
                    source,
                })?;
                inventory.roots = inventory.roots.checked_add(1).ok_or(Error::Allocation)?;
                if inventory.roots > MAX_ROOTS {
                    return Err(Error::Limit {
                        resource: "typed store strong pin roots",
                        limit: MAX_ROOTS,
                        actual: inventory.roots,
                    });
                }
                let file_type = entry.file_type().map_err(|source| Error::Io {
                    operation: "inspect typed store pin-root entry",
                    path: entry.path(),
                    source,
                })?;
                if file_type.is_symlink() || !file_type.is_dir() {
                    return Err(Error::Shape(entry.path()));
                }
                let root_path = entry.path();
                let bytes = validate_one_member(&root_path)?;
                let record = Record::decode(&bytes).map_err(|_| Error::Corrupt {
                    path: root_path.join("root"),
                    reason: "typed store pin-root decoding failed",
                })?;
                if record.root.role != role
                    || entry.file_name().as_encoded_bytes()
                        != record.root.owner.directory().as_bytes()
                {
                    return Err(Error::Corrupt {
                        path: root_path,
                        reason: "typed store pin-root placement changed",
                    });
                }
                inventory.edges = inventory
                    .edges
                    .checked_add(record.root.references.len())
                    .ok_or(Error::Allocation)?;
                inventory.bytes = inventory
                    .bytes
                    .checked_add(bytes.len())
                    .ok_or(Error::Allocation)?;
                if inventory.edges > MAX_EDGES || inventory.bytes > MAX_TOTAL_BYTES {
                    return Err(Error::Limit {
                        resource: "typed store strong pin edges",
                        limit: MAX_EDGES,
                        actual: inventory.edges,
                    });
                }
                located.push(Located {
                    directory: root_path,
                    bytes,
                    record,
                });
            }
        }
        located.sort_unstable_by(|left, right| left.directory.cmp(&right.directory));
        Ok((inventory, located))
    }

    fn admit(&self, edges: usize, bytes: usize) -> Result<(), Error> {
        let roots = self.roots.checked_add(1).ok_or(Error::Allocation)?;
        if roots > MAX_ROOTS {
            return Err(Error::Limit {
                resource: "typed store strong pin roots",
                limit: MAX_ROOTS,
                actual: roots,
            });
        }
        let edges = self.edges.checked_add(edges).ok_or(Error::Allocation)?;
        if edges > MAX_EDGES {
            return Err(Error::Limit {
                resource: "typed store strong pin edges",
                limit: MAX_EDGES,
                actual: edges,
            });
        }
        let bytes = self.bytes.checked_add(bytes).ok_or(Error::Allocation)?;
        if bytes > MAX_TOTAL_BYTES {
            return Err(Error::Limit {
                resource: "typed store strong pin bytes",
                limit: MAX_TOTAL_BYTES,
                actual: bytes,
            });
        }
        Ok(())
    }
}

impl Snapshot {

    fn read_with_recovery(
        _publication: &crate::store::lease::Publication<'_>,
        store: &Path,
    ) -> Result<Self, Error> {
        let (_, located) = Inventory::catalog(store)?;
        let mut protected = Vec::new();
        let mut stale = Vec::new();
        let mut active = Vec::new();
        protected
            .try_reserve_exact(MAX_EDGES)
            .map_err(|_| Error::Allocation)?;
        stale
            .try_reserve_exact(located.len())
            .map_err(|_| Error::Allocation)?;
        active
            .try_reserve_exact(located.len())
            .map_err(|_| Error::Allocation)?;
        for located in located {
            let path = located.directory.join("root");
            match open_snapshot(&path, &located.bytes)? {
                SnapshotLock::Active(file) => {
                    protected.extend_from_slice(&located.record.root.references);
                    active.push(Active {
                        project: located.record.project,
                        references: located.record.root.references,
                        _file: file,
                    });
                }
                SnapshotLock::Stale(file) => stale.push(Stale {
                    directory: located.directory,
                    project: located.record.project,
                    role: located.record.root.role,
                    owner: located.record.root.owner,
                    references: located.record.root.references,
                    charge: pin_charge(located.bytes.len())?,
                    _file: file,
                }),
            }
        }
        protected.sort_unstable();
        protected.dedup();
        let protected = protected.into_boxed_slice();
        let stale = stale.into_boxed_slice();
        let active = active.into_boxed_slice();
        Ok(Self {
            protected,
            stale,
            active,
        })
    }

    pub(crate) fn protected(&self) -> &[Reference] {
        &self.protected
    }

    pub(crate) fn stale(&self) -> &[Stale] {
        &self.stale
    }

    pub(crate) fn active(&self) -> &[Active] {
        &self.active
    }

    pub(super) fn into_stale(self) -> Box<[Stale]> {
        self.stale
    }
}

impl Stale {
    pub(crate) fn project(&self) -> Digest {
        self.project
    }

    pub(crate) fn role(&self) -> Role {
        self.role
    }

    pub(crate) fn owner(&self) -> Digest {
        self.owner.0
    }

    pub(crate) fn references(&self) -> &[Reference] {
        &self.references
    }

    pub(super) fn retire(
        self,
        publication: &crate::store::lease::Publication<'_>,
        store: &Path,
        budget: &mut super::super::recovery::Budget,
    ) -> Result<(), Error> {
        budget.deletion(self.charge)?;
        let temporary = super::super::entry::stage_retirement(publication, store, &self.directory)?;
        fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
            operation: "retire replaced typed store pin root",
            path: temporary.clone(),
            source,
        })?;
        super::super::entry::sync_directory(temporary.parent().ok_or(Error::State(
            "typed store pin root has no temporary directory",
        ))?)
    }
}

impl Active {
    pub(crate) fn project(&self) -> Digest {
        self.project
    }

    pub(crate) fn references(&self) -> &[Reference] {
        &self.references
    }
}

enum SnapshotLock {
    Active(Guard),
    Stale(Guard),
}

pub(super) fn recover_replaced(
    publication: &crate::store::lease::Publication<'_>,
    store: &Path,
    registrations: &[crate::store::accounting::Registered],
    budget: &mut super::super::recovery::Budget,
) -> Result<Snapshot, Error> {
    let snapshot = Snapshot::read_with_recovery(publication, store)?;
    let Snapshot {
        protected,
        stale,
        active,
    } = snapshot;
    let mut projects = BTreeMap::<Digest, Vec<Stale>>::new();
    for stale in stale {
        let values = projects.entry(stale.project).or_default();
        values.push(stale);
    }
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(projects.values().map(Vec::len).sum())
        .map_err(|_| Error::Allocation)?;
    for (project, stale) in projects {
        let owners = super::owners_recovery(store, project, registrations)?;
        for stale in stale {
            if owners.binary_search(&(stale.role, stale.owner)).is_err() {
                stale.retire(publication, store, budget)?;
            } else {
                retained.push(stale);
            }
        }
    }
    Ok(Snapshot {
        protected,
        stale: retained.into_boxed_slice(),
        active,
    })
}

fn pin_charge(bytes: usize) -> Result<Charge, Error> {
    let bytes = u64::try_from(bytes).map_err(|_| Error::Allocation)?;
    let file = bytes
        .checked_add(4095)
        .map(|bytes| bytes / 4096 * 4096)
        .ok_or(Error::Allocation)?;
    Ok(Charge {
        bytes: file.checked_add(4096).ok_or(Error::Allocation)?,
        allocations: 0,
        members: 2,
    })
}

fn validate_record_file(root: &Path, expected: &[u8]) -> Result<(), Error> {
    let observed = validate_one_member(root)?;
    let record = Record::decode(&observed).map_err(|_| Error::Corrupt {
        path: root.join("root"),
        reason: "typed store pin-root decoding failed",
    })?;
    if record.encode()?.as_ref() != expected || observed.as_ref() != expected {
        return Err(Error::Corrupt {
            path: root.join("root"),
            reason: "typed store pin-root conflicts with its owner identity",
        });
    }
    Ok(())
}

fn validate_one_member(root: &Path) -> Result<Box<[u8]>, Error> {
    let mut entries = fs::read_dir(root).map_err(|source| Error::Io {
        operation: "read typed store pin root",
        path: root.to_path_buf(),
        source,
    })?;
    let entry = entries
        .next()
        .transpose()
        .map_err(|source| Error::Io {
            operation: "read typed store pin-root member",
            path: root.to_path_buf(),
            source,
        })?
        .ok_or_else(|| Error::Corrupt {
            path: root.to_path_buf(),
            reason: "typed store pin root is empty",
        })?;
    if entry.file_name() != "root" || entries.next().is_some() {
        return Err(Error::Corrupt {
            path: root.to_path_buf(),
            reason: "typed store pin root has unexpected members",
        });
    }
    super::super::entry::read_file(&entry.path(), MAX_RECORD_BYTES)
}

fn open_shared(path: &Path, expected: &[u8]) -> Result<Guard, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store pin-root lock",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let mut file = File::open(path).map_err(|source| Error::Io {
        operation: "open typed store pin-root lock",
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockShared).map_err(
            |source| Error::Io {
                operation: "acquire typed store pin-root lock",
                path: path.to_path_buf(),
                source: std::io::Error::from_raw_os_error(source.raw_os_error()),
            },
        )?;
    }
    #[cfg(not(unix))]
    {
        return Err(Error::Io {
            operation: "acquire typed store pin-root lock",
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "typed store pin roots require Unix advisory locks",
            ),
        });
    }
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened typed store pin-root lock",
        path: path.to_path_buf(),
        source,
    })?;
    let current = fs::metadata(path).map_err(|source| Error::Io {
        operation: "reinspect typed store pin-root lock",
        path: path.to_path_buf(),
        source,
    })?;
    if hard_linked(&opened) || !same_file(&opened, &current) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let length = usize::try_from(opened.len()).unwrap_or(usize::MAX);
    if length > MAX_RECORD_BYTES {
        return Err(Error::Limit {
            resource: "typed store pin-root bytes",
            limit: MAX_RECORD_BYTES,
            actual: length,
        });
    }
    let mut observed = Vec::new();
    observed
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    (&mut file)
        .take((MAX_RECORD_BYTES as u64).saturating_add(1))
        .read_to_end(&mut observed)
        .map_err(|source| Error::Io {
            operation: "read locked typed store pin root",
            path: path.to_path_buf(),
            source,
        })?;
    if observed.len() != length || observed.as_slice() != expected {
        return Err(Error::Corrupt {
            path: path.to_path_buf(),
            reason: "typed store pin-root bytes changed while locking",
        });
    }
    Ok(Guard { _file: file })
}

fn open_snapshot(path: &Path, expected: &[u8]) -> Result<SnapshotLock, Error> {
    open_snapshot_with(path, expected, true)?
        .ok_or(Error::State("ordinary pin snapshot lost its active guard"))
}

fn open_snapshot_with(
    path: &Path,
    expected: &[u8],
    retain_active: bool,
) -> Result<Option<SnapshotLock>, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store pin-root snapshot",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let mut file = File::open(path).map_err(|source| Error::Io {
        operation: "open typed store pin-root snapshot",
        path: path.to_path_buf(),
        source,
    })?;
    let state = match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => false,
        Err(source) if source == rustix::io::Errno::WOULDBLOCK => {
            if !retain_active {
                return Ok(None);
            }
            rustix::fs::flock(&file, rustix::fs::FlockOperation::LockShared).map_err(
                |source| Error::Io {
                    operation: "retain active typed store pin-root snapshot",
                    path: path.to_path_buf(),
                    source: std::io::Error::from_raw_os_error(source.raw_os_error()),
                },
            )?;
            true
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "probe typed store pin-root snapshot",
                path: path.to_path_buf(),
                source: std::io::Error::from_raw_os_error(source.raw_os_error()),
            });
        }
    };
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened typed store pin-root snapshot",
        path: path.to_path_buf(),
        source,
    })?;
    let current = fs::metadata(path).map_err(|source| Error::Io {
        operation: "reinspect typed store pin-root snapshot",
        path: path.to_path_buf(),
        source,
    })?;
    if hard_linked(&opened) || !same_file(&opened, &current) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let length = usize::try_from(opened.len()).unwrap_or(usize::MAX);
    if length > MAX_RECORD_BYTES {
        return Err(Error::Limit {
            resource: "typed store pin-root bytes",
            limit: MAX_RECORD_BYTES,
            actual: length,
        });
    }
    let mut observed = Vec::new();
    observed
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    (&mut file)
        .take((MAX_RECORD_BYTES as u64).saturating_add(1))
        .read_to_end(&mut observed)
        .map_err(|source| Error::Io {
            operation: "read typed store pin-root snapshot",
            path: path.to_path_buf(),
            source,
        })?;
    if observed.len() != length || observed.as_slice() != expected {
        return Err(Error::Corrupt {
            path: path.to_path_buf(),
            reason: "typed store pin-root bytes changed during snapshot",
        });
    }
    if state {
        Ok(Some(SnapshotLock::Active(Guard { _file: file })))
    } else {
        Ok(Some(SnapshotLock::Stale(Guard { _file: file })))
    }
}

#[cfg(test)]
pub(in crate::store) fn fixture_counts(store: &Path) -> Result<(usize, usize), Error> {
    let mut total = 0usize;
    let mut active = 0usize;
    for role in [Role::Live, Role::LastGood, Role::Prepared, Role::InFlight] {
        let directory = store.join("pins").join(role.directory());
        for entry in fs::read_dir(&directory).map_err(|source| Error::Io {
            operation: "read typed store pin-role fixture directory",
            path: directory.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                operation: "read typed store pin-root fixture entry",
                path: directory.clone(),
                source,
            })?;
            total = total.checked_add(1).ok_or(Error::Allocation)?;
            let bytes = validate_one_member(&entry.path())?;
            Record::decode(&bytes)?;
            if independently_locked_path(&entry.path().join("root"))? {
                active = active.checked_add(1).ok_or(Error::Allocation)?;
            }
        }
    }
    Ok((total, active))
}

#[cfg(test)]
fn independently_locked_path(path: &Path) -> Result<bool, Error> {
    let file = File::open(path).map_err(|source| Error::Io {
        operation: "open typed store pin-root contention fixture",
        path: path.to_path_buf(),
        source,
    })?;
    match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(false),
        Err(source) if source == rustix::io::Errno::WOULDBLOCK => Ok(true),
        Err(source) => Err(Error::Io {
            operation: "probe typed store pin-root contention",
            path: path.to_path_buf(),
            source: std::io::Error::from_raw_os_error(source.raw_os_error()),
        }),
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    bytes
        .get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(Error::Codec("typed store pin-root is truncated"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(Error::Codec("typed store pin-root is truncated"))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], Error> {
    bytes
        .get(offset..offset + 32)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Error::Codec("typed store pin-root digest is truncated"))
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
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
