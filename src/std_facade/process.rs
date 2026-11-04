//! std's `process::id()`, `process::exit()` and `process::abort()` for
//! Rawr code: the id of the process the program runs in, and the end of
//! that process, for a program its engine grants the process
//! (`Access::process`). A program without its process ends its own run
//! instead, as a script's `exit` does, and leaves its host's process be.

use crate::Dynamic;

/// Bind the module's items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.with_context("process::id", |call, _| {
        // std panics with this message on a platform without processes,
        // such as the browser's wasm, and a program its engine grants no
        // process (`Access::process`) has none either.
        if cfg!(target_family = "wasm") || !call.access().process() {
            return Err(crate::EvalAltResult::ErrorRuntime(
                "no pids on this platform".into(),
                crate::Position::NONE,
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
        Err(crate::EvalAltResult::Exit(Dynamic::from(code), crate::Position::NONE).into())
    });
    binds.with_context("process::abort", |call, _| {
        if call.access().process() {
            std::process::abort();
        }
        Err(crate::EvalAltResult::Exit(Dynamic::UNIT, crate::Position::NONE).into())
    });
}
