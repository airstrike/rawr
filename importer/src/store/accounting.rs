use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node::{ActionKey, Digest, Error, ExecutionKey, Kind};

const LEDGER_MAGIC: &[u8; 8] = b"RAWRLEDR";
const RECORD_MAGIC: &[u8; 8] = b"RAWRALOC";
/// Typed (action and content) records.
const RECORD_SCHEMA: u16 = 4;
/// Bundle records: schema 4 plus the sealed logical-geometry commitment.
const BUNDLE_RECORD_SCHEMA: u16 = 5;
/// The store's format. Any other value is a clean miss: the store is rebuilt.
const LEDGER_SCHEMA: u16 = 6;
const LEDGER_HEADER_BYTES: usize = 64;
const RESERVATION_BYTES: usize = 136;
const ALLOCATION_BYTES: usize = 96;
const REGISTERED_BYTES: usize = 115;
pub(super) const RECORD_BYTES: usize = 184;
/// Exact length of a schema-5 bundle record: the schema-4 layout and geometry.
pub(super) const BUNDLE_RECORD_BYTES: usize = RECORD_BYTES + 32;
/// Upper bound for reading any allocation record.
pub(super) const MAX_RECORD_BYTES: usize = BUNDLE_RECORD_BYTES;
const MAX_LEDGER_BYTES: usize = 8 * 1024 * 1024;
const BLOCK_BYTES: u64 = 4 * 1024;
const INSTALLATION_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const INSTALLATION_ALLOCATIONS: u64 = 65_536;
pub(super) const INSTALLATION_MEMBERS: u64 = 2_097_152;
const MAX_RESERVATIONS: usize = 64;
const MAX_EVICTION_ALLOCATIONS: usize = PROJECT_ALLOCATIONS;
const PROJECT_ALLOCATIONS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Charge {
    pub(crate) bytes: u64,
    pub(crate) allocations: u64,
    pub(crate) members: u64,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LeaseKey {
    Action(ActionKey),
    Execution(ExecutionKey),
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Class {
    Action,
    Content,
    Bundle,
}

#[derive(Clone, Copy)]
pub(crate) struct Reservation {
    key: LeaseKey,
    charge: Charge,
    plan: Option<Plan>,
}

pub(crate) struct Capacity<'lease> {
    publication: &'lease super::lease::Publication<'lease>,
    root: PathBuf,
    ledger: Ledger,
    admitted: super::control::Admitted<'lease>,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Plan {
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
    bundle: bool,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Allocation {
    pub(crate) class: Class,
    pub(crate) kind: Kind,
    pub(crate) schema: u32,
    pub(crate) key: Digest,
    pub(crate) charge: Charge,
    pub(crate) record: [u8; 32],
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Registered {
    allocation: Allocation,
    sequence: u64,
    publisher: Digest,
    content: Digest,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Reference(Allocation);

#[derive(Clone, Copy)]
pub(crate) struct Identity {
    pub(crate) class: Class,
    pub(crate) kind: Kind,
    pub(crate) schema: u32,
    pub(crate) key: Digest,
    pub(crate) reservation: LeaseKey,
    pub(crate) publisher: Digest,
}

#[derive(Clone, Copy)]
pub(crate) struct Record {
    identity: Identity,
    length: u64,
    digest: [u8; 32],
    charge: Charge,
    /// Sealed logical geometry; `None` only for typed records.
    geometry: Option<[u8; 32]>,
}

#[derive(Clone)]
pub(super) struct Ledger {
    used: Charge,
    reservations: Vec<Reservation>,
    allocations: Vec<Registered>,
    evictions: Vec<Registered>,
    next_sequence: u64,
}

impl Charge {
    pub(crate) const fn zero() -> Self {
        Self {
            bytes: 0,
            allocations: 0,
            members: 0,
        }
    }

    pub(crate) fn node(output_limit: usize, envelope_limit: usize) -> Result<Self, Error> {
        Self::allocation(output_limit)?.checked_add(Self::allocation(envelope_limit)?)
    }

    fn allocation(payload_bytes: usize) -> Result<Self, Error> {
        let payload = u64::try_from(payload_bytes).map_err(|_| Error::Limit {
            resource: "typed store reserved bytes",
            limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
            actual: usize::MAX,
        })?;
        let payload = round_block(payload)?;
        let bytes = payload
            .checked_add(BLOCK_BYTES)
            .and_then(|bytes| bytes.checked_add(BLOCK_BYTES))
            .ok_or(Error::Limit {
                resource: "typed store reserved bytes",
                limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
                actual: usize::MAX,
            })?;
        Ok(Self {
            bytes,
            allocations: 1,
            members: 3,
        })
    }

    pub(crate) fn checked_add(self, other: Self) -> Result<Self, Error> {
        Ok(Self {
            bytes: self.bytes.checked_add(other.bytes).ok_or(Error::Limit {
                resource: "typed store physical bytes",
                limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
                actual: usize::MAX,
            })?,
            allocations: self
                .allocations
                .checked_add(other.allocations)
                .ok_or(Error::Limit {
                    resource: "typed store allocations",
                    limit: usize::try_from(INSTALLATION_ALLOCATIONS).unwrap_or(usize::MAX),
                    actual: usize::MAX,
                })?,
            members: self
                .members
                .checked_add(other.members)
                .ok_or(Error::Limit {
                    resource: "typed store filesystem members",
                    limit: usize::try_from(INSTALLATION_MEMBERS).unwrap_or(usize::MAX),
                    actual: usize::MAX,
                })?,
        })
    }

    fn checked_sub(self, other: Self) -> Result<Self, Error> {
        Ok(Self {
            bytes: self.bytes.checked_sub(other.bytes).ok_or(Error::State(
                "typed store physical byte accounting underflowed",
            ))?,
            allocations: self
                .allocations
                .checked_sub(other.allocations)
                .ok_or(Error::State(
                    "typed store allocation accounting underflowed",
                ))?,
            members: self
                .members
                .checked_sub(other.members)
                .ok_or(Error::State("typed store member accounting underflowed"))?,
        })
    }

    fn fits_within(self, limit: Self) -> bool {
        self.bytes <= limit.bytes
            && self.allocations <= limit.allocations
            && self.members <= limit.members
    }
}

impl LeaseKey {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Action(_) => 1,
            Self::Execution(_) => 2,
        }
    }

    pub(crate) fn bytes(self) -> [u8; 32] {
        match self {
            Self::Action(key) => key.as_bytes(),
            Self::Execution(key) => key.as_bytes(),
        }
    }

    pub(crate) fn decode(code: u8, bytes: [u8; 32]) -> Result<Self, Error> {
        match code {
            1 => Ok(Self::Action(ActionKey::from_bytes(bytes))),
            2 => Ok(Self::Execution(ExecutionKey::from_bytes(bytes))),
            _ => Err(Error::Codec("typed store reservation lease class changed")),
        }
    }
}

impl Reservation {
    pub(crate) const fn key(&self) -> LeaseKey {
        self.key
    }

    pub(super) const fn execution(&self) -> Option<ExecutionKey> {
        match self.key {
            LeaseKey::Execution(execution) => Some(execution),
            LeaseKey::Action(_) => None,
        }
    }

    pub(super) const fn charge(&self) -> Charge {
        self.charge
    }

    pub(super) const fn planned(&self) -> Option<(Kind, u32, ActionKey, Digest, bool)> {
        match self.plan {
            Some(plan) => Some((
                plan.kind,
                plan.schema,
                plan.action,
                plan.content,
                plan.bundle,
            )),
            None => None,
        }
    }
}

impl Capacity<'_> {
    pub(crate) fn reserve(mut self, key: LeaseKey, charge: Charge) -> Result<Reservation, Error> {
        self.ledger.insert_reservation(key, charge)?;
        let bytes = self.ledger.encode()?;
        let target = ledger_path(&self.root);
        super::entry::replace_admitted_file(
            self.admitted,
            self.publication,
            &self.root,
            &target,
            &bytes,
        )?;
        Ok(Reservation {
            key,
            charge,
            plan: None,
        })
    }
}

impl Reference {
    pub(crate) const fn charge(self) -> Charge {
        self.0.charge
    }

    pub(crate) fn encode(self, bytes: &mut Vec<u8>) {
        encode_allocation(self.0, bytes);
    }

    pub(crate) fn decode(bytes: &[u8], offset: usize) -> Result<Self, Error> {
        decode_allocation(bytes, offset).map(Self)
    }

    pub(crate) const fn encoded_bytes() -> usize {
        ALLOCATION_BYTES
    }

    pub(crate) fn hash_into(self, digest: &mut Sha256) {
        digest.update([match self.0.class {
            Class::Action => 1,
            Class::Content => 2,
            Class::Bundle => 3,
        }]);
        digest.update([self.0.kind as u8]);
        digest.update([0; 2]);
        digest.update(self.0.schema.to_le_bytes());
        digest.update(self.0.key.as_bytes());
        digest.update(self.0.charge.bytes.to_le_bytes());
        digest.update(self.0.charge.allocations.to_le_bytes());
        digest.update(self.0.charge.members.to_le_bytes());
        digest.update(self.0.record);
    }

    pub(crate) const fn is_runner_root(self) -> bool {
        matches!(self.0.class, Class::Action) && matches!(self.0.kind, Kind::Runner)
    }

    #[cfg(test)]
    pub(crate) fn fixture(index: u64, charge: Charge) -> Self {
        Self::fixture_kind(index, Kind::NativeProduct, charge)
    }

    #[cfg(test)]
    pub(crate) fn fixture_kind(index: u64, kind: Kind, charge: Charge) -> Self {
        let mut key = [0; 32];
        key[24..].copy_from_slice(&index.to_be_bytes());
        Self(Allocation {
            class: Class::Action,
            kind,
            schema: 1,
            key: Digest::from_bytes(key),
            charge,
            record: Sha256::digest(index.to_be_bytes()).into(),
        })
    }
}

impl Registered {
    pub(crate) const fn allocation(self) -> Allocation {
        self.allocation
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }

    pub(crate) const fn publisher(self) -> Digest {
        self.publisher
    }

    pub(crate) const fn content(self) -> Digest {
        self.content
    }

    pub(crate) const fn reference(self) -> Reference {
        Reference(self.allocation)
    }
}

impl Record {
    pub(crate) fn seal(
        identity: Identity,
        payload: &[u8],
        metadata: &fs::Metadata,
    ) -> Result<(Box<[u8]>, Self), Error> {
        if !metadata.is_file() || metadata.len() != u64::try_from(payload.len()).unwrap_or(u64::MAX)
        {
            return Err(Error::Codec(
                "typed store payload metadata changed before sealing",
            ));
        }
        let logical = round_block(metadata.len())?;
        let allocated = round_block(allocated_bytes(metadata)?)?;
        let payload_charge = logical.max(allocated);
        let charge = Charge {
            bytes: payload_charge
                .checked_add(BLOCK_BYTES)
                .and_then(|bytes| bytes.checked_add(BLOCK_BYTES))
                .ok_or(Error::Limit {
                    resource: "typed store physical bytes",
                    limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
                    actual: usize::MAX,
                })?,
            allocations: 1,
            members: 3,
        };
        let record = Self {
            identity,
            length: metadata.len(),
            digest: Sha256::digest(payload).into(),
            charge,
            geometry: None,
        };
        Ok((record.encode(), record))
    }

    /// Seal a schema-5 bundle record: the charge, its counts and the
    /// geometry commitment are fixed here and never re-recorded.
    pub(crate) fn seal_bundle(
        identity: Identity,
        payload: &[u8],
        metadata: &fs::Metadata,
        charge: Charge,
        geometry: [u8; 32],
    ) -> Result<(Box<[u8]>, Self), Error> {
        if identity.class != Class::Bundle
            || !metadata.is_file()
            || metadata.len() != u64::try_from(payload.len()).unwrap_or(u64::MAX)
            || charge.allocations != 1
            || charge.members < 3
        {
            return Err(Error::Codec("typed store bundle allocation changed"));
        }
        let record = Self {
            identity,
            length: metadata.len(),
            digest: Sha256::digest(payload).into(),
            charge,
            geometry: Some(geometry),
        };
        Ok((record.encode(), record))
    }

    pub(crate) fn read(path: &Path) -> Result<Self, Error> {
        let bytes = super::entry::read_file(&path.join("allocation"), RECORD_BYTES)?;
        Self::decode(&bytes)
    }

    pub(crate) fn read_budgeted(
        path: &Path,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let bytes =
            super::entry::read_file_budgeted(&path.join("allocation"), RECORD_BYTES, budget)?;
        Self::decode(&bytes)
    }

    pub(super) fn read_candidate_budgeted(
        path: &Path,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let bytes = super::entry::read_candidate_file_budgeted(
            &path.join("allocation"),
            RECORD_BYTES,
            budget,
        )?;
        Self::decode(&bytes)
    }

    /// Read a runner-bundle allocation record.
    pub(crate) fn read_bundle(path: &Path) -> Result<Self, Error> {
        let bytes = super::entry::read_file(&path.join("allocation"), MAX_RECORD_BYTES)?;
        Self::decode(&bytes)
    }

    pub(crate) fn read_bundle_budgeted(
        path: &Path,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let bytes =
            super::entry::read_file_budgeted(&path.join("allocation"), MAX_RECORD_BYTES, budget)?;
        Self::decode(&bytes)
    }

    pub(crate) fn validate(
        &self,
        class: Class,
        kind: Kind,
        schema: u32,
        key: Digest,
        payload: &[u8],
        metadata: &fs::Metadata,
    ) -> Result<(), Error> {
        self.validate_digest(
            class,
            kind,
            schema,
            key,
            u64::try_from(payload.len()).unwrap_or(u64::MAX),
            Sha256::digest(payload).into(),
            metadata,
        )
    }

    /// `validate` over the payload's already-computed length and SHA-256.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_digest(
        &self,
        class: Class,
        kind: Kind,
        schema: u32,
        key: Digest,
        length: u64,
        digest: [u8; 32],
        metadata: &fs::Metadata,
    ) -> Result<(), Error> {
        let logical = round_block(metadata.len())?;
        let allocated = round_block(allocated_bytes(metadata)?)?;
        let expected_charge = Charge {
            bytes: logical
                .max(allocated)
                .checked_add(BLOCK_BYTES * 2)
                .ok_or(Error::Allocation)?,
            allocations: 1,
            members: 3,
        };
        if self.identity.class != class
            || self.identity.kind != kind
            || self.identity.schema != schema
            || self.identity.key != key
            || self.length != length
            || self.digest != digest
            || self.charge != expected_charge
        {
            return Err(Error::Codec("typed store allocation identity changed"));
        }
        Ok(())
    }

    pub(crate) fn validate_bundle(
        &self,
        kind: Kind,
        schema: u32,
        key: Digest,
        payload: &[u8],
        metadata: &fs::Metadata,
        measured: super::bundle::Measured,
    ) -> Result<(), Error> {
        if super::bundle::check(self.charge, self.geometry, measured).is_err() {
            return Err(Error::Codec(
                "typed store bundle allocation identity changed",
            ));
        }
        self.validate_bundle_commitment(kind, schema, key, payload, metadata)
    }

    const fn encoded_len(&self) -> usize {
        match self.geometry {
            Some(_) => BUNDLE_RECORD_BYTES,
            None => RECORD_BYTES,
        }
    }

    /// The sealed geometry; `None` for a typed record.
    pub(crate) const fn sealed_geometry(&self) -> Option<[u8; 32]> {
        self.geometry
    }

    /// Authenticate the recorded seal identity without claiming that the whole
    /// allocation's current physical charge has been measured.
    pub(crate) fn validate_bundle_commitment(
        &self,
        kind: Kind,
        schema: u32,
        key: Digest,
        payload: &[u8],
        metadata: &fs::Metadata,
    ) -> Result<(), Error> {
        if self.identity.class != Class::Bundle
            || self.identity.kind != kind
            || self.identity.schema != schema
            || self.identity.key != key
            || self.length != u64::try_from(payload.len()).unwrap_or(u64::MAX)
            || self.digest != <[u8; 32]>::from(Sha256::digest(payload))
        {
            return Err(Error::Codec(
                "typed store bundle allocation identity changed",
            ));
        }
        if !metadata.is_file() || metadata.len() != self.length {
            return Err(Error::Codec("typed store bundle seal metadata changed"));
        }
        Ok(())
    }

    pub(super) fn validate_bundle_descriptor(
        &self,
        kind: Kind,
        schema: u32,
        key: Digest,
        seal_bytes: usize,
        seal_digest: [u8; 32],
    ) -> Result<(), Error> {
        if self.identity.class != Class::Bundle
            || self.identity.kind != kind
            || self.identity.schema != schema
            || self.identity.key != key
            || self.length != u64::try_from(seal_bytes).map_err(|_| Error::Allocation)?
            || self.digest != seal_digest
        {
            return Err(Error::Codec(
                "candidate bundle commitment differs from allocation record",
            ));
        }
        Ok(())
    }

    pub(crate) const fn charge(&self) -> Charge {
        self.charge
    }

    pub(crate) fn allocation(&self) -> Allocation {
        Allocation {
            class: self.identity.class,
            kind: self.identity.kind,
            schema: self.identity.schema,
            key: self.identity.key,
            charge: self.charge,
            record: Sha256::digest(self.encode()).into(),
        }
    }

    pub(crate) fn reservation(&self) -> LeaseKey {
        self.identity.reservation
    }

    pub(crate) fn publisher(&self) -> Digest {
        self.identity.publisher
    }

    fn encode(&self) -> Box<[u8]> {
        let mut bytes = Vec::with_capacity(MAX_RECORD_BYTES);
        bytes.extend_from_slice(RECORD_MAGIC);
        let schema = match self.geometry {
            Some(_) => BUNDLE_RECORD_SCHEMA,
            None => RECORD_SCHEMA,
        };
        bytes.extend_from_slice(&schema.to_le_bytes());
        bytes.push(match self.identity.class {
            Class::Action => 1,
            Class::Content => 2,
            Class::Bundle => 3,
        });
        bytes.push(self.identity.kind as u8);
        bytes.extend_from_slice(&self.identity.schema.to_le_bytes());
        bytes.extend_from_slice(&self.identity.key.as_bytes());
        bytes.push(self.identity.reservation.code());
        bytes.extend_from_slice(&[0; 7]);
        bytes.extend_from_slice(&self.identity.reservation.bytes());
        bytes.extend_from_slice(&self.length.to_le_bytes());
        bytes.extend_from_slice(&self.digest);
        bytes.extend_from_slice(&self.charge.bytes.to_le_bytes());
        bytes.extend_from_slice(&self.charge.allocations.to_le_bytes());
        bytes.extend_from_slice(&self.charge.members.to_le_bytes());
        bytes.extend_from_slice(&self.identity.publisher.as_bytes());
        debug_assert_eq!(bytes.len(), RECORD_BYTES);
        if let Some(geometry) = self.geometry {
            bytes.extend_from_slice(&geometry);
            debug_assert_eq!(bytes.len(), BUNDLE_RECORD_BYTES);
        }
        bytes.into_boxed_slice()
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < RECORD_BYTES || &bytes[..8] != RECORD_MAGIC {
            return Err(Error::Codec("typed store allocation record changed"));
        }
        let geometry = match (u16_at(bytes, 8)?, bytes.len()) {
            (RECORD_SCHEMA, RECORD_BYTES) if bytes[10] != 3 => None,
            (BUNDLE_RECORD_SCHEMA, BUNDLE_RECORD_BYTES) if bytes[10] == 3 => {
                Some(array_at(bytes, RECORD_BYTES)?)
            }
            _ => return Err(Error::Codec("typed store allocation record changed")),
        };
        let class = match bytes[10] {
            1 => Class::Action,
            2 => Class::Content,
            3 => Class::Bundle,
            _ => return Err(Error::Codec("typed store allocation class changed")),
        };
        let kind = Kind::from_code(bytes[11])
            .ok_or(Error::Codec("typed store allocation node kind changed"))?;
        let schema = u32_at(bytes, 12)?;
        let key = Digest::from_bytes(array_at(bytes, 16)?);
        if bytes[49..56].iter().any(|byte| *byte != 0) {
            return Err(Error::Codec("typed store allocation padding changed"));
        }
        let reservation = LeaseKey::decode(bytes[48], array_at(bytes, 56)?)?;
        let length = u64_at(bytes, 88)?;
        let digest = array_at(bytes, 96)?;
        let charge = Charge {
            bytes: u64_at(bytes, 128)?,
            allocations: u64_at(bytes, 136)?,
            members: u64_at(bytes, 144)?,
        };
        if charge.allocations != 1
            || (class == Class::Bundle && charge.members < 3)
            || (class != Class::Bundle && charge.members != 3)
        {
            return Err(Error::Codec("typed store allocation charge changed"));
        }
        let publisher = Digest::from_bytes(array_at(bytes, 152)?);
        let record = Self {
            identity: Identity {
                class,
                kind,
                schema,
                key,
                reservation,
                publisher,
            },
            length,
            digest,
            charge,
            geometry,
        };
        if record.encode().as_ref() != bytes {
            return Err(Error::Codec(
                "typed store allocation record is not canonical",
            ));
        }
        Ok(record)
    }
}

