//! Opaque symbolic imported type identity.

/// Stable package/source/definition identity with no native `TypeId` conversion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity([u8; 32]);

impl Identity {
    #[cfg(feature = "rust-import")]
    pub(crate) const fn new(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    /// Borrow the exact symbolic identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
