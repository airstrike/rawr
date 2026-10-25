//! rawr-import: runs Rawr programs that use Rust crates from their
//! `Cargo.toml`, or native modules, in a runner the importer generates. The
//! runner links the `rawr` crate and the program's own dependencies, built in
//! the project's own build directory with the units `cargo build` builds.
//!
//! A host for a framework, such as rawr-iced, runs its programs through the
//! same owner with its own [`Profile`], which adds what that host needs to
//! the runner. The [`plain`] profile adds nothing.

#![deny(unsafe_code)]
// The owner and profiles return rawr-cli's diagnostic Report and this crate's
// public Error unboxed, as the other command crates do. Boxing them would
// change both public APIs.
#![allow(clippy::result_large_err)]

pub mod command;
pub mod native;
pub mod owner;
pub mod plain;
pub mod project;
mod release;
pub mod report;

use std::fmt;
use std::path::{Path, PathBuf};

/// What `rawr-import --rawr-manifest` prints: the programs `rawr` hands to
/// rawr-import (those with Rust dependencies in `Cargo.toml`, or native
/// modules) and the commands it adds.
pub const MANIFEST: &str = include_str!("../manifest.toml");

/// How a program's runner is prepared: the importer request, the checked
/// program and the runner source a host adds to it.
pub trait Profile {
    /// Resolve, extract, check and seal the runner request for `target`,
    /// from `graph` when the caller already loaded it.
    fn prepare(
        &self,
        target: &rawr::project::Target,
        graph: Option<rawr::package::Graph>,
        revision: rawr_import::supervisor::Revision,
        scope: &rawr_import::supervisor::snapshot::attempt::Scope,
    ) -> Result<Prepared, Error>;

    /// A socket under the project's build directory that the host's window
    /// listens on, which the owner removes when the run ends.
    fn socket(&self, _project: &Path) -> Option<PathBuf> {
        None
    }
}

/// One runner request paired with its exact source snapshot.
pub struct Prepared {
    request: rawr_import::bridge::GenerationRequestKey<rawr::package::import::Brand>,
    snapshot: rawr_import::supervisor::snapshot::Bytes,
    project: PathBuf,
    inputs: rawr::reload::source::Inputs,
    manifests: owner::resident::Manifests,
    limits: rawr_import::cargo::Limits,
}

impl Prepared {
    /// Bundle a sealed request with the snapshot, project root, watched
    /// inputs and limits it was prepared from. `graph` is the load it was
    /// prepared from, whose `Cargo.toml` a later save is compared against.
    #[must_use]
    pub fn new(
        request: rawr_import::bridge::GenerationRequestKey<rawr::package::import::Brand>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        project: PathBuf,
        inputs: rawr::reload::source::Inputs,
        graph: &rawr::package::Graph,
        limits: rawr_import::cargo::Limits,
    ) -> Self {
        Self {
            request,
            snapshot,
            project,
            inputs,
            manifests: owner::resident::Manifests::from_graph(graph),
            limits,
        }
    }

    /// The limits profile this preparation captured; later routes reuse it.
    #[must_use]
    pub const fn limits(&self) -> rawr_import::cargo::Limits {
        self.limits
    }

    /// Consume the bundle into the build request and exact source transport.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        rawr_import::bridge::GenerationRequestKey<rawr::package::import::Brand>,
        rawr_import::supervisor::snapshot::Bytes,
        PathBuf,
        rawr::reload::source::Inputs,
    ) {
        (self.request, self.snapshot, self.project, self.inputs)
    }
}

/// Failure while preparing a program's runner.
#[derive(Debug)]
pub enum Error {
    /// The program does not load or check; its report says why.
    Rawr(rawr::package::Error),
    /// The importer or the profile failed.
    Other(String),
}

impl Error {
    /// Any other failure, by its message.
    pub fn other(error: impl fmt::Display) -> Self {
        Self::Other(error.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rawr(error) => error.fmt(formatter),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rawr(error) => Some(error),
            Self::Other(_) => None,
        }
    }
}

/// The Rust path a failed compilation missed that a new preparation may
/// resolve: one below a wholesale crate re-export, or one below a
/// dependency extracted from demand, when not yet demanded.
/// A dependency nothing demands is extracted whole and has no more to give.
#[must_use]
pub fn new_demand<'error>(
    error: &'error rawr::package::Error,
    demand: &[Box<str>],
) -> Option<&'error str> {
    let root = |path: &str| path.split("::").next().map(str::to_owned);
    let path = error.rust_demand().or_else(|| {
        error
            .rust_miss()
            .filter(|path| demand.iter().any(|known| root(known) == root(path)))
    })?;
    (!demand.iter().any(|known| **known == *path)).then_some(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rawr_knows_rawr_import_as_it_describes_itself() {
        let manifest = rawr_cli::layer::Manifest::parse(super::MANIFEST).unwrap();
        let known = rawr_cli::layer::catalog()
            .into_iter()
            .find(|known| known.manifest.name == manifest.name)
            .expect("rawr's catalog lists rawr-import");
        assert_eq!(known.manifest, manifest);
        assert!(known
            .install
            .iter()
            .any(|line| line.ends_with(" rawr-import-cli")));
    }
}