impl Ledger {
    fn empty() -> Self {
        Self {
            used: Charge::zero(),
            reservations: Vec::new(),
            allocations: Vec::new(),
            evictions: Vec::new(),
            next_sequence: 1,
        }
    }

    fn read(root: &Path) -> Result<Self, Error> {
        Self::read_with_recovery(root)
    }

    fn read_budgeted(root: &Path, budget: &crate::node::GraphBudget) -> Result<Self, Error> {
        let path = ledger_path(root);
        let bytes = super::entry::read_file_budgeted(&path, MAX_LEDGER_BYTES, budget)?;
        Self::decode(&bytes).map_err(|_| Error::Corrupt {
            path,
            reason: "installation ledger decoding failed",
        })
    }

    fn read_recovery(
        root: &Path,
        budget: &mut super::recovery::Budget,
    ) -> Result<std::sync::Arc<Self>, Error> {
        let before = recovery_metadata(root)?;
        if let Some(ledger) = budget.installation(root, &before) {
            return Ok(ledger);
        }
        let ledger = std::sync::Arc::new(Self::read_with_recovery(root)?);
        let after = recovery_metadata(root)?;
        if !super::recovery::same_installation(&before, &after) {
            return Err(Error::CleanupRequired(
                "installation ledger changed during recovery read",
            ));
        }
        budget.retain_installation(root, std::sync::Arc::clone(&ledger), after);
        Ok(ledger)
    }

    fn read_with_recovery(root: &Path) -> Result<Self, Error> {
        let path = ledger_path(root);
        let bytes = super::entry::read_file(&path, MAX_LEDGER_BYTES)?;
        Self::decode(&bytes).map_err(|_| Error::Corrupt {
            path,
            reason: "installation ledger decoding failed",
        })
    }

    fn insert_reservation(&mut self, key: LeaseKey, charge: Charge) -> Result<(), Error> {
        if self
            .reservations
            .iter()
            .any(|reservation| reservation.key == key)
        {
            return Err(Error::CleanupRequired(
                "an earlier typed store reservation still owns this execution",
            ));
        }
        if self.reservations.len() == MAX_RESERVATIONS {
            return Err(Error::Limit {
                resource: "typed store concurrent reservations",
                limit: MAX_RESERVATIONS,
                actual: MAX_RESERVATIONS + 1,
            });
        }
        self.reservations.push(Reservation {
            key,
            charge,
            plan: None,
        });
        self.reservations
            .sort_unstable_by_key(|reservation| reservation.key);
        Ok(())
    }

    fn cancel(&mut self, reservation: &Reservation) -> Result<(), Error> {
        let Some(index) = self.reservations.iter().position(|current| {
            current.key == reservation.key && current.charge == reservation.charge
        }) else {
            return Err(Error::State(
                "typed store reservation disappeared before cancellation",
            ));
        };
        if self.reservations[index].plan.is_some() {
            return Err(Error::CleanupRequired(
                "a planned typed store reservation requires whole-unit recovery",
            ));
        }
        self.reservations.remove(index);
        Ok(())
    }

    fn plan(&mut self, reservation: &Reservation, plan: Plan) -> Result<(), Error> {
        let current = self
            .reservations
            .iter_mut()
            .find(|current| current.key == reservation.key && current.charge == reservation.charge)
            .ok_or(Error::State(
                "typed store reservation disappeared before planning",
            ))?;
        match current.plan {
            None => current.plan = Some(plan),
            Some(existing) if existing == plan => {}
            Some(_) => return Err(Error::State("typed store reservation plan changed")),
        }
        Ok(())
    }

