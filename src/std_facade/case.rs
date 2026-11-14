//! std's `char::ToUppercase` and `char::ToLowercase` for Rawr code: Rust's
//! own iterators of the chars a `char`'s case maps to, which
//! `char::to_uppercase` and `char::to_lowercase` give.

use std::char::{ToLowercase, ToUppercase};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// What rawr runs for each iterator's value.
const RUNS: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Iterator,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::char::ToUppercase",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .type_::<ToUppercase>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
    Held::Value(held::Value {
        std: "std::char::ToLowercase",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .type_::<ToLowercase>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
];

/// Register `Debug` and `Display` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<ToUppercase>();
    crate::host::debug::register::<ToLowercase>();
    crate::host::display::register::<ToUppercase>();
    crate::host::display::register::<ToLowercase>();
}

/// `c.to_uppercase()`, or `c.to_lowercase()` when not `upper`, as Rawr
/// holds it.
pub(crate) fn mapped(c: char, upper: bool) -> Dynamic {
    if upper {
        Dynamic::from(c.to_uppercase())
    } else {
        Dynamic::from(c.to_lowercase())
    }
}

fn expected(found: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(
        format!("expected a case mapping, found `{found}`").into(),
        Position::NONE,
    )
    .into()
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    macro_rules! mapping {
        ($ty:ident, $path:literal) => {
            bind(concat!($path, "::next"), |args| {
                let found = args[0].type_name();
                let mut chars = args[0].write_lock::<$ty>().ok_or_else(|| expected(found))?;
                Ok(crate::option::from(chars.next()))
            });
            bind(concat!($path, "::to_string"), |args| {
                let chars = args[0]
                    .read_lock::<$ty>()
                    .ok_or_else(|| expected(args[0].type_name()))?;
                Ok(Dynamic::from(ImmutableString::from(chars.to_string())))
            });
        };
    }
    mapping!(ToUppercase, "char::ToUppercase");
    mapping!(ToLowercase, "char::ToLowercase");
}
