//! std's `process::id()` for Rawr code: the id of the process the program
//! runs in, for a program its engine grants the process
//! (`Access::process`).

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
        Ok(crate::Dynamic::from(i64::from(std::process::id())))
    });
}
