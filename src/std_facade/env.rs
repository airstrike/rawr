//! std's `env` functions for Rawr code. `env::args()` gives the process's
//! own arguments, which `rawr run` sets to the program's name and the
//! arguments after `--`, as `cargo run` does. `env::current_dir()` gives
//! the process's working directory as Rust's `io::Result<PathBuf>`, and
//! `env::var` and `env::var_os` read the process's environment. A program
//! its engine grants no environment (`Access::env`) has no arguments and
//! no variables, as `env!` has none, and its `current_dir()` fails with
//! `PermissionDenied`.

use std::env::VarError;

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::env::VarError",
    parameters: &[],
    lent: &[],
    also: &[],
    register: |builder, path, parameters, capabilities| {
        builder
            .value_type::<VarError>(path, parameters, capabilities)
            .map(drop)
    },
    crosses: true,
    runs: &[
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::PartialEq,
        capability::Kind::Eq,
        capability::Kind::Send,
        capability::Kind::Static,
        capability::Kind::Error,
    ],
})];

/// Register `Debug` and `Display` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<VarError>();
    crate::host::display::register::<VarError>();
}

/// The variable `key` names, as `call`'s program may read it: none for a
/// program without the environment.
fn variable(
    call: &crate::host::runtime::Call<'_>,
    key: &Dynamic,
) -> Result<Option<std::ffi::OsString>, Box<EvalAltResult>> {
    let key = super::path::os_like(key)?;
    Ok(call.access().env().then(|| std::env::var_os(key)).flatten())
}

/// The `VarError` a program passes.
fn var_error(value: &Dynamic) -> Result<VarError, Box<EvalAltResult>> {
    value
        .read_lock::<VarError>()
        .map(|error| error.clone())
        .ok_or_else(|| {
            EvalAltResult::ErrorRuntime(
                format!("expected `VarError`, found `{}`", value.type_name()).into(),
                Position::NONE,
            )
            .into()
        })
}

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
    binds.with_context("env::var", |call, arguments| {
        Ok(
            match variable(call, arguments[0])?.map(std::ffi::OsString::into_string) {
                Some(Ok(text)) => crate::result::ok(Dynamic::from(ImmutableString::from(text))),
                Some(Err(raw)) => crate::result::err(Dynamic::from(VarError::NotUnicode(raw))),
                None => crate::result::err(Dynamic::from(VarError::NotPresent)),
            },
        )
    });
    binds.with_context("env::var_os", |call, arguments| {
        Ok(crate::option::from(variable(call, arguments[0])?))
    });
    // `VarError`'s variants, and the reader a pattern reads
    // `NotUnicode`'s text by.
    binds.plain("env::VarError::NotPresent", |_| {
        Ok(Dynamic::from(VarError::NotPresent))
    });
    binds.plain("env::VarError::NotUnicode", |arguments| {
        Ok(Dynamic::from(VarError::NotUnicode(super::path::os_like(
            arguments[0],
        )?)))
    });
    binds.plain("env::VarError::to_string", |arguments| {
        Ok(Dynamic::from(ImmutableString::from(
            var_error(arguments[0])?.to_string(),
        )))
    });
    binds.reader("env::VarError::NotUnicode", |arguments| {
        Ok(match var_error(arguments[0])? {
            VarError::NotUnicode(raw) => crate::option::some(Dynamic::from(raw)),
            VarError::NotPresent => crate::option::none(),
        })
    });
}
