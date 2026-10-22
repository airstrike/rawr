//! Opaque runner-local native value handle.

/// A native value confined to the runner epoch that produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle {
    epoch: [u8; 32],
    slot: u32,
    generation: u64,
}

impl Handle {
    pub(crate) const fn new(epoch: [u8; 32], slot: u32, generation: u64) -> Self {
        Self {
            epoch,
            slot,
            generation,
        }
    }

    pub(crate) const fn epoch(&self) -> &[u8; 32] {
        &self.epoch
    }

    pub(crate) const fn slot(&self) -> u32 {
        self.slot
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }
}
