//! Admitted symbolic type-alias value plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    ty: super::convert::Type,
    parameters: Box<[super::signature::Parameter]>,
}

impl Plan {
    pub(crate) const fn new(
        ty: super::convert::Type,
        parameters: Box<[super::signature::Parameter]>,
    ) -> Self {
        Self { ty, parameters }
    }

    #[must_use]
    pub const fn ty(&self) -> &super::convert::Type {
        &self.ty
    }

    #[must_use]
    pub const fn parameters(&self) -> &[super::signature::Parameter] {
        &self.parameters
    }
}
