//! std's `path`, `ffi` and `io` types for every program (D528.4). Every
//! [`Engine`](crate::Engine) made by `Engine::new` declares them, and every
//! runtime registry binds them, so a plain program, an embedding and an
//! iced app all name `std::path::PathBuf` (`resolve::std_reexport` sends
//! `std::path`, `std::ffi` and `std::io` here). Nothing here reads or
//! writes a file.

pub(crate) mod io;
mod path;

use crate::host::{self, facade};

/// The facade's root.
pub(crate) const ROOT: &str = "rawr_std";

/// The facade's compile-time definition, built once per thread.
pub(crate) fn definition() -> facade::Definition {
    thread_local! {
        static DEFINITION: facade::Definition = build().expect("the std facade is valid");
    }
    DEFINITION.with(Clone::clone)
}

fn build() -> Result<facade::Definition, host::Error> {
    path::register_debug();
    io::register_debug();
    let mut builder = facade::Builder::new(ROOT, facade::Abi::new(1)?)?;
    path::declare(&mut builder)?;
    io::declare(&mut builder)?;
    builder.finish()
}

/// The facade with its runtime bindings.
pub(crate) fn runtime() -> host::runtime::Facade {
    let mut builder = host::runtime::Builder::new(definition());
    path::bind(&mut builder);
    io::bind(&mut builder);
    builder.finish()
}
