//! Content identity for one exact normal-build input proof.

use serde::Serialize;

/// A SHA-256 identity of exact content-based normal-build inputs.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub(super) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the exact SHA-256 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