    fn commit(&mut self, reservation: &Reservation, records: &[Record]) -> Result<(), Error> {
        if records.is_empty() || records.len() > 3 {
            return Err(Error::State("typed store publication record count changed"));
        }
        let Some(index) = self.reservations.iter().position(|current| {
            current.key == reservation.key && current.charge == reservation.charge
        }) else {
            return Err(Error::State(
                "typed store reservation disappeared before commit",
            ));
        };
        let plan = self.reservations[index]
            .plan
            .ok_or(Error::State("typed store publication was not planned"))?;
        let mut actual = Charge::zero();
        let mut action = false;
        let mut content = false;
        let mut bundle = false;
        let mut publisher = None;
        let sequence = self.next_sequence;
        let next_sequence = sequence.checked_add(1).ok_or(Error::Limit {
            resource: "typed store seal sequence",
            limit: usize::MAX,
            actual: usize::MAX,
        })?;
        let mut allocations: Vec<Registered> = Vec::new();
        allocations
            .try_reserve_exact(records.len())
            .map_err(|_| Error::Allocation)?;
        for record in records {
            if record.reservation() != reservation.key {
                return Err(Error::State("typed store allocation changed reservation"));
            }
            match publisher {
                Some(expected) if expected != record.publisher() => {
                    return Err(Error::State(
                        "typed store publication changed publisher within one seal unit",
                    ));
                }
                None => publisher = Some(record.publisher()),
                Some(_) => {}
            }
            let allocation = record.allocation();
            match allocation.class {
                Class::Action
                    if allocation.kind == plan.kind
                        && allocation.schema == plan.schema
                        && allocation.key == Digest::from_bytes(plan.action.as_bytes()) =>
                {
                    action = true;
                }
                Class::Content
                    if allocation.kind == plan.kind
                        && allocation.schema == plan.schema
                        && allocation.key == plan.content =>
                {
                    content = true;
                }
                Class::Bundle
                    if allocation.kind == plan.kind
                        && allocation.schema == plan.schema
                        && allocation.key == plan.content =>
                {
                    bundle = true;
                }
                _ => return Err(Error::State("typed store allocation changed its plan")),
            }
            if self.contains_identity(allocation)
                || allocations
                    .iter()
                    .any(|current| same_identity(current.allocation, allocation))
            {
                return Err(Error::State("typed store allocation was committed twice"));
            }
            actual = actual.checked_add(allocation.charge)?;
            allocations.push(Registered {
                allocation,
                sequence,
                publisher: record.publisher(),
                content: plan.content,
            });
        }
        if !action
            || bundle != plan.bundle
            || (!content
                && !self.contains_identity(Allocation {
                    class: Class::Content,
                    kind: plan.kind,
                    schema: plan.schema,
                    key: plan.content,
                    charge: Charge::zero(),
                    record: [0; 32],
                }))
        {
            return Err(Error::State(
                "typed store publication omitted a planned allocation",
            ));
        }
        if !actual.fits_within(reservation.charge) {
            return Err(Error::State(
                "typed store publication exceeded its reservation",
            ));
        }
        let used = self.used.checked_add(actual)?;
        self.reservations.remove(index);
        self.allocations.extend(allocations);
        self.allocations
            .sort_unstable_by_key(|registered| allocation_identity(registered.allocation));
        self.used = used;
        self.next_sequence = next_sequence;
        Ok(())
    }

    fn registered(&self, allocation: Allocation) -> bool {
        self.find_identity(allocation) == Some(allocation)
    }

    fn begin_eviction(&mut self, selected: &[Registered]) -> Result<(), Error> {
        if !self.evictions.is_empty() {
            return Err(Error::CleanupRequired(
                "an earlier typed store eviction requires recovery",
            ));
        }
        validate_eviction_selection(&self.allocations, selected)?;
        let mut indexes = Vec::new();
        indexes
            .try_reserve_exact(selected.len())
            .map_err(|_| Error::Allocation)?;
        for selected in selected {
            let index = self
                .allocations
                .binary_search_by(|current| {
                    allocation_identity(current.allocation)
                        .cmp(&allocation_identity(selected.allocation))
                })
                .map_err(|_| Error::State("typed store eviction allocation disappeared"))?;
            if self.allocations[index] != *selected {
                return Err(Error::State("typed store eviction allocation changed"));
            }
            indexes.push(index);
        }
        indexes.sort_unstable();
        if indexes.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::State(
                "typed store eviction selected one allocation twice",
            ));
        }
        for index in indexes.into_iter().rev() {
            self.evictions.push(self.allocations.remove(index));
        }
        self.evictions
            .sort_unstable_by_key(|registered| allocation_identity(registered.allocation));
        Ok(())
    }

    fn finish_eviction(&mut self) -> Result<(), Error> {
        if self.evictions.is_empty() {
            return Err(Error::State("typed store eviction journal is empty"));
        }
        let removed = self
            .evictions
            .iter()
            .try_fold(Charge::zero(), |total, registered| {
                total.checked_add(registered.allocation.charge)
            })?;
        self.used = self.used.checked_sub(removed)?;
        self.evictions.clear();
        Ok(())
    }

    fn contains_identity(&self, allocation: Allocation) -> bool {
        self.find_identity(allocation).is_some()
    }

    fn find_identity(&self, allocation: Allocation) -> Option<Allocation> {
        self.find_registered(allocation).map(Registered::allocation)
    }

    fn find_registered(&self, allocation: Allocation) -> Option<Registered> {
        self.allocations
            .binary_search_by(|current| {
                allocation_identity(current.allocation).cmp(&allocation_identity(allocation))
            })
            .ok()
            .map(|index| self.allocations[index])
    }

    fn total(&self) -> Result<Charge, Error> {
        self.reservations
            .iter()
            .try_fold(self.used, |total, reservation| {
                total.checked_add(reservation.charge)
            })
    }

    fn encode(&self) -> Result<Box<[u8]>, Error> {
        let mut publishers = self
            .allocations
            .iter()
            .chain(self.evictions.iter())
            .map(|registered| registered.publisher)
            .collect::<Vec<_>>();
        publishers.sort_unstable();
        publishers.dedup();
        if publishers.len() > super::pins::MAX_PROJECT_LEDGERS {
            return Err(Error::Limit {
                resource: "typed store installation publishers",
                limit: super::pins::MAX_PROJECT_LEDGERS,
                actual: publishers.len(),
            });
        }
        let mut sequences = self
            .allocations
            .iter()
            .chain(self.evictions.iter())
            .map(|registered| registered.sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        sequences.dedup();
        let canonical_next_sequence = u64::try_from(sequences.len())
            .map_err(|_| Error::Allocation)?
            .checked_add(1)
            .ok_or(Error::Allocation)?;
        let bundle_members = self
            .allocations
            .iter()
            .chain(self.evictions.iter())
            .filter(|registered| registered.allocation.class == Class::Bundle)
            .map(|registered| registered.allocation.charge.members)
            .collect::<Vec<_>>();
        if bundle_members.len() > super::bundle::MAX_BUNDLES {
            return Err(Error::Limit {
                resource: "typed store installation runner bundles",
                limit: super::bundle::MAX_BUNDLES,
                actual: bundle_members.len(),
            });
        }
        if self.reservations.len() > MAX_RESERVATIONS
            || self
                .reservations
                .windows(2)
                .any(|pair| pair[0].key >= pair[1].key)
            || self.allocations.len()
                > usize::try_from(INSTALLATION_ALLOCATIONS).unwrap_or(usize::MAX)
            || self.allocations.windows(2).any(|pair| {
                allocation_identity(pair[0].allocation) >= allocation_identity(pair[1].allocation)
            })
            || self.next_sequence == 0
            || sequences
                .last()
                .is_some_and(|sequence| *sequence >= self.next_sequence)
            || self.evictions.len() > MAX_EVICTION_ALLOCATIONS
            || self.evictions.windows(2).any(|pair| {
                allocation_identity(pair[0].allocation) >= allocation_identity(pair[1].allocation)
            })
            || self.allocations.iter().any(|registered| {
                registered.sequence == 0
                    || registered.sequence >= self.next_sequence
                    || !registered_shape(*registered)
            })
            || self.evictions.iter().any(|registered| {
                registered.sequence == 0
                    || registered.sequence >= self.next_sequence
                    || !registered_shape(*registered)
            })
            || self.evictions.iter().any(|evicting| {
                self.allocations.iter().any(|registered| {
                    allocation_identity(registered.allocation)
                        == allocation_identity(evicting.allocation)
                })
            })
        {
            return Err(Error::State("typed store ledger ordering changed"));
        }
        let allocations = self
            .allocations
            .iter()
            .chain(self.evictions.iter())
            .try_fold(Charge::zero(), |total, registered| {
                total.checked_add(registered.allocation.charge)
            })?;
        if allocations != self.used {
            return Err(Error::State("typed store allocation total changed"));
        }
        let length = LEDGER_HEADER_BYTES
            .checked_add(
                self.reservations
                    .len()
                    .checked_mul(RESERVATION_BYTES)
                    .ok_or(Error::Allocation)?,
            )
            .and_then(|length| {
                length.checked_add(self.allocations.len().checked_mul(REGISTERED_BYTES)?)
            })
            .and_then(|length| {
                length.checked_add(self.evictions.len().checked_mul(REGISTERED_BYTES)?)
            })
            .and_then(|length| length.checked_add(publishers.len().checked_mul(32)?))
            .and_then(|length| length.checked_add(bundle_members.len().checked_mul(4)?))
            .ok_or(Error::Allocation)?;
        if length > MAX_LEDGER_BYTES {
            return Err(Error::Limit {
                resource: "typed store installation ledger bytes",
                limit: MAX_LEDGER_BYTES,
                actual: length,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(LEDGER_MAGIC);
        bytes.extend_from_slice(&LEDGER_SCHEMA.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&self.used.bytes.to_le_bytes());
        bytes.extend_from_slice(&self.used.allocations.to_le_bytes());
        bytes.extend_from_slice(&self.used.members.to_le_bytes());
        bytes.extend_from_slice(&(self.reservations.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.allocations.len() as u32).to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(canonical_next_sequence)
                .map_err(|_| Error::Allocation)?
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&(bundle_members.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.evictions.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(publishers.len() as u32).to_le_bytes());
        for reservation in &self.reservations {
            bytes.push(reservation.key.code());
            bytes.extend_from_slice(&[0; 7]);
            bytes.extend_from_slice(&reservation.key.bytes());
            bytes.extend_from_slice(&reservation.charge.bytes.to_le_bytes());
            bytes.extend_from_slice(&reservation.charge.allocations.to_le_bytes());
            bytes.extend_from_slice(&reservation.charge.members.to_le_bytes());
            if let Some(plan) = reservation.plan {
                bytes.push(1);
                bytes.push(plan.kind as u8);
                bytes.push(u8::from(plan.bundle));
                bytes.push(0);
                bytes.extend_from_slice(&plan.schema.to_le_bytes());
                bytes.extend_from_slice(&plan.action.as_bytes());
                bytes.extend_from_slice(&plan.content.as_bytes());
            } else {
                bytes.extend_from_slice(&[0; 72]);
            }
        }
        for publisher in &publishers {
            bytes.extend_from_slice(&publisher.as_bytes());
        }
        for members in bundle_members {
            bytes.extend_from_slice(
                &u32::try_from(members)
                    .map_err(|_| Error::Allocation)?
                    .to_le_bytes(),
            );
        }
        for registered in &self.allocations {
            encode_registered(*registered, &publishers, &sequences, &mut bytes)?;
        }
        for registered in &self.evictions {
            encode_registered(*registered, &publishers, &sequences, &mut bytes)?;
        }
        debug_assert_eq!(bytes.len(), length);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < LEDGER_HEADER_BYTES
            || &bytes[..8] != LEDGER_MAGIC
            || u16_at(bytes, 8)? != LEDGER_SCHEMA
            || bytes[10..16].iter().any(|byte| *byte != 0)
        {
            return Err(Error::Codec("typed store installation ledger changed"));
        }
        let count = usize::try_from(u32_at(bytes, 40)?)
            .map_err(|_| Error::Codec("typed store reservation count is not addressable"))?;
        let allocation_count = usize::try_from(u32_at(bytes, 44)?)
            .map_err(|_| Error::Codec("typed store allocation count is not addressable"))?;
        let bundle_count = usize::try_from(u32_at(bytes, 52)?)
            .map_err(|_| Error::Codec("typed store bundle count is not addressable"))?;
        let eviction_count = usize::try_from(u32_at(bytes, 56)?)
            .map_err(|_| Error::Codec("typed store eviction count is not addressable"))?;
        let publisher_count = usize::try_from(u32_at(bytes, 60)?)
            .map_err(|_| Error::Codec("typed store publisher count is not addressable"))?;
        if count > MAX_RESERVATIONS
            || allocation_count > usize::try_from(INSTALLATION_ALLOCATIONS).unwrap_or(usize::MAX)
            || eviction_count > MAX_EVICTION_ALLOCATIONS
            || bundle_count > super::bundle::MAX_BUNDLES
            || publisher_count > super::pins::MAX_PROJECT_LEDGERS
            || LEDGER_HEADER_BYTES
                .checked_add(
                    count
                        .checked_mul(RESERVATION_BYTES)
                        .ok_or(Error::Allocation)?,
                )
                .and_then(|length| {
                    length.checked_add(allocation_count.checked_mul(REGISTERED_BYTES)?)
                })
                .and_then(|length| {
                    length.checked_add(eviction_count.checked_mul(REGISTERED_BYTES)?)
                })
                .and_then(|length| length.checked_add(publisher_count.checked_mul(32)?))
                .and_then(|length| length.checked_add(bundle_count.checked_mul(4)?))
                != Some(bytes.len())
        {
            return Err(Error::Codec("typed store reservation count changed"));
        }
        let used = Charge {
            bytes: u64_at(bytes, 16)?,
            allocations: u64_at(bytes, 24)?,
            members: u64_at(bytes, 32)?,
        };
        let next_sequence = u64::from(u32_at(bytes, 48)?);
        if next_sequence == 0 {
            return Err(Error::Codec("typed store seal sequence changed"));
        }
        let mut reservations = Vec::new();
        reservations
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let mut offset = LEDGER_HEADER_BYTES;
        for _ in 0..count {
            if bytes[offset + 1..offset + 8].iter().any(|byte| *byte != 0) {
                return Err(Error::Codec("typed store reservation padding changed"));
            }
            reservations.push(Reservation {
                key: LeaseKey::decode(bytes[offset], array_at(bytes, offset + 8)?)?,
                charge: Charge {
                    bytes: u64_at(bytes, offset + 40)?,
                    allocations: u64_at(bytes, offset + 48)?,
                    members: u64_at(bytes, offset + 56)?,
                },
                plan: match bytes[offset + 64] {
                    0 if bytes[offset + 65..offset + RESERVATION_BYTES]
                        .iter()
                        .all(|byte| *byte == 0) =>
                    {
                        None
                    }
                    1 => {
                        if bytes[offset + 66] > 1 || bytes[offset + 67] != 0 {
                            return Err(Error::Codec("typed store plan padding changed"));
                        }
                        Some(Plan {
                            kind: Kind::from_code(bytes[offset + 65])
                                .ok_or(Error::Codec("typed store reservation plan kind changed"))?,
                            schema: u32_at(bytes, offset + 68)?,
                            action: ActionKey::from_bytes(array_at(bytes, offset + 72)?),
                            content: Digest::from_bytes(array_at(bytes, offset + 104)?),
                            bundle: bytes[offset + 66] == 1,
                        })
                    }
                    _ => return Err(Error::Codec("typed store reservation plan changed")),
                },
            });
            offset += RESERVATION_BYTES;
        }
        let mut publishers = Vec::new();
        publishers
            .try_reserve_exact(publisher_count)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..publisher_count {
            let publisher = Digest::from_bytes(array_at(bytes, offset)?);
            if publishers
                .last()
                .is_some_and(|previous| previous >= &publisher)
            {
                return Err(Error::Codec("typed store publisher table ordering changed"));
            }
            publishers.push(publisher);
            offset += 32;
        }
        let mut bundle_members = Vec::new();
        bundle_members
            .try_reserve_exact(bundle_count)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..bundle_count {
            bundle_members.push(u64::from(u32_at(bytes, offset)?));
            offset += 4;
        }
        let mut next_bundle = 0usize;
        let mut allocations = Vec::new();
        allocations
            .try_reserve_exact(allocation_count)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..allocation_count {
            let registered = decode_registered(
                bytes,
                offset,
                &publishers,
                &bundle_members,
                &mut next_bundle,
            )?;
            let sequence = registered.sequence;
            if sequence == 0 || sequence >= next_sequence {
                return Err(Error::Codec("typed store seal sequence changed"));
            }
            allocations.push(registered);
            offset += REGISTERED_BYTES;
        }
        let mut evictions = Vec::new();
        evictions
            .try_reserve_exact(eviction_count)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..eviction_count {
            let registered = decode_registered(
                bytes,
                offset,
                &publishers,
                &bundle_members,
                &mut next_bundle,
            )?;
            let sequence = registered.sequence;
            if sequence == 0 || sequence >= next_sequence {
                return Err(Error::Codec("typed store seal sequence changed"));
            }
            evictions.push(registered);
            offset += REGISTERED_BYTES;
        }
        if next_bundle != bundle_members.len() {
            return Err(Error::Codec("typed store bundle member table changed"));
        }
        let ledger = Self {
            used,
            reservations,
            allocations,
            evictions,
            next_sequence,
        };
        let mut sequences = ledger
            .allocations
            .iter()
            .chain(ledger.evictions.iter())
            .map(|registered| registered.sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        sequences.dedup();
        if next_sequence
            != u64::try_from(sequences.len())
                .map_err(|_| Error::Allocation)?
                .checked_add(1)
                .ok_or(Error::Allocation)?
        {
            return Err(Error::Codec("typed store seal sequence changed"));
        }
        if !ledger.evictions.is_empty() {
            validate_eviction_selection(&ledger.allocations, &ledger.evictions)
                .map_err(|_| Error::Codec("typed store eviction journal changed ownership"))?;
        }
        if ledger.encode()?.as_ref() != bytes {
            return Err(Error::Codec(
                "typed store installation ledger is not canonical",
            ));
        }
        Ok(ledger)
    }
}

/// Whether the store's installation ledger is this format's, or `None` when
/// the store has none yet.
pub(super) fn current_format(root: &Path) -> Result<Option<bool>, Error> {
    let path = ledger_path(root);
    let mut header = [0; 10];
    match fs::File::open(&path).and_then(|mut file| {
        use std::io::Read as _;
        file.read_exact(&mut header)
    }) {
        Ok(()) => Ok(Some(
            &header[..8] == LEDGER_MAGIC
                && u16::from_le_bytes([header[8], header[9]]) == LEDGER_SCHEMA,
        )),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) if source.kind() == std::io::ErrorKind::UnexpectedEof => Ok(Some(false)),
        Err(source) => Err(Error::Io {
            operation: "read typed store format",
            path,
            source,
        }),
    }
}

pub(crate) fn initialize(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let path = ledger_path(root);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            Ledger::read(root)?;
            Ok(())
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            super::recovery::recover_temporaries(publication, root, budget)?;
            let bytes = Ledger::empty().encode()?;
            if publication.root() != root {
                return Err(Error::State(
                    "typed store bootstrap publication authority changed root",
                ));
            }
            let plan = super::control::Plan::replacement(&path, bytes.len());
            let admitted = super::control::Admitted::seal(publication, root, plan)?;
            super::entry::replace_admitted_file(admitted, publication, root, &path, &bytes)?;
            let ledger = Ledger::read(root)?;
            physical(root, &ledger).map(|_| ())
        }
        Err(source) => Err(Error::Io {
            operation: "inspect typed store installation ledger",
            path,
            source,
        }),
    }
}

pub(crate) fn reserve(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    key: LeaseKey,
    charge: Charge,
) -> Result<Reservation, Error> {
    let mut ledger = Ledger::read(root)?;
    ledger.insert_reservation(key, charge)?;
    save(publication, root, &ledger)?;
    Ok(Reservation {
        key,
        charge,
        plan: None,
    })
}

pub(crate) fn available<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    companion: super::control::Plan,
) -> Result<(Capacity<'lease>, super::control::Admitted<'lease>), Error> {
    if publication.root() != root {
        return Err(Error::State(
            "typed store capacity authority changed its canonical root",
        ));
    }
    let ledger = Ledger::read(root)?;
    let next_ledger_length = ledger
        .encode()?
        .len()
        .checked_add(RESERVATION_BYTES)
        .ok_or(Error::Allocation)?;
    let target = ledger_path(root);
    let plan = super::control::Plan::replacement(&target, next_ledger_length);
    let admitted = super::control::Admitted::seal(publication, root, plan)?;
    let companion = super::control::Admitted::seal(publication, root, companion)?;
    Ok((
        Capacity {
            publication,
            root: root.to_path_buf(),
            ledger,
            admitted,
        },
        companion,
    ))
}

pub(crate) fn bundle_count(root: &Path) -> Result<usize, Error> {
    Ok(Ledger::read(root)?
        .allocations
        .iter()
        .filter(|allocation| allocation.allocation.class == Class::Bundle)
        .count())
}

pub(super) fn registrations_recovery(
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<Box<[Registered]>, Error> {
    Ok(Ledger::read_recovery(root, budget)?
        .allocations
        .clone()
        .into_boxed_slice())
}

pub(super) fn garbage_recovery(
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<Option<(PathBuf, Charge)>, Error> {
    let ledger = Ledger::read_recovery(root, budget)?;
    super::control::garbage(
        root,
        &ledger.allocations,
        &ledger.evictions,
        &ledger.reservations,
    )
}

pub(crate) fn begin_eviction(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    selected: &[Registered],
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let mut ledger = (*Ledger::read_recovery(root, budget)?).clone();
    ledger.begin_eviction(selected)?;
    save_recovery(publication, root, ledger, budget)
}

pub(super) fn begin_repair_eviction(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    selected: &[Registered],
    repair: &super::bundle::Repair,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let mut ledger = (*Ledger::read_recovery(root, budget)?).clone();
    ledger.begin_eviction(selected)?;
    let bytes = ledger.encode()?;
    let target = ledger_path(root);
    let plan = super::control::Plan::replacement(&target, bytes.len());
    let admitted = admit_repair_control_with(publication, root, &ledger, plan, repair)?;
    super::entry::replace_admitted_file(admitted, publication, root, &target, &bytes)?;
    budget.retain_installation(root, std::sync::Arc::new(ledger), recovery_metadata(root)?);
    Ok(())
}

pub(super) fn admit_repair_control<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    plan: super::control::Plan,
    repair: &super::bundle::Repair,
    budget: &mut super::recovery::Budget,
) -> Result<super::control::Admitted<'lease>, Error> {
    let ledger = Ledger::read_recovery(root, budget)?;
    admit_repair_control_with(publication, root, &ledger, plan, repair)
}

fn admit_repair_control_with<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    ledger: &Ledger,
    plan: super::control::Plan,
    repair: &super::bundle::Repair,
) -> Result<super::control::Admitted<'lease>, Error> {
    if publication.root() != root {
        return Err(Error::State("bundle repair publication changed store"));
    }
    super::control::inventory_for_repair(
        root,
        &ledger.allocations,
        &ledger.evictions,
        &ledger.reservations,
        &plan,
        repair,
    )?;
    super::control::Admitted::seal(publication, root, plan)
}

