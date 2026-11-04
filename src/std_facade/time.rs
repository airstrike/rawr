//! std's `time::SystemTime` and `time::SystemTimeError` for Rawr code.
//! Each value is Rust's own; a time is read from the system clock.
//! `Duration` and `Instant` are rawr's own (`Builtin::Duration`).

use std::time::{Duration, SystemTime, SystemTimeError};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::time::SystemTime",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .value_type::<SystemTime>(path, &[], capabilities)
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
        constants: &["std::time::UNIX_EPOCH", "std::time::SystemTime::UNIX_EPOCH"],
    }),
    Held::Value(held::Value {
        std: "std::time::SystemTimeError",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .type_::<SystemTimeError>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
        constants: &[],
    }),
];

/// Register `Debug` for each type and `std::time`'s others, and an
/// error's `Display`.
pub(crate) fn register_debug() {
    crate::host::debug::register::<SystemTime>();
    crate::host::debug::register::<SystemTimeError>();
    crate::host::display::register::<SystemTimeError>();
    // rawr's own `Duration` and `Instant`, which a derived `Debug` writes.
    crate::host::debug::register::<Duration>();
    crate::host::debug::register::<std::time::Instant>();
}

fn read<T: Clone + 'static>(value: &Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            EvalAltResult::ErrorRuntime(
                format!("expected `{expected}`, found `{}`", value.type_name()).into(),
                Position::NONE,
            )
            .into()
        })
}

/// A `Result<Duration, SystemTimeError>` as Rawr holds one.
fn since(result: Result<Duration, SystemTimeError>) -> Dynamic {
    match result {
        Ok(duration) => crate::result::ok(Dynamic::from(duration)),
        Err(error) => crate::result::err(Dynamic::from(error)),
    }
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("time::UNIX_EPOCH", |_| {
        Ok(Dynamic::from(SystemTime::UNIX_EPOCH))
    });
    bind("time::SystemTime::UNIX_EPOCH", |_| {
        Ok(Dynamic::from(SystemTime::UNIX_EPOCH))
    });
    bind("time::SystemTime::now", |_| {
        Ok(Dynamic::from(SystemTime::now()))
    });
    bind("time::SystemTime::duration_since", |args| {
        let earlier = read::<SystemTime>(args[1], "SystemTime")?;
        Ok(since(
            read::<SystemTime>(args[0], "SystemTime")?.duration_since(earlier),
        ))
    });
    bind("time::SystemTime::elapsed", |args| {
        Ok(since(read::<SystemTime>(args[0], "SystemTime")?.elapsed()))
    });
    bind("time::SystemTime::checked_add", |args| {
        let duration = read::<Duration>(args[1], "Duration")?;
        let time = read::<SystemTime>(args[0], "SystemTime")?;
        Ok(crate::option::from(time.checked_add(duration)))
    });
    bind("time::SystemTime::checked_sub", |args| {
        let duration = read::<Duration>(args[1], "Duration")?;
        let time = read::<SystemTime>(args[0], "SystemTime")?;
        Ok(crate::option::from(time.checked_sub(duration)))
    });
    bind("time::SystemTimeError::duration", |args| {
        let error = read::<SystemTimeError>(args[0], "SystemTimeError")?;
        Ok(Dynamic::from(error.duration()))
    });
    bind("time::SystemTimeError::to_string", |args| {
        let error = read::<SystemTimeError>(args[0], "SystemTimeError")?;
        Ok(Dynamic::from(ImmutableString::from(error.to_string())))
    });
}
