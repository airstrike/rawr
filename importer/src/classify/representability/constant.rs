//! Admitted compile-time constant plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    ty: super::convert::Type,
    value: super::convert::Constant,
}

impl Plan {
    pub(crate) const fn new(ty: super::convert::Type, value: super::convert::Constant) -> Self {
        Self { ty, value }
    }

    #[must_use]
    pub const fn ty(&self) -> &super::convert::Type {
        &self.ty
    }

    #[must_use]
    pub const fn value(&self) -> &super::convert::Constant {
        &self.value
    }
}
