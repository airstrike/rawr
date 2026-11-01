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
mod signals;

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
    settlement: rawr_import::bindings::Settlement,
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
            settlement: rawr_import::bindings::Settlement::default(),
        }
    }

    /// The shared bindings this preparation used or extracted, settled once
    /// the runner builds.
    #[must_use]
    pub fn with_settlement(mut self, settlement: rawr_import::bindings::Settlement) -> Self {
        self.settlement = settlement;
        self
    }

    /// Take the shared bindings to settle after the runner builds.
    pub fn take_settlement(&mut self) -> rawr_import::bindings::Settlement {
        std::mem::take(&mut self.settlement)
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
    /// A cache entry the preparation read went stale before it was used.
    /// Preparing again rebuilds it ([`again_while_stale`]).
    Stale(String),
}

impl Error {
    /// Any other failure, by its message.
    pub fn other(error: impl fmt::Display) -> Self {
        Self::Other(error.to_string())
    }

    /// An importer failure, which is [`Error::Stale`] when a cache entry
    /// went stale.
    pub fn cache(error: impl Stale + fmt::Display) -> Self {
        if error.is_stale() {
            Self::Stale(error.to_string())
        } else {
            Self::other(error)
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rawr(error) => error.fmt(formatter),
            Self::Other(message) | Self::Stale(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rawr(error) => Some(error),
            Self::Other(_) | Self::Stale(_) => None,
        }
    }
}

/// How many times one preparation starts again for entries that went stale.
const STALE_RESTARTS: usize = 2;

/// A failure that may be a cache entry gone stale while the run used it.
pub trait Stale {
    /// Whether it is.
    fn is_stale(&self) -> bool;
}

impl Stale for Error {
    fn is_stale(&self) -> bool {
        matches!(self, Self::Stale(_))
    }
}

macro_rules! stale {
    ($($error:ty),* $(,)?) => {$(
        impl Stale for $error {
            fn is_stale(&self) -> bool {
                <$error>::is_stale(self)
            }
        }
    )*};
}

stale!(
    rawr_import::Error,
    rawr_import::checked::Error,
    rawr_import::node::Error,
    rawr_import::profile::Error,
    rawr_import::bridge::request::Error,
    rawr_import::runner::BuildError,
);

/// Run `attempt` again while it fails only because a cache entry it read
/// went stale: the next attempt's lookups miss and rebuild the entry, as
/// Cargo rebuilds what changed (#609, #613). Inputs that keep changing end
/// the restarts with their error.
pub fn again_while_stale<T, E: Stale>(mut attempt: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    let mut restarts = Restarts::default();
    loop {
        match attempt() {
            Err(error) if restarts.again(&error) => {}
            result => return result,
        }
    }
}

/// The restarts left to one preparation for entries that went stale.
#[derive(Default)]
pub(crate) struct Restarts(usize);

impl Restarts {
    /// Whether `error` starts the preparation again, which counts.
    pub(crate) fn again(&mut self, error: &impl Stale) -> bool {
        let again = error.is_stale() && self.0 < STALE_RESTARTS;
        self.0 += usize::from(again);
        again
    }
}

#[cfg(test)]
mod stale {
    use super::{again_while_stale, Error, STALE_RESTARTS};

    /// A stale entry prepares again until it is rebuilt, a bounded number
    /// of times, and any other failure is reported at once (#613).
    #[test]
    fn stale_entries_prepare_again() {
        let stale = || Error::cache(rawr_import::profile::Error::Stale);
        let mut attempts = 0;
        let rebuilt = again_while_stale(|| {
            attempts += 1;
            if attempts == 1 {
                Err(stale())
            } else {
                Ok(attempts)
            }
        });
        assert_eq!(rebuilt.ok(), Some(2));

        let mut attempts = 0;
        let changing = again_while_stale(|| -> Result<(), Error> {
            attempts += 1;
            Err(stale())
        });
        assert!(matches!(changing, Err(Error::Stale(_))));
        assert_eq!(attempts, STALE_RESTARTS + 1);

        let mut attempts = 0;
        let failed = again_while_stale(|| -> Result<(), Error> {
            attempts += 1;
            Err(Error::other("the program does not compile"))
        });
        assert!(matches!(failed, Err(Error::Other(_))));
        assert_eq!(attempts, 1);
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
