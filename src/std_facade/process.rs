//! std's `process::id()`, `process::exit()` and `process::abort()` for
//! Rawr code: the id of the process the program runs in, and the end of
//! that process, for a program its engine grants the process
//! (`Access::process`). A program without its process ends its own run
//! instead, as a script's `exit` does, and leaves its host's process be.
//!
//! std's `process::ExitCode` is the status a `main` that returns it ends
//! with (`program::Termination`).

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::process::ExitCode",
    parameters: &[],
    lent: &[],
    also: &[],
    register: |builder, path, parameters, capabilities| {
        builder
            .default_value_type::<ExitCode>(path, parameters, capabilities)
            .map(drop)
    },
    crosses: false,
    runs: &[
        capability::Kind::Default,
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::PartialEq,
        capability::Kind::Send,
        capability::Kind::Static,
    ],
})];

/// std's `ExitCode`, which keeps its status private: rawr holds the status
/// itself, which a `main` returning it ends with.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExitCode(u8);

impl ExitCode {
    /// The status the process ends with.
    pub(crate) const fn status(self) -> u8 {
        self.0
    }
}

impl std::fmt::Debug for ExitCode {
    /// std's, on a Unix host: `ExitCode(unix_exit_status(3))`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExitCode(unix_exit_status({}))", self.0)
    }
}

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<ExitCode>();
}

/// Bind the module's items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.with_context("process::id", |call, _| {
        // std panics with this message on a platform without processes,
        // such as the browser's wasm, and a program its engine grants no
        // process (`Access::process`) has none either.
        if cfg!(target_family = "wasm") || !call.access().process() {
            return Err(EvalAltResult::ErrorRuntime(
                "no pids on this platform".into(),
                Position::NONE,
            )
            .into());
        }
        Ok(Dynamic::from(i64::from(std::process::id())))
    });
    binds.with_context("process::exit", |call, arguments| {
        let code = arguments[0].as_int().unwrap_or_default();
        if call.access().process() {
            // std's `exit` flushes stdout and ends the process without
            // running destructors. The code is an `i32` by its signature.
            std::process::exit(i32::try_from(code).unwrap_or_default());
        }
        Err(EvalAltResult::Exit(Dynamic::from(code), Position::NONE).into())
    });
    binds.with_context("process::abort", |call, _| {
        if call.access().process() {
            std::process::abort();
        }
        Err(EvalAltResult::Exit(Dynamic::UNIT, Position::NONE).into())
    });
    binds.plain("process::ExitCode::SUCCESS", |_| {
        Ok(Dynamic::from(ExitCode(0)))
    });
    binds.plain("process::ExitCode::FAILURE", |_| {
        Ok(Dynamic::from(ExitCode(1)))
    });
    // `impl From<u8> for ExitCode`, and `From<T> for T`.
    binds.plain("process::ExitCode::from", |arguments| {
        if let Some(code) = arguments[0].read_lock::<ExitCode>() {
            return Ok(Dynamic::from(*code));
        }
        let status = arguments[0]
            .as_int()
            .ok()
            .and_then(|status| u8::try_from(status).ok())
            .ok_or_else(|| {
                EvalAltResult::ErrorRuntime("expected a `u8` status".into(), Position::NONE)
            })?;
        Ok(Dynamic::from(ExitCode(status)))
    });
}
