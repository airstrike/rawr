//! Immutable candidates for script-native program replacement.

pub mod callback;
pub mod compatibility;
pub mod installation;
#[cfg(not(feature = "no_std"))]
pub mod scheduler;
pub mod source;

use std::fmt;
use std::sync::Arc;

use crate::{preview, program};

/// Immutable metadata proving one completely validated generation.
///
/// The executable payload is deliberately absent. A compiled [`Candidate`]
/// owns that one-shot payload until bootstrap or compatibility-proven binding
/// consumes it. Installed hosts retain this shareable proof for later
/// compatibility comparisons without cloning evaluator authority.
pub struct Generation {
    id: program::GenerationId,
    source: source::Graph,
    schemas: program::schema::Manifest,
    callbacks: callback::Manifest,
    application: bool,
    previews: Box<[preview::Definition]>,
}

/// One compiled generation awaiting its sole native-runtime binding.
///
/// Compatibility may inspect the immutable metadata, but only bootstrap or a
/// consumed compatibility plan can move the private catalog to runtime
/// binding.
pub struct Candidate {
    generation: Arc<Generation>,
    catalog: preview::Catalog,
}

impl Candidate {
    pub(crate) fn from_package(
        executable: program::Executable,
        source: source::Graph,
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        let schemas = executable.schemas().clone();
        let catalog = preview::Catalog::from_package_generation_executable(executable)?;
        Ok(Self::from_catalog(catalog, schemas, source))
    }

    /// A generation of any program, a plain one included, with no host
    /// profile to check its application against.
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_plain(
        executable: program::Executable,
        source: source::Graph,
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        let schemas = executable.schemas().clone();
        let catalog = preview::Catalog::from_package_generation_executable_plain(executable)?;
        Ok(Self::from_catalog(catalog, schemas, source))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_checked(
        executable: program::Executable,
        source: source::Graph,
        cycle: &crate::host::facade::ProfileCycle,
        expected: &[u8; 32],
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        let schemas = executable.schemas().clone();
        let catalog = preview::Catalog::from_package_generation_executable_checked(
            executable, cycle, expected,
        )?;
        Ok(Self::from_catalog(catalog, schemas, source))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_derived(
        executable: program::Executable,
        source: source::Graph,
        cycle: &crate::host::facade::ProfileCycle,
    ) -> Result<
        (Self, Option<crate::application::SemanticFingerprint>),
        (program::source::Id, preview::Error),
    > {
        let schemas = executable.schemas().clone();
        let catalog = preview::Catalog::from_package_generation_executable(executable)?;
        let semantic = catalog.application_semantic(cycle)?;
        Ok((Self::from_catalog(catalog, schemas, source), semantic))
    }

    // Generation handles deliberately use Arc for stable cross-owner proof
    // identity even when one concrete catalog is confined to the host thread.
    #[allow(clippy::arc_with_non_send_sync)]
    fn from_catalog(
        catalog: preview::Catalog,
        schemas: program::schema::Manifest,
        source: source::Graph,
    ) -> Self {
        let id = catalog.generation();
        let callbacks = callback::Manifest::from_catalog(&catalog, &schemas);
        let application = catalog.has_application();
        let previews = catalog.previews().to_vec().into_boxed_slice();
        Self {
            generation: Arc::new(Generation {
                id,
                source,
                schemas,
                callbacks,
                application,
                previews,
            }),
            catalog,
        }
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

    /// Return the normalized callback proofs partitioned by live owner.
    #[must_use]
    pub fn callbacks(&self) -> &callback::Manifest {
        self.generation.callbacks()
    }

    /// Return whether this candidate contains a validated application.
    #[must_use]
    pub fn has_application(&self) -> bool {
        self.generation.has_application()
    }

    /// Return all compiler-validated preview descriptors in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[preview::Definition] {
        self.generation.previews()
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

    /// Return the normalized application and preview callback proofs.
    #[must_use]
    pub const fn callbacks(&self) -> &callback::Manifest {
        &self.callbacks
    }

    /// Return whether this generation contains a validated application.
    #[must_use]
    pub const fn has_application(&self) -> bool {
        self.application
    }

    /// Return all compiler-validated preview descriptors in stable key order.
    #[must_use]
    pub const fn previews(&self) -> &[preview::Definition] {
        &self.previews
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
            .field("has_application", &self.has_application())
            .field("previews", &self.previews().len())
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
            .field("has_application", &self.has_application())
            .field("previews", &self.previews().len())
            .finish_non_exhaustive()
    }
}