pub(super) fn pending_eviction_recovery(
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<Box<[Registered]>, Error> {
    Ok(Ledger::read_recovery(root, budget)?
        .evictions
        .clone()
        .into_boxed_slice())
}

pub(crate) fn finish_eviction(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let mut ledger = (*Ledger::read_recovery(root, budget)?).clone();
    ledger.finish_eviction()?;
    save_recovery(publication, root, ledger, budget)
}

pub(crate) fn cancel(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    reservation: &Reservation,
) -> Result<(), Error> {
    let mut ledger = Ledger::read(root)?;
    ledger.cancel(reservation)?;
    save(publication, root, &ledger)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    reservation: &Reservation,
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
    bundle: bool,
) -> Result<(), Error> {
    // The planned allocations' placement, not the whole store, is checked
    // before the plan is recorded.
    if !bundle {
        for relative in [
            super::action_relative(kind, action),
            super::content_relative(kind, schema, content),
        ] {
            let path = root.join(relative);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(Error::Shape(path));
                }
                Ok(_) => {}
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: "inspect typed store planned allocation",
                        path,
                        source,
                    })
                }
            }
        }
    }
    let mut ledger = Ledger::read(root)?;
    ledger.plan(
        reservation,
        Plan {
            kind,
            schema,
            action,
            content,
            bundle,
        },
    )?;
    save_reserved(publication, root, &ledger, reservation)
}

pub(crate) fn commit(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    reservation: &Reservation,
    records: &[Record],
) -> Result<(), Error> {
    let mut ledger = Ledger::read(root)?;
    ledger.commit(reservation, records)?;
    save(publication, root, &ledger)
}

pub(crate) fn registered_with_budget(
    root: &Path,
    record: &Record,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    let ledger = match budget {
        Some(budget) => Ledger::read_budgeted(root, budget)?,
        None => Ledger::read(root)?,
    };
    registered_in(root, &ledger, record, budget)
}

