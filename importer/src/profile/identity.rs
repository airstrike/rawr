//! Stable profile identity and rule version.

use sha2::{Digest as _, Sha256};

/// Stable semantic identity of one framework profile.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity(pub(super) [u8; 32]);

/// Nonzero semantic rule version chosen by the profile owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Version(pub(super) u32);

impl Identity {
    /// Transform one bounded canonical profile name into its stable identity.
    /// rawr names its profiles itself, so an empty name panics (D664).
    #[must_use]
    pub fn new(name: &str, limits: &crate::cargo::Limits) -> Self {
        assert!(!name.is_empty(), "rawr names each profile");
        // The name length is an accounting target recorded on a profile
        // account, never a refusal.
        crate::surface::account::Account::profile().exceeded(
            "profile identity bytes",
            limits.string_bytes,
            name.len(),
        );
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.profile.identity/v1");
        digest.update(u64::try_from(name.len()).unwrap_or(u64::MAX).to_le_bytes());
        digest.update(name.as_bytes());
        Self(digest.finalize().into())
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Borrow the stable identity bytes for observational evidence.
    #[doc(hidden)]
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Version {
    /// Transform a nonzero profile rule version into sealed evidence.
    /// rawr numbers its profiles itself, so version 0 panics (D664).
    #[must_use]
    pub const fn new(version: u32) -> Self {
        assert!(version != 0, "rawr numbers each profile's rules from 1");
        Self(version)
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }

    /// Return the semantic rule version for observational evidence.
    #[doc(hidden)]
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}
