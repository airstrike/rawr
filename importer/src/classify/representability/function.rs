//! Admitted monomorphic free or associated function plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    signature: super::signature::Signature,
}

impl Plan {
    pub(crate) const fn new(signature: super::signature::Signature) -> Self {
        Self { signature }
    }

    #[must_use]
    pub const fn signature(&self) -> &super::signature::Signature {
        &self.signature
    }
}