fn registered_in(
    root: &Path,
    ledger: &Ledger,
    record: &Record,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<(), Error> {
    if let Some(budget) = budget {
        budget.hash(record.encoded_len())?;
    }
    let allocation = record.allocation();
    if ledger.registered(allocation) {
        Ok(())
    } else if ledger.contains_identity(allocation) {
        Err(Error::Corrupt {
            path: ledger_path(root),
            reason: "typed store allocation registry digest changed",
        })
    } else {
        Err(Error::CleanupRequired(
            "typed store allocation is sealed but absent from its installation ledger",
        ))
    }
}

pub(super) fn has_reservation_recovery(
    root: &Path,
    key: LeaseKey,
    budget: &mut super::recovery::Budget,
) -> Result<bool, Error> {
    Ok(Ledger::read_recovery(root, budget)?
        .reservations
        .iter()
        .any(|reservation| reservation.key == key))
}

pub(crate) fn references(
    root: &Path,
    entries: &[crate::node::read_set::Entry],
) -> Result<Box<[Reference]>, Error> {
    if entries.len() > PROJECT_ALLOCATIONS {
        return Err(Error::Limit {
            resource: "typed store project logical allocations",
            limit: PROJECT_ALLOCATIONS,
            actual: entries.len(),
        });
    }
    let ledger = Ledger::read(root)?;
    references_with(&ledger, entries)
}

pub(super) fn references_recovery(
    root: &Path,
    entries: &[crate::node::read_set::Entry],
    budget: &mut super::recovery::Budget,
) -> Result<Box<[Reference]>, Error> {
    let ledger = Ledger::read_recovery(root, budget)?;
    references_with(&ledger, entries)
}

fn references_with(
    ledger: &Ledger,
    entries: &[crate::node::read_set::Entry],
) -> Result<Box<[Reference]>, Error> {
    if entries.len() > PROJECT_ALLOCATIONS {
        return Err(Error::Limit {
            resource: "typed store project logical allocations",
            limit: PROJECT_ALLOCATIONS,
            actual: entries.len(),
        });
    }
    let capacity = entries.len().checked_mul(3).ok_or(Error::Allocation)?;
    let mut references = Vec::new();
    references
        .try_reserve_exact(capacity)
        .map_err(|_| Error::Allocation)?;
    for entry in entries {
        let action = ledger
            .find_registered(Allocation {
                class: Class::Action,
                kind: entry.kind,
                schema: entry.schema,
                key: Digest::from_bytes(entry.action.as_bytes()),
                charge: Charge::zero(),
                record: [0; 32],
            })
            .ok_or(Error::CleanupRequired(
                "a read-set action is absent from the installation registry",
            ))?;
        let content = ledger
            .find_registered(Allocation {
                class: Class::Content,
                kind: entry.kind,
                schema: entry.schema,
                key: entry.content,
                charge: Charge::zero(),
                record: [0; 32],
            })
            .ok_or(Error::CleanupRequired(
                "a read-set content is absent from the installation registry",
            ))?;
        if action.content != entry.content || content.content != entry.content {
            return Err(Error::CleanupRequired(
                "a read-set allocation changed its whole-unit content ownership",
            ));
        }
        references.extend([action.reference(), content.reference()]);

        let bundle = ledger.find_registered(Allocation {
            class: Class::Bundle,
            kind: entry.kind,
            schema: entry.schema,
            key: entry.content,
            charge: Charge::zero(),
            record: [0; 32],
        });
        let owned_bundle = bundle.filter(|bundle| same_seal_unit(action, *bundle));
        match (entry.bundled, owned_bundle) {
            (true, Some(bundle)) => references.push(bundle.reference()),
            (true, None) => {
                return Err(Error::CleanupRequired(
                    "a bundled read-set action is missing its co-owned immutable bundle",
                ));
            }
            (false, Some(_)) => {
                return Err(Error::CleanupRequired(
                    "a read-set action omitted its co-owned immutable bundle",
                ));
            }
            (false, None) => {}
        }
    }
    references.sort_unstable();
    references.dedup();
    if references.len() > PROJECT_ALLOCATIONS {
        return Err(Error::Limit {
            resource: "typed store project logical allocations",
            limit: PROJECT_ALLOCATIONS,
            actual: references.len(),
        });
    }
    Ok(references.into_boxed_slice())
}

pub(crate) fn action_is_bundled_with_budget(
    root: &Path,
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<bool, Error> {
    let ledger = match budget {
        Some(budget) => Ledger::read_budgeted(root, budget)?,
        None => Ledger::read(root)?,
    };
    action_is_bundled_in(&ledger, kind, schema, action, content)
}

/// Check both held seals and their bundle ownership in one current registry read.
pub(super) fn registered_action(
    root: &Path,
    action: &Record,
    content: &Record,
    budget: &crate::node::GraphBudget,
) -> Result<bool, Error> {
    let ledger = Ledger::read_budgeted(root, budget)?;
    registered_in(root, &ledger, action, Some(budget))?;
    registered_in(root, &ledger, content, Some(budget))?;
    action_is_bundled_in(
        &ledger,
        action.identity.kind,
        action.identity.schema,
        ActionKey::from_bytes(action.identity.key.as_bytes()),
        content.identity.key,
    )
}

fn action_is_bundled_in(
    ledger: &Ledger,
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
) -> Result<bool, Error> {
    let action = ledger
        .find_registered(Allocation {
            class: Class::Action,
            kind,
            schema,
            key: Digest::from_bytes(action.as_bytes()),
            charge: Charge::zero(),
            record: [0; 32],
        })
        .ok_or(Error::CleanupRequired(
            "a read-set action is absent from the installation registry",
        ))?;
    if action.content != content {
        return Err(Error::CleanupRequired(
            "a read-set action changed its whole-unit content ownership",
        ));
    }
    Ok(ledger
        .find_registered(Allocation {
            class: Class::Bundle,
            kind,
            schema,
            key: content,
            charge: Charge::zero(),
            record: [0; 32],
        })
        .is_some_and(|bundle| same_seal_unit(action, bundle)))
}

pub(crate) fn validate_references(root: &Path, references: &[Reference]) -> Result<(), Error> {
    let ledger = Ledger::read(root)?;
    validate_reference_set(&ledger.allocations, references)
}

pub(super) fn validate_reference_set(
    registrations: &[Registered],
    references: &[Reference],
) -> Result<(), Error> {
    if references.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(Error::Codec(
            "typed store project allocation references changed order",
        ));
    }
    for reference in references {
        let registered = registrations
            .binary_search_by(|registered| {
                allocation_identity(registered.allocation).cmp(&allocation_identity(reference.0))
            })
            .ok()
            .is_some_and(|index| registrations[index].allocation == reference.0);
        if !registered {
            return Err(Error::CleanupRequired(
                "a project logical allocation left the installation registry",
            ));
        }
    }
    Ok(())
}

pub(crate) fn recover_key(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    key: LeaseKey,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let mut ledger = (*Ledger::read_recovery(root, budget)?).clone();
    let Some(index) = ledger
        .reservations
        .iter()
        .position(|reservation| reservation.key == key)
    else {
        return Ok(());
    };
    let reservation = ledger.reservations[index];
    if let Some(plan) = reservation.plan {
        recover_planned(
            publication,
            root,
            &ledger,
            reservation.key,
            plan,
            Class::Action,
            Digest::from_bytes(plan.action.as_bytes()),
            "envelope",
            crate::node::codec::MAX_ENVELOPE_BYTES,
            budget,
        )?;
        recover_planned(
            publication,
            root,
            &ledger,
            reservation.key,
            plan,
            Class::Content,
            plan.content,
            "payload",
            crate::node::MAX_CONTENT_BYTES,
            budget,
        )?;
        if plan.bundle {
            let registered = ledger.find_identity(Allocation {
                class: Class::Bundle,
                kind: plan.kind,
                schema: plan.schema,
                key: plan.content,
                charge: Charge::zero(),
                record: [0; 32],
            });
            super::bundle::recover_planned(
                root,
                reservation.key,
                plan.kind,
                plan.schema,
                plan.content,
                registered,
                budget,
            )?;
        }
    }
    ledger.reservations.remove(index);
    save_recovery(publication, root, ledger, budget)
}

#[cfg(test)]
pub(super) fn reservation_keys_recovery(
    root: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<Box<[LeaseKey]>, Error> {
    let reservations = Ledger::read_recovery(root, budget)?.reservations.clone();
    let mut keys = Vec::new();
    keys.try_reserve_exact(reservations.len())
        .map_err(|_| Error::Allocation)?;
    for reservation in reservations {
        keys.push(reservation.key);
    }
    Ok(keys.into_boxed_slice())
}

pub(super) fn reservation_keys_header_recovery(root: &Path) -> Result<Box<[LeaseKey]>, Error> {
    let path = ledger_path(root);
    let mut source = fs::File::open(&path).map_err(|source| Error::Io {
        operation: "open typed store installation ledger header",
        path: path.clone(),
        source,
    })?;
    let length = source
        .metadata()
        .map_err(|source| Error::Io {
            operation: "inspect typed store installation ledger header",
            path: path.clone(),
            source,
        })?
        .len();
    if length > u64::try_from(MAX_LEDGER_BYTES).unwrap_or(u64::MAX) {
        return Err(Error::Limit {
            resource: "typed store installation ledger bytes",
            limit: MAX_LEDGER_BYTES,
            actual: usize::try_from(length).unwrap_or(usize::MAX),
        });
    }
    let mut header = [0_u8; LEDGER_HEADER_BYTES];
    source.read_exact(&mut header).map_err(|source| Error::Io {
        operation: "read typed store installation ledger header",
        path: path.clone(),
        source,
    })?;
    if &header[..8] != LEDGER_MAGIC
        || u16_at(&header, 8)? != LEDGER_SCHEMA
        || header[10..16].iter().any(|byte| *byte != 0)
    {
        return Err(Error::Corrupt {
            path,
            reason: "installation ledger header decoding failed",
        });
    }
    let reservation_count = usize::try_from(u32_at(&header, 40)?)
        .map_err(|_| Error::Codec("typed store reservation count is not addressable"))?;
    let allocation_count = usize::try_from(u32_at(&header, 44)?)
        .map_err(|_| Error::Codec("typed store allocation count is not addressable"))?;
    let bundle_count = usize::try_from(u32_at(&header, 52)?)
        .map_err(|_| Error::Codec("typed store bundle count is not addressable"))?;
    let eviction_count = usize::try_from(u32_at(&header, 56)?)
        .map_err(|_| Error::Codec("typed store eviction count is not addressable"))?;
    let publisher_count = usize::try_from(u32_at(&header, 60)?)
        .map_err(|_| Error::Codec("typed store publisher count is not addressable"))?;
    let expected = LEDGER_HEADER_BYTES
        .checked_add(
            reservation_count
                .checked_mul(RESERVATION_BYTES)
                .ok_or(Error::Allocation)?,
        )
        .and_then(|length| length.checked_add(publisher_count.checked_mul(32)?))
        .and_then(|length| length.checked_add(bundle_count.checked_mul(4)?))
        .and_then(|length| length.checked_add(allocation_count.checked_mul(REGISTERED_BYTES)?))
        .and_then(|length| length.checked_add(eviction_count.checked_mul(REGISTERED_BYTES)?))
        .ok_or(Error::Allocation)?;
    if reservation_count > MAX_RESERVATIONS
        || allocation_count > usize::try_from(INSTALLATION_ALLOCATIONS).unwrap_or(usize::MAX)
        || bundle_count > super::bundle::MAX_BUNDLES
        || eviction_count > MAX_EVICTION_ALLOCATIONS
        || publisher_count > super::pins::MAX_PROJECT_LEDGERS
        || u64::try_from(expected).unwrap_or(u64::MAX) != length
    {
        return Err(Error::Corrupt {
            path,
            reason: "installation ledger header counts changed",
        });
    }
    let reservation_bytes = reservation_count
        .checked_mul(RESERVATION_BYTES)
        .ok_or(Error::Allocation)?;
    let mut bytes = vec![0_u8; reservation_bytes];
    source.read_exact(&mut bytes).map_err(|source| Error::Io {
        operation: "read typed store installation reservations",
        path: path.clone(),
        source,
    })?;
    let mut keys = Vec::new();
    keys.try_reserve_exact(reservation_count)
        .map_err(|_| Error::Allocation)?;
    for offset in (0..reservation_bytes).step_by(RESERVATION_BYTES) {
        if bytes[offset + 1..offset + 8].iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt {
                path,
                reason: "installation reservation header changed",
            });
        }
        let key = LeaseKey::decode(bytes[offset], array_at(&bytes, offset + 8)?)?;
        if keys.last().is_some_and(|previous| previous >= &key) {
            return Err(Error::Corrupt {
                path,
                reason: "installation reservation ordering changed",
            });
        }
        keys.push(key);
    }
    Ok(keys.into_boxed_slice())
}

#[cfg(test)]
pub(crate) fn usage(root: &Path) -> Result<(Charge, usize), Error> {
    let ledger = Ledger::read(root)?;
    Ok((ledger.used, ledger.reservations.len()))
}

fn save(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: &Ledger,
) -> Result<(), Error> {
    let bytes = ledger.encode()?;
    if publication.root() != root {
        return Err(Error::State(
            "typed store accounting publication authority changed root",
        ));
    }
    let target = ledger_path(root);
    let plan = super::control::Plan::replacement(&target, bytes.len());
    // A ledger write is admitted by what it changes; the whole-store
    // shape walk stays with bootstrap, garbage recovery and repair.
    let admitted = super::control::Admitted::seal(publication, root, plan)?;
    super::entry::replace_admitted_file(admitted, publication, root, &target, &bytes)
}

fn recovery_metadata(root: &Path) -> Result<fs::Metadata, Error> {
    let path = ledger_path(root);
    let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
        operation: "inspect recovery installation snapshot",
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Shape(path));
    }
    Ok(metadata)
}

fn save_recovery(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: Ledger,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    save(publication, root, &ledger)?;
    budget.retain_installation(root, std::sync::Arc::new(ledger), recovery_metadata(root)?);
    Ok(())
}

fn save_reserved(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: &Ledger,
    reservation: &Reservation,
) -> Result<(), Error> {
    if !ledger
        .reservations
        .iter()
        .any(|current| current.key == reservation.key && current.charge == reservation.charge)
    {
        return Err(Error::State(
            "typed store admitted reservation disappeared before its planned mutation",
        ));
    }
    let bytes = ledger.encode()?;
    if publication.root() != root {
        return Err(Error::State(
            "typed store planned publication authority changed root",
        ));
    }
    let target = ledger_path(root);
    let plan = super::control::Plan::replacement(&target, bytes.len());
    // A ledger write is admitted by what it changes; the whole-store
    // shape walk stays with bootstrap, garbage recovery and repair.
    let admitted = super::control::Admitted::seal(publication, root, plan)?;
    super::entry::replace_admitted_file(admitted, publication, root, &target, &bytes)
}

pub(super) fn admit_control<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    plan: super::control::Plan,
) -> Result<super::control::Admitted<'lease>, Error> {
    admit_control_with(publication, root, &Ledger::read(root)?, plan)
}

