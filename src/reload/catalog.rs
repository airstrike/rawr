//! One intact executable awaiting its native runtime binding.

use crate::{host, program};
use std::fmt;

/// One intact executable with the native boundary it needs.
pub(crate) struct Catalog {
    executable: program::Executable,
    dependencies: Box<[host::facade::Dependency]>,
    projections: Box<[host::projection::Site]>,
}

// A candidate is made only by a rust-import build.
#[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
impl Catalog {
    /// A generation of any program, a plain one (a `main` run once) included.
    pub(crate) fn from_executable(executable: program::Executable) -> Self {
        let dependencies = executable.host_dependencies().into();
        let mut projections = executable.projection_requirements().to_vec();
        projections.sort();
        projections.dedup();
        Self {
            executable,
            dependencies,
            projections: projections.into_boxed_slice(),
        }
    }

    #[cfg(all(feature = "rust-import", feature = "sync"))]
    pub(crate) fn items(&self) -> impl ExactSizeIterator<Item = &crate::item::Value> {
        self.executable.items()
    }

    /// Return the immutable executable generation owned by this catalog.
    pub(crate) const fn generation(&self) -> program::GenerationId {
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

    /// Authorize the executable against the engine that compiled it and
    /// bind it to exact native facade implementations.
    pub(crate) fn bind(
        self,
        engine: &crate::Shared<crate::Engine>,
        runtime: host::runtime::Registry,
    ) -> Result<super::bound::BoundCatalog, super::installation::Error> {
        let authority = engine
            .authorize(self.executable)
            .map_err(super::installation::Error::Origin)?;
        runtime
            .validate(
                &self.dependencies,
                authority.runtime_requirements(),
                &self.projections,
            )
            .map_err(super::installation::Error::Runtime)?;
        Ok(super::bound::BoundCatalog::new(
            authority,
            crate::Shared::new(runtime),
        ))
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalog")
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
            .compile_executable("fn answer() -> i64 { 42 } panic!(\"top-level sentinel\");")
            .unwrap();
        let ordinary = executable.ast().clone();
        let catalog = super::Catalog::from_executable(executable);
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
