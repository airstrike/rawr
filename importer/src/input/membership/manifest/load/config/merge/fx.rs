// Copyright the rustc-hash contributors. MIT OR Apache-2.0.
// See fx/PROVENANCE.md and fx/LICENSE-*.

//! The allocation-free rustc-hash 2.1.2 closure reached by str and Unix Path.
//! Callers must admit key work before table operations, including rehashing.

use std::hash::{BuildHasher, Hasher};

#[derive(Clone, Copy, Default)]
pub(super) struct Build;

#[derive(Default)]
pub(super) struct Hash {
    value: usize,
}

impl BuildHasher for Build {
    type Hasher = Hash;
    fn build_hasher(&self) -> Hash {
        Hash::default()
    }
}

impl Hash {
    fn add(&mut self, value: usize) {
        #[cfg(target_pointer_width = "64")]
        const K: usize = 0xf1357aea2e62a9c5;
        #[cfg(target_pointer_width = "32")]
        const K: usize = 0x93d765dd;
        self.value = self.value.wrapping_add(value).wrapping_mul(K);
    }
}

impl Hasher for Hash {
    fn write(&mut self, bytes: &[u8]) {
        let value = bytes_hash(bytes);
        self.add(value as usize);
        #[cfg(target_pointer_width = "32")]
        self.add((value >> 32) as usize);
    }

    fn write_u8(&mut self, value: u8) {
        self.add(value as usize);
    }

    fn write_usize(&mut self, value: usize) {
        self.add(value);
    }

    fn finish(&self) -> u64 {
        #[cfg(target_pointer_width = "64")]
        const ROTATE: u32 = 26;
        #[cfg(target_pointer_width = "32")]
        const ROTATE: u32 = 15;
        self.value.rotate_left(ROTATE) as u64
    }
}

fn mix(x: u64, y: u64) -> u64 {
    if cfg!(any(
        all(
            target_pointer_width = "64",
            not(any(target_arch = "sparc64", target_arch = "wasm64"))
        ),
        target_arch = "aarch64",
        target_arch = "x86_64",
        all(target_family = "wasm", target_feature = "wide-arithmetic"),
    )) {
        let full = (x as u128).wrapping_mul(y as u128);
        (full as u64) ^ ((full >> 64) as u64)
    } else {
        let a = (x as u32 as u64).wrapping_mul(y >> 32);
        let b = (x >> 32).wrapping_mul(y as u32 as u64);
        a ^ b.rotate_right(32)
    }
}

/// The closed callers pass exactly four or eight bytes. Explicit decoding
/// avoids the reference's fixed-width slice-conversion unwraps.
fn word(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .take(8)
        .enumerate()
        .fold(0, |value, (index, byte)| {
            value | (u64::from(*byte) << (index * 8))
        })
}

fn bytes_hash(bytes: &[u8]) -> u64 {
    let length = bytes.len();
    let mut s0 = 0x243f6a8885a308d3;
    let mut s1 = 0x13198a2e03707344;
    if length <= 16 {
        if length >= 8 {
            s0 ^= word(&bytes[..8]);
            s1 ^= word(&bytes[length - 8..]);
        } else if length >= 4 {
            s0 ^= word(&bytes[..4]);
            s1 ^= word(&bytes[length - 4..]);
        } else if length > 0 {
            s0 ^= u64::from(bytes[0]);
            s1 ^= (u64::from(bytes[length - 1]) << 8) | u64::from(bytes[length / 2]);
        }
    } else {
        let mut bulk = &bytes[..length - 1];
        while let Some((chunk, rest)) = bulk.split_first_chunk::<16>() {
            let next = mix(
                s0 ^ word(&chunk[..8]),
                0xa4093822299f31d0 ^ word(&chunk[8..]),
            );
            s0 = s1;
            s1 = next;
            bulk = rest;
        }
        let suffix = &bytes[length - 16..];
        s0 ^= word(&suffix[..8]);
        s1 ^= word(&suffix[8..]);
    }
    mix(s0, s1) ^ (length as u64)
}

#[cfg(test)]
mod tests;
