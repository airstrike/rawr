//! Retained protected bytes, without a completed universe association.

use sha2::{Digest as _, Sha256};

use super::codec::{Reader, Writer};
use super::{Error, Result};
use crate::cargo::universe;

const DOMAIN: &[u8] = b"rawr.import.cargo.protected-source-build/v1";

/// Historical bytes with a checked digest and outer schema tag. Their protected
/// entry grammar/canonical form and current universe association are unproved;
/// a consumer must authenticate them against the current exact expected asset.
pub(crate) struct Projection {
    bytes: Bytes,
    digest: [u8; 32],
}

enum Bytes {
    Captured(universe::BuildProjection),
    Decoded(Vec<u8>),
}

impl Projection {
    pub(super) fn capture(value: universe::BuildProjection) -> Self {
        Self {
            digest: *value.digest().as_bytes(),
            bytes: Bytes::Captured(value),
        }
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        match &self.bytes {
            Bytes::Captured(value) => value.bytes(),
            Bytes::Decoded(bytes) => bytes,
        }
    }
    pub(crate) const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    #[cfg(test)]
    pub(crate) const fn schema(&self) -> u32 {
        universe::BUILD_PROJECTION_SCHEMA
    }
}

impl Writer<'_> {
    pub(super) fn projection(&mut self, value: Option<&Projection>) -> Result<()> {
        match value {
            None => self.byte(0),
            Some(value) => {
                self.byte(1)?;
                self.u32(universe::BUILD_PROJECTION_SCHEMA)?;
                self.bytes(value.bytes())?;
                self.raw(&value.digest)
            }
        }
    }
}

impl Reader<'_, '_, '_> {
    pub(super) fn projection(&mut self) -> Result<Option<Projection>> {
        match self.byte()? {
            0 => Ok(None),
            1 => {
                if self.u32()? != universe::BUILD_PROJECTION_SCHEMA {
                    return Err(Error::Encoding("unknown protected projection schema"));
                }
                let bytes = self.bytes()?;
                if bytes.is_empty() {
                    return Err(Error::Encoding("empty protected projection"));
                }
                let digest = self.digest()?;
                self.budget.hash(
                    DOMAIN
                        .len()
                        .checked_add(bytes.len())
                        .ok_or(Error::Encoding("projection hash length overflow"))?,
                )?;
                let mut actual = Sha256::new();
                actual.update(DOMAIN);
                actual.update(bytes);
                if <[u8; 32]>::from(actual.finalize()) != digest {
                    return Err(Error::Encoding("protected projection digest differs"));
                }
                Ok(Some(Projection {
                    bytes: Bytes::Decoded(self.budget.bytes(bytes)?),
                    digest,
                }))
            }
            _ => Err(Error::Encoding("unknown protected projection tag")),
        }
    }
}
