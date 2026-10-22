use std::fmt;

use sha2::{Digest as _, Sha256};

use super::content::Edge;
use super::{Kind, STORAGE_SCHEMA};

/// Complete identity of one direct transform action.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct ActionKey([u8; 32]);

/// Bounded non-authoritative bucket identity for discoverable candidates.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct LookupKey([u8; 32]);

/// Pre-execution identity used to serialize one discoverable attempt.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExecutionKey([u8; 32]);

/// SHA-256 identity retained without exposing mutable bytes.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 32]);

impl ActionKey {
    pub(crate) fn derive(
        kind: Kind,
        schema: u32,
        context: Digest,
        semantic_inputs: &[u8],
        output_limit: usize,
        parents: &[impl Edge],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.node.action/v1");
        digest.update(STORAGE_SCHEMA.to_le_bytes());
        digest.update([kind as u8]);
        digest.update(schema.to_le_bytes());
        digest.update(context.0);
        digest.update((output_limit as u64).to_le_bytes());
        digest.update((semantic_inputs.len() as u64).to_le_bytes());
        digest.update(semantic_inputs);
        retain_semantic_parents(&mut digest, parents);
        Self(digest.finalize().into())
    }

    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn directory(self) -> String {
        lower_hex(&self.0)
    }
}

impl LookupKey {
    pub(super) fn derive(
        kind: Kind,
        schema: u32,
        context: Digest,
        fixed_inputs: &[u8],
        output_limit: usize,
        parents: &[impl Edge],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.node.lookup/v1");
        digest.update(STORAGE_SCHEMA.to_le_bytes());
        digest.update([kind as u8]);
        digest.update(schema.to_le_bytes());
        digest.update(context.0);
        digest.update((output_limit as u64).to_le_bytes());
        digest.update((fixed_inputs.len() as u64).to_le_bytes());
        digest.update(fixed_inputs);
        retain_semantic_parents(&mut digest, parents);
        Self(digest.finalize().into())
    }

    pub(crate) const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn directory(self) -> String {
        lower_hex(&self.0)
    }
}

impl ExecutionKey {
    pub(super) fn derive(lookup: LookupKey) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.node.execution/v1");
        digest.update(lookup.0);
        Self(digest.finalize().into())
    }

    pub(crate) fn directory(self) -> String {
        lower_hex(&self.0)
    }

    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl Digest {
    pub(super) fn recipe(context: Self, semantic_inputs: &[u8], parents: &[impl Edge]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.node.recipe/v1");
        digest.update(context.0);
        digest.update((semantic_inputs.len() as u64).to_le_bytes());
        digest.update(semantic_inputs);
        retain_semantic_parents(&mut digest, parents);
        Self(digest.finalize().into())
    }

    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn directory(self) -> String {
        lower_hex(&self.0)
    }
}

fn retain_semantic_parents(digest: &mut Sha256, parents: &[impl Edge]) {
    digest.update((parents.len() as u64).to_le_bytes());
    for parent in parents {
        digest.update([parent.kind() as u8]);
        digest.update(parent.schema().to_le_bytes());
        digest.update(parent.content().as_bytes());
    }
}

impl fmt::Debug for ActionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for ActionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&lower_hex(&self.0))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Debug for LookupKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for LookupKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&lower_hex(&self.0))
    }
}

impl fmt::Debug for ExecutionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for ExecutionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&lower_hex(&self.0))
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&lower_hex(&self.0))
    }
}

fn lower_hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
