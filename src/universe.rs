//! A crate's own interpreter (#1387): the generated bindings of the
//! crate's dependencies, installed once in an engine, and a marked file
//! loaded against them, which runs in this process and reloads in it.
//!
//! The bindings are the ones `rawr-import` generates for a runner: each
//! native call the program makes, adapted to the crate that defines it,
//! linked into the same binary as the crate's own code (D608: one iced per
//! project, built by Cargo). Whatever links them implements [`Bindings`]:
//! the runner rawr-import builds does, and `?play`'s universe runner serves
//! each snippet through a [`Universe`] (#434, #515).
//!
//! A [`Universe`] compiles a [`Program`] against those bindings. The
//! program runs on the thread that loaded it, which owns it from then on,
//! as the process's main thread owns a program `rawr run` runs (D533). A
//! [`Reloader`] compiles a new text on any thread, against the same
//! engine, and offers it to that thread, which takes it at its next call
//! from native code into script code (D553.3). Each reload says what it did
//! as an [`Outcome`], with the words `rawr-reload` uses: `queued`,
//! `swapped`, `failed` and `restarting`.
//!
//! A native call the bindings don't cover is the program's compile error,
//! where the program makes it, as on `?play`: the bindings cover what the
//! program called when they were generated.

mod error;
mod outcome;
mod program;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub use error::Error;
pub use outcome::Outcome;
pub use program::{Program, Reloader};

use crate::host::facade::Definition;
use crate::host::runtime::Registry;
use crate::{host, package, Access, Engine, Shared};

/// The generated bindings a binary links: what installs them in an engine
/// and binds their runtime. A generated runner implements it over its own
/// adapters.
pub trait Bindings {
    /// A new engine with the bindings installed, whose programs may reach
    /// `access` (D683.4), and the facades the runtime binds.
    ///
    /// # Errors
    ///
    /// Returns the error installing the bindings gave.
    fn engine(&self, access: Access) -> std::io::Result<(Shared<Engine>, Box<[Definition]>)>;

    /// The runtime that binds the facades `engine` was given, which each
    /// generation the engine compiles binds a generation of (D475.2).
    ///
    /// # Errors
    ///
    /// Returns the error binding the runtime gave.
    fn runtime(
        &self,
        engine: &Engine,
        definitions: &[Definition],
    ) -> Result<Registry, host::runtime::Error>;
}

/// Generated bindings installed in an engine with their runtime bound: what
/// every program it loads compiles against. Made once, as `?play`'s server
/// makes it before it forks for each snippet (#515).
///
/// The program it loads last is the one this thread runs: native code's
/// calls reach one program at a time.
pub struct Universe {
    engine: Shared<Engine>,
    /// The runtime the first program binds. Each later one binds a next
    /// generation of it.
    runtime: RefCell<Option<Registry>>,
}

impl Universe {
    /// Install `bindings` in a new engine and bind their runtime. Its
    /// programs may reach `access`: [`Access::all`] for a program that runs
    /// as Cargo builds it, less for one that isn't the user's own.
    ///
    /// # Errors
    ///
    /// Returns the error installing or binding the bindings gave.
    pub fn new(bindings: &dyn Bindings, access: Access) -> Result<Self, Error> {
        let (engine, definitions) = bindings.engine(access).map_err(Error::Install)?;
        let runtime = bindings
            .runtime(&engine, &definitions)
            .map_err(Error::Runtime)?;
        Ok(Self {
            engine,
            runtime: RefCell::new(Some(runtime)),
        })
    }

    /// Load the marked file at `path` with its package: the `Cargo.toml`
    /// nearest above it, whose dependencies its `use`s name, and the Rust
    /// files beside and below it, which its `mod`s may name. It compiles
    /// against the bindings, its top-level items are evaluated, and this
    /// thread owns it from now on.
    ///
    /// # Errors
    ///
    /// Returns the error reading its files gave, or the program's compile
    /// error, rendered as rustc renders it.
    pub fn load(&self, path: impl AsRef<Path>) -> Result<Program, Error> {
        let sources = program::Sources::read(path.as_ref())?;
        Program::load(self, sources)
    }

    /// The program whose crate root at `entry` holds `text`, with `files`
    /// for the rest of its package, such as its `Cargo.toml`: one that
    /// isn't on disk, as a `?play` snippet isn't. [`Reloader::reload`]
    /// rereads nothing for it, and [`Reloader::replace`] gives it new text.
    ///
    /// # Errors
    ///
    /// Returns the program's compile error, rendered as rustc renders it.
    pub fn program(
        &self,
        entry: PathBuf,
        text: impl Into<Box<str>>,
        files: Vec<(PathBuf, Box<str>)>,
    ) -> Result<Program, Error> {
        Program::load(self, program::Sources::given(entry, text.into(), files))
    }

    /// The runtime the next program loaded binds: the first one, and a
    /// next generation of it for each one after.
    fn bind_runtime(&self) -> Option<Registry> {
        self.runtime.borrow_mut().take()
    }
}

/// Compile `snapshot` against `engine`'s bindings alone, as a universe
/// runner compiles a snippet: a native call they don't cover is the
/// program's compile error.
fn compile(
    engine: &Shared<Engine>,
    snapshot: &package::snapshot::Snapshot,
) -> Result<(crate::reload::Candidate, crate::AST), Error> {
    let compiled = engine
        .compile_package_snapshot_covered(snapshot)
        .map_err(Error::compile)?;
    let (compiled, ast) = compiled.with_top_level();
    let (candidate, _, _) = compiled.into_parts();
    Ok((candidate, ast))
}
