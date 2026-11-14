//! std's `num::IntErrorKind` for Rawr code: Rust's own value, which a
//! `ParseIntError`'s `kind` gives.

use std::num::{IntErrorKind, NonZeroU8};

use super::held::{self, Held};
use crate::{capability, Dynamic};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::num::IntErrorKind",
    lent: &[],
    also: &[],
    register: |builder, path, capabilities| {
        builder
            .value_type::<IntErrorKind>(path, &[], capabilities)
            .map(drop)
    },
    crosses: true,
    runs: &[
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::PartialEq,
        capability::Kind::Eq,
        capability::Kind::Send,
        capability::Kind::Static,
    ],
})];

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<IntErrorKind>();
}

/// The kind of the `ParseIntError` whose message is `message`, as Rawr
/// holds the error: the kind of std's own error with that message.
pub(crate) fn int_error_kind(message: &str) -> Option<IntErrorKind> {
    ["", "x", "256"]
        .into_iter()
        .filter_map(|text| text.parse::<u8>().err())
        .chain("-129".parse::<i8>().err())
        .chain("0".parse::<NonZeroU8>().err())
        .find(|error| error.to_string() == message)
        .map(|error| *error.kind())
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    macro_rules! kinds {
        ($($name:ident)*) => {
            $(binds.plain(
                concat!("num::IntErrorKind::", stringify!($name)),
                |_| Ok(Dynamic::from(IntErrorKind::$name)),
            );)*
        };
    }
    kinds!(Empty InvalidDigit PosOverflow NegOverflow Zero);
}
