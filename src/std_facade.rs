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
//! the std table (`declare`) when the table or a bind changes and kept in
//! its final form ([`declared`]), which a new engine installs as it is
//! ([`step`]): a start reads nothing of the table (#1526).

mod case;
#[cfg(test)]
mod declare;
mod declared;
mod env;
mod fs;
mod held;
mod io;
mod num;
mod path;
mod process;
mod step;
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

/// Where the modules' binds go: `declare` reads only their paths, which
/// name the items it declares, and [`runtime`] their callbacks.
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
#[cfg(test)]
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
            .unwrap_or_else(|_| panic!("the std table does not declare `std::{path}`"));
    }

    fn with_context(
        &mut self,
        path: &str,
        callback: impl Fn(&Call<'_>, &mut [&mut Dynamic]) -> Outcome + SendSync + 'static,
    ) {
        self.bind_with_context(format!("{ROOT}::{path}"), callback)
            .unwrap_or_else(|_| panic!("the std table does not declare `std::{path}`"));
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

/// The public paths of the std types Rawr's std holds as the facade type
/// whose definition's path is `definition`, its own first:
/// `std::path::PathBuf` and `std::path::Path` of
/// `rawr_std::path::PathBuf`. Empty for a type the facade doesn't hold.
pub(crate) fn held_forms(definition: &str) -> Vec<&'static str> {
    held_value(definition)
        .map(|value| value.forms().collect())
        .unwrap_or_default()
}

/// The unsized std type a borrow of the facade type whose definition's
/// path is `definition` stands for: `std::path::Path` of
/// `rawr_std::path::PathBuf`, as rustc names a `&Path`.
pub(crate) fn lent_form(definition: &str) -> Option<&'static str> {
    held_value(definition)?.lent.first().copied()
}

/// The held type whose definition's path is `definition`.
fn held_value(definition: &str) -> Option<held::Value> {
    let path = definition.strip_prefix(ROOT)?.strip_prefix("::")?;
    held().into_iter().find_map(|held| match held {
        held::Held::Value(value) if held::Value::facade(value.std) == path => Some(value),
        _ => None,
    })
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
    let mut builder = facade::Builder::new(ROOT, facade::Abi::new(1)?)?;
    step::install(&mut builder, &held(), declared::STEPS)?;
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

#[cfg(test)]
mod tests {
    use crate::host::facade;

    /// The steps `declared` holds are the declarations the std table and
    /// the binds give: the facade a new engine installs is the one a
    /// builder declares straight from the table. `RAWR_STD_FACADE_WRITE=1`
    /// writes them again.
    #[test]
    fn std_facade_declared_is_the_std_tables() {
        let held = super::held();
        let mut bound = Vec::new();
        super::bind(&mut bound);
        if std::env::var_os("RAWR_STD_FACADE_WRITE").is_some() {
            let mut text = super::declare::Text::default();
            super::declare::declare(&mut text, &held, &bound).expect("the std bindings are valid");
            let file = concat!(env!("CARGO_MANIFEST_DIR"), "/src/std_facade/declared.rs");
            std::fs::write(file, text.finish()).expect("src/std_facade is writable");
            return;
        }
        let mut builder = facade::Builder::new(super::ROOT, facade::Abi::new(1).unwrap()).unwrap();
        super::declare::declare(&mut builder, &held, &bound).unwrap();
        let declared = builder.finish().unwrap();
        let installed = super::build().unwrap();
        let stale = "src/std_facade/declared.rs is not what the std table and the binds \
                     declare; run scripts/std-table.sh, or `RAWR_STD_FACADE_WRITE=1 cargo test \
                     -p rawr --lib std_facade_declared_is_the_std_tables` and rustfmt it";
        assert_eq!(declared.fingerprint(), installed.fingerprint(), "{stale}");
        assert_eq!(format!("{declared:?}"), format!("{installed:?}"), "{stale}");
    }
}
