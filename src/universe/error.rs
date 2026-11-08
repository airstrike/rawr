//! Why a universe couldn't install its bindings, or a program couldn't
//! load or run.

use std::path::PathBuf;

use crate::package::import::resident;
use crate::{diagnostic, program, reload};

/// Why a [`super::Universe`] or a [`super::Program`] failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Installing the generated bindings in an engine failed.
    #[error("rawr: the bindings can't be installed: {0}")]
    Install(#[source] std::io::Error),
    /// Binding the generated runtime failed.
    #[error("rawr: the bindings' runtime can't be bound: {0}")]
    Runtime(#[source] crate::host::runtime::Error),
    /// A file of the program couldn't be read.
    #[error("couldn't read `{}`: {source}", path.display())]
    Read {
        /// The file or directory.
        path: PathBuf,
        /// Why it couldn't be read.
        #[source]
        source: std::io::Error,
    },
    /// The program's files don't make a package.
    #[error("{0}")]
    Snapshot(#[from] crate::package::snapshot::Error),
    /// The program doesn't compile against the bindings.
    #[error("{}", rendered(.0))]
    Compile(#[source] Box<resident::Error>),
    /// The compiled program couldn't bind the bindings' runtime.
    #[error("{0}")]
    Bind(#[source] reload::installation::Error),
    /// The program's top-level items failed as they were evaluated.
    #[error("{0}")]
    Evaluate(#[source] Box<crate::EvalAltResult>),
    /// The program has no function of this name at its crate root.
    #[error("error[E0425]: cannot find function `{0}` in the program's crate root")]
    Function(Box<str>),
    /// A call into the program failed as it ran.
    #[error("{0}")]
    Call(#[source] program::Error),
    /// No program runs on this thread: native code's calls reach the
    /// program on the thread that loaded it.
    #[error("rawr: no program runs on this thread")]
    Unowned,
}

impl Error {
    pub(super) fn compile(error: resident::Error) -> Self {
        Self::Compile(Box::new(error))
    }
}

/// A compile error as `rawr check` prints it: the report with the source it
/// points at.
fn rendered(error: &resident::Error) -> String {
    match error {
        resident::Error::Source(source) => match source.report() {
            Ok(report) => diagnostic::render(&report),
            Err(_) => source.to_string(),
        },
        error => error.to_string(),
    }
}
