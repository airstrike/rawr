//! std's `env::args()` for Rawr code: the process's own arguments, which
//! `rawr run` sets to the program's name and the arguments after `--`, as
//! `cargo run` does. A program its engine grants no environment
//! (`Access::env`) has none.

use crate::{Dynamic, ImmutableString};

/// Bind the module's items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.with_context("env::args", |call, _| {
        // A program without the environment is given no arguments, as a
        // WASI module the host passes none.
        if !call.access().env() {
            return Ok(Dynamic::from(crate::Array::new()));
        }
        Ok(Dynamic::from(
            std::env::args_os()
                .map(|argument| {
                    Dynamic::from(ImmutableString::from(argument.to_string_lossy().as_ref()))
                })
                .collect::<crate::Array>(),
        ))
    });
}
