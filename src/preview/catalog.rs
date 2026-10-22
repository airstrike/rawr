//! Opaque promotion of one intact executable into preview authority metadata.

use crate::{application, host, preview, program};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

type Parts = (
    program::Executable,
    Option<(application::descriptor::Contract, application::Fingerprint)>,
    Box<[host::facade::Dependency]>,
    Box<[host::projection::Site]>,
    Box<[host::runtime::type_ref::Type]>,
);

/// One intact executable proven to contain local previews.
pub struct Catalog {
    executable: program::Executable,
    application: Option<(application::descriptor::Contract, application::Fingerprint)>,
    dependencies: Box<[host::facade::Dependency]>,
    projections: Box<[host::projection::Site]>,
    boundary_types: Box<[host::runtime::type_ref::Type]>,
}

impl Catalog {
    pub(crate) fn from_executable(executable: program::Executable) -> Result<Self, preview::Error> {
        if executable.previews().is_empty() {
            return Err(preview::Error::Missing);
        }
        let application = executable
            .application()
            .map(|entry| {
                application::descriptor::Contract::validate(entry, &executable).map(|contract| {
                    let fingerprint = contract.fingerprint();
                    (contract, fingerprint)
                })
            })
            .transpose()?;
        let mut dependencies = application::descriptor::Dependencies::default();
        for dependency in executable.host_dependencies() {
            dependencies.insert(dependency.clone())?;
        }
        if let Some((contract, _)) = &application {
            for dependency in contract.dependencies() {
                dependencies.insert(dependency.clone())?;
            }
        }
        for definition in executable.previews() {
            dependencies.preview(definition)?;
        }
        Self::finish(executable, application, dependencies)
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_package_generation_executable(
        executable: program::Executable,
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        Self::from_package_executable_with_presence(executable, false, false)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_generation_executable_checked(
        executable: program::Executable,
        cycle: &host::facade::ProfileCycle,
        expected: &[u8; 32],
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        // A plain program (no application, a `main` run once) has neither an
        // application nor previews.
        let plain = *expected == *application::descriptor::SemanticFingerprint::plain().as_bytes();
        let catalog = Self::from_package_executable_with_presence(executable, false, plain)?;
        if let Some(actual) = catalog.application_semantic(cycle)?
            && actual.as_bytes() != expected
        {
            return Err((
                catalog.executable.model_source(),
                preview::Error::ApplicationSemanticMismatch,
            ));
        }
        Ok(catalog)
    }

    fn finish(
        executable: program::Executable,
        application: Option<(application::descriptor::Contract, application::Fingerprint)>,
        dependencies: application::descriptor::Dependencies,
    ) -> Result<Self, preview::Error> {
        let mut projections = executable.projection_requirements().to_vec();
        let mut boundary_types = Vec::new();
        if let Some((contract, _)) = &application {
            projections.extend(contract.boundary_projections().cloned());
            boundary_types.extend(contract.boundary_types());
        }
        for definition in executable.previews() {
            projections.extend(definition.projection_site().cloned());
            boundary_types.extend(definition.boundary_types()?);
        }
        projections.sort();
        projections.dedup();
        boundary_types.sort();
        boundary_types.dedup();
        Ok(Self {
            executable,
            application,
            dependencies: dependencies.finish(),
            projections: projections.into_boxed_slice(),
            boundary_types: boundary_types.into_boxed_slice(),
        })
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_package_executable(
        executable: program::Executable,
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        Self::from_package_executable_with_presence(executable, true, false)
    }

    #[cfg(not(feature = "no_std"))]
    fn from_package_executable_with_presence(
        executable: program::Executable,
        require_preview: bool,
        plain: bool,
    ) -> Result<Self, (program::source::Id, preview::Error)> {
        let fallback = executable.model_source();
        if require_preview && executable.previews().is_empty() {
            return Err((fallback, preview::Error::Missing));
        }
        if !plain && executable.application().is_none() && executable.previews().is_empty() {
            return Err((fallback, preview::Error::Missing));
        }
        let application = match executable.application() {
            Some(entry) => match application::descriptor::Contract::validate(entry, &executable) {
                Ok(contract) => {
                    let fingerprint = contract.fingerprint();
                    Some((contract, fingerprint))
                }
                Err(error) => {
                    let source = executable.application_error_source(&error);
                    return Err((source, preview::Error::Application(error)));
                }
            },
            None => None,
        };
        let mut dependencies = application::descriptor::Dependencies::default();
        for dependency in executable.host_dependencies() {
            dependencies
                .insert(dependency.clone())
                .map_err(|error| (fallback, preview::Error::Application(error)))?;
        }
        if let Some((contract, _)) = &application {
            for dependency in contract.dependencies() {
                dependencies
                    .insert(dependency.clone())
                    .map_err(|error| (fallback, preview::Error::Application(error)))?;
            }
        }
        for definition in executable.previews() {
            dependencies
                .preview(definition)
                .map_err(|error| (fallback, preview::Error::Application(error)))?;
        }
        Self::finish(executable, application, dependencies).map_err(|error| (fallback, error))
    }

    pub(crate) fn application_contract(&self) -> Option<&application::descriptor::Contract> {
        self.application.as_ref().map(|(contract, _)| contract)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn application_semantic(
        &self,
        cycle: &host::facade::ProfileCycle,
    ) -> Result<Option<application::SemanticFingerprint>, (program::source::Id, preview::Error)>
    {
        self.application_contract()
            .map(|contract| {
                contract.semantic_fingerprint(cycle).map_err(|error| {
                    let source = self.executable.application_error_source(&error);
                    (source, preview::Error::Application(error))
                })
            })
            .transpose()
    }

    pub(crate) fn preview_fingerprint(&self) -> &preview::Fingerprint {
        self.executable.preview_fingerprint()
    }

    pub(crate) fn items(&self) -> impl ExactSizeIterator<Item = &crate::item::Value> {
        self.executable.items()
    }

    pub(crate) fn widgets(&self) -> &[crate::widget::Definition] {
        self.executable.widgets()
    }

    /// Return whether this catalog also contains one real application selection.
    #[must_use]
    pub const fn has_application(&self) -> bool {
        self.application.is_some()
    }

    /// Return every compiler-validated preview in stable key order.
    #[must_use]
    pub const fn previews(&self) -> &[preview::Definition] {
        self.executable.previews()
    }

    /// Return the immutable executable generation owned by this catalog.
    #[must_use]
    pub const fn generation(&self) -> program::GenerationId {
        self.executable.generation()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn top_level_ast(&self) -> crate::AST {
        self.executable.ast().clone()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retain_imported(
        &mut self,
        receipt: crate::package::import::runner::account::support::Completed,
    ) {
        self.executable.retain_imported(receipt);
    }

    pub(crate) fn into_parts(self) -> Parts {
        (
            self.executable,
            self.application,
            self.dependencies,
            self.projections,
            self.boundary_types,
        )
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("has_application", &self.has_application())
            .field("preview_count", &self.previews().len())
            .field("executable", &"<private>")
            .finish()
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod top_level_tests {
    #[test]
    fn retained_ast_preserves_statements_without_evaluating_them() {
        let engine = crate::Engine::new();
        let executable = engine
            .compile_executable("fn answer() { 42 } throw \"top-level sentinel\";")
            .unwrap();
        let ordinary = executable.ast().clone();
        let catalog = super::Catalog::finish(
            executable,
            None,
            crate::application::descriptor::Dependencies::default(),
        )
        .unwrap();
        let ast = catalog.top_level_ast();
        assert_eq!(
            ast.iter_functions().count(),
            ordinary.iter_functions().count()
        );
        for retained in [&ast, &ordinary] {
            let error = engine.eval_ast::<crate::Dynamic>(retained).unwrap_err();
            assert!(error.to_string().contains("top-level sentinel"), "{error}");
        }
    }
}
