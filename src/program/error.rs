//! Structured failures while authorizing or invoking symbolic items.

use crate::{item, program, EvalAltResult};
#[cfg(feature = "no_std")]
use core_error::Error as StdError;
#[cfg(not(feature = "no_std"))]
use std::error::Error as StdError;
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The stable metadata aspect that differs from a retained symbolic item.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Aspect {
    /// The semantic item kind changed.
    Kind,
    /// The stable nominal owner changed.
    Owner,
    /// The receiver mode changed.
    Receiver,
    /// The canonical callable signature changed.
    Signature,
    /// The callable capability set changed.
    Capabilities,
}

impl fmt::Display for Aspect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kind => formatter.write_str("item kind"),
            Self::Owner => formatter.write_str("owner"),
            Self::Receiver => formatter.write_str("receiver"),
            Self::Signature => formatter.write_str("signature"),
            Self::Capabilities => formatter.write_str("capabilities"),
        }
    }
}

/// A symbolic item could not be authorized or invoked in one generation.
#[derive(Debug)]
pub enum Error {
    /// The authorizing engine did not compile the executable.
    EngineOriginMismatch,
    /// The current executable has no matching stable item.
    MissingItem {
        /// Retained stable key that could not be resolved.
        key: item::Key,
    },
    /// Stable lookup found an item whose proof metadata has changed.
    Mismatch {
        /// Retained stable key being rebound.
        key: item::Key,
        /// Exact proof component that differs.
        aspect: Aspect,
    },
    /// The invocation supplied the wrong number of values.
    InvalidArity {
        /// Authorized item key.
        key: item::Key,
        /// Number of values required, including a receiver when present.
        expected: usize,
        /// Number of values supplied.
        actual: usize,
    },
    /// A protected application callback was invoked outside its typed boundary.
    ApplicationBoundary {
        /// Protected symbolic item key.
        key: item::Key,
        /// Boundary that must consume this callback.
        boundary: &'static str,
    },
    /// Recursive nominal construction validation failed.
    Construction {
        /// Authorized nominal constructor key.
        key: item::Key,
        /// Structured proof or runtime-model mismatch.
        error: program::construction::Error,
    },
    /// The paired evaluator rejected the authorized call.
    Evaluation {
        /// Authorized item key.
        key: item::Key,
        /// Error returned from the paired AST/support execution path.
        error: Box<EvalAltResult>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EngineOriginMismatch => formatter.write_str(
                "executable authority requires the exact Engine that compiled the program",
            ),
            Self::MissingItem { key } => {
                write!(
                    formatter,
                    "symbolic item `{key}` is not present in this program"
                )
            }
            Self::Mismatch { key, aspect } => write!(
                formatter,
                "symbolic item `{key}` has a different {aspect} in this program"
            ),
            Self::InvalidArity {
                key,
                expected,
                actual,
            } => write!(
                formatter,
                "symbolic item `{key}` expects {expected} value(s), found {actual}"
            ),
            Self::ApplicationBoundary { key, boundary } => write!(
                formatter,
                "symbolic item `{key}` can only be invoked through the {boundary} boundary"
            ),
            Self::Construction { key, error } => {
                write!(
                    formatter,
                    "construction by symbolic item `{key}` failed: {error}"
                )
            }
            Self::Evaluation { key, error } => {
                write!(
                    formatter,
                    "invocation of symbolic item `{key}` failed: {error}"
                )
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Construction { error, .. } => Some(error),
            Self::Evaluation { error, .. } => Some(error.as_ref()),
            Self::EngineOriginMismatch
            | Self::MissingItem { .. }
            | Self::Mismatch { .. }
            | Self::InvalidArity { .. }
            | Self::ApplicationBoundary { .. } => None,
        }
    }
}
