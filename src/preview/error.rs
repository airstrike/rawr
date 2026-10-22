//! Structured preview catalog and execution failures.

use crate::{application, host, program};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A complete executable could not promote into a preview catalog.
#[derive(Debug)]
pub enum Error {
    /// The executable declares no compiler-validated previews.
    Missing,
    /// The optional real application failed its normal contract validation.
    Application(application::Error),
    /// The final application does not realize the sealed staging semantics.
    ApplicationSemanticMismatch,
    /// A compiler-proven preview role had no exact runtime representation.
    RuntimeType {
        /// Human-readable preview role.
        role: &'static str,
    },
}

/// A compiler-proven preview operation failed under its bound catalog.
#[derive(Debug)]
pub enum RuntimeError {
    /// The requested preview key is not part of this catalog.
    UnknownPreview,
    /// Default fixture construction failed against the current executable model.
    SeedConstruction(program::construction::Error),
    /// A symbolic preview callback failed under current authority.
    Invocation(program::Error),
    /// Exact preview State or Message validation failed.
    Value(host::runtime::ValueError),
    /// The preview source location belonged to another executable source table.
    ForeignSource,
    /// A supposedly retained source key had no corresponding source snapshot.
    MissingSource,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("program declares no `#[preview]` entries"),
            Self::Application(error) => error.fmt(formatter),
            Self::ApplicationSemanticMismatch => formatter.write_str(
                "final application contract does not match its sealed staging semantics",
            ),
            Self::RuntimeType { role } => {
                write!(
                    formatter,
                    "preview {role} has no exact runtime representation"
                )
            }
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPreview => formatter.write_str("preview does not belong to this catalog"),
            Self::SeedConstruction(error) => error.fmt(formatter),
            Self::Invocation(error) => error.fmt(formatter),
            Self::Value(error) => error.fmt(formatter),
            Self::ForeignSource => {
                formatter.write_str("preview source location belongs to another catalog")
            }
            Self::MissingSource => formatter.write_str("preview source location is unavailable"),
        }
    }
}

impl From<application::Error> for Error {
    fn from(error: application::Error) -> Self {
        Self::Application(error)
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Missing | Self::ApplicationSemanticMismatch | Self::RuntimeType { .. } => None,
            Self::Application(error) => Some(error),
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::Missing | Self::ApplicationSemanticMismatch | Self::RuntimeType { .. } => None,
            Self::Application(error) => Some(error),
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnknownPreview | Self::ForeignSource | Self::MissingSource => None,
            Self::SeedConstruction(error) => Some(error),
            Self::Invocation(error) => Some(error),
            Self::Value(error) => Some(error),
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::UnknownPreview | Self::ForeignSource | Self::MissingSource => None,
            Self::SeedConstruction(error) => Some(error),
            Self::Invocation(error) => Some(error),
            Self::Value(error) => Some(error),
        }
    }
}
