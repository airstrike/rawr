//! Admitted compile-time constant plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    ty: super::convert::Type,
    /// A literal const's value; a `static` has none, and is read where the
    /// bridge runs.
    value: Option<super::convert::Constant>,
}

impl Plan {
    pub(crate) const fn new(
        ty: super::convert::Type,
        value: Option<super::convert::Constant>,
    ) -> Self {
        Self { ty, value }
    }

    #[must_use]
    pub const fn ty(&self) -> &super::convert::Type {
        &self.ty
    }

    #[must_use]
    pub const fn value(&self) -> Option<&super::convert::Constant> {
        self.value.as_ref()
    }
}
