//! Origin-checked preview catalog launch proof.

use crate::{application, host, preview, program, Shared};
use std::fmt;

/// A preview catalog bound to the exact Engine that compiled it.
pub struct Launch {
    application: Option<(application::descriptor::Contract, application::Fingerprint)>,
    dependencies: Box<[host::facade::Dependency]>,
    projections: Box<[host::projection::Site]>,
    boundary_types: Box<[host::runtime::type_ref::Type]>,
    authority: program::Authority,
}

impl Launch {
    pub(crate) fn new(
        application: Option<(application::descriptor::Contract, application::Fingerprint)>,
        dependencies: Box<[host::facade::Dependency]>,
        projections: Box<[host::projection::Site]>,
        boundary_types: Box<[host::runtime::type_ref::Type]>,
        authority: program::Authority,
    ) -> Self {
        Self {
            application,
            dependencies,
            projections,
            boundary_types,
            authority,
        }
    }

    /// Bind this catalog to exact native facade implementations.
    pub fn bind(
        self,
        runtime: host::runtime::Registry,
    ) -> Result<preview::BoundCatalog, host::runtime::Error> {
        runtime.validate(
            &self.dependencies,
            self.authority.runtime_requirements(),
            &self.projections,
            self.authority.widget_requirements(),
            &self.boundary_types,
        )?;
        let runtime = Shared::new(runtime);
        let application = self.application.map(|(contract, fingerprint)| {
            let launch = application::Launch::new(contract, fingerprint, self.authority.clone());
            application::BoundLaunch::new(launch, runtime.clone())
        });
        Ok(preview::BoundCatalog::new(
            self.authority,
            runtime,
            application,
        ))
    }

    /// Return whether this launch also contains one real application.
    #[must_use]
    pub const fn has_application(&self) -> bool {
        self.application.is_some()
    }

    /// Return compiler-validated previews in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[preview::Definition] {
        self.authority.previews()
    }

    /// Return the immutable generation shared by every selection.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.authority.generation()
    }
}

impl fmt::Debug for Launch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Launch")
            .field("has_application", &self.has_application())
            .field("preview_count", &self.previews().len())
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}
