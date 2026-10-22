//! Owned direct-child output for one scripted widget.

use rawr::host;

use crate::widget;

/// One sealed list of direct child widget descriptors.
///
/// The facade's logical `Message` parameter is compiler-owned. It is not a
/// Rust generic retained by this carrier.
#[derive(Clone)]
pub struct List {
    witness: host::runtime::Witness,
    values: Vec<widget::Value>,
}

impl List {
    pub(crate) const fn new(witness: host::runtime::Witness, values: Vec<widget::Value>) -> Self {
        Self { witness, values }
    }

    pub(crate) const fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }

    pub(crate) fn values(&self) -> &[widget::Value] {
        &self.values
    }
}
