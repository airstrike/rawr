//! Rawr contract fingerprint distinct from raw Rust API identity.

/// SHA-256 identity of one canonical classified compiler contract.
///
/// ```compile_fail
/// let _ = rawr_import::contract::fingerprint::Fingerprint([0; 32]);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint(pub(crate) [u8; 32]);

impl Fingerprint {
    /// Borrow the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
