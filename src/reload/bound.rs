//! Execution through exact launch-bound authority.

use crate::{host, program, Shared};
#[cfg(feature = "rust-import")]
use crate::{item, Dynamic};
use std::fmt;

/// A catalog whose complete native runtime boundary is proven.
pub struct BoundCatalog {
    authority: program::Authority,
    runtime: Shared<host::runtime::Registry>,
}

impl BoundCatalog {
    pub(crate) fn new(
        authority: program::Authority,
        runtime: Shared<host::runtime::Registry>,
    ) -> Self {
        Self { authority, runtime }
    }

    /// Call the root package's `fn main` once. `None` when the package has
    /// no `main`.
    #[doc(hidden)]
    pub fn invoke_main(&self) -> Option<Result<crate::Dynamic, program::Error>> {
        let package = self.authority.package();
        let main = self.authority.items().find(|item| {
            let key = item.key();
            item.kind() == crate::item::Kind::Function
                && key.package() == package
                && key.modules().is_empty()
                && key.name() == "main"
        })?;
        Some(
            self.authority
                .invoke_bound(main, &mut [], self.runtime.clone()),
        )
    }

    /// The generation's items and runtime model, for native islands.
    #[cfg(feature = "rust-import")]
    pub(crate) const fn authority(&self) -> &program::Authority {
        &self.authority
    }

    /// Call `item` with `args` against this catalog's native runtime. A
    /// receiver or a `&mut` parameter the call changes is changed in `args`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn invoke_item(
        &self,
        item: &item::Value,
        args: &mut [Dynamic],
    ) -> Result<Dynamic, program::Error> {
        self.authority
            .invoke_bound(item, args, self.runtime.clone())
    }

    /// Return the immutable generation.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.authority.generation()
    }

    /// The program and its runtime, for calls from other threads (D490).
    #[cfg(all(feature = "rust-import", feature = "sync"))]
    pub(crate) fn shared(&self) -> (program::Authority, Shared<host::runtime::Registry>) {
        (self.authority.clone(), self.runtime.clone())
    }

    /// The runtime the program's witnesses are issued by.
    #[cfg(feature = "rust-import")]
    pub(crate) const fn runtime(&self) -> &Shared<host::runtime::Registry> {
        &self.runtime
    }

    /// Validate one value against the exact type a runtime witness proves.
    #[cfg(feature = "rust-import")]
    pub(crate) fn validate_witness(
        &self,
        witness: &host::runtime::Witness,
        value: &Dynamic,
    ) -> Result<(), host::runtime::ValueError> {
        self.authority
            .validate_witness(witness, value, &self.runtime)
    }
}

impl fmt::Debug for BoundCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundCatalog")
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}
