use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node::{Digest, Error};

use super::accounting::{self, Charge, LeaseKey, Reference};

pub(super) mod lock;
pub(crate) mod transaction;

pub(crate) use lock::Guard;

const MAGIC: &[u8; 8] = b"RAWRPRJT";
const SCHEMA: u16 = 1;
const HEADER_BYTES: usize = 88;
const RESERVATION_BYTES: usize = 64;
const ROOT_BYTES: usize = 80;
const MAX_LEDGER_BYTES: usize = 8 * 1024 * 1024;
const MAX_ROOTS: usize = 4;
const MAX_RUNNER_ROOTS: usize = 8;
const MAX_RESERVATIONS: usize = 1;
const MAX_EDGES: usize = 4_096;
pub(super) const MAX_PROJECT_LEDGERS: usize = 4_096;
const PROJECT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const PROJECT_ALLOCATIONS: u64 = 4_096;
const PROJECT_MEMBERS: u64 = 262_144;
const BLOCK_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub(crate) enum Role {
    Live = 1,
    LastGood = 2,
    Prepared = 3,
    InFlight = 4,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Owner(Digest);

#[derive(Clone)]
pub(crate) struct Reservation {
    key: LeaseKey,
    charge: Charge,
}

pub(crate) struct Capacity {
    ledger: Ledger,
    key: LeaseKey,
    target: PathBuf,
    length: usize,
}

impl Capacity {
    pub(super) fn plan(&self) -> super::control::Plan {
        super::control::Plan::replacement(&self.target, self.length)
    }

    pub(crate) fn reserve(
        mut self,
        admitted: super::control::Admitted<'_>,
        publication: &super::lease::Publication<'_>,
        root: &Path,
        charge: Charge,
    ) -> Result<Reservation, Error> {
        let key = self.key;
        let [reservation] = self.reservations_mut()? else {
            return Err(Error::State(
                "project capacity lost its exact pending reservation",
            ));
        };
        if reservation.key != key || reservation.charge != Charge::zero() {
            return Err(Error::State(
                "project capacity changed its pending reservation",
            ));
        }
        reservation.charge = charge;
        self.ledger.enforce_with_index(root)?;
        let bytes = self.ledger.encode()?;
        if bytes.len() != self.length {
            return Err(Error::State(
                "project reservation changed its admitted ledger length",
            ));
        }
        super::entry::replace_admitted_file(admitted, publication, root, &self.target, &bytes)?;
        Ok(Reservation {
            key: self.key,
            charge,
        })
    }

    fn reservations_mut(&mut self) -> Result<&mut [Reservation], Error> {
        if self.ledger.reservations.len() != 1 {
            return Err(Error::State(
                "project capacity changed its reservation cardinality",
            ));
        }
        Ok(self.ledger.reservations.as_mut_slice())
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct Root {
    role: Role,
    owner: Owner,
    proof: Digest,
    references: Box<[Reference]>,
}

#[derive(Clone)]
struct Ledger {
    project: Digest,
    reservations: Vec<Reservation>,
    roots: Vec<Root>,
}

impl Role {
    const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Live),
            2 => Some(Self::LastGood),
            3 => Some(Self::Prepared),
            4 => Some(Self::InFlight),
            _ => None,
        }
    }

    const fn directory(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::LastGood => "last-good",
            Self::Prepared => "prepared",
            Self::InFlight => "in-flight",
        }
    }
}

impl Owner {
    fn derive(project: Digest, role: Role, proof: Digest, references: &[Reference]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.store.pin-owner/v1");
        digest.update(project.as_bytes());
        digest.update([role as u8]);
        digest.update(proof.as_bytes());
        digest.update((references.len() as u64).to_le_bytes());
        for reference in references {
            reference.hash_into(&mut digest);
        }
        Self(Digest::from_bytes(digest.finalize().into()))
    }

    fn directory(self) -> String {
        self.0.directory()
    }
}

impl Root {
    fn new(project: Digest, role: Role, proof: Digest, references: Box<[Reference]>) -> Self {
        Self {
            role,
            owner: Owner::derive(project, role, proof, &references),
            proof,
            references,
        }
    }
}

