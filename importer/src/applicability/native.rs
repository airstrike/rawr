//! Consuming normal-build authority, separate from independent API inventories.

use sha2::{Digest as _, Sha256};

use super::Catalog;
use crate::{cargo, input, process, Result};

#[cfg(test)]
mod tests;

/// A revalidated normal build with no metadata or contextual API capture.
pub struct Inputs {
    proof: input::proof::Proof,
    partitions: input::native::partitions::Partitions,
}

/// A consuming build whose exact native inputs agree with every inventory unit.
pub struct Bound {
    inputs: Inputs,
    catalog: [u8; 32],
    fingerprint: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native input equivalence is unknown for compiler unit {0}")]
    Unknown(Box<str>),
}

impl Inputs {
    pub fn capture(
        request: cargo::Request,
        native: &process::Native,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let resolution = request.native_inputs(native, limits)?;
        resolution.publication(native, limits, |resolution| {
            resolution.into_native_inputs(limits)
        })
    }

    pub(crate) fn from_capture(
        proof: input::proof::Proof,
        partitions: input::native::partitions::Partitions,
    ) -> Self {
        Self { proof, partitions }
    }

    pub const fn fingerprint(&self) -> input::fingerprint::Fingerprint {
        self.proof.fingerprint()
    }
}

impl Catalog<'_> {
    pub fn bind_native(&self, inputs: Inputs) -> std::result::Result<Bound, Error> {
        for partition in self.partitions.values() {
            if inputs.partitions.get(&partition.native_unit) != Some(partition.native_inputs) {
                return Err(Error::Unknown(partition.unit.crate_name().into()));
            }
        }
        let mut identity = Sha256::new();
        identity.update(b"rawr.import.applicability.native/v1");
        identity.update(self.fingerprint());
        identity.update(inputs.proof.fingerprint().as_bytes());
        Ok(Bound {
            inputs,
            catalog: *self.fingerprint(),
            fingerprint: identity.finalize().into(),
        })
    }
}

impl Bound {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub const fn catalog(&self) -> &[u8; 32] {
        &self.catalog
    }

    pub const fn proof(&self) -> &input::proof::Proof {
        &self.inputs.proof
    }
}
