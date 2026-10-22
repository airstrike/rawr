//! Durable symbolic scripted-widget data awaiting authority binding.

use crate::{widget, Dynamic};

/// One durable scripted widget with no executable program authority.
#[derive(Clone, Debug)]
pub struct Descriptor {
    promotion: widget::Promotion,
    data: Dynamic,
}

impl Descriptor {
    pub(crate) const fn new(promotion: widget::Promotion, data: Dynamic) -> Self {
        Self { promotion, data }
    }

    /// Return the compiler-selected promotion proof.
    #[must_use]
    pub const fn promotion(&self) -> &widget::Promotion {
        &self.promotion
    }

    /// Return the stable structural expression identity.
    #[must_use]
    pub const fn site(&self) -> &widget::Site {
        self.promotion.site()
    }

    pub(crate) const fn data(&self) -> &Dynamic {
        &self.data
    }
}