impl Ledger {
    fn empty(project: Digest) -> Self {
        Self {
            project,
            reservations: Vec::new(),
            roots: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.reservations.is_empty() && self.roots.is_empty()
    }

    fn release(&mut self, role: Role, proof: Digest) -> Result<Root, Error> {
        let index = self
            .roots
            .binary_search_by_key(&role, |root| root.role)
            .map_err(|_| Error::State("project logical pin role disappeared before release"))?;
        if self.roots[index].proof != proof {
            return Err(Error::State(
                "project logical pin proof changed before release",
            ));
        }
        Ok(self.roots.remove(index))
    }

    fn read(root: &Path, project: Digest) -> Result<Self, Error> {
        let path = ledger_path(root, project);
        let bytes = super::entry::read_file(&path, MAX_LEDGER_BYTES)?;
        let ledger = Self::decode(&bytes).map_err(|_| Error::Corrupt {
            path,
            reason: "project logical ledger decoding failed",
        })?;
        if ledger.project != project {
            return Err(Error::Corrupt {
                path: ledger_path(root, project),
                reason: "project logical ledger identity changed",
            });
        }
        for pinned in &ledger.roots {
            accounting::validate_references(root, &pinned.references)?;
        }
        Ok(ledger)
    }

    fn read_or_empty(root: &Path, project: Digest) -> Result<Self, Error> {
        let path = ledger_path(root, project);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(Error::Shape(path))
            }
            Ok(_) => Self::read(root, project),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self::empty(project))
            }
            Err(source) => Err(Error::Io {
                operation: "inspect project logical ledger",
                path,
                source,
            }),
        }
    }

    fn read_or_empty_recovery(
        root: &Path,
        project: Digest,
        registrations: &[accounting::Registered],
    ) -> Result<Self, Error> {
        let path = ledger_path(root, project);
        let ledger = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(Error::Shape(path));
            }
            Ok(_) => {
                let bytes = super::entry::read_file(&path, MAX_LEDGER_BYTES)?;
                Self::decode(&bytes).map_err(|_| Error::Corrupt {
                    path: path.clone(),
                    reason: "project logical ledger decoding failed",
                })?
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Self::empty(project),
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect project logical ledger",
                    path,
                    source,
                });
            }
        };
        if ledger.project != project {
            return Err(Error::Corrupt {
                path: ledger_path(root, project),
                reason: "project logical ledger identity changed",
            });
        }
        for pinned in &ledger.roots {
            accounting::validate_reference_set(registrations, &pinned.references)?;
        }
        Ok(ledger)
    }

    fn reserve(&mut self, key: LeaseKey, charge: Charge) -> Result<(), Error> {
        if !self.reservations.is_empty() {
            return Err(Error::CleanupRequired(
                "an earlier project logical reservation requires recovery",
            ));
        }
        self.reservations.push(Reservation { key, charge });
        self.enforce().map(|_| ())
    }

    fn cancel(&mut self, reservation: &Reservation) -> Result<(), Error> {
        let Some(index) = self.reservations.iter().position(|current| {
            current.key == reservation.key && current.charge == reservation.charge
        }) else {
            return Err(Error::State(
                "project logical reservation disappeared before completion",
            ));
        };
        self.reservations.remove(index);
        Ok(())
    }

    fn bind(
        &mut self,
        role: Role,
        proof: Digest,
        references: Box<[Reference]>,
    ) -> Result<Root, Error> {
        let pinned = Root::new(self.project, role, proof, references);
        match self
            .roots
            .binary_search_by_key(&role, |current| current.role)
        {
            Ok(index) if self.roots[index] == pinned => return Ok(pinned),
            Ok(index) => self.roots[index] = pinned.clone(),
            Err(index) => self.roots.insert(index, pinned.clone()),
        }
        self.enforce()?;
        Ok(pinned)
    }

    fn enforce(&self) -> Result<Charge, Error> {
        if self.roots.len() > MAX_ROOTS
            || self.reservations.len() > MAX_RESERVATIONS
            || self
                .roots
                .windows(2)
                .any(|pair| pair[0].role >= pair[1].role)
        {
            return Err(Error::State("project logical ledger ordering changed"));
        }
        self.charge(&self.roots)
    }

    fn charge(&self, roots: &[Root]) -> Result<Charge, Error> {
        let edges = roots
            .iter()
            .try_fold(0usize, |total, pinned| {
                total.checked_add(pinned.references.len())
            })
            .ok_or(Error::Allocation)?;
        if edges > MAX_EDGES {
            return Err(Error::Limit {
                resource: "typed store project pin edges",
                limit: MAX_EDGES,
                actual: edges,
            });
        }
        let mut references = Vec::new();
        references
            .try_reserve_exact(edges)
            .map_err(|_| Error::Allocation)?;
        for pinned in roots {
            if pinned.references.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(Error::Codec(
                    "project logical root references changed order",
                ));
            }
            if pinned.owner
                != Owner::derive(self.project, pinned.role, pinned.proof, &pinned.references)
            {
                return Err(Error::Codec("project logical pin owner changed"));
            }
            references.extend_from_slice(&pinned.references);
        }
        references.sort_unstable();
        references.dedup();
        let runner_roots = references
            .iter()
            .filter(|reference| reference.is_runner_root())
            .count();
        if runner_roots > MAX_RUNNER_ROOTS {
            return Err(Error::Limit {
                resource: "typed store project retained runner roots",
                limit: MAX_RUNNER_ROOTS,
                actual: runner_roots,
            });
        }
        let mut charge = references
            .iter()
            .try_fold(Charge::zero(), |total, reference| {
                total.checked_add(reference.charge())
            })?;
        charge = self
            .reservations
            .iter()
            .try_fold(charge, |total, reservation| {
                total.checked_add(reservation.charge)
            })?;
        let metadata_allocations = u64::try_from(roots.len())
            .unwrap_or(u64::MAX)
            .saturating_add(2);
        let length = HEADER_BYTES
            .checked_add(self.reservations.len().saturating_mul(RESERVATION_BYTES))
            .and_then(|length| length.checked_add(roots.len().saturating_mul(ROOT_BYTES)))
            .and_then(|length| length.checked_add(edges.saturating_mul(Reference::encoded_bytes())))
            .filter(|length| *length <= MAX_LEDGER_BYTES)
            .ok_or(Error::Allocation)?;
        charge = charge.checked_add(Charge {
            bytes: round_block(u64::try_from(length).unwrap_or(u64::MAX))?
                .checked_add(metadata_allocations.saturating_sub(1) * BLOCK_BYTES)
                .ok_or(Error::Allocation)?,
            allocations: metadata_allocations,
            members: metadata_allocations,
        })?;
        Ok(charge)
    }

    fn enforce_with_index(&self, root: &Path) -> Result<Charge, Error> {
        self.enforce()?
            .checked_add(super::index::project_charge(root, self.project)?)
    }

    fn encoded_length(&self, edges: usize) -> Result<usize, Error> {
        HEADER_BYTES
            .checked_add(
                self.reservations
                    .len()
                    .checked_mul(RESERVATION_BYTES)
                    .ok_or(Error::Allocation)?,
            )
            .and_then(|length| length.checked_add(self.roots.len().checked_mul(ROOT_BYTES)?))
            .and_then(|length| length.checked_add(edges.checked_mul(Reference::encoded_bytes())?))
            .filter(|length| *length <= MAX_LEDGER_BYTES)
            .ok_or(Error::Limit {
                resource: "typed store project ledger bytes",
                limit: MAX_LEDGER_BYTES,
                actual: usize::MAX,
            })
    }

    fn encode(&self) -> Result<Box<[u8]>, Error> {
        let edges = self
            .roots
            .iter()
            .try_fold(0usize, |total, pinned| {
                total.checked_add(pinned.references.len())
            })
            .ok_or(Error::Allocation)?;
        let length = self.encoded_length(edges)?;
        let charge = self.enforce()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&SCHEMA.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&self.project.as_bytes());
        bytes.extend_from_slice(&(self.roots.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.reservations.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(edges as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&charge.bytes.to_le_bytes());
        bytes.extend_from_slice(&charge.allocations.to_le_bytes());
        bytes.extend_from_slice(&charge.members.to_le_bytes());
        for reservation in &self.reservations {
            bytes.push(reservation.key.code());
            bytes.extend_from_slice(&[0; 7]);
            bytes.extend_from_slice(&reservation.key.bytes());
            bytes.extend_from_slice(&reservation.charge.bytes.to_le_bytes());
            bytes.extend_from_slice(&reservation.charge.allocations.to_le_bytes());
            bytes.extend_from_slice(&reservation.charge.members.to_le_bytes());
        }
        for pinned in &self.roots {
            bytes.push(pinned.role as u8);
            bytes.extend_from_slice(&[0; 7]);
            bytes.extend_from_slice(&pinned.owner.0.as_bytes());
            bytes.extend_from_slice(&pinned.proof.as_bytes());
            bytes.extend_from_slice(&(pinned.references.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&[0; 4]);
            for reference in &pinned.references {
                reference.encode(&mut bytes);
            }
        }
        debug_assert_eq!(bytes.len(), length);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_BYTES
            || &bytes[..8] != MAGIC
            || u16_at(bytes, 8)? != SCHEMA
            || bytes[10..16].iter().any(|byte| *byte != 0)
            || bytes[60..64].iter().any(|byte| *byte != 0)
        {
            return Err(Error::Codec("project logical ledger header changed"));
        }
        let project = Digest::from_bytes(array_at(bytes, 16)?);
        let root_count = usize::try_from(u32_at(bytes, 48)?)
            .map_err(|_| Error::Codec("project logical root count is not addressable"))?;
        let reservation_count = usize::try_from(u32_at(bytes, 52)?)
            .map_err(|_| Error::Codec("project logical reservation count is not addressable"))?;
        let edge_count = usize::try_from(u32_at(bytes, 56)?)
            .map_err(|_| Error::Codec("project logical edge count is not addressable"))?;
        if root_count > MAX_ROOTS || reservation_count > MAX_RESERVATIONS || edge_count > MAX_EDGES
        {
            return Err(Error::Codec("project logical ledger count changed"));
        }
        let retained = Charge {
            bytes: u64_at(bytes, 64)?,
            allocations: u64_at(bytes, 72)?,
            members: u64_at(bytes, 80)?,
        };
        let mut offset = HEADER_BYTES;
        let mut reservations = Vec::new();
        reservations
            .try_reserve_exact(reservation_count)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..reservation_count {
            if bytes
                .get(offset + 1..offset + 8)
                .ok_or(Error::Codec("project logical reservation is truncated"))?
                .iter()
                .any(|byte| *byte != 0)
            {
                return Err(Error::Codec("project logical reservation padding changed"));
            }
            reservations.push(Reservation {
                key: LeaseKey::decode(
                    *bytes
                        .get(offset)
                        .ok_or(Error::Codec("project logical reservation is truncated"))?,
                    array_at(bytes, offset + 8)?,
                )?,
                charge: Charge {
                    bytes: u64_at(bytes, offset + 40)?,
                    allocations: u64_at(bytes, offset + 48)?,
                    members: u64_at(bytes, offset + 56)?,
                },
            });
            offset += RESERVATION_BYTES;
        }
        let mut roots = Vec::new();
        roots
            .try_reserve_exact(root_count)
            .map_err(|_| Error::Allocation)?;
        let mut observed_edges = 0usize;
        for _ in 0..root_count {
            if bytes
                .get(offset + 1..offset + 8)
                .ok_or(Error::Codec("project logical root is truncated"))?
                .iter()
                .any(|byte| *byte != 0)
                || bytes
                    .get(offset + 76..offset + 80)
                    .ok_or(Error::Codec("project logical root is truncated"))?
                    .iter()
                    .any(|byte| *byte != 0)
            {
                return Err(Error::Codec("project logical root padding changed"));
            }
            let role = Role::from_code(
                *bytes
                    .get(offset)
                    .ok_or(Error::Codec("project logical root is truncated"))?,
            )
            .ok_or(Error::Codec("project logical pin role changed"))?;
            let owner = Owner(Digest::from_bytes(array_at(bytes, offset + 8)?));
            let proof = Digest::from_bytes(array_at(bytes, offset + 40)?);
            let count = usize::try_from(u32_at(bytes, offset + 72)?)
                .map_err(|_| Error::Codec("project logical edge count is not addressable"))?;
            observed_edges = observed_edges.checked_add(count).ok_or(Error::Allocation)?;
            if observed_edges > edge_count {
                return Err(Error::Codec("project logical edge count changed"));
            }
            offset += ROOT_BYTES;
            let mut references = Vec::new();
            references
                .try_reserve_exact(count)
                .map_err(|_| Error::Allocation)?;
            for _ in 0..count {
                references.push(Reference::decode(bytes, offset)?);
                offset += Reference::encoded_bytes();
            }
            roots.push(Root {
                role,
                owner,
                proof,
                references: references.into_boxed_slice(),
            });
        }
        if observed_edges != edge_count || offset != bytes.len() {
            return Err(Error::Codec("project logical ledger length changed"));
        }
        let ledger = Self {
            project,
            reservations,
            roots,
        };
        if ledger.enforce()? != retained || ledger.encode()?.as_ref() != bytes {
            return Err(Error::Codec("project logical ledger is not canonical"));
        }
        Ok(ledger)
    }
}

pub(crate) fn reserve(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    key: LeaseKey,
    charge: Charge,
) -> Result<Reservation, Error> {
    let mut ledger = Ledger::read_or_empty(root, project)?;
    ledger.reserve(key, charge)?;
    ledger.enforce_with_index(root)?;
    save(publication, root, &ledger)?;
    Ok(Reservation { key, charge })
}

pub(crate) fn available(root: &Path, project: Digest, key: LeaseKey) -> Result<Capacity, Error> {
    let mut ledger = Ledger::read_or_empty(root, project)?;
    if !ledger.reservations.is_empty() {
        return Err(Error::CleanupRequired(
            "an earlier project logical reservation requires recovery",
        ));
    }
    ledger.reservations.push(Reservation {
        key,
        charge: Charge::zero(),
    });
    ledger.enforce_with_index(root)?;
    let target = ledger_path(root, project);
    let length = ledger.encode()?.len();
    Ok(Capacity {
        ledger,
        key,
        target,
        length,
    })
}

pub(crate) fn cancel(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    reservation: &Reservation,
) -> Result<(), Error> {
    let mut ledger = Ledger::read(root, project)?;
    ledger.cancel(reservation)?;
    save_or_retire(publication, root, &ledger)
}

pub(crate) fn release_in_flight(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    proof: Digest,
) -> Result<(), Error> {
    let mut ledger = Ledger::read(root, project)?;
    let released = ledger.release(Role::InFlight, proof)?;
    save_or_retire(publication, root, &ledger)?;
    lock::retire_released(publication, root, project, &released)
}

pub(crate) fn recover(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    current: LeaseKey,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let registrations = accounting::registrations_recovery(root, budget)?;
    lock::recover_replaced(publication, root, &registrations, budget)?;
    let mut ledger = Ledger::read_or_empty_recovery(root, project, &registrations)?;
    let Some(existing) = ledger.reservations.first() else {
        return if ledger.is_empty() {
            save_or_retire(publication, root, &ledger)
        } else {
            Ok(())
        };
    };
    if existing.key != current && accounting::has_reservation_recovery(root, existing.key, budget)?
    {
        return Err(Error::CleanupRequired(
            "another project mutation still has an installation reservation",
        ));
    }
    ledger.reservations.clear();
    save_or_retire(publication, root, &ledger)
}

#[cfg(test)]
pub(super) fn recover_abandoned(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let registrations = accounting::registrations_recovery(root, budget)?;
    let live = accounting::reservation_keys_recovery(root, budget)?;
    let directory = root.join("ledgers/projects");
    let mut projects = Vec::new();
    projects
        .try_reserve_exact(MAX_PROJECT_LEDGERS)
        .map_err(|_| Error::Allocation)?;
    for entry in fs::read_dir(&directory).map_err(|source| Error::Io {
        operation: "read project ledgers during reservation recovery",
        path: directory.clone(),
        source,
    })? {
        if projects.len() == MAX_PROJECT_LEDGERS {
            return Err(Error::Limit {
                resource: "typed store installation project ledgers",
                limit: MAX_PROJECT_LEDGERS,
                actual: MAX_PROJECT_LEDGERS + 1,
            });
        }
        let entry = entry.map_err(|source| Error::Io {
            operation: "read project ledger during reservation recovery",
            path: directory.clone(),
            source,
        })?;
        let metadata = entry.file_type().map_err(|source| Error::Io {
            operation: "inspect project ledger during reservation recovery",
            path: entry.path(),
            source,
        })?;
        if metadata.is_symlink() || !metadata.is_file() {
            return Err(Error::Shape(entry.path()));
        }
        projects.push(
            project_from_name(&entry.file_name()).ok_or(Error::CleanupRequired(
                "a project logical ledger changed canonical placement",
            ))?,
        );
    }
    projects.sort_unstable();
    for project in projects {
        let mut ledger = Ledger::read_or_empty_recovery(root, project, &registrations)?;
        let before = ledger.reservations.len();
        ledger
            .reservations
            .retain(|reservation| live.binary_search(&reservation.key).is_ok());
        if ledger.reservations.len() != before || ledger.is_empty() {
            save_or_retire(publication, root, &ledger)?;
        }
    }
    Ok(())
}

pub(super) fn recover_empty_before_new_project(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let target = ledger_path(root, project);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(Error::Shape(target));
        }
        Ok(_) => return Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect project ledger before project admission",
                path: target,
                source,
            });
        }
    }

    let directory = root.join("ledgers/projects");
    let mut projects = Vec::new();
    projects
        .try_reserve_exact(MAX_PROJECT_LEDGERS)
        .map_err(|_| Error::Allocation)?;
    for entry in fs::read_dir(&directory).map_err(|source| Error::Io {
        operation: "read project ledgers before project admission",
        path: directory.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            operation: "read project ledger before project admission",
            path: directory.clone(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| Error::Io {
            operation: "inspect project ledger before project admission",
            path: entry.path(),
            source,
        })?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(Error::Shape(entry.path()));
        }
        let length = entry.metadata().map_err(|source| Error::Io {
            operation: "measure project ledger before project admission",
            path: entry.path(),
            source,
        })?;
        if projects.len() == MAX_PROJECT_LEDGERS {
            return Err(Error::Limit {
                resource: "typed store installation project ledgers",
                limit: MAX_PROJECT_LEDGERS,
                actual: MAX_PROJECT_LEDGERS + 1,
            });
        }
        let project = project_from_name(&entry.file_name()).ok_or(Error::CleanupRequired(
            "a project logical ledger changed canonical placement",
        ))?;
        projects.push((project, length.len()));
    }
    if projects.len() == MAX_PROJECT_LEDGERS {
        let live = accounting::reservation_keys_header_recovery(root)?;
        projects.sort_unstable();
        for (project, length) in projects {
            if length != HEADER_BYTES as u64 && length != (HEADER_BYTES + RESERVATION_BYTES) as u64
            {
                continue;
            }
            let path = ledger_path(root, project);
            let bytes = super::entry::read_file(&path, length as usize)?;
            let mut ledger = Ledger::decode(&bytes).map_err(|_| Error::Corrupt {
                path: path.clone(),
                reason: "reclaimable project logical ledger decoding failed",
            })?;
            if ledger.project != project || !ledger.roots.is_empty() {
                return Err(Error::Corrupt {
                    path,
                    reason: "reclaimable project logical ledger identity changed",
                });
            }
            if ledger
                .reservations
                .first()
                .is_some_and(|reservation| live.binary_search(&reservation.key).is_ok())
            {
                continue;
            }
            ledger.reservations.clear();
            budget.deletion(super::recovery::tree_charge(&path, 1)?)?;
            save_or_retire(publication, root, &ledger)?;
            return Ok(());
        }
        return Err(Error::Limit {
            resource: "typed store installation project ledgers",
            limit: MAX_PROJECT_LEDGERS,
            actual: MAX_PROJECT_LEDGERS + 1,
        });
    }
    Ok(())
}

