//! What a program compiled by [`Engine::compile`](crate::Engine::compile)
//! needs from its engine's exported items (D43 as D494 item 15 scopes it to
//! embedders).
//!
//! The program is its own generation. On its first evaluation it is bound
//! against its exact facade dependencies: an engine of another origin, a
//! missing facade, a changed fingerprint or a missing binding fails before
//! any callback runs. Later evaluations reuse the binding.

use crate::host;
use crate::{EvalAltResult, Position, Shared};

#[cfg(feature = "std")]
type OnceCell<T> = once_cell::sync::OnceCell<T>;
#[cfg(not(feature = "std"))]
type OnceCell<T> = once_cell::race::OnceBox<T>;

/// A compiled program's exact host requirements and, once evaluated, its
/// bound runtime generation.
pub(crate) struct Hosted {
    origin: super::origin::Origin,
    dependencies: Box<[host::facade::Dependency]>,
    requirements: Box<[host::runtime::Requirement]>,
    projections: Box<[host::projection::Site]>,
    widgets: Box<[host::runtime::widget::Requirement]>,
    bound: OnceCell<Shared<host::runtime::Registry>>,
}

impl Hosted {
    pub(crate) fn new(
        origin: super::origin::Origin,
        dependencies: Box<[host::facade::Dependency]>,
        requirements: Box<[host::runtime::Requirement]>,
        projections: Box<[host::projection::Site]>,
        widgets: Box<[host::runtime::widget::Requirement]>,
    ) -> Self {
        Self {
            origin,
            dependencies,
            requirements,
            projections,
            widgets,
            bound: OnceCell::new(),
        }
    }

    /// This program's runtime generation of `engine`'s registry, bound on
    /// first use. `None` when the engine exports nothing, which leaves each
    /// host call to report its missing binding.
    pub(crate) fn bind(
        &self,
        engine: &crate::Engine,
    ) -> Result<Option<Shared<host::runtime::Registry>>, Box<EvalAltResult>> {
        let Some(registry) = engine.host_runtime.as_ref() else {
            return Ok(None);
        };
        if engine.origin() != self.origin {
            return Err(EvalAltResult::ErrorRuntime(
                "this program was compiled by another engine; compile it with the engine that runs it"
                    .into(),
                Position::NONE,
            )
            .into());
        }
        let bind = || -> Result<Shared<host::runtime::Registry>, Box<EvalAltResult>> {
            registry
                .validate(
                    &self.dependencies,
                    &self.requirements,
                    &self.projections,
                    &self.widgets,
                    &[],
                )
                .map_err(|error| {
                    EvalAltResult::ErrorRuntime(error.to_string().into(), Position::NONE)
                })?;
            Ok(Shared::new(registry.next_generation()))
        };
        #[cfg(feature = "std")]
        let bound = self.bound.get_or_try_init(bind)?;
        #[cfg(not(feature = "std"))]
        let bound = self.bound.get_or_try_init(|| bind().map(Box::new))?;
        Ok(Some(bound.clone()))
    }
}

impl crate::Engine {
    /// Freeze the exports when a compiled program depends on one of them.
    pub(crate) fn freeze_exports(&self, dependencies: &[host::facade::Dependency]) {
        if let Some(registry) = &self.host_runtime
            && dependencies
                .iter()
                .any(|dependency| registry.contains(dependency.root()))
        {
            self.exports_frozen
                .store(true, core::sync::atomic::Ordering::Release);
        }
    }

    /// Bind `ast`'s exported items for one evaluation, unless the caller
    /// bound a runtime already (an application launch).
    pub(crate) fn bind_exports(
        &self,
        ast: &crate::AST,
        global: &mut crate::eval::GlobalRuntimeState,
    ) -> Result<(), Box<EvalAltResult>> {
        if global.host_runtime.is_none()
            && let Some(hosted) = ast.host()
        {
            global.host_runtime = hosted.bind(self)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Hosted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hosted")
            .field("dependencies", &self.dependencies)
            .finish_non_exhaustive()
    }
}
