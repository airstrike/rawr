//! A value's encoding as the store keeps it.

use std::path::Path;

use crate::node::Error;

/// What a publication hands the store for one content payload.
pub enum Payload {
    /// The encoding, kept as one file.
    Whole(Box<[u8]>),
    /// The encoding, kept as the manifest of its parts (`super::parts`).
    Parts(Box<[u8]>),
    /// The manifest of a value read back from the store, whose parts the
    /// store already keeps.
    Manifest(Box<[u8]>),
}

impl Payload {
    /// The bytes the content payload holds, keeping any part not yet kept.
    /// Runs under the publication lease, so no collection runs meanwhile.
    pub(super) fn stored(self, parts: &Path) -> Result<Box<[u8]>, Error> {
        match self {
            Self::Whole(bytes) => Ok(bytes),
            Self::Parts(bytes) => super::parts::write(parts, &bytes),
            Self::Manifest(manifest) => {
                super::parts::present(parts, &manifest)?;
                Ok(manifest)
            }
        }
    }
}
