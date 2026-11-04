//! std's `path`, `ffi`, `io`, `env`, `fs` and `process` items for every
//! program (D528.4). Every [`Engine`](crate::Engine) made by `Engine::new`
//! declares them, and every runtime registry binds them, so a plain
//! program, an embedding and an iced app all name `std::path::PathBuf`
//! (`resolve::std_reexport` sends those modules here). The `fs` functions,
//! `env::args` and `process::id` reach the operating system only as far as
//! the calling program's [`Access`](crate::Access) grants (D683.4).
//!
//! The facade states its binds and nothing else (D711 step 6): each module
//! binds std items to the Rust code rawr runs for them and says how rawr
//! holds std's types ([`held`]). Every declaration is std's own, read from
//! the std table ([`declare`]).

mod case;
mod declare;
mod env;
mod fs;
mod held;
mod io;
mod num;
mod path;
mod process;
mod time;

pub(crate) use case::mapped as case_mapped;
pub(crate) use num::int_error_kind;

use crate::func::SendSync;
use crate::host::{self, facade, runtime::Call};
use crate::{Dynamic, EvalAltResult};

/// The facade's root.
pub(crate) const ROOT: &str = "rawr_std";

/// What a bind's callback gives.
type Outcome = Result<Dynamic, Box<EvalAltResult>>;

/// Where the modules' binds go: [`definition`] reads only their paths,
/// which name the items it declares, and [`runtime`] their callbacks.
trait Bind {
    /// Run `callback` for the item at `path`, relative to the facade's
    /// root: `path::PathBuf::join`.
    fn plain(
        &mut self,
        path: &str,
        callback: impl Fn(&mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    );

    /// [`plain`](Self::plain), for a callback that reads the call's
    /// program's access.
    fn with_context(
        &mut self,
        path: &str,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    );
}

/// The bound paths.
impl Bind for Vec<String> {
    fn plain(
        &mut self,
        path: &str,
        _: impl Fn(&mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    ) {
        self.push(path.to_owned());
    }

    fn with_context(
        &mut self,
        path: &str,
        _: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    ) {
        self.push(path.to_owned());
    }
}

/// The runtime's bindings: the definition declares every bound path.
impl Bind for host::runtime::Builder {
    fn plain(
        &mut self,
        path: &str,
        callback: impl Fn(&mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    ) {
        self.bind(format!("{ROOT}::{path}"), callback)
            .unwrap_or_else(|_| panic!("the std table declares no `std::{path}` rawr runs"));
    }

    fn with_context(
        &mut self,
        path: &str,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    ) {
        self.bind_with_context(format!("{ROOT}::{path}"), callback)
            .unwrap_or_else(|_| panic!("the std table declares no `std::{path}` rawr runs"));
    }
}

/// Whether the type at `path` under the root is one Rawr's std holds as
/// std's own Rust value, which a crate takes, gives and borrows as itself
/// (`crossing::Std`).
pub(crate) fn crosses(path: &str) -> bool {
    held()
        .iter()
        .any(|held| matches!(held, held::Held::Value(value) if value.crosses && held::Value::facade(value.std) == path))
}

/// The facade's compile-time definition, built once per thread.
pub(crate) fn definition() -> facade::Definition {
    thread_local! {
        static DEFINITION: facade::Definition = build().expect("the std bindings are valid");
    }
    DEFINITION.with(Clone::clone)
}

fn build() -> Result<facade::Definition, host::Error> {
    path::register_debug();
    io::register_debug();
    num::register_debug();
    case::register_debug();
    time::register_debug();
    let mut bound = Vec::new();
    bind(&mut bound);
    let held = held();
    let mut builder = facade::Builder::new(ROOT, facade::Abi::new(1)?)?;
    declare::declare(&mut builder, &held, &bound)?;
    builder.finish()
}

/// Every module's held types.
fn held() -> Vec<held::Held> {
    [
        path::HELD.as_slice(),
        io::HELD.as_slice(),
        case::HELD.as_slice(),
        num::HELD.as_slice(),
        time::HELD.as_slice(),
    ]
    .concat()
}

/// Every module's binds.
fn bind(binds: &mut impl Bind) {
    path::bind(binds);
    io::bind(binds);
    env::bind(binds);
    fs::bind(binds);
    process::bind(binds);
    num::bind(binds);
    case::bind(binds);
    time::bind(binds);
}

/// The facade with its runtime bindings.
pub(crate) fn runtime() -> host::runtime::Facade {
    let mut builder = host::runtime::Builder::new(definition());
    bind(&mut builder);
    builder.finish()
}
