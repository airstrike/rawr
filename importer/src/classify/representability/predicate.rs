//! Exact positive trait obligations retained for demanded generic instances.

use serde::{Deserialize, Serialize};

/// One `Parameter: Trait` obligation whose implementation must be proven for
/// every concrete demanded instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Bound {
    parameter: u32,
    trait_definition: crate::surface::identity::Definition,
}

impl Bound {
    pub(crate) const fn new(
        parameter: u32,
        trait_definition: crate::surface::identity::Definition,
    ) -> Self {
        Self {
            parameter,
            trait_definition,
        }
    }

    #[must_use]
    pub const fn parameter(self) -> u32 {
        self.parameter
    }

    #[must_use]
    pub const fn trait_definition(self) -> crate::surface::identity::Definition {
        self.trait_definition
    }
}
