use crate::eval::GlobalRuntimeState;
use crate::func::SendSync;
use crate::{Engine, Position, RawrResultOf, Scope, SharedModule, AST};

mod collection;
mod dummy;
mod file;
mod stat;

pub use collection::ModuleResolversCollection;
pub use dummy::DummyModuleResolver;
#[cfg(any(not(target_family = "wasm"), not(target_os = "unknown")))]
pub use file::FileModuleResolver;
pub use stat::StaticModuleResolver;

/// Trait that encapsulates a module resolution service.
pub trait ModuleResolver: SendSync {
    /// Resolve a module based on a path string.
    fn resolve(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        pos: Position,
    ) -> RawrResultOf<SharedModule>;

    /// Resolve a module based on a path string, given a [`GlobalRuntimeState`] and the current [`Scope`].
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.
    #[allow(unused_variables)]
    fn resolve_raw(
        &self,
        engine: &Engine,
        global: &mut GlobalRuntimeState,
        scope: &mut Scope,
        path: &str,
        pos: Position,
    ) -> RawrResultOf<SharedModule> {
        self.resolve(engine, global.source(), path, pos)
    }

    /// Resolve an `AST` based on a path string.
    ///
    /// Returns [`None`] (default) if such resolution is not supported
    /// (e.g. if the module is Rust-based).
    ///
    /// # WARNING - Low Level API
    ///
    /// Override the default implementation of this method if the module resolver
    /// serves modules based on compiled Rawr scripts.
    #[allow(unused_variables)]
    #[must_use]
    fn resolve_ast(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        pos: Position,
    ) -> Option<RawrResultOf<AST>> {
        None
    }
}
