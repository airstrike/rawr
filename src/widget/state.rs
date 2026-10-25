//! Iced tree-local state retained for a scripted widget.

use crate::application;

/// The exact local-state contract established by the optional `state` hook.
// Boxing `Nominal` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Type {
    /// The protocol is stateless and uses iced's unit state.
    Unit,
    /// The protocol retains one exact nominal Rawr value in iced's tree.
    Nominal(application::Type),
}

impl Type {
    pub(crate) fn nominal(ty: application::Type) -> Self {
        Self::Nominal(ty)
    }

    /// Return the nominal state type, when the protocol is stateful.
    #[must_use]
    pub const fn nominal_type(&self) -> Option<&application::Type> {
        match self {
            Self::Unit => None,
            Self::Nominal(ty) => Some(ty),
        }
    }
}
