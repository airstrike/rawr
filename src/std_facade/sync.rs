//! std's `sync::mpsc` errors of a receive for Rawr code: `RecvError`,
//! `TryRecvError` and `RecvTimeoutError`, each Rust's own value, which a
//! channel's `recv`, `try_recv` and `recv_timeout` give.

use std::sync::mpsc::{RecvError, RecvTimeoutError, TryRecvError};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// What rawr runs for each error's value.
const RUNS: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The types the module holds.
pub(super) const HELD: [Held; 3] = [
    Held::Value(held::Value {
        std: "std::sync::mpsc::RecvError",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .value_type::<RecvError>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
    Held::Value(held::Value {
        std: "std::sync::mpsc::TryRecvError",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .value_type::<TryRecvError>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
    Held::Value(held::Value {
        std: "std::sync::mpsc::RecvTimeoutError",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .value_type::<RecvTimeoutError>(path, &[], capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
];

/// Register `Debug` and `Display` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<RecvError>();
    crate::host::debug::register::<TryRecvError>();
    crate::host::debug::register::<RecvTimeoutError>();
    crate::host::display::register::<RecvError>();
    crate::host::display::register::<TryRecvError>();
    crate::host::display::register::<RecvTimeoutError>();
}

/// The `Display` of the error `value`, as `to_string` gives it.
fn text<T: std::fmt::Display + Clone + 'static>(
    value: &Dynamic,
    expected: &str,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let error = value.read_lock::<T>().map(|error| error.to_string());
    error
        .map(|text| Dynamic::from(ImmutableString::from(text)))
        .ok_or_else(|| {
            EvalAltResult::ErrorRuntime(
                format!("expected `{expected}`, found `{}`", value.type_name()).into(),
                Position::NONE,
            )
            .into()
        })
}

/// Bind the types' variants and their `to_string`.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("sync::mpsc::TryRecvError::Empty", |_| {
        Ok(Dynamic::from(TryRecvError::Empty))
    });
    bind("sync::mpsc::TryRecvError::Disconnected", |_| {
        Ok(Dynamic::from(TryRecvError::Disconnected))
    });
    bind("sync::mpsc::RecvTimeoutError::Timeout", |_| {
        Ok(Dynamic::from(RecvTimeoutError::Timeout))
    });
    bind("sync::mpsc::RecvTimeoutError::Disconnected", |_| {
        Ok(Dynamic::from(RecvTimeoutError::Disconnected))
    });
    bind("sync::mpsc::RecvError::to_string", |args| {
        text::<RecvError>(args[0], "RecvError")
    });
    bind("sync::mpsc::TryRecvError::to_string", |args| {
        text::<TryRecvError>(args[0], "TryRecvError")
    });
    bind("sync::mpsc::RecvTimeoutError::to_string", |args| {
        text::<RecvTimeoutError>(args[0], "RecvTimeoutError")
    });
}
