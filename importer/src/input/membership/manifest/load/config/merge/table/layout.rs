// Copyright the hashbrown contributors. MIT OR Apache-2.0.
// See LICENSE-MIT and LICENSE-APACHE beside this file.

//! hashbrown 0.17.1 layout specialization for (&str, usize) with std Global.
//! See PROVENANCE.md; not a generic allocation estimator.

use std::alloc::Layout;

fn group() -> usize {
    if cfg!(all(
        target_feature = "sse2",
        any(target_arch = "x86", target_arch = "x86_64"),
        not(miri)
    )) || cfg!(all(
        target_arch = "loongarch64",
        target_feature = "lsx",
        not(miri)
    )) {
        16
    } else if cfg!(any(
        target_pointer_width = "64",
        target_arch = "aarch64",
        target_arch = "x86_64",
        target_arch = "wasm32"
    )) {
        8
    } else {
        4
    }
}

pub(super) fn capacity(buckets: usize) -> usize {
    if buckets == 0 {
        0
    } else if buckets <= 8 {
        buckets - 1
    } else {
        (buckets / 8) * 7
    }
}

pub(super) fn growth(required: usize) -> Option<(usize, Layout)> {
    if required == 0 {
        return None;
    }
    // This fixed entry is >= 12 bytes, above every tiny-item exception.
    let buckets = if required < 4 {
        4
    } else if required < 8 {
        8
    } else if required < 15 {
        16
    } else {
        (required.checked_mul(8)? / 7).checked_next_power_of_two()?
    };
    let entry = Layout::new::<(&str, usize)>();
    let group = group();
    let alignment = entry.align().max(group);
    let offset = entry
        .size()
        .checked_mul(buckets)?
        .checked_add(alignment - 1)?
        & !(alignment - 1);
    let bytes = offset.checked_add(buckets)?.checked_add(group)?;
    let allocation = Layout::from_size_align(bytes, alignment).ok()?;
    Some((buckets, allocation))
}
