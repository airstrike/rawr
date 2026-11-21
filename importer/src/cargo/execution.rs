//! Installation ownership for native work outside a cached node executor.

use sha2::{Digest as _, Sha256};

use crate::context::Context;
use crate::node::ExecutionKey;

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Toolchain,
    Harness,
    Build,
    Probe,
    Publication,
}

/// Exact execution partition bound to the caller's captured installation.
pub(crate) struct Scope {
    context: std::rc::Rc<Context>,
    key: ExecutionKey,
}

impl Scope {
    pub(crate) fn bind(
        context: &std::rc::Rc<Context>,
        operation: Operation,
        authority: [u8; 32],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.cargo.uncached-execution/v1");
        digest.update(context.identity());
        digest.update([operation as u8]);
        digest.update(authority);
        Self {
            context: std::rc::Rc::clone(context),
            key: ExecutionKey::from_bytes(digest.finalize().into()),
        }
    }

    /// Take the project's owner lease, then the exact partition. A scope
    /// inside a node or another scope of the same project finds the owner
    /// lease already held on its thread and takes only the partition. Scopes
    /// of two projects run at once, since the key carries the context.
    pub(crate) fn run<T>(&self, execute: impl FnOnce() -> crate::Result<T>) -> crate::Result<T> {
        let owned = || {
            let store = self.context.open_store()?;
            store.with_heavy_lease(|| store.with_execution_lease(self.key, || Ok(execute())))
        };
        owned().map_err(|source| crate::Error::Node {
            operation: "native execution ownership",
            source: Box::new(source),
        })?
    }
}