pub(super) fn admit_control_recovery<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    plan: super::control::Plan,
    budget: &mut super::recovery::Budget,
) -> Result<super::control::Admitted<'lease>, Error> {
    let ledger = Ledger::read_recovery(root, budget)?;
    admit_control_with(publication, root, &ledger, plan)
}

fn admit_control_with<'lease>(
    publication: &'lease super::lease::Publication<'lease>,
    root: &Path,
    ledger: &Ledger,
    plan: super::control::Plan,
) -> Result<super::control::Admitted<'lease>, Error> {
    if publication.root() != root {
        return Err(Error::State(
            "typed store control publication authority changed root",
        ));
    }
    super::control::admit_plan(root, &ledger.allocations, &plan)?;
    super::control::Admitted::seal(publication, root, plan)
}

fn physical(root: &Path, ledger: &Ledger) -> Result<Charge, Error> {
    ledger.total()?.checked_add(super::control::inventory(
        root,
        &ledger.allocations,
        &ledger.evictions,
        &ledger.reservations,
    )?)
}

#[cfg(test)]
pub(super) fn physical_usage(root: &Path) -> Result<(Charge, Charge), Error> {
    let ledger = Ledger::read(root)?;
    let control = super::control::inventory(
        root,
        &ledger.allocations,
        &ledger.evictions,
        &ledger.reservations,
    )?;
    Ok((ledger.total()?, control))
}

#[allow(clippy::too_many_arguments)]
fn recover_planned(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    ledger: &Ledger,
    reservation: LeaseKey,
    plan: Plan,
    class: Class,
    key: Digest,
    payload_name: &'static str,
    payload_limit: usize,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    let relative = match class {
        Class::Action => PathBuf::from("actions")
            .join(plan.kind.directory())
            .join(crate::node::STORAGE_DIRECTORY)
            .join(plan.action.directory()),
        Class::Content => PathBuf::from("content")
            .join(plan.kind.directory())
            .join(format!("{:08x}", plan.schema))
            .join(plan.content.directory()),
        Class::Bundle => {
            return Err(Error::State(
                "bundle recovery requires its fixed execution placement",
            ));
        }
    };
    let registered = ledger.find_identity(Allocation {
        class,
        kind: plan.kind,
        schema: plan.schema,
        key,
        charge: Charge::zero(),
        record: [0; 32],
    });
    let directory = match super::entry::existing_directory(root, &relative) {
        Ok(Some(directory)) => directory,
        Ok(None) if registered.is_none() => return Ok(()),
        Ok(None) => {
            return Err(Error::CleanupRequired(
                "a registered typed store allocation is missing during recovery",
            ));
        }
        Err(Error::Shape(_) | Error::OutsideRoot(_)) => {
            return Err(Error::CleanupRequired(
                "a planned typed store allocation has an unsafe path shape",
            ));
        }
        Err(error) => return Err(error),
    };
    validate_recovery_members(&directory, payload_name)?;
    let record = Record::read(&directory).map_err(recovery_record_error)?;
    let length = usize::try_from(record.length).map_err(|_| {
        Error::CleanupRequired("a planned typed store allocation length is not addressable")
    })?;
    if length > payload_limit {
        return Err(Error::CleanupRequired(
            "a planned typed store allocation exceeds its payload limit",
        ));
    }
    let payload_path = directory.join(payload_name);
    let payload =
        super::entry::read_file(&payload_path, payload_limit).map_err(recovery_record_error)?;
    let metadata = fs::metadata(&payload_path).map_err(|source| Error::Io {
        operation: "inspect planned typed store recovery payload",
        path: payload_path,
        source,
    })?;
    record
        .validate(class, plan.kind, plan.schema, key, &payload, &metadata)
        .map_err(recovery_record_error)?;
    if let Some(registered) = registered {
        if registered != record.allocation() || record.reservation() == reservation {
            return Err(Error::CleanupRequired(
                "a registered typed store allocation conflicts with its recovery plan",
            ));
        }
        return Ok(());
    }
    if record.reservation() != reservation {
        return Err(Error::CleanupRequired(
            "a planned typed store allocation belongs to another reservation",
        ));
    }
    let allocation = record.allocation();
    budget.deletion(allocation.charge)?;
    let temporary = super::entry::stage_retirement(publication, root, &directory)?;
    fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
        operation: "remove planned typed store allocation during recovery",
        path: temporary.clone(),
        source,
    })?;
    super::entry::sync_directory(temporary.parent().ok_or(Error::State(
        "typed store recovery allocation has no temporary parent",
    ))?)
}

fn validate_recovery_members(directory: &Path, payload_name: &'static str) -> Result<(), Error> {
    let entries = fs::read_dir(directory).map_err(|source| Error::Io {
        operation: "read planned typed store allocation during recovery",
        path: directory.to_path_buf(),
        source,
    })?;
    let mut payload = false;
    let mut allocation = false;
    let mut count = 0usize;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            operation: "read planned typed store allocation member during recovery",
            path: directory.to_path_buf(),
            source,
        })?;
        count = count.saturating_add(1);
        if entry.file_name() == payload_name {
            payload = true;
        } else if entry.file_name() == "allocation" {
            allocation = true;
        } else {
            return Err(Error::CleanupRequired(
                "a planned typed store allocation has an unexpected member",
            ));
        }
    }
    if count != 2 || !payload || !allocation {
        return Err(Error::CleanupRequired(
            "a planned typed store allocation is incomplete",
        ));
    }
    Ok(())
}

fn recovery_record_error(error: Error) -> Error {
    match error {
        Error::Io { .. } | Error::Limit { .. } | Error::Allocation => error,
        _ => Error::CleanupRequired(
            "a planned typed store allocation failed exact recovery validation",
        ),
    }
}

fn ledger_path(root: &Path) -> PathBuf {
    root.join("ledgers/installation")
}

/// The installation size target and the ledger's bundle table are eviction
/// triggers, never refusals: room for one more bundle is made before a write.
pub(super) fn over_cap(root: &Path, additional: Charge) -> Result<bool, Error> {
    // Committed allocations only: a pending reservation is not yet a size.
    let ledger = Ledger::read(root)?;
    let charge = ledger.used.checked_add(additional)?;
    let bundles = ledger
        .allocations
        .iter()
        .filter(|registered| registered.allocation.class == Class::Bundle)
        .count();
    Ok(charge.bytes > INSTALLATION_BYTES
        || charge.allocations > INSTALLATION_ALLOCATIONS
        || charge.members > INSTALLATION_MEMBERS
        || bundles >= super::bundle::MAX_BUNDLES)
}

fn round_block(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK_BYTES - 1)
        .map(|bytes| bytes / BLOCK_BYTES * BLOCK_BYTES)
        .ok_or(Error::Limit {
            resource: "typed store rounded physical bytes",
            limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
            actual: usize::MAX,
        })
}

fn same_identity(left: Allocation, right: Allocation) -> bool {
    left.class == right.class
        && left.kind == right.kind
        && left.schema == right.schema
        && left.key == right.key
}

fn allocation_identity(allocation: Allocation) -> (Class, Kind, u32, Digest) {
    (
        allocation.class,
        allocation.kind,
        allocation.schema,
        allocation.key,
    )
}

fn registered_shape(registered: Registered) -> bool {
    match registered.allocation.class {
        Class::Action => true,
        Class::Content | Class::Bundle => registered.allocation.key == registered.content,
    }
}

fn same_seal_unit(action: Registered, bundle: Registered) -> bool {
    action.allocation.class == Class::Action
        && bundle.allocation.class == Class::Bundle
        && action.allocation.kind == bundle.allocation.kind
        && action.allocation.schema == bundle.allocation.schema
        && action.sequence == bundle.sequence
        && action.publisher == bundle.publisher
        && action.content == bundle.content
        && bundle.allocation.key == action.content
}