pub(super) fn recover_reservation_only(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    key: LeaseKey,
) -> Result<(), Error> {
    let directory = root.join("ledgers/projects");
    let mut candidates = Vec::new();
    candidates
        .try_reserve_exact(MAX_PROJECT_LEDGERS)
        .map_err(|_| Error::Allocation)?;
    let mut seen = 0usize;
    for entry in fs::read_dir(&directory).map_err(|source| Error::Io {
        operation: "read project ledgers during targeted reservation recovery",
        path: directory.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            operation: "read project ledger during targeted reservation recovery",
            path: directory.clone(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| Error::Io {
            operation: "inspect project ledger during targeted reservation recovery",
            path: entry.path(),
            source,
        })?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(Error::Shape(entry.path()));
        }
        seen = seen.checked_add(1).ok_or(Error::Allocation)?;
        if seen > MAX_PROJECT_LEDGERS {
            return Err(Error::Limit {
                resource: "typed store installation project ledgers",
                limit: MAX_PROJECT_LEDGERS,
                actual: MAX_PROJECT_LEDGERS + 1,
            });
        }
        let length = entry
            .metadata()
            .map_err(|source| Error::Io {
                operation: "measure project ledger during targeted reservation recovery",
                path: entry.path(),
                source,
            })?
            .len();
        let project = project_from_name(&entry.file_name()).ok_or(Error::CleanupRequired(
            "a project logical ledger changed canonical placement",
        ))?;
        if length == (HEADER_BYTES + RESERVATION_BYTES) as u64 {
            candidates.push(project);
        }
    }
    candidates.sort_unstable();
    for project in candidates {
        let path = ledger_path(root, project);
        let mut source = fs::File::open(&path).map_err(|source| Error::Io {
            operation: "open reservation-only project logical ledger",
            path: path.clone(),
            source,
        })?;
        source
            .seek(SeekFrom::Start(HEADER_BYTES as u64))
            .map_err(|source| Error::Io {
                operation: "seek reservation-only project logical ledger",
                path: path.clone(),
                source,
            })?;
        let mut reservation = [0_u8; 40];
        source
            .read_exact(&mut reservation)
            .map_err(|source| Error::Io {
                operation: "read reservation-only project logical ledger key",
                path: path.clone(),
                source,
            })?;
        if reservation[1..8].iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt {
                path,
                reason: "reservation-only project logical ledger padding changed",
            });
        }
        let observed = LeaseKey::decode(reservation[0], array_at(&reservation, 8)?)?;
        if observed != key {
            continue;
        }
        let bytes = super::entry::read_file(&path, HEADER_BYTES + RESERVATION_BYTES)?;
        let mut ledger = Ledger::decode(&bytes).map_err(|_| Error::Corrupt {
            path: path.clone(),
            reason: "reservation-only project logical ledger decoding failed",
        })?;
        if ledger.project != project || !ledger.roots.is_empty() {
            return Err(Error::Corrupt {
                path,
                reason: "reservation-only project logical ledger identity changed",
            });
        }
        if ledger
            .reservations
            .first()
            .is_some_and(|reservation| reservation.key == key)
        {
            ledger.reservations.clear();
            return save_or_retire(publication, root, &ledger);
        }
    }
    Ok(())
}

