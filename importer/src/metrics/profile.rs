//! Sealed profile-fixture observation with no selection authority.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Closed source of one feature-built profile definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum Selection {
    Inactive,
    Baseline,
    Variant,
}

/// Effective linked fixture after inactive selection resolves to the baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum Fixture {
    Baseline,
    Variant,
}

/// Exact profile facts retained after normal classification and generation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Evidence {
    pub selection: Selection,
    pub fixture: Fixture,
    pub identity: [u8; 32],
    pub version: u32,
    pub rules: [u8; 32],
    pub host_agreement: [u8; 32],
}

impl Evidence {
    /// Seal already-selected profile facts without returning product authority.
    #[must_use]
    pub fn observe(
        selection: Selection,
        identity: [u8; 32],
        version: u32,
        rules: [u8; 32],
    ) -> Self {
        let fixture = match selection {
            Selection::Inactive | Selection::Baseline => Fixture::Baseline,
            Selection::Variant => Fixture::Variant,
        };
        let mut agreement = Sha256::new();
        agreement.update(b"rawr.phase9.profile-host-agreement/v1");
        agreement.update([fixture as u8]);
        agreement.update(identity);
        agreement.update(version.to_le_bytes());
        agreement.update(rules);
        Self {
            selection,
            fixture,
            identity,
            version,
            rules,
            host_agreement: agreement.finalize().into(),
        }
    }

    pub(super) fn valid(&self) -> bool {
        *self == Self::observe(self.selection, self.identity, self.version, self.rules)
            && self.identity != [0; 32]
            && self.version != 0
            && self.rules != [0; 32]
    }
}
