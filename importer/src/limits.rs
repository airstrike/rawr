//! The resource envelope every import works inside: how many items, bytes
//! and nesting levels a contract, a surface and a runner may hold.
//!
//! The model checks these figures wherever it runs. The host adds the
//! physical bounds of the processes it starts ([`crate::process::Limits`]),
//! which a runner on wasm has none of.

#[cfg(not(target_arch = "wasm32"))]
use sha2::{Digest as _, Sha256};

/// The standard Cargo JSON figure. It is distinct from the physical child
/// capture bound `process::CAPTURE_BYTES`, which refuses.
pub(crate) const JSON_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MANIFEST_BYTES: usize = 1024 * 1024;
pub(crate) const PACKAGES: usize = 4_096;
pub(crate) const UNITS: usize = 500_000;
pub(crate) const EDGES: usize = 1_000_000;
pub(crate) const ITEMS: usize = 2_500_000;
pub(crate) const CANONICAL_ITEMS: usize = 19_000_000;
/// The canonical bytes figure the driver's plan carries; an accounting
/// figure, never a bound.
pub(crate) const CANONICAL_BYTES: usize = 1_207_959_552;
pub(crate) const INPUT_PROOF_BYTES: usize = 512 * 1024 * 1024;

pub(crate) const RETAINED_GRAPH_ITEMS: usize = 6_500_000;
pub(crate) const RETAINED_GRAPH_BYTES: usize = 512 * 1024 * 1024;
pub(crate) const CLASSIFIED_CONTRACT_ITEMS: usize = 6_500_000;
pub(crate) const CLASSIFIED_CONTRACT_BYTES: usize = 640 * 1024 * 1024;
pub(crate) const CONTAINERS: usize = 1_500_000;
pub(crate) const STRING_BYTES: usize = 1024 * 1024;
pub(crate) const DEPTH: usize = 256;

/// Frozen allocation and subprocess limits required by Cargo resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub(crate) json_bytes: usize,
    pub(crate) manifest_bytes: usize,
    pub(crate) packages: usize,
    pub(crate) units: usize,
    pub(crate) edges: usize,
    pub(crate) items: usize,
    pub(crate) canonical_items: usize,
    pub(crate) canonical_bytes: usize,
    pub(crate) input_proof_bytes: usize,
    pub(crate) retained_graph_items: usize,
    pub(crate) retained_graph_bytes: usize,
    pub(crate) classified_contract_items: usize,
    pub(crate) classified_contract_bytes: usize,
    pub(crate) containers: usize,
    pub(crate) string_bytes: usize,
    pub(crate) depth: usize,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) process: crate::process::Limits,
}

impl Limits {
    /// Return the aggregate retained-item ceiling for generated consumers.
    #[doc(hidden)]
    #[must_use]
    pub const fn item_limit(&self) -> usize {
        self.items
    }

    /// Return the aggregate retained-byte ceiling for generated consumers.
    #[doc(hidden)]
    #[must_use]
    pub const fn byte_limit(&self) -> usize {
        self.json_bytes
    }
}

impl Limits {
    /// The frozen Phase-1 resolution envelope.
    pub const fn standard() -> Self {
        Self {
            json_bytes: JSON_BYTES,
            manifest_bytes: MANIFEST_BYTES,
            packages: PACKAGES,
            units: UNITS,
            edges: EDGES,
            items: ITEMS,
            canonical_items: CANONICAL_ITEMS,
            canonical_bytes: CANONICAL_BYTES,
            input_proof_bytes: INPUT_PROOF_BYTES,
            retained_graph_items: RETAINED_GRAPH_ITEMS,
            retained_graph_bytes: RETAINED_GRAPH_BYTES,
            classified_contract_items: CLASSIFIED_CONTRACT_ITEMS,
            classified_contract_bytes: CLASSIFIED_CONTRACT_BYTES,
            containers: CONTAINERS,
            string_bytes: STRING_BYTES,
            depth: DEPTH,
            #[cfg(not(target_arch = "wasm32"))]
            process: crate::process::Limits::capture(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) const fn process_limits(self) -> crate::process::Limits {
        self.process
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn bind_node_context(&self, digest: &mut Sha256) {
        // Only the physical bounds that still refuse bind node identity.
        digest.update(b"rawr.import.cargo.limits/v3");
        for value in [
            self.depth as u64,
            self.process.stdout() as u64,
            self.process.stderr() as u64,
        ] {
            digest.update(value.to_le_bytes());
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_depth(self, depth: usize) -> Self {
        Self { depth, ..self }
    }

    #[cfg(test)]
    pub(crate) const fn fixture(json_bytes: usize, items: usize, string_bytes: usize) -> Self {
        Self {
            json_bytes,
            items,
            string_bytes,
            ..Self::standard()
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::standard()
    }
}