fn validate_eviction_selection(
    retained: &[Registered],
    selected: &[Registered],
) -> Result<(), Error> {
    if selected.is_empty() || selected.len() > MAX_EVICTION_ALLOCATIONS {
        return Err(Error::State(
            "typed store eviction allocation count changed",
        ));
    }
    if selected.windows(2).any(|pair| {
        allocation_identity(pair[0].allocation) >= allocation_identity(pair[1].allocation)
    }) {
        return Err(Error::State(
            "typed store eviction allocations changed canonical order",
        ));
    }
    for registered in selected.iter().copied() {
        if !registered_shape(registered) {
            return Err(Error::State(
                "typed store eviction selected an invalid allocation shape",
            ));
        }
        match registered.allocation.class {
            Class::Action => {
                let owned_bundle =
                    retained
                        .iter()
                        .chain(selected.iter())
                        .copied()
                        .find(|candidate| {
                            candidate.allocation.class == Class::Bundle
                                && same_seal_unit(registered, *candidate)
                        });
                if owned_bundle.is_some_and(|bundle| !selected.contains(&bundle)) {
                    return Err(Error::State(
                        "typed store eviction split an action from its immutable bundle",
                    ));
                }
            }
            Class::Bundle => {
                if !selected.iter().copied().any(|action| {
                    action.allocation.class == Class::Action && same_seal_unit(action, registered)
                }) {
                    return Err(Error::State(
                        "typed store eviction split an immutable bundle from its action",
                    ));
                }
            }
            Class::Content => {
                if retained.iter().any(|action| {
                    action.allocation.class == Class::Action
                        && action.allocation.kind == registered.allocation.kind
                        && action.allocation.schema == registered.allocation.schema
                        && action.content == registered.content
                        && !selected.contains(action)
                }) {
                    return Err(Error::State(
                        "typed store eviction selected content retained by another action",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn encode_registered(
    registered: Registered,
    publishers: &[Digest],
    sequences: &[u64],
    bytes: &mut Vec<u8>,
) -> Result<(), Error> {
    let allocation = registered.allocation;
    let sequence = sequences
        .binary_search(&registered.sequence)
        .map_err(|_| Error::State("typed store seal sequence disappeared"))?
        .checked_add(1)
        .and_then(|sequence| u32::try_from(sequence).ok())
        .ok_or(Error::Allocation)?;
    let publisher = publishers
        .binary_search(&registered.publisher)
        .map_err(|_| Error::State("typed store publisher disappeared"))?;
    let publisher = u16::try_from(publisher).map_err(|_| Error::Allocation)?;
    let class = match allocation.class {
        Class::Action => 1,
        Class::Content => 2,
        Class::Bundle => 3,
    };
    bytes.push((class << 4) | allocation.kind as u8);
    bytes.extend_from_slice(&allocation.schema.to_le_bytes());
    bytes.extend_from_slice(&allocation.key.as_bytes());
    bytes.extend_from_slice(&allocation.charge.bytes.to_le_bytes());
    bytes.extend_from_slice(&allocation.record);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&publisher.to_le_bytes());
    bytes.extend_from_slice(&registered.content.as_bytes());
    Ok(())
}

fn decode_registered(
    bytes: &[u8],
    offset: usize,
    publishers: &[Digest],
    bundle_members: &[u64],
    next_bundle: &mut usize,
) -> Result<Registered, Error> {
    let tag = *bytes
        .get(offset)
        .ok_or(Error::Codec("typed store ledger allocation is truncated"))?;
    let class = match tag >> 4 {
        1 => Class::Action,
        2 => Class::Content,
        3 => Class::Bundle,
        _ => return Err(Error::Codec("typed store ledger allocation class changed")),
    };
    let kind = Kind::from_code(tag & 0x0f)
        .ok_or(Error::Codec("typed store ledger allocation kind changed"))?;
    let publisher = usize::from(u16_at(bytes, offset + 81)?);
    let publisher = publishers
        .get(publisher)
        .copied()
        .ok_or(Error::Codec("typed store publisher index changed"))?;
    let members = match class {
        Class::Action | Class::Content => 3,
        Class::Bundle => {
            let members = bundle_members
                .get(*next_bundle)
                .copied()
                .ok_or(Error::Codec("typed store bundle member table is truncated"))?;
            *next_bundle = next_bundle.checked_add(1).ok_or(Error::Allocation)?;
            members
        }
    };
    Ok(Registered {
        allocation: Allocation {
            class,
            kind,
            schema: u32_at(bytes, offset + 1)?,
            key: Digest::from_bytes(array_at(bytes, offset + 5)?),
            charge: Charge {
                bytes: u64_at(bytes, offset + 37)?,
                allocations: 1,
                members,
            },
            record: array_at(bytes, offset + 45)?,
        },
        sequence: u64::from(u32_at(bytes, offset + 77)?),
        publisher,
        content: Digest::from_bytes(array_at(bytes, offset + 83)?),
    })
}

fn encode_allocation(allocation: Allocation, bytes: &mut Vec<u8>) {
    bytes.push(match allocation.class {
        Class::Action => 1,
        Class::Content => 2,
        Class::Bundle => 3,
    });
    bytes.push(allocation.kind as u8);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&allocation.schema.to_le_bytes());
    bytes.extend_from_slice(&allocation.key.as_bytes());
    bytes.extend_from_slice(&allocation.charge.bytes.to_le_bytes());
    bytes.extend_from_slice(&allocation.charge.allocations.to_le_bytes());
    bytes.extend_from_slice(&allocation.charge.members.to_le_bytes());
    bytes.extend_from_slice(&allocation.record);
}

fn decode_allocation(bytes: &[u8], offset: usize) -> Result<Allocation, Error> {
    let class = match bytes.get(offset) {
        Some(1) => Class::Action,
        Some(2) => Class::Content,
        Some(3) => Class::Bundle,
        _ => return Err(Error::Codec("typed store ledger allocation class changed")),
    };
    let kind = bytes
        .get(offset + 1)
        .copied()
        .and_then(Kind::from_code)
        .ok_or(Error::Codec("typed store ledger allocation kind changed"))?;
    if bytes
        .get(offset + 2..offset + 4)
        .ok_or(Error::Codec("typed store ledger allocation is truncated"))?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(Error::Codec(
            "typed store ledger allocation padding changed",
        ));
    }
    Ok(Allocation {
        class,
        kind,
        schema: u32_at(bytes, offset + 4)?,
        key: Digest::from_bytes(array_at(bytes, offset + 8)?),
        charge: Charge {
            bytes: u64_at(bytes, offset + 40)?,
            allocations: u64_at(bytes, offset + 48)?,
            members: u64_at(bytes, offset + 56)?,
        },
        record: array_at(bytes, offset + 64)?,
    })
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    use std::os::unix::fs::MetadataExt as _;

    metadata.blocks().checked_mul(512).ok_or(Error::Limit {
        resource: "typed store allocated bytes",
        limit: usize::try_from(INSTALLATION_BYTES).unwrap_or(usize::MAX),
        actual: usize::MAX,
    })
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    Ok(metadata.len())
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    bytes
        .get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(Error::Codec("typed store accounting record is truncated"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(Error::Codec("typed store accounting record is truncated"))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(Error::Codec("typed store accounting record is truncated"))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], Error> {
    bytes
        .get(offset..offset + 32)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Error::Codec("typed store accounting digest is truncated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_recovers_atomic_replacement_temporary_and_rebuilds_a_partial_final_ledger() {
        let root = std::env::temp_dir().join(format!(
            "rawr-store-accounting-bootstrap-crash-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the bootstrap crash fixture root exists");
        let store =
            crate::store::Store::open(&root, [0x81; 32]).expect("the initial bootstrap completes");
        drop(store);

        let ledger = root.join("ledgers/installation");
        fs::remove_file(&ledger).expect("the final ledger is removed to model pre-rename crash");
        let temporary = root.join("temporaries/91-1");
        fs::write(&temporary, b"partial replacement")
            .expect("the replacement temporary survives the crash");
        let reopened = crate::store::Store::open(&root, [0x81; 32])
            .expect("bootstrap recovers the replacement temporary before retrying atomically");
        assert!(!temporary.exists());
        drop(reopened);

        // A ledger this format did not write is a clean miss: the store is
        // rebuilt, never refused.
        fs::write(&ledger, b"partial final")
            .expect("the fixture replaces the final ledger with partial bytes");
        drop(crate::store::Store::open(&root, [0x81; 32]).expect("the store rebuilds"));
        assert_eq!(
            current_format(&root).expect("the rebuilt ledger reads"),
            Some(true)
        );
        fs::remove_dir_all(root).expect("the bootstrap crash fixture retires");
    }

    fn fixture_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rawr-store-accounting-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the accounting fixture root exists");
        let _store = crate::store::Store::open(&root, [0x91; 32])
            .expect("the accounting fixture store opens");
        let root = fs::canonicalize(root).expect("the accounting fixture canonicalizes");
        published(&root, |publication| {
            super::super::entry::ensure_admitted_directory(
                publication,
                &root,
                Path::new("actions/toolchain/0002"),
            )?;
            super::super::entry::ensure_admitted_directory(
                publication,
                &root,
                Path::new("content/toolchain/00000001"),
            )?;
            Ok(())
        });
        root
    }

    fn published<T>(
        root: &Path,
        evaluate: impl FnOnce(&super::super::lease::Publication<'_>) -> Result<T, Error>,
    ) -> T {
        super::super::lease::with_publication(root, evaluate)
            .expect("the accounting fixture publication succeeds")
    }

    fn publish_fixture(
        root: &Path,
        target: &Path,
        payload_name: &'static str,
        payload: &[u8],
        identity: Identity,
    ) -> Record {
        let (publication, record) = super::super::entry::publish_sealed_directory(
            root,
            target,
            payload_name,
            payload,
            |metadata| Record::seal(identity, payload, metadata),
        )
        .expect("the accounting fixture publishes");
        assert!(matches!(
            publication,
            super::super::entry::Publication::Published
        ));
        record.expect("the new fixture retains its allocation record")
    }

    fn fixture_identity(class: Class, key: Digest, reservation: LeaseKey) -> Identity {
        Identity {
            class,
            kind: Kind::Toolchain,
            schema: 1,
            key,
            reservation,
            publisher: Digest::from_bytes([9; 32]),
        }
    }

    #[test]
    fn ledger_codec_and_exact_installation_floors_are_canonical() {
        let mut ledger = Ledger::empty();
        let key = LeaseKey::Action(ActionKey::from_bytes([7; 32]));
        let charge = Charge {
            bytes: INSTALLATION_BYTES,
            allocations: INSTALLATION_ALLOCATIONS,
            members: INSTALLATION_MEMBERS,
        };
        ledger.reservations.push(Reservation {
            key,
            charge,
            plan: None,
        });
        let bytes = ledger.encode().expect("the exact-cap ledger encodes");
        let decoded = Ledger::decode(&bytes).expect("the exact-cap ledger decodes");
        assert_eq!(decoded.encode().expect("the ledger reencodes"), bytes);
    }

    #[test]
    fn bundle_record_schema_five_round_trips_and_schema_four_bundles_reject() {
        let key = LeaseKey::Execution(ExecutionKey::from_bytes([7; 32]));
        let record = Record {
            identity: fixture_identity(Class::Bundle, Digest::from_bytes([9; 32]), key),
            length: 4,
            digest: Sha256::digest(b"seal").into(),
            charge: Charge {
                bytes: 9 * BLOCK_BYTES,
                allocations: 1,
                members: 9,
            },
            geometry: Some([0x5e; 32]),
        };
        // Schema 5 round-trips the geometry commitment.
        let current = record.encode();
        assert_eq!(current.len(), BUNDLE_RECORD_BYTES);
        assert_eq!(u16_at(&current, 8).unwrap(), BUNDLE_RECORD_SCHEMA);
        let decoded = Record::decode(&current).expect("schema 5 decodes");
        assert_eq!(decoded.sealed_geometry(), Some([0x5e; 32]));
        assert_eq!(decoded.encode(), current);
        // A bundle record without its geometry is another format's.
        let mut legacy = current[..RECORD_BYTES].to_vec();
        legacy[8..10].copy_from_slice(&RECORD_SCHEMA.to_le_bytes());
        assert!(
            Record::decode(&legacy).is_err(),
            "schema 4 bundle records reject"
        );
        // Unknown schemas, length/schema mismatches and typed schema-5 records reject.
        for schema in [3_u16, 6, u16::MAX] {
            let mut unknown = current.to_vec();
            unknown[8..10].copy_from_slice(&schema.to_le_bytes());
            assert!(Record::decode(&unknown).is_err(), "schema {schema}");
        }
        let short = current[..RECORD_BYTES].to_vec();
        assert!(
            Record::decode(&short).is_err(),
            "schema 5 needs its geometry"
        );
        let mut long = legacy.to_vec();
        long.extend_from_slice(&[0x5e; 32]);
        assert!(Record::decode(&long).is_err(), "schema 4 has no geometry");
        let mut typed = current.to_vec();
        typed[10] = 2;
        assert!(
            Record::decode(&typed).is_err(),
            "only bundle records use schema 5"
        );
        // Typed records are unaffected by the bundle bump.
        let content = Record {
            identity: fixture_identity(Class::Content, Digest::from_bytes([3; 32]), key),
            length: 0,
            digest: Sha256::digest([]).into(),
            charge: Charge::allocation(0).expect("allocation charge"),
            geometry: None,
        };
        let content_bytes = content.encode();
        assert_eq!(content_bytes.len(), RECORD_BYTES);
        assert_eq!(u16_at(&content_bytes, 8).unwrap(), RECORD_SCHEMA);
        assert!(Record::decode(&content_bytes).is_ok());
    }

    #[test]
    fn compact_ledger_encodes_all_sixty_five_thousand_allocations_below_recovery_cap() {
        let mut ledger = Ledger::empty();
        ledger
            .allocations
            .try_reserve_exact(INSTALLATION_ALLOCATIONS as usize)
            .expect("the exact allocation registry reserves");
        for index in 0..INSTALLATION_ALLOCATIONS {
            let mut key = [0_u8; 32];
            key[24..].copy_from_slice(&index.to_be_bytes());
            let key = Digest::from_bytes(key);
            let mut publisher = [0_u8; 32];
            publisher[24..].copy_from_slice(
                &(index % super::super::pins::MAX_PROJECT_LEDGERS as u64).to_be_bytes(),
            );
            ledger.allocations.push(Registered {
                allocation: Allocation {
                    class: Class::Content,
                    kind: Kind::Toolchain,
                    schema: 1,
                    key,
                    charge: Charge {
                        bytes: 0,
                        allocations: 1,
                        members: 3,
                    },
                    record: Sha256::digest(index.to_be_bytes()).into(),
                },
                sequence: index + 1,
                publisher: Digest::from_bytes(publisher),
                content: key,
            });
        }
        ledger.used = Charge {
            bytes: 0,
            allocations: INSTALLATION_ALLOCATIONS,
            members: INSTALLATION_ALLOCATIONS * 3,
        };
        ledger.next_sequence = INSTALLATION_ALLOCATIONS + 1;
        let bytes = ledger
            .encode()
            .expect("all declared installation allocations encode");
        assert_eq!(
            bytes.len(),
            LEDGER_HEADER_BYTES
                + INSTALLATION_ALLOCATIONS as usize * REGISTERED_BYTES
                + super::super::pins::MAX_PROJECT_LEDGERS * 32
        );
        assert!(bytes.len() < MAX_LEDGER_BYTES);
        assert!(
            LEDGER_HEADER_BYTES
                + MAX_RESERVATIONS * RESERVATION_BYTES
                + super::super::pins::MAX_PROJECT_LEDGERS * 32
                + super::super::bundle::MAX_BUNDLES * 4
                + INSTALLATION_ALLOCATIONS as usize * REGISTERED_BYTES
                <= MAX_LEDGER_BYTES,
            "every legal registry, reservation, publisher, and bundle-member row fits the recovery byte cap"
        );
        let decoded = Ledger::decode(&bytes).expect("the exact allocation registry decodes");
        assert_eq!(decoded.allocations.len(), INSTALLATION_ALLOCATIONS as usize);
        assert_eq!(
            decoded.encode().expect("the exact registry reencodes"),
            bytes
        );
    }

    #[test]
    fn recovery_snapshot_refreshes_after_intervening_publication() {
        for inspect_garbage in [false, true] {
            let root = fixture_root(if inspect_garbage {
                "epoch-garbage"
            } else {
                "epoch-reservation"
            });
            let abandoned = LeaseKey::Action(ActionKey::from_bytes([71; 32]));
            published(&root, |publication| {
                reserve(
                    publication,
                    &root,
                    abandoned,
                    Charge::node(4096, 4096).expect("abandoned charge"),
                )
            });
            let mut budget = super::super::recovery::Budget::new();
            published(&root, |_| Ledger::read_recovery(&root, &mut budget));

            // A separate publication epoch completes an unrelated direct node.
            let action = ActionKey::from_bytes([72; 32]);
            let content = Digest::from_bytes([73; 32]);
            let key = LeaseKey::Action(action);
            let reservation = published(&root, |publication| {
                reserve(
                    publication,
                    &root,
                    key,
                    Charge::node(4096, 4096).expect("live charge"),
                )
            });
            published(&root, |publication| {
                plan(
                    publication,
                    &root,
                    &reservation,
                    Kind::Toolchain,
                    1,
                    action,
                    content,
                    false,
                )
            });
            let content_path = root
                .join("content/toolchain/00000001")
                .join(content.directory());
            let action_path = root
                .join("actions/toolchain")
                .join(crate::node::STORAGE_DIRECTORY)
                .join(action.directory());
            let content_record = publish_fixture(
                &root,
                &content_path,
                "payload",
                b"live content",
                fixture_identity(Class::Content, content, key),
            );
            let action_record = publish_fixture(
                &root,
                &action_path,
                "envelope",
                b"live action",
                fixture_identity(Class::Action, Digest::from_bytes(action.as_bytes()), key),
            );
            published(&root, |publication| {
                commit(
                    publication,
                    &root,
                    &reservation,
                    &[content_record, action_record],
                )
            });
            if inspect_garbage {
                assert!(
                    published(&root, |_| garbage_recovery(&root, &mut budget)).is_none(),
                    "new registered content must not be garbage in an old snapshot"
                );
            }
            published(&root, |publication| {
                recover_key(publication, &root, abandoned, &mut budget)
            });
            let current = Ledger::read(&root).expect("current ledger");
            assert_eq!(
                current.allocations.len(),
                2,
                "recovery must preserve the intervening publication"
            );
            assert!(current.reservations.is_empty());
            assert!(content_path.exists() && action_path.exists());
            fs::remove_dir_all(root).expect("epoch fixture retires");
        }
    }

    #[test]
    fn block_rounding_is_exact() {
        assert_eq!(
            Charge::allocation(0)
                .expect("the empty payload charge seals")
                .bytes,
            BLOCK_BYTES * 2
        );
        assert_eq!(
            Charge::allocation(1)
                .expect("the first byte charge seals")
                .bytes,
            BLOCK_BYTES * 3
        );
        assert_eq!(
            Charge::allocation(BLOCK_BYTES as usize)
                .expect("the exact block charge seals")
                .bytes,
            BLOCK_BYTES * 3
        );
        assert_eq!(
            Charge::allocation(BLOCK_BYTES as usize + 1)
                .expect("the block-plus-one charge seals")
                .bytes,
            BLOCK_BYTES * 4
        );
    }

    #[test]
    fn allocation_record_round_trips_and_binds_exact_payload() {
        let root = std::env::temp_dir().join(format!("rawr-store-record-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the allocation fixture root exists");
        let payload = root.join("payload");
        fs::write(&payload, b"payload").expect("the fixture payload exists");
        let metadata = fs::metadata(&payload).expect("the fixture payload has metadata");
        let identity = Identity {
            class: Class::Content,
            kind: Kind::Toolchain,
            schema: 1,
            key: Digest::from_bytes([3; 32]),
            reservation: LeaseKey::Execution(ExecutionKey::from_bytes([4; 32])),
            publisher: Digest::from_bytes([5; 32]),
        };
        let (bytes, record) =
            Record::seal(identity, b"payload", &metadata).expect("the allocation record seals");
        let decoded = Record::decode(&bytes).expect("the allocation record decodes");
        decoded
            .validate(
                Class::Content,
                Kind::Toolchain,
                1,
                Digest::from_bytes([3; 32]),
                b"payload",
                &metadata,
            )
            .expect("the exact payload validates");
        assert_eq!(decoded.charge(), record.charge());
        let registered = decoded.allocation();
        let mut changed_audit = decoded;
        changed_audit.identity.publisher = Digest::from_bytes([6; 32]);
        assert!(
            registered != changed_audit.allocation(),
            "the registry digest binds the complete immutable allocation record"
        );
        assert!(decoded
            .validate(
                Class::Content,
                Kind::Toolchain,
                1,
                Digest::from_bytes([3; 32]),
                b"changed",
                &metadata,
            )
            .is_err());
        fs::remove_dir_all(root).expect("the allocation fixture retires");
    }

    #[test]
    fn current_key_recovery_removes_only_its_exact_unregistered_units() {
        let root = fixture_root("recover-unregistered");
        let action = ActionKey::from_bytes([11; 32]);
        let content = Digest::from_bytes([12; 32]);
        let key = LeaseKey::Action(action);
        let reservation = published(&root, |publication| {
            reserve(
                publication,
                &root,
                key,
                Charge::node(4_096, 4_096).expect("the fixture reservation fits"),
            )
        });
        published(&root, |publication| {
            plan(
                publication,
                &root,
                &reservation,
                Kind::Toolchain,
                1,
                action,
                content,
                false,
            )
        });
        let content_path = root
            .join("content/toolchain/00000001")
            .join(content.directory());
        let action_path = root
            .join("actions/toolchain")
            .join(crate::node::STORAGE_DIRECTORY)
            .join(action.directory());
        publish_fixture(
            &root,
            &content_path,
            "payload",
            b"content",
            fixture_identity(Class::Content, content, key),
        );
        publish_fixture(
            &root,
            &action_path,
            "envelope",
            b"action",
            fixture_identity(Class::Action, Digest::from_bytes(action.as_bytes()), key),
        );

        published(&root, |publication| {
            recover_key(
                publication,
                &root,
                key,
                &mut crate::store::recovery::Budget::new(),
            )
        });
        assert!(!content_path.exists());
        assert!(!action_path.exists());
        assert_eq!(
            usage(&root).expect("the recovered ledger reads"),
            (Charge::zero(), 0)
        );
        fs::remove_dir_all(root).expect("the recovery fixture retires");
    }

    #[test]
    fn current_key_recovery_preserves_registered_shared_content() {
        let root = fixture_root("recover-shared");
        let first_action = ActionKey::from_bytes([21; 32]);
        let content = Digest::from_bytes([22; 32]);
        let first_key = LeaseKey::Action(first_action);
        let first = published(&root, |publication| {
            reserve(
                publication,
                &root,
                first_key,
                Charge::node(4_096, 4_096).expect("the first reservation fits"),
            )
        });
        published(&root, |publication| {
            plan(
                publication,
                &root,
                &first,
                Kind::Toolchain,
                1,
                first_action,
                content,
                false,
            )
        });
        let content_path = root
            .join("content/toolchain/00000001")
            .join(content.directory());
        let first_action_path = root
            .join("actions/toolchain")
            .join(crate::node::STORAGE_DIRECTORY)
            .join(first_action.directory());
        let content_record = publish_fixture(
            &root,
            &content_path,
            "payload",
            b"shared",
            fixture_identity(Class::Content, content, first_key),
        );
        let action_record = publish_fixture(
            &root,
            &first_action_path,
            "envelope",
            b"first",
            fixture_identity(
                Class::Action,
                Digest::from_bytes(first_action.as_bytes()),
                first_key,
            ),
        );
        published(&root, |publication| {
            commit(publication, &root, &first, &[content_record, action_record])
        });
        let sealed = Ledger::read(&root).expect("the sequenced ledger reads");
        assert_eq!(sealed.next_sequence, 2);
        assert!(sealed.allocations.iter().all(|registered| {
            registered.sequence == 1 && registered.publisher == Digest::from_bytes([9; 32])
        }));
        let committed = usage(&root).expect("the committed ledger reads");

        let second_action = ActionKey::from_bytes([23; 32]);
        let second_key = LeaseKey::Action(second_action);
        let second = published(&root, |publication| {
            reserve(
                publication,
                &root,
                second_key,
                Charge::node(4_096, 4_096).expect("the second reservation fits"),
            )
        });
        published(&root, |publication| {
            plan(
                publication,
                &root,
                &second,
                Kind::Toolchain,
                1,
                second_action,
                content,
                false,
            )
        });
        published(&root, |publication| {
            recover_key(
                publication,
                &root,
                second_key,
                &mut crate::store::recovery::Budget::new(),
            )
        });

        assert!(content_path.is_dir());
        assert!(first_action_path.is_dir());
        assert_eq!(usage(&root).expect("the recovered ledger reads"), committed);
        fs::remove_dir_all(root).expect("the shared recovery fixture retires");
    }

    #[test]
    fn one_seal_sequence_rejects_mixed_publishers() {
        let action = ActionKey::from_bytes([0x61; 32]);
        let content = Digest::from_bytes([0x62; 32]);
        let key = LeaseKey::Action(action);
        let charge = Charge {
            bytes: BLOCK_BYTES * 3,
            allocations: 1,
            members: 3,
        };
        let reservation = Reservation {
            key,
            charge: charge.checked_add(charge).expect("the charges add"),
            plan: Some(Plan {
                kind: Kind::Toolchain,
                schema: 1,
                action,
                content,
                bundle: false,
            }),
        };
        let record = |class, digest, publisher| Record {
            identity: Identity {
                class,
                kind: Kind::Toolchain,
                schema: 1,
                key: digest,
                reservation: key,
                publisher,
            },
            length: 0,
            digest: [0; 32],
            charge,
            geometry: None,
        };
        let records = [
            record(
                Class::Action,
                Digest::from_bytes(action.as_bytes()),
                Digest::from_bytes([0x63; 32]),
            ),
            record(Class::Content, content, Digest::from_bytes([0x64; 32])),
        ];
        let mut ledger = Ledger::empty();
        ledger.reservations.push(reservation);

        assert!(matches!(
            ledger.commit(&reservation, &records),
            Err(Error::State(
                "typed store publication changed publisher within one seal unit"
            ))
        ));
        assert!(ledger.allocations.is_empty());
        assert_eq!(ledger.reservations.len(), 1);
        assert_eq!(ledger.used, Charge::zero());
        assert_eq!(ledger.next_sequence, 1);
    }

    #[test]
    fn runner_read_set_rejects_a_committed_node_without_its_bundle_allocation() {
        let root = fixture_root("runner-reference");
        published(&root, |publication| {
            super::super::entry::ensure_admitted_directory(
                publication,
                &root,
                Path::new("actions/runner/0002"),
            )?;
            super::super::entry::ensure_admitted_directory(
                publication,
                &root,
                Path::new("content/runner/00000001"),
            )?;
            Ok(())
        });
        let action = ActionKey::from_bytes([0x71; 32]);
        let content = Digest::from_bytes([0x72; 32]);
        let key = LeaseKey::Action(action);
        let reservation = published(&root, |publication| {
            reserve(
                publication,
                &root,
                key,
                Charge::node(4_096, 4_096).expect("the runner reservation fits"),
            )
        });
        published(&root, |publication| {
            plan(
                publication,
                &root,
                &reservation,
                Kind::Runner,
                1,
                action,
                content,
                false,
            )
        });
        let identity = |class, digest| Identity {
            class,
            kind: Kind::Runner,
            schema: 1,
            key: digest,
            reservation: key,
            publisher: Digest::from_bytes([0x73; 32]),
        };
        let content_record = publish_fixture(
            &root,
            &root
                .join("content/runner/00000001")
                .join(content.directory()),
            "payload",
            b"runner-content",
            identity(Class::Content, content),
        );
        let action_record = publish_fixture(
            &root,
            &root
                .join("actions/runner")
                .join(crate::node::STORAGE_DIRECTORY)
                .join(action.directory()),
            "envelope",
            b"runner-action",
            identity(Class::Action, Digest::from_bytes(action.as_bytes())),
        );
        published(&root, |publication| {
            commit(
                publication,
                &root,
                &reservation,
                &[content_record, action_record],
            )
        });

        let entry = crate::node::read_set::Entry {
            kind: Kind::Runner,
            schema: 1,
            action,
            recipe: Digest::from_bytes([0x74; 32]),
            content,
            bundled: true,
        };
        assert!(matches!(
            references(&root, &[entry]),
            Err(Error::CleanupRequired(
                "a bundled read-set action is missing its co-owned immutable bundle"
            ))
        ));
        fs::remove_dir_all(root).expect("the runner-reference fixture retires");
    }

    #[test]
    fn current_key_recovery_cancels_an_unplanned_reservation() {
        let root = fixture_root("recover-unplanned");
        let key = LeaseKey::Execution(ExecutionKey::from_bytes([31; 32]));
        published(&root, |publication| {
            reserve(
                publication,
                &root,
                key,
                Charge::node(4_096, 4_096).expect("the fixture reservation fits"),
            )
        });
        published(&root, |publication| {
            recover_key(
                publication,
                &root,
                key,
                &mut crate::store::recovery::Budget::new(),
            )
        });
        assert_eq!(
            usage(&root).expect("the recovered ledger reads"),
            (Charge::zero(), 0)
        );
        fs::remove_dir_all(root).expect("the unplanned recovery fixture retires");
    }

    #[cfg(unix)]
    #[test]
    fn current_key_recovery_rejects_a_foreign_path_without_deleting_it() {
        use std::os::unix::fs::symlink;

        let root = fixture_root("recover-foreign");
        let outside = fixture_root("recover-foreign-outside");
        let action = ActionKey::from_bytes([41; 32]);
        let content = Digest::from_bytes([42; 32]);
        let key = LeaseKey::Action(action);
        let reservation = published(&root, |publication| {
            reserve(
                publication,
                &root,
                key,
                Charge::node(4_096, 4_096).expect("the fixture reservation fits"),
            )
        });
        published(&root, |publication| {
            plan(
                publication,
                &root,
                &reservation,
                Kind::Toolchain,
                1,
                action,
                content,
                false,
            )
        });
        let action_path = root
            .join("actions/toolchain")
            .join(crate::node::STORAGE_DIRECTORY)
            .join(action.directory());
        symlink(&outside, &action_path).expect("the foreign fixture link exists");

        assert!(matches!(
            super::super::lease::with_publication(&root, |publication| {
                recover_key(
                    publication,
                    &root,
                    key,
                    &mut crate::store::recovery::Budget::new(),
                )
            }),
            Err(Error::CleanupRequired(_))
        ));
        assert!(action_path
            .symlink_metadata()
            .expect("the foreign fixture link remains")
            .file_type()
            .is_symlink());
        assert!(outside.is_dir());
        assert_eq!(usage(&root).expect("the blocked ledger reads").1, 1);
        fs::remove_dir_all(root).expect("the foreign recovery fixture retires");
        fs::remove_dir_all(outside).expect("the outside fixture retires");
    }
}
