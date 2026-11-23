//! std's `hash::DefaultHasher` for Rawr code: Rust's own value, SipHash-1-3
//! with zero keys as `DefaultHasher::new()` makes it, so `finish` gives
//! what rustc's build of the program prints.

use std::hash::{DefaultHasher, Hasher};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::hash::DefaultHasher",
    parameters: &[],
    lent: &[],
    also: &["std::collections::hash_map::DefaultHasher"],
    register: |builder, path, parameters, capabilities| {
        builder
            .default_type::<DefaultHasher>(path, parameters, capabilities)
            .map(drop)
    },
    crosses: true,
    runs: &[
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Default,
        capability::Kind::Send,
        capability::Kind::Static,
    ],
})];

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<DefaultHasher>();
}

/// The runtime error of a value that is not what a bind takes.
fn expected(what: &str, found: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(
        format!("expected `{what}`, found `{found}`").into(),
        Position::NONE,
    )
    .into()
}

/// Write into the hasher `value` holds with `write`.
fn written(
    value: &mut Dynamic,
    write: impl FnOnce(&mut DefaultHasher),
) -> Result<Dynamic, Box<EvalAltResult>> {
    let found = value.type_name();
    let mut hasher = value
        .write_lock::<DefaultHasher>()
        .ok_or_else(|| expected("DefaultHasher", found))?;
    write(&mut hasher);
    Ok(Dynamic::UNIT)
}

/// The integer `value` holds: a runtime integer, a `u64`'s bits as its
/// kind writes them.
fn int(value: &Dynamic) -> Result<crate::INT, Box<EvalAltResult>> {
    value
        .as_int()
        .map_err(|found| expected("an integer", found))
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.plain("hash::DefaultHasher::new", |_| {
        Ok(Dynamic::from(DefaultHasher::new()))
    });
    binds.plain("hash::DefaultHasher::default", |_| {
        Ok(Dynamic::from(DefaultHasher::default()))
    });
    binds.plain("hash::DefaultHasher::write", |args| {
        let bytes = super::fs::contents(args[1])?;
        written(args[0], |hasher| hasher.write(&bytes))
    });
    binds.plain("hash::DefaultHasher::finish", |args| {
        let finished = args[0]
            .read_lock::<DefaultHasher>()
            .map(|hasher| hasher.finish())
            .ok_or_else(|| expected("DefaultHasher", args[0].type_name()))?;
        // A `u64` is its bits in the runtime's integer.
        Ok(Dynamic::from_int(finished as crate::INT))
    });
    // Each provided `write_*` writes the integer's native-endian bytes, as
    // std's does; a value is its kind's bits, truncated to its width. std's
    // table declares no 128-bit integer input rawr reads yet.
    macro_rules! writes {
        ($($name:ident: $kind:ty),* $(,)?) => {
            $(binds.plain(
                concat!("hash::DefaultHasher::", stringify!($name)),
                |args| {
                    let value = int(args[1])? as $kind;
                    written(args[0], |hasher| hasher.$name(value))
                },
            );)*
        };
    }
    writes!(
        write_u8: u8,
        write_u16: u16,
        write_u32: u32,
        write_u64: u64,
        write_usize: usize,
        write_i8: i8,
        write_i16: i16,
        write_i32: i32,
        write_i64: i64,
        write_isize: isize,
    );
}
