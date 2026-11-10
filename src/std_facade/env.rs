//! std's `env` functions for Rawr code. `env::args()` gives the process's
//! own arguments, which `rawr run` sets to the program's name and the
//! arguments after `--`, as `cargo run` does. `env::current_dir()` gives
//! the process's working directory as Rust's `io::Result<PathBuf>`. A
//! program its engine grants no environment (`Access::env`) has no
//! arguments, and its `current_dir()` fails with `PermissionDenied`.

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
    binds.with_context("env::current_dir", |call, _| {
        let directory = match call.access().env() {
            true => std::env::current_dir(),
            false => Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        };
        Ok(match directory {
            Ok(directory) => crate::result::ok(Dynamic::from(directory)),
            Err(failure) => crate::result::err(super::io::made(failure)),
        })
    });
}
