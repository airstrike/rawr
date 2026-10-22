//! Compiler proof for one implicit nominal-widget promotion.

use crate::{application, widget};

/// One exact compiler-selected nominal promotion into a widget boundary.
#[derive(Clone, Debug)]
pub struct Promotion {
    definition: widget::Definition,
    message: application::Type,
    site: widget::Site,
    location: widget::Location,
}

impl Promotion {
    pub(crate) const fn new(
        definition: widget::Definition,
        message: application::Type,
        site: widget::Site,
        location: widget::Location,
    ) -> Self {
        Self {
            definition,
            message,
            site,
            location,
        }
    }

    /// Return the nominal widget protocol selected at this site.
    #[must_use]
    pub const fn definition(&self) -> &widget::Definition {
        &self.definition
    }

    /// Return the exact application Message instantiated at this boundary.
    #[must_use]
    pub const fn message(&self) -> &application::Type {
        &self.message
    }

    /// Return the stable structural expression identity.
    #[must_use]
    pub const fn site(&self) -> &widget::Site {
        &self.site
    }

    /// Return exact generation-local diagnostic evidence for this expression.
    #[must_use]
    pub const fn location(&self) -> widget::Location {
        self.location
    }

    pub(crate) fn compatible(&self, retained: &Self) -> bool {
        self.site == retained.site
            && self.message == retained.message
            && self.definition.owner() == retained.definition.owner()
            && self.definition.schema() == retained.definition.schema()
            && self.definition.dependency() == retained.definition.dependency()
            && self.definition.fingerprint() == retained.definition.fingerprint()
    }
}
