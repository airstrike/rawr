//! std's `env::args()` for Rawr code (`resolve::std_reexport` sends
//! `std::env` here): the process's own arguments, which `rawr run` sets to
//! the program's name and the arguments after `--`, as `cargo run` does.

use crate::host::{self, facade, item, signature, type_ref};
use crate::{Dynamic, ImmutableString};

/// Declare the module's items in the std facade, under `env`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::{Builtin, Type};

    builder.module("env")?;
    let mut args = signature::Builder::new();
    args.output(Type::iterator(Type::builtin(Builtin::String)));
    builder.item(item::Definition::function("env::args", args.finish())?)?;
    Ok(())
}

/// Bind the module's items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    runtime
        .bind(format!("{}::env::args", super::ROOT), |_| {
            Ok(Dynamic::from(
                std::env::args_os()
                    .map(|argument| {
                        Dynamic::from(ImmutableString::from(argument.to_string_lossy().as_ref()))
                    })
                    .collect::<crate::Array>(),
            ))
        })
        .unwrap_or_else(|_| panic!("the std facade must retain env::args"));
}
