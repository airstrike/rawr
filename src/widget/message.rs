//! Scripted-widget application message inference.

use crate::application;

/// The single message contract inferred for a widget protocol.
// Boxing `Exact` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Type {
    /// A purely visual protocol remains generic until fresh descriptor lowering.
    Generic,
    /// Message-bearing hooks agree on one exact structural type.
    Exact(application::Type),
}

impl Type {
    /// Return the exact constrained type, when one was inferred.
    #[must_use]
    pub const fn exact(&self) -> Option<&application::Type> {
        match self {
            Self::Generic => None,
            Self::Exact(ty) => Some(ty),
        }
    }
}
