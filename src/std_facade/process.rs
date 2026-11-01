//! std's `process::id()` for Rawr code (`resolve::std_reexport` sends
//! `std::process` here): the id of the process the program runs in, for a
//! program its engine grants the process (`Access::process`).

use crate::host::{self, facade, item, signature, type_ref};

/// Declare the module's items in the std facade, under `process`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::{Builtin, Type};

    builder.module("process")?;
    let mut id = signature::Builder::new();
    id.output(Type::builtin(Builtin::U32));
    builder.item(item::Definition::function("process::id", id.finish())?)?;
    Ok(())
}

/// Bind the module's items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    runtime
        .bind_with_context(format!("{}::process::id", super::ROOT), |call, _| {
            // std panics with this message on a platform without processes,
            // such as the browser's wasm, and a program its engine grants
            // no process (`Access::process`) has none either.
            if cfg!(target_family = "wasm") || !call.access().process() {
                return Err(crate::EvalAltResult::ErrorRuntime(
                    "no pids on this platform".into(),
                    crate::Position::NONE,
                )
                .into());
            }
            Ok(crate::Dynamic::from(i64::from(std::process::id())))
        })
        .unwrap_or_else(|_| panic!("the std bindings must retain process::id"));
}
