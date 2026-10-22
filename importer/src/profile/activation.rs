//! Profile route identity used by staged runtime transitions.

/// Exact supervisor route bound to one generation and activation receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route {
    key: crate::supervisor::GenerationKey,
    receipt: crate::supervisor::activation::Receipt,
}

impl Route {
    pub(crate) const fn new(
        key: crate::supervisor::GenerationKey,
        receipt: crate::supervisor::activation::Receipt,
    ) -> Self {
        Self { key, receipt }
    }

    /// Return the exact complete generation identity.
    #[must_use]
    pub const fn key(&self) -> crate::supervisor::GenerationKey {
        self.key
    }

    /// Return the consumed activation receipt.
    #[must_use]
    pub const fn receipt(&self) -> crate::supervisor::activation::Receipt {
        self.receipt
    }

    /// Bind deterministic integration-fixture bytes to one exact route.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub const fn fixture(key: crate::supervisor::GenerationKey, receipt: [u8; 32]) -> Self {
        Self {
            key,
            receipt: crate::supervisor::activation::Receipt::fixture(receipt),
        }
    }

    pub(crate) fn advance(self, key: crate::supervisor::GenerationKey) -> Self {
        Self {
            key,
            receipt: self.receipt.advance(key),
        }
    }
}
