//! Monotonic generation authority owned by the runner supervisor.

use std::num::NonZeroU64;

pub mod activation;
pub mod bootstrap;
pub mod slot;
pub mod snapshot;

use sha2::{Digest as _, Sha256};

/// One supervisor-assigned symbolic compilation revision.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(NonZeroU64);

/// One supervisor-assigned whole-process epoch.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Epoch(NonZeroU64);

/// Complete request, epoch, and artifact identity carried by state changes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GenerationKey([u8; 32]);

/// The sole allocator of generation revisions.
#[derive(Debug, Default)]
pub struct Owner {
    revision: u64,
    epoch: u64,
}

impl Owner {
    /// Create an empty supervisor revision authority.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            revision: 0,
            epoch: 0,
        }
    }

    /// Allocate the next revision, failing closed at numeric exhaustion.
    pub fn revision(&mut self) -> Result<Revision, Error> {
        self.revision = self.revision.checked_add(1).ok_or(Error::Exhausted)?;
        NonZeroU64::new(self.revision)
            .map(Revision)
            .ok_or(Error::Exhausted)
    }

    /// Allocate the next whole-process epoch.
    pub fn epoch(&mut self) -> Result<Epoch, Error> {
        self.epoch = self.epoch.checked_add(1).ok_or(Error::Exhausted)?;
        NonZeroU64::new(self.epoch)
            .map(Epoch)
            .ok_or(Error::Exhausted)
    }

    /// Issue one activation capability bound to a complete generation key.
    pub fn activation(&mut self, key: GenerationKey) -> Result<activation::Capability, Error> {
        activation::Capability::issue(key).map_err(Error::Random)
    }
}

impl Revision {
    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }
}

impl Epoch {
    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }
}

impl GenerationKey {
    pub(crate) fn resident(evidence: &crate::protocol::resident::Compiled) -> Self {
        Self(evidence.key())
    }

    pub(crate) fn seal(
        build: crate::bridge::BuildRequestKey,
        snapshot: crate::surface::Snapshot,
        demand: &[u8],
        revision: Revision,
        epoch: Epoch,
        artifact: &[u8; 32],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.generation-key/v1");
        digest.update(build.as_bytes());
        digest.update(snapshot.as_bytes());
        digest.update(Sha256::digest(demand));
        digest.update(revision.get().to_le_bytes());
        digest.update(epoch.get().to_le_bytes());
        digest.update(artifact);
        Self(digest.finalize().into())
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Bind deterministic integration-fixture bytes to one generation key.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub const fn fixture(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Supervisor identity allocation failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("supervisor revision space is exhausted")]
    Exhausted,
    #[error("cannot acquire cryptographic supervisor capability bytes")]
    Random(#[source] std::io::Error),
}
