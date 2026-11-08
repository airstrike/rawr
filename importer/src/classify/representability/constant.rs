//! Admitted compile-time constant plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    ty: super::convert::Type,
    /// A literal const's value; a `static` has none, and is read where the
    /// bridge runs.
    value: Option<super::convert::Constant>,
    /// A `static` of a crate value, as a page's `pub static META: Meta`:
    /// read where it is, as a place that lives as long as the program
    /// (#1442), never copied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    lasting: bool,
}

impl Plan {
    pub(crate) const fn new(
        ty: super::convert::Type,
        value: Option<super::convert::Constant>,
    ) -> Self {
        Self {
            ty,
            value,
            lasting: false,
        }
    }

    /// A `static` of the crate value `ty`, read where it is (#1442).
    pub(crate) const fn lasting(ty: super::convert::Type) -> Self {
        Self {
            ty,
            value: None,
            lasting: true,
        }
    }

    #[must_use]
    pub const fn ty(&self) -> &super::convert::Type {
        &self.ty
    }

    #[must_use]
    pub const fn value(&self) -> Option<&super::convert::Constant> {
        self.value.as_ref()
    }

    /// Whether this is a `static` of a crate value, read where it is.
    #[must_use]
    pub const fn is_lasting(&self) -> bool {
        self.lasting
    }
}
