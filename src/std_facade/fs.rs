//! std's `fs::read_to_string` for Rawr code (`resolve::std_reexport` sends
//! `std::fs` here). A process that denies files to its programs
//! (`resolve::deny_files`) gives an error instead.

use crate::host::{self, facade, item, signature, type_ref};
use crate::{Dynamic, ImmutableString};

/// Declare the module's items in the std facade, under `fs`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::{Builtin, Type};

    builder.module("fs")?;
    let mut read = signature::Builder::new();
    read.parameter(Type::builtin(Builtin::String));
    read.output(Type::result(
        Type::builtin(Builtin::String),
        Type::registered("io::Error")?,
    ));
    builder.item(item::Definition::function(
        "fs::read_to_string",
        read.finish(),
    )?)?;
    Ok(())
}

/// Bind the module's items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    runtime
        .bind(format!("{}::fs::read_to_string", super::ROOT), |args| {
            let path = args[0].clone().into_immutable_string().map_err(|actual| {
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
                None => std::fs::read_to_string(path.as_str()),
            };
            Ok(match read {
                Ok(text) => crate::result::ok(Dynamic::from(ImmutableString::from(text))),
                Err(error) => crate::result::err(super::io::made(error)),
            })
        })
        .unwrap_or_else(|_| panic!("the std facade must retain fs::read_to_string"));
}