pub(crate) fn snapshot(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    registrations: &[accounting::Registered],
    budget: &mut super::recovery::Budget,
) -> Result<lock::Snapshot, Error> {
    lock::recover_replaced(publication, root, registrations, budget)
}

fn owners_recovery(
    root: &Path,
    project: Digest,
    registrations: &[accounting::Registered],
) -> Result<Vec<(Role, Owner)>, Error> {
    let ledger = Ledger::read_or_empty_recovery(root, project, registrations)?;
    Ok(ledger
        .roots
        .iter()
        .map(|root| (root.role, root.owner))
        .collect())
}

pub(crate) fn retire_project_runner(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    snapshot: lock::Snapshot,
    project: Digest,
    runner: Reference,
    registrations: &[accounting::Registered],
    budget: &mut super::recovery::Budget,
) -> Result<bool, Error> {
    if !runner.is_runner_root() {
        return Err(Error::State("pin retirement requires one runner root"));
    }
    if snapshot.active().iter().any(|active| {
        active.project() == project && active.references().binary_search(&runner).is_ok()
    }) {
        return Err(Error::CleanupRequired(
            "an active OS pin still owns the selected runner root",
        ));
    }
    let physically_unreferenced = snapshot.protected().binary_search(&runner).is_err()
        && !snapshot.stale().iter().any(|stale| {
            stale.project() != project && stale.references().binary_search(&runner).is_ok()
        });
    let selected = snapshot
        .stale()
        .iter()
        .filter(|stale| {
            stale.project() == project && stale.references().binary_search(&runner).is_ok()
        })
        .map(|stale| (stale.role(), stale.owner()))
        .collect::<Vec<_>>();
    if !selected.is_empty() {
        let mut ledger = Ledger::read_or_empty_recovery(root, project, registrations)?;
        for (role, owner) in &selected {
            let index = ledger
                .roots
                .binary_search_by_key(role, |current| current.role)
                .map_err(|_| {
                    Error::CleanupRequired("a stale physical pin root left its project ledger")
                })?;
            if ledger.roots[index].owner.0 != *owner {
                return Err(Error::CleanupRequired(
                    "a stale physical pin root changed project ownership",
                ));
            }
            ledger.roots.remove(index);
        }
        ledger.enforce_with_index(root)?;
        save_or_retire(publication, root, &ledger)?;
    }
    for stale in snapshot.into_stale() {
        if stale.project() == project && stale.references().binary_search(&runner).is_ok() {
            stale.retire(publication, root, budget)?;
        }
    }
    Ok(physically_unreferenced)
}

