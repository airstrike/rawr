//! rustc's deny-by-default `overflowing_literals` lint: an integer literal
//! its type can't hold is an error, "literal out of range for `u8`".

use super::{Error, Int, Wide};
use crate::Span;

/// The lint's error for the literal `written`, whose number is `value`, of
/// the type `kind`, at `at`.
pub(super) fn error(kind: Int, value: Wide, written: &str, at: Span) -> Error {
    let ty = kind.name();
    let radix = matches!(
        written.trim_start_matches('-').get(..2),
        Some("0x" | "0o" | "0b")
    );
    let note = if radix {
        format!(
            "the literal `{written}` (decimal `{value}`) does not fit into the type `{ty}` and \
             will become `{}{ty}`",
            wrapped(kind, value)
        )
    } else {
        let (low, high) = bounds(kind);
        format!("the literal `{written}` does not fit into the type `{ty}` whose range is `{low}..={high}`")
    };
    // rustc offers a type only for a signed one, or a literal in another
    // radix.
    let help = (radix || kind.signed())
        .then(|| wider(kind, value))
        .flatten()
        .map(|wider| format!("consider using the type `{}` instead", wider.name()).into());
    Error::OverflowingLiteral {
        ty: ty.into(),
        note: note.into(),
        help,
        literal: at,
    }
}

/// The smallest type rustc suggests that holds `value`: the unsigned type
/// of `kind`'s width or a wider one, then a wider signed one.
fn wider(kind: Int, value: Wide) -> Option<Int> {
    use Int::*;
    let candidates: &[Int] = match kind {
        I8 => &[U8, I16, I32, I64, I128],
        I16 => &[U16, I32, I64, I128],
        I32 => &[U32, I64, I128],
        I64 => &[U64, I128],
        I128 => &[U128],
        U8 => &[U16, U32, U64, U128],
        U16 => &[U32, U64, U128],
        U32 => &[U64, U128],
        U64 => &[U128],
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .find(|candidate| value.fits(*candidate).is_ok())
}

/// `kind`'s bounds as rustc prints them.
fn bounds(kind: Int) -> (String, String) {
    match kind {
        Int::U128 => ("0".into(), u128::MAX.to_string()),
        kind => {
            let (low, high) = kind.full_range();
            (low.to_string(), high.to_string())
        }
    }
}

/// `value` wrapped to `kind`'s width, as `as` would make it.
fn wrapped(kind: Int, value: Wide) -> String {
    let bits = kind.bits();
    let pattern = value.unsigned() & (u128::MAX >> (128 - bits));
    if kind.signed() {
        let shift = 128 - bits;
        (((pattern << shift) as i128) >> shift).to_string()
    } else {
        pattern.to_string()
    }
}
