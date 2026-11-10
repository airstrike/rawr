//! Immutable candidates for script-native program replacement.

mod bound;
mod catalog;
pub mod installation;
pub mod scheduler;
pub mod source;

use std::fmt;
use std::sync::Arc;

use crate::program;

pub use bound::BoundCatalog;

/// Immutable metadata proving one completely validated generation.
///
/// The executable payload is deliberately absent. A compiled [`Candidate`]
/// owns that one-shot payload until bootstrap binding consumes it.
pub struct Generation {
    id: program::GenerationId,
    source: source::Graph,
    schemas: program::schema::Manifest,
}

/// One compiled generation awaiting its sole native-runtime binding.
///
/// Only bootstrap can move the private catalog to runtime binding.
pub struct Candidate {
    generation: Arc<Generation>,
    catalog: catalog::Catalog,
    /// Where its steps are, for a debugger registered before it compiled.
    #[cfg(feature = "debugging")]
    located: Option<Arc<crate::inspect::Located>>,
    /// A crate built for its tests: its `#[test]` functions, which run in
    /// place of its `main`.
    tests: Option<Box<[crate::package::Test]>>,
}

impl Candidate {
    /// A generation of any program, a plain one included. It keeps the
    /// program's positions, which place a panic and a debugger's steps.
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_plain(
        executable: program::Executable,
        source: source::Graph,
    ) -> Self {
        #[cfg(feature = "debugging")]
        let located = executable.located().map(Arc::new);
        let schemas = executable.schemas().clone();
        let candidate = Self::from_catalog(
            catalog::Catalog::from_executable(executable),
            schemas,
            source,
        );
        #[cfg(feature = "debugging")]
        let candidate = Self {
            located,
            ..candidate
        };
        candidate
    }

    /// Where this generation's steps are in its files.
    #[cfg(feature = "debugging")]
    #[must_use]
    pub const fn located(&self) -> Option<&Arc<crate::inspect::Located>> {
        self.located.as_ref()
    }

    // Generation handles deliberately use Arc for stable cross-owner proof
    // identity even when one concrete catalog is confined to the host thread.
    #[allow(clippy::arc_with_non_send_sync)]
    // A candidate is made only by a rust-import build.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    fn from_catalog(
        catalog: catalog::Catalog,
        schemas: program::schema::Manifest,
        source: source::Graph,
    ) -> Self {
        let id = catalog.generation();
        Self {
            generation: Arc::new(Generation {
                id,
                source,
                schemas,
            }),
            catalog,
            #[cfg(feature = "debugging")]
            located: None,
            tests: None,
        }
    }

    /// This candidate with `tests`, its crate's when it is built for them.
    #[cfg(feature = "rust-import")]
    pub(crate) fn with_tests(self, tests: Option<Box<[crate::package::Test]>>) -> Self {
        Self { tests, ..self }
    }

    /// Take the crate's `#[test]` functions, sorted by name, when it is
    /// built for its tests: they run in place of its `main`.
    pub fn take_tests(&mut self) -> Option<Box<[crate::package::Test]>> {
        self.tests.take()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn with_top_level(self) -> (Self, crate::AST) {
        let ast = self.catalog.top_level_ast();
        (self, ast)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retain_imported(
        mut self,
        receipt: crate::package::import::runner::account::support::Completed,
    ) -> Self {
        self.catalog.retain_imported(receipt);
        self
    }

    /// The items this candidate declares, lifted closures among them.
    #[cfg(all(feature = "rust-import", feature = "sync"))]
    pub(crate) fn items(&self) -> impl ExactSizeIterator<Item = &crate::item::Value> {
        self.catalog.items()
    }

    /// Return this candidate's immutable metadata proof.
    #[must_use]
    pub const fn generation(&self) -> &Arc<Generation> {
        &self.generation
    }

    /// Return this candidate's unique executable generation identifier.
    #[must_use]
    pub fn id(&self) -> program::GenerationId {
        self.generation.id()
    }

    /// Return the exact canonical source graph consumed by compilation.
    #[must_use]
    pub fn source(&self) -> &source::Graph {
        self.generation.source()
    }

    /// Return every stable nominal schema proof from the resolved program.
    #[must_use]
    pub fn schemas(&self) -> &program::schema::Manifest {
        self.generation.schemas()
    }
}

impl Generation {
    /// Return this current proof's unique executable generation identifier.
    #[must_use]
    pub const fn id(&self) -> program::GenerationId {
        self.id
    }

    /// Return the exact canonical source graph consumed by compilation.
    #[must_use]
    pub const fn source(&self) -> &source::Graph {
        &self.source
    }

    /// Return every stable nominal schema proof from the resolved program.
    #[must_use]
    pub const fn schemas(&self) -> &program::schema::Manifest {
        &self.schemas
    }
}

impl fmt::Debug for Candidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Candidate")
            .field("id", &self.id())
            .field("packages", &self.source().packages().len())
            .field("sources", &self.source().sources().len())
            .field("schemas", &self.schemas().len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Generation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Generation")
            .field("id", &self.id())
            .field("packages", &self.source.packages().len())
            .field("sources", &self.source.sources().len())
            .field("schemas", &self.schemas.len())
            .finish_non_exhaustive()
    }
}
