// Copyright the ripgrep contributors. MIT OR Unlicense.
//! The unchanged globset 0.4.18 fnv.rs hash policy; no seed or native state.

pub(super) type Build = std::hash::BuildHasherDefault<Hasher>;

pub(super) struct Hasher(u64);

impl Default for Hasher {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl std::hash::Hasher for Hasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}
