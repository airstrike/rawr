//! Single-use activation proof bound to a complete generation.

use std::fs::File;
use std::io::Read as _;

use sha2::{Digest as _, Sha256};

/// Unforgeable, single-use candidate activation authority.
pub struct Capability {
    bytes: [u8; 32],
    key: super::GenerationKey,
}

/// Exact proof returned after consuming a generation-bound capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt([u8; 32]);

impl Capability {
    pub(super) fn issue(key: super::GenerationKey) -> std::io::Result<Self> {
        let mut random = File::open("/dev/urandom")?;
        let mut bytes = [0_u8; 32];
        random.read_exact(&mut bytes)?;
        Ok(Self { bytes, key })
    }

    pub(crate) fn consume(self, key: super::GenerationKey) -> Result<([u8; 32], Receipt), Error> {
        if self.key != key {
            return Err(Error::Foreign);
        }
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.activation-receipt/v1");
        digest.update(key.as_bytes());
        digest.update(self.bytes);
        Ok((self.bytes, Receipt(digest.finalize().into())))
    }
}

impl Receipt {
    #[cfg(feature = "test-fixtures")]
    pub(crate) const fn fixture(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn advance(self, key: super::GenerationKey) -> Self {
        Self(advance(self.0, key.as_bytes()))
    }
}

pub(crate) fn advance(previous: [u8; 32], key: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.activation-receipt/advance/v1");
    digest.update(previous);
    digest.update(key);
    digest.finalize().into()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("activation capability belongs to a different complete generation")]
    Foreign,
}
