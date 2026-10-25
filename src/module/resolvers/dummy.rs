use crate::{Engine, ModuleResolver, Position, RawrResultOf, SharedModule, ERR};

/// Empty/disabled [module][crate::Module] resolution service that acts as a dummy.
///
/// # Example
///
/// ```
/// use rawr::{Engine, Module};
/// use rawr::module_resolvers::DummyModuleResolver;
///
/// let resolver = DummyModuleResolver::new();
/// let mut engine = Engine::new();
/// engine.set_module_resolver(resolver);
/// ```
#[derive(Debug, Copy, Eq, PartialEq, Clone, Default, Hash)]
pub struct DummyModuleResolver;

impl DummyModuleResolver {
    /// Create a new [`DummyModuleResolver`].
    ///
    /// # Example
    ///
    /// ```
    /// use rawr::{Engine, Module};
    /// use rawr::module_resolvers::DummyModuleResolver;
    ///
    /// let resolver = DummyModuleResolver::new();
    /// let mut engine = Engine::new();
    /// engine.set_module_resolver(resolver);
    /// ```
    #[inline(always)]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ModuleResolver for DummyModuleResolver {
    #[inline(always)]
    fn resolve(
        &self,
        _: &Engine,
        _: Option<&str>,
        path: &str,
        pos: Position,
    ) -> RawrResultOf<SharedModule> {
        Err(ERR::ErrorModuleNotFound(path.into(), pos).into())
    }
}
