//! std's `fs::read` and `fs::read_to_string` for Rawr code
//! (`resolve::std_reexport` sends `std::fs` here). A process that denies
//! files to its programs (`resolve::deny_files`) gives an error instead.

use crate::host::{self, facade, item, signature, type_ref};
use crate::{Array, Dynamic, ImmutableString, RawrResult};

/// Declare the module's items in the std facade, under `fs`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::{Builtin, Type};

    builder.module("fs")?;
    // `read` gives the file's bytes, a `Vec<u8>`; `read_to_string` its text.
    for (name, output) in [
        ("fs::read", Type::slice(Type::builtin(Builtin::U8))),
        ("fs::read_to_string", Type::builtin(Builtin::String)),
    ] {
        let mut read = signature::Builder::new();
        read.parameter(Type::builtin(Builtin::String));
        read.output(Type::result(output, Type::registered("io::Error")?));
        builder.item(item::Definition::function(name, read.finish())?)?;
    }
    Ok(())
}

/// Bind the module's items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    runtime
        .bind(format!("{}::fs::read", super::ROOT), |args| {
            read(args[0], |path| {
                std::fs::read(path).map(|bytes| {
                    let bytes: Array = bytes
                        .into_iter()
                        .map(|byte| Dynamic::from(i64::from(byte)))
                        .collect();
                    Dynamic::from(bytes)
                })
            })
        })
        .unwrap_or_else(|_| panic!("the std bindings must retain fs::read"));
    runtime
        .bind(format!("{}::fs::read_to_string", super::ROOT), |args| {
            read(args[0], |path| {
                std::fs::read_to_string(path).map(|text| Dynamic::from(ImmutableString::from(text)))
            })
        })
        .unwrap_or_else(|_| panic!("the std bindings must retain fs::read_to_string"));
}

/// The file at `path` read by `read`, as Rust's `Result`.
fn read(path: &Dynamic, read: impl FnOnce(&str) -> std::io::Result<Dynamic>) -> RawrResult {
    let path = path.clone().into_immutable_string().map_err(|actual| {
        crate::EvalAltResult::ErrorRuntime(
            format!("expected a path, found `{actual}`").into(),
            crate::Position::NONE,
        )
    })?;
    let read = match crate::resolve::files_denied() {
        Some(reason) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            reason,
        )),
        None => read(path.as_str()),
    };
    Ok(match read {
        Ok(value) => crate::result::ok(value),
        Err(error) => crate::result::err(super::io::made(error)),
    })
}
