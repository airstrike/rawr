//! Admitted monomorphic method plan.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    receiver: super::ownership::Receiver,
    signature: super::signature::Signature,
}

impl Plan {
    pub(crate) const fn new(
        receiver: super::ownership::Receiver,
        signature: super::signature::Signature,
    ) -> Self {
        Self {
            receiver,
            signature,
        }
    }

    #[must_use]
    pub const fn receiver(&self) -> super::ownership::Receiver {
        self.receiver
    }

    #[must_use]
    pub const fn signature(&self) -> &super::signature::Signature {
        &self.signature
    }
}