#[cfg(test)]
pub(crate) fn usage(root: &Path, project: Digest) -> Result<(Charge, usize, usize), Error> {
    let ledger = Ledger::read_or_empty(root, project)?;
    Ok((
        ledger.enforce_with_index(root)?,
        ledger.reservations.len(),
        ledger.roots.len(),
    ))
}

/// The project size target is an eviction trigger, never a refusal.
pub(super) fn over_cap(root: &Path, project: Digest, additional: Charge) -> Result<bool, Error> {
    let charge = Ledger::read_or_empty(root, project)?
        .enforce_with_index(root)?
        .checked_add(additional)?;
    Ok(charge.bytes > PROJECT_BYTES
        || charge.allocations > PROJECT_ALLOCATIONS
        || charge.members > PROJECT_MEMBERS)
}

fn save(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: &Ledger,
) -> Result<(), Error> {
    save_bytes(publication, root, ledger.project, &ledger.encode()?)
}

fn save_or_retire(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: &Ledger,
) -> Result<(), Error> {
    if !ledger.is_empty() {
        return save(publication, root, ledger);
    }
    let target = ledger_path(root, ledger.project);

    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(Error::Shape(target))
        }
        Ok(_) => {
            let temporary = super::entry::stage_retirement(publication, root, &target)?;
            fs::remove_file(&temporary).map_err(|source| Error::Io {
                operation: "retire empty project logical ledger",
                path: temporary.clone(),
                source,
            })?;
            super::entry::sync_directory(temporary.parent().ok_or(Error::State(
                "empty project logical ledger has no temporary directory",
            ))?)
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            operation: "inspect empty project logical ledger",
            path: target,
            source,
        }),
    }
}

