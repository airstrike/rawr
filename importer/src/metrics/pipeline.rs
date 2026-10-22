//! Stable baseline observations of the current linear transform products.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub mod target;

/// Exact title-independent identities at every pre-runner transform seam.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Evidence {
    pub toolchain: [u8; 32],
    pub resolution: [u8; 32],
    pub api: [u8; 32],
    pub generic: [u8; 32],
    pub profile: [u8; 32],
    pub facade: [u8; 32],
    pub checked: [u8; 32],
    pub profile_proof: [u8; 32],
    pub native_inputs: [u8; 32],
    pub native_product: [u8; 32],
    pub overlay: [u8; 32],
    /// Actual owned resolution association; absent in historical baseline evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_target: Option<target::Target>,
    /// Actual owned runner association; absent in historical baseline evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_target: Option<target::Target>,
}

impl Evidence {
    pub(super) fn valid(&self) -> bool {
        [
            self.toolchain,
            self.resolution,
            self.api,
            self.generic,
            self.profile,
            self.facade,
            self.checked,
            self.profile_proof,
            self.native_inputs,
            self.native_product,
            self.overlay,
        ]
        .into_iter()
        .all(|identity| identity != [0; 32])
            && self.resolution_target.is_none_or(target::Target::valid)
            && self.runner_target.is_none_or(target::Target::valid)
    }
}

/// Domain-separate two sealed identities without allocating an authority object.
#[must_use]
pub fn digest_pair(domain: &'static [u8], left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.pipeline-pair-observation/v1");
    digest.update((domain.len() as u64).to_le_bytes());
    digest.update(domain);
    digest.update(left);
    digest.update(right);
    digest.finalize().into()
}

/// Domain-separate one canonical byte product for a read-only report identity.
#[must_use]
pub fn digest(domain: &'static [u8], bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.pipeline-observation/v1");
    digest.update((domain.len() as u64).to_le_bytes());
    digest.update(domain);
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn historical_pipeline_bytes_round_trip_without_new_null_fields() {
        // Ordered historical wire shape, before bundle associations existed.
        // Compare bytes, not JSON values: capsule commitments hash canonical bytes.
        let fields = [
            "toolchain",
            "resolution",
            "api",
            "generic",
            "profile",
            "facade",
            "checked",
            "profile_proof",
            "native_inputs",
            "native_product",
            "overlay",
        ];
        let old = format!(
            "{{{}}}",
            fields
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    format!(
                        "\"{name}\":{}",
                        serde_json::to_string(&[index as u8 + 1; 32]).unwrap()
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        let observed: super::Evidence = serde_json::from_str(&old).unwrap();
        assert!(observed.valid());
        assert_eq!(observed.resolution_target, None);
        assert_eq!(observed.runner_target, None);
        assert_eq!(serde_json::to_vec(&observed).unwrap(), old.as_bytes());

        let target = super::target::Target::observe(crate::store::bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes([1; 32]),
            crate::store::bundle::BundleId::from_bytes([2; 32]),
        ));
        let mut current = observed;
        current.resolution_target = Some(target);
        let bytes = serde_json::to_vec(&current).unwrap();
        let decoded: super::Evidence = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, current);
        assert_eq!(decoded.runner_target, None);
        assert_ne!(bytes, old.as_bytes());
    }
}
