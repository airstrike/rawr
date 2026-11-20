//! std's `fmt::Alignment` for Rawr code: Rust's own value, which a
//! `Formatter`'s `align` gives.

use std::fmt::Alignment;

use super::held::{self, Held};
use crate::{capability, Dynamic};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::fmt::Alignment",
    parameters: &[],
    lent: &[],
    also: &[],
    register: |builder, path, parameters, capabilities| {
        builder
            .value_type::<Alignment>(path, parameters, capabilities)
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
    crate::host::debug::register::<Alignment>();
}

/// The alignment a format spec's `<`, `>` or `^` names.
pub(crate) fn alignment(align: char) -> Option<Alignment> {
    match align {
        '<' => Some(Alignment::Left),
        '>' => Some(Alignment::Right),
        '^' => Some(Alignment::Center),
        _ => None,
    }
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    macro_rules! alignments {
        ($($name:ident)*) => {
            $(binds.plain(
                concat!("fmt::Alignment::", stringify!($name)),
                |_| Ok(Dynamic::from(Alignment::$name)),
            );)*
        };
    }
    alignments!(Left Right Center);
}
