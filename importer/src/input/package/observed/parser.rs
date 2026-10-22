//! Prospective temporary storage for the pinned, closed Marker deserializer.

use crate::input::stored::{Budget, Result};

/// serde_json 1.0.151 de.rs:31–65 owns one reusable string scratch Vec;
/// read.rs:494–542 borrows unescaped strings and only copies on an escape.
/// Its decoded byte extent cannot exceed the admitted marker input extent.
///
/// Marker contains only u32: struct/sequence -> field/u32 is the whole visitor
/// path (de.rs:319–342,1820–1866,2215–2227). Unknown fields reject immediately;
/// no nested ignored values, retained strings, or recursive containers exist.
/// The pinned default number path uses fixed numeric state (de.rs:933–935).
/// The owner verified serde_json features `default,std` in the locked offline
/// CLI import-baseline and importer rustc-driver,test-fixtures graphs. Neither
/// float_roundtrip nor arbitrary_precision is part of this source closure.
///
/// error.rs:230–312,315–347,435–493 can allocate one custom message, its boxed
/// shrink, a primary error box, a position replacement and a closing-container
/// error. Four error boxes cover those paths. Each has a code containing at
/// most a fat pointer plus discriminant and two usize coordinates; eight usize
/// slots conservatively cover each supported target's complete padded layout.
/// The longest custom message renders an input string with Debug escaping
/// (at most six bytes per input byte), plus fewer than 128 fixed/numeric bytes.
///
/// Pinned alloc/string.rs:3102–3106 specializes fmt::Arguments::to_string to
/// alloc::fmt::format (fmt.rs:649–660). Its initial estimated capacity is at
/// most twice the literal bytes (core/fmt/mod.rs:754–807), hence at most 2L.
/// raw_vec/mod.rs:502–534 then grows by max(2*capacity,need,8).
/// For logical extent L, all replacement requests sum to less than 4*max(L,8),
/// with at most bit_length(max(L,8)) requests. The final message box may request
/// its full logical extent once more. No budget is refunded after parsing.
pub(super) fn admit(bytes: &[u8], budget: &mut Budget<'_>) -> Result<()> {
    budget.check()?;
    let message = bytes
        .len()
        .checked_mul(6)
        .and_then(|value| value.checked_add(128))
        .ok_or_else(|| budget.overflow())?;
    let (mut items, mut owned) = growth(message, budget)?;
    owned = owned
        .checked_add(message)
        .and_then(|value| value.checked_add(4 * 8 * std::mem::size_of::<usize>()))
        .ok_or_else(|| budget.overflow())?;
    items = items.checked_add(4).ok_or_else(|| budget.overflow())?;
    if bytes.contains(&b'\\') {
        let (scratch_items, scratch_bytes) = growth(bytes.len(), budget)?;
        items = items
            .checked_add(scratch_items)
            .ok_or_else(|| budget.overflow())?;
        owned = owned
            .checked_add(scratch_bytes)
            .ok_or_else(|| budget.overflow())?;
    }
    budget.storage(items, owned)
}

fn growth(length: usize, budget: &mut Budget<'_>) -> Result<(usize, usize)> {
    let length = length.max(8);
    let bytes = length.checked_mul(4).ok_or_else(|| budget.overflow())?;
    let items = (usize::BITS - length.leading_zeros()) as usize;
    Ok((items, bytes))
}
