//! One-shot native binding for bootstrap.

use std::fmt;
use std::sync::Arc;

use crate::{host, program, reload, Dynamic, Engine, Shared};

/// One native-bound generation prepared for initial host bootstrap.
pub struct Bootstrap {
    generation: Arc<reload::Generation>,
    binding: Arc<reload::BoundCatalog>,
    /// Where the generation's steps are in its files, which place a panic.
    #[cfg(feature = "debugging")]
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    located: Option<Arc<crate::inspect::Located>>,
}

/// A compiled candidate could not cross exact engine-origin or runtime binding.
#[derive(Debug)]
pub enum Error {
    /// The candidate did not originate from the supplied compiler engine.
    Origin(program::Error),
    /// Native facade implementations did not satisfy the candidate contract.
    Runtime(host::runtime::Error),
}

impl reload::Candidate {
    /// Consume this candidate into the distinct initial-host transformation.
    // Bound catalogs share one exact identity across host-owned adapters;
    // they are intentionally not transferred to worker threads.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn bind(
        self,
        engine: &Shared<Engine>,
        runtime: host::runtime::Registry,
    ) -> Result<Bootstrap, Error> {
        let reload::Candidate {
            generation,
            catalog,
            #[cfg(feature = "debugging")]
            located,
        } = self;
        let catalog = catalog.bind(engine, runtime)?;
        debug_assert_eq!(generation.id(), catalog.generation());
        Ok(Bootstrap {
            generation,
            binding: Arc::new(catalog),
            #[cfg(feature = "debugging")]
            located,
        })
    }
}

impl Bootstrap {
    /// Where the generation's steps are in its files.
    #[cfg(feature = "debugging")]
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) const fn located(&self) -> Option<&Arc<crate::inspect::Located>> {
        self.located.as_ref()
    }

    /// Return the immutable metadata proof for this initial generation.
    #[must_use]
    pub const fn generation(&self) -> &Arc<reload::Generation> {
        &self.generation
    }

    /// Call the root package's `fn main` once. `None` when the package has
    /// no `main`.
    #[doc(hidden)]
    pub fn invoke_main(&self) -> Option<Result<Dynamic, program::Error>> {
        self.binding.invoke_main()
    }

    /// Call the root package's function `name` with `args`. `None` when
    /// the package has no such function.
    #[doc(hidden)]
    pub fn invoke_root(
        &self,
        name: &str,
        args: &mut [Dynamic],
    ) -> Option<Result<Dynamic, program::Error>> {
        self.binding.invoke_root(name, args)
    }

    /// Evaluate the generation's top level with its runtime bound, as its
    /// functions are called.
    ///
    /// # Errors
    ///
    /// Returns the error the evaluation stopped at.
    #[cfg(all(feature = "rust-import", feature = "sync"))]
    pub(crate) fn evaluate_top_level(&self) -> Result<(), Box<crate::EvalAltResult>> {
        self.binding
            .authority()
            .evaluate_top_level(self.binding.runtime().clone())
    }

    /// The bound catalog native islands call into.
    #[cfg(feature = "rust-import")]
    pub(crate) fn catalog(&self) -> &reload::BoundCatalog {
        &self.binding
    }
}

impl fmt::Debug for Bootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bootstrap")
            .field("generation", &self.generation.id())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Origin(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}
