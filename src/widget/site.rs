//! Stable compiler-owned identities for scripted-widget expression sites.

use crate::item;

/// One stable scripted-widget expression site within a callable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Site {
    callable: item::Key,
    path: Box<[u32]>,
}

impl Site {
    pub(crate) fn new(callable: item::Key, path: Box<[u32]>) -> Self {
        Self { callable, path }
    }

    /// Return the stable callable containing this expression site.
    #[must_use]
    pub const fn callable(&self) -> &item::Key {
        &self.callable
    }

    /// Return the compiler-owned structural child-index path.
    #[must_use]
    pub const fn path(&self) -> &[u32] {
        &self.path
    }
}