fn save_bytes(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    project: Digest,
    bytes: &[u8],
) -> Result<(), Error> {
    let target = ledger_path(root, project);
    let admitted = super::accounting::admit_control(
        publication,
        root,
        super::control::Plan::replacement(&target, bytes.len()),
    )?;
    super::entry::replace_admitted_file(admitted, publication, root, &target, bytes)
}

fn ledger_path(root: &Path, project: Digest) -> PathBuf {
    root.join("ledgers/projects").join(project.directory())
}

fn project_from_name(name: &std::ffi::OsStr) -> Option<Digest> {
    let bytes = name.as_encoded_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let high = decode_nibble(pair[0])?;
        let low = decode_nibble(pair[1])?;
        digest[index] = high.checked_mul(16)?.checked_add(low)?;
    }
    Some(Digest::from_bytes(digest))
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn round_block(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK_BYTES - 1)
        .map(|bytes| bytes / BLOCK_BYTES * BLOCK_BYTES)
        .ok_or(Error::Limit {
            resource: "typed store project rounded logical bytes",
            limit: usize::try_from(PROJECT_BYTES).unwrap_or(usize::MAX),
            actual: usize::MAX,
        })
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    bytes
        .get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(Error::Codec("project logical ledger is truncated"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(Error::Codec("project logical ledger is truncated"))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(Error::Codec("project logical ledger is truncated"))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], Error> {
    bytes
        .get(offset..offset + 32)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Error::Codec("project logical ledger digest is truncated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_store(label: &str) -> crate::store::Store {
        let root =
            std::env::temp_dir().join(format!("rawr-store-pins-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the pin fixture root exists");
        crate::store::Store::open(&root, [0x95; 32]).expect("the pin fixture store opens")
    }

    struct Artifact(u8);

    impl crate::node::Value for Artifact {
        const KIND: crate::node::Kind = crate::node::Kind::NativeProduct;
        const SCHEMA: u32 = 1;

        fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
            Ok(vec![self.0].into_boxed_slice())
        }

        fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
            match bytes {
                [value] => Ok(Self(*value)),
                _ => Err(Error::Codec("project-cap fixture content changed")),
            }
        }
    }

    fn fill_project_ledgers(store: &crate::store::Store, reclaim_last: bool) -> PathBuf {
        let mut last = PathBuf::new();
        for index in 0..MAX_PROJECT_LEDGERS {
            let mut identity = [0_u8; 32];
            identity[24..].copy_from_slice(&(index as u64).to_be_bytes());
            let project = Digest::from_bytes(identity);
            let mut ledger = Ledger::empty(project);
            if !reclaim_last || index + 1 < MAX_PROJECT_LEDGERS {
                ledger.roots.push(Root::new(
                    project,
                    Role::Live,
                    Digest::from_bytes([0x4a; 32]),
                    Box::new([]),
                ));
            }
            last = ledger_path(store.root(), project);
            fs::write(&last, ledger.encode().unwrap()).unwrap();
        }
        last
    }

    #[test]
    fn first_parentless_reservation_reclaims_the_last_sorted_ledger_at_the_exact_cap() {
        let store = fixture_store("first-parentless-ledger-cap");
        let last = fill_project_ledgers(&store, true);
        let recipe = crate::node::Direct::<Artifact>::capture(
            [0x32; 32],
            b"first-parentless",
            4_096,
            &[],
            1,
        )
        .unwrap();
        let value = crate::node::Evaluator::new(&store, &crate::metrics::Noop)
            .direct(&recipe, || {
                assert!(!last.exists(), "recovery precedes the transform");
                assert!(store.fixture_project_ledger_exists());
                assert_eq!(
                    fs::read_dir(store.root().join("ledgers/projects"))
                        .unwrap()
                        .count(),
                    MAX_PROJECT_LEDGERS
                );
                Ok(Artifact(7))
            })
            .expect("the first parentless node reclaims the final empty ledger before reserving");
        assert_eq!(value.value().0, 7);
        assert!(!last.exists());
        assert!(
            !store.fixture_project_ledger_exists(),
            "publication cancels its temporary project reservation"
        );
        assert_eq!(
            fs::read_dir(store.root().join("ledgers/projects"))
                .unwrap()
                .count(),
            MAX_PROJECT_LEDGERS - 1
        );
        store.retire_fixture().unwrap();
    }

    #[test]
    fn first_parentless_reservation_rejects_a_full_ledger_set_without_transform_work() {
        let store = fixture_store("first-parentless-no-reclaim");
        fill_project_ledgers(&store, false);
        let recipe = crate::node::Direct::<Artifact>::capture(
            [0x32; 32],
            b"first-parentless-no-reclaim",
            4_096,
            &[],
            1,
        )
        .unwrap();
        let before = fs::read(store.root().join("ledgers/installation")).unwrap();
        let result = crate::node::Evaluator::new(&store, &crate::metrics::Noop)
            .direct(&recipe, || {
                panic!("the 4097th project must reject before its transform")
            });
        assert!(matches!(result, Err(Error::Limit {
            resource: "typed store installation project ledgers",
            limit: MAX_PROJECT_LEDGERS,
            actual,
        }) if actual == MAX_PROJECT_LEDGERS + 1));
        assert!(!store.fixture_project_ledger_exists());
        assert_eq!(
            fs::read_dir(store.root().join("ledgers/projects"))
                .unwrap()
                .count(),
            MAX_PROJECT_LEDGERS
        );
        assert_eq!(
            fs::read(store.root().join("ledgers/installation")).unwrap(),
            before
        );
        store.retire_fixture().unwrap();
    }

    #[test]
    fn first_parentless_bundle_reservation_obeys_the_same_exact_project_ledger_cap() {
        for reclaim_last in [false, true] {
            let store = fixture_store(if reclaim_last {
                "bundle-ledger-reclaim"
            } else {
                "bundle-ledger-full"
            });
            let last = fill_project_ledgers(&store, reclaim_last);
            let recipe = crate::node::Discoverable::<Artifact>::capture(
                [0x32; 32],
                b"first-parentless-bundle",
                4_096,
                &[],
                1,
            )
            .unwrap();
            let before = fs::read(store.root().join("ledgers/installation")).unwrap();
            let result = store.with_ownership(|ownership| {
                store.with_heavy_lease(|| {
                    store.with_execution_lease(recipe.execution(), || {
                        store.with_execution_bundle_reservation(
                            ownership,
                            &recipe,
                            None,
                            |attempt| {
                                assert!(
                                    reclaim_last,
                                    "the 4097th project must reject before bundle execution"
                                );
                                assert!(!last.exists());
                                assert!(store.fixture_project_ledger_exists());
                                assert_eq!(
                                    fs::read_dir(store.root().join("ledgers/projects"))
                                        .unwrap()
                                        .count(),
                                    MAX_PROJECT_LEDGERS
                                );
                                drop(attempt);
                                Ok(crate::store::Reserved::Reuse(()))
                            },
                        )
                    })
                })
            });
            if reclaim_last {
                result.expect("the bundle reservation reclaims the last empty ledger");
                assert!(!last.exists());
            } else {
                assert!(matches!(result, Err(Error::Limit {
                    resource: "typed store installation project ledgers",
                    limit: MAX_PROJECT_LEDGERS,
                    actual,
                }) if actual == MAX_PROJECT_LEDGERS + 1));
                assert!(last.exists());
            }
            assert!(!store.fixture_project_ledger_exists());
            assert_eq!(
                fs::read(store.root().join("ledgers/installation")).unwrap(),
                before
            );
            store.retire_fixture().unwrap();
        }
    }

    #[test]
    fn new_project_recovery_precharges_empty_ledger_deletion_in_the_same_pass() {
        for floor in [false, true] {
            let store = fixture_store(if floor {
                "empty-ledger-deletion-floor"
            } else {
                "empty-ledger-deletion-cap"
            });
            let last = fill_project_ledgers(&store, true);
            let charge = super::super::recovery::tree_charge(&last, 1).unwrap();
            let mut budget = super::super::recovery::Budget::new();
            budget
                .deletion(Charge {
                    bytes: 8 * 1024 * 1024 * 1024 - charge.bytes + u64::from(floor),
                    allocations: 0,
                    members: 262_144 - charge.members,
                })
                .unwrap();
            let result = super::super::lease::with_publication(store.root(), |publication| {
                recover_empty_before_new_project(
                    publication,
                    store.root(),
                    store.project,
                    &mut budget,
                )
            });
            // One byte past the pass's deletion target still retires.
            result.expect("the remaining deletion charge never refuses");
            assert!(!last.exists());
            store.retire_fixture().unwrap();
        }
    }

    #[test]
    fn abandoned_recovery_retires_an_existing_empty_project_ledger() {
        let store = fixture_store("empty-project-ledger");
        let project = Digest::from_bytes([0x96; 32]);
        super::super::lease::with_publication(store.root(), |publication| {
            save(publication, store.root(), &Ledger::empty(project))
        })
        .expect("the empty project ledger is staged");
        let path = ledger_path(store.root(), project);
        assert!(path.is_file());

        super::super::lease::with_publication(store.root(), |publication| {
            recover_abandoned(
                publication,
                store.root(),
                &mut super::super::recovery::Budget::new(),
            )
        })
        .expect("abandoned recovery retires the empty ledger");
        assert!(!path.exists());
        store
            .retire_fixture()
            .expect("the empty-ledger fixture retires");
    }

    #[test]
    fn pin_first_project_reclaims_empty_ledgers_at_the_exact_installation_cap() {
        let store = fixture_store("pin-first-ledger-cap");
        for index in 0..MAX_PROJECT_LEDGERS {
            let mut identity = [0_u8; 32];
            identity[24..].copy_from_slice(&(index as u64).to_be_bytes());
            let project = Digest::from_bytes(identity);
            let mut ledger = Ledger::empty(project);
            if index + 1 < MAX_PROJECT_LEDGERS {
                ledger.roots.push(Root::new(
                    project,
                    Role::Live,
                    Digest::from_bytes([0x4a; 32]),
                    vec![Reference::fixture(
                        index as u64,
                        Charge {
                            bytes: BLOCK_BYTES,
                            allocations: 1,
                            members: 3,
                        },
                    )]
                    .into_boxed_slice(),
                ));
            }
            fs::write(
                ledger_path(store.root(), project),
                ledger.encode().expect("the fixture ledger encodes"),
            )
            .expect("the fixture ledger writes");
        }
        let project = Digest::from_bytes([0xff; 32]);
        let mut recovery = super::super::recovery::Budget::new();
        super::super::lease::with_publication(store.root(), |publication| {
            recover_empty_before_new_project(publication, store.root(), project, &mut recovery)?;
            save(publication, store.root(), &Ledger::empty(project))
        })
        .expect("pin-first admission reclaims exact-cap empty ledgers before replacement");
        assert_eq!(
            fs::read_dir(store.root().join("ledgers/projects"))
                .expect("the reclaimed project ledgers read")
                .count(),
            MAX_PROJECT_LEDGERS
        );
        let mut reclaimed = [0_u8; 32];
        reclaimed[24..].copy_from_slice(&((MAX_PROJECT_LEDGERS - 1) as u64).to_be_bytes());
        assert!(!ledger_path(store.root(), Digest::from_bytes(reclaimed)).exists());
        assert!(ledger_path(store.root(), project).is_file());
        store
            .retire_fixture()
            .expect("the pin-first ledger-cap fixture retires");
    }

    #[test]
    fn project_ledger_accepts_exact_edge_cap_and_rejects_one_more() {
        let project = Digest::from_bytes([7; 32]);
        let references = (0..MAX_EDGES)
            .map(|index| {
                Reference::fixture(
                    index as u64,
                    Charge {
                        bytes: 0,
                        allocations: 0,
                        members: 0,
                    },
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let proof = Digest::from_bytes([8; 32]);
        let owner = Owner::derive(project, Role::Live, proof, &references);
        let exact = Ledger {
            project,
            reservations: Vec::new(),
            roots: vec![Root {
                role: Role::Live,
                owner,
                proof,
                references,
            }],
        };
        exact.enforce().expect("the exact edge cap admits");
        let encoded = exact.encode().expect("the exact edge ledger encodes");
        assert_eq!(
            Ledger::decode(&encoded)
                .expect("the exact edge ledger decodes")
                .encode()
                .expect("the decoded ledger re-encodes"),
            encoded
        );

        let mut over = exact.roots[0].references.to_vec();
        over.push(Reference::fixture(
            MAX_EDGES as u64,
            Charge {
                bytes: 0,
                allocations: 0,
                members: 0,
            },
        ));
        let owner = Owner::derive(project, Role::Live, proof, &over);
        let over = Ledger {
            project,
            reservations: Vec::new(),
            roots: vec![Root {
                role: Role::Live,
                owner,
                proof,
                references: over.into_boxed_slice(),
            }],
        };
        assert!(matches!(
            over.enforce(),
            Err(Error::Limit {
                resource: "typed store project pin edges",
                limit: MAX_EDGES,
                actual,
            }) if actual == MAX_EDGES + 1
        ));
    }

    #[test]
    fn project_ledger_rejects_owner_or_reference_tampering() {
        let project = Digest::from_bytes([9; 32]);
        let proof = Digest::from_bytes([10; 32]);
        let references = vec![Reference::fixture(
            1,
            Charge {
                bytes: BLOCK_BYTES,
                allocations: 1,
                members: 3,
            },
        )]
        .into_boxed_slice();
        let owner = Owner::derive(project, Role::Prepared, proof, &references);
        let ledger = Ledger {
            project,
            reservations: Vec::new(),
            roots: vec![Root {
                role: Role::Prepared,
                owner,
                proof,
                references,
            }],
        };
        let mut owner_bytes = ledger
            .encode()
            .expect("the owner-bound project ledger encodes")
            .into_vec();
        owner_bytes[HEADER_BYTES + 8] ^= 1;
        assert!(matches!(
            Ledger::decode(&owner_bytes),
            Err(Error::Codec("project logical pin owner changed"))
        ));

        let mut reference_bytes = ledger
            .encode()
            .expect("the reference-bound project ledger encodes")
            .into_vec();
        reference_bytes[HEADER_BYTES + ROOT_BYTES + 8] ^= 1;
        assert!(matches!(
            Ledger::decode(&reference_bytes),
            Err(Error::Codec("project logical pin owner changed"))
                | Err(Error::Codec("project logical ledger is not canonical"))
        ));
    }

    #[test]
    fn project_ledger_accepts_eight_runner_roots_and_rejects_nine() {
        let project = Digest::from_bytes([11; 32]);
        let proof = Digest::from_bytes([12; 32]);
        let ledger = |count: usize| {
            let references = (0..count)
                .map(|index| {
                    Reference::fixture_kind(
                        index as u64,
                        crate::node::Kind::Runner,
                        Charge {
                            bytes: 0,
                            allocations: 0,
                            members: 0,
                        },
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let owner = Owner::derive(project, Role::Live, proof, &references);
            Ledger {
                project,
                reservations: Vec::new(),
                roots: vec![Root {
                    role: Role::Live,
                    owner,
                    proof,
                    references,
                }],
            }
        };
        ledger(MAX_RUNNER_ROOTS)
            .enforce()
            .expect("eight retained runner roots admit");
        assert!(matches!(
            ledger(MAX_RUNNER_ROOTS + 1).enforce(),
            Err(Error::Limit {
                resource: "typed store project retained runner roots",
                limit: MAX_RUNNER_ROOTS,
                actual,
            }) if actual == MAX_RUNNER_ROOTS + 1
        ));
    }
}
