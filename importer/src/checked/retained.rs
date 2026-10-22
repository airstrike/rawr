//! Historical checked bytes for the audited profile continuation.

use crate::node;

/// Exact owned checked output without current-parent authority.
pub struct Retained {
    pub(crate) product: node::staging::Owned<super::Product>,
    pub(crate) context: crate::context::Context,
    pub(crate) retention: node::read_set::Retention,
}

impl Retained {
    /// Read the authenticated profile fingerprint.
    pub fn checked_fingerprint(&self) -> [u8; 32] {
        self.product.value().checked
    }

    /// Borrow authenticated profile bytes; this does not establish currency.
    pub fn checked_payload(&self) -> &[u8] {
        &self.product.value().payload
    }

    /// Reconstruct symbolic demand under the supplied current contract.
    pub fn replay_demand<B>(
        &self,
        brand: B,
        contract: &crate::Contract<crate::Classified>,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::Demand<crate::Sealed, B>, super::Error> {
        self.product
            .value()
            .replay_demand(brand, contract, snapshot, limits)
    }
}

pub(crate) trait Output: Sized {
    fn complete(
        context: crate::context::Context,
        product: node::staging::Owned<super::Product>,
        retention: node::read_set::Retention,
        store: &crate::store::Store,
    ) -> Result<Self, super::Error>;
}

impl Output for super::Cached {
    fn complete(
        context: crate::context::Context,
        product: node::staging::Owned<super::Product>,
        retention: node::read_set::Retention,
        store: &crate::store::Store,
    ) -> Result<Self, super::Error> {
        let product = match product {
            node::staging::Owned::Current(value) => value,
            node::staging::Owned::Staged(value) => value
                .validate_current(store)
                .map_err(|error| super::Error::Node(Box::new(error)))?,
        };
        Self::from_validated(context, product, Some(retention))
    }
}

impl Output for Retained {
    fn complete(
        context: crate::context::Context,
        product: node::staging::Owned<super::Product>,
        retention: node::read_set::Retention,
        _: &crate::store::Store,
    ) -> Result<Self, super::Error> {
        // The batch (or all-current branch) already owns this exact output.
        // Check complete coverage rather than appending its root a second time.
        retention
            .covers_parents(std::slice::from_ref(&product))
            .map_err(|error| super::Error::Node(Box::new(error)))?;
        Ok(Self {
            product,
            context,
            retention,
        })
    }
}
