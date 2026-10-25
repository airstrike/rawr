//! Structured failures while proving nominal construction values.
//!
//! The consumed construction proof is deliberately not public:
//!
//! ```compile_fail
//! use rawr::program::model::Construction;
//! ```

use super::GenerationId;
use crate::{type_def, ImmutableString};
use std::error::Error as StdError;
use std::fmt;

/// The nominal construction kind expected by the runtime model.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    /// A struct with ordered named fields.
    Struct,
    /// An enum with one stable variant and ordered payload values.
    Enum,
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Struct => formatter.write_str("struct"),
            Self::Enum => formatter.write_str("enum"),
        }
    }
}

/// Recursive runtime-model validation failed before nominal construction.
#[derive(Debug)]
pub enum Error {
    /// A proof was presented to a different executable generation.
    CrossGeneration {
        /// Generation that issued the proof.
        issued: GenerationId,
        /// Generation asked to consume the proof.
        current: GenerationId,
    },
    /// The executable has no definition for the requested stable identity.
    UnknownTarget {
        /// Stable identity requested by the caller.
        target: type_def::Identity,
    },
    /// The current executable model cannot materialize the requested default.
    MissingDefault {
        /// Stable target lacking a runtime default implementation.
        target: type_def::Identity,
    },
    /// Default construction reached the same nominal type recursively.
    RecursiveDefault {
        /// Stable target that closed the default-construction cycle.
        target: type_def::Identity,
    },
    /// A proof was substituted for a different stable target.
    TargetMismatch {
        /// Stable target required by the construction site.
        expected: type_def::Identity,
        /// Stable target carried by the proof or value.
        actual: type_def::Identity,
    },
    /// The target exists but has the wrong nominal kind.
    KindMismatch {
        /// Stable target whose kind differs.
        target: type_def::Identity,
        /// Kind required by the construction site.
        expected: Kind,
        /// Kind declared by the runtime model or carried proof.
        actual: Kind,
    },
    /// The stable identity matched but its exact schema did not.
    SchemaMismatch {
        /// Stable target whose schema is stale or substituted.
        target: type_def::Identity,
    },
    /// The runtime model has no such stable enum variant.
    UnknownVariant {
        /// Stable enum target.
        target: type_def::Identity,
        /// Stable variant requested by the caller or carried value.
        variant: ImmutableString,
    },
    /// A proof was substituted for another variant of the same enum.
    VariantMismatch {
        /// Stable enum target.
        target: type_def::Identity,
        /// Variant required by the construction site.
        expected: ImmutableString,
        /// Variant carried by the proof.
        actual: ImmutableString,
    },
    /// The ordered value count did not match the declared shape.
    InvalidArity {
        /// Stable nominal target.
        target: type_def::Identity,
        /// Stable enum variant, or `None` for a struct.
        variant: Option<ImmutableString>,
        /// Declared number of ordered values.
        expected: usize,
        /// Number supplied by the caller or carried value.
        actual: usize,
    },
    /// One recursively validated member failed its declared model.
    Member {
        /// Stable nominal target containing the member.
        target: type_def::Identity,
        /// Ordered field or payload index.
        index: usize,
        /// Structured nested failure.
        source: Box<Self>,
    },
    /// A value did not have the declared runtime type.
    ValueType {
        /// Human-readable declared type.
        expected: ImmutableString,
        /// Runtime type reported by `Dynamic`.
        actual: ImmutableString,
    },
    /// Recursive validation encountered a shared-value cycle.
    Cycle {
        /// Stable nominal target active when the cycle closed.
        target: type_def::Identity,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CrossGeneration { .. } => {
                formatter.write_str("nominal construction proof belongs to another generation")
            }
            Self::UnknownTarget { target } => {
                write!(
                    formatter,
                    "nominal target {} is not present in this program",
                    Name(target)
                )
            }
            Self::MissingDefault { target } => write!(
                formatter,
                "nominal target {} has no executable Default implementation",
                Name(target)
            ),
            Self::RecursiveDefault { target } => write!(
                formatter,
                "nominal target {} has a recursive Default implementation",
                Name(target)
            ),
            Self::TargetMismatch { expected, actual } => write!(
                formatter,
                "nominal construction target {} was substituted with {}",
                Name(expected),
                Name(actual)
            ),
            Self::KindMismatch {
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "nominal target {} is a {actual}, expected a {expected}",
                Name(target)
            ),
            Self::SchemaMismatch { target } => {
                write!(
                    formatter,
                    "nominal target {} has a stale schema",
                    Name(target)
                )
            }
            Self::UnknownVariant { target, variant } => write!(
                formatter,
                "nominal enum {} has no variant `{variant}`",
                Name(target)
            ),
            Self::VariantMismatch {
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "nominal enum {} variant `{expected}` was substituted with `{actual}`",
                Name(target)
            ),
            Self::InvalidArity {
                target,
                variant,
                expected,
                actual,
            } => {
                write!(formatter, "nominal {}", Name(target))?;
                if let Some(variant) = variant {
                    write!(formatter, "::{variant}")?;
                }
                write!(formatter, " expects {expected} value(s), found {actual}")
            }
            Self::Member {
                target,
                index,
                source,
            } => write!(
                formatter,
                "nominal {} member {index} failed validation: {source}",
                Name(target)
            ),
            Self::ValueType { expected, actual } => {
                write!(formatter, "expected `{expected}`, found `{actual}`")
            }
            Self::Cycle { target } => write!(
                formatter,
                "nominal {} contains a recursive shared-value cycle",
                Name(target)
            ),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Member { source, .. } => Some(source.as_ref()),
            Self::CrossGeneration { .. }
            | Self::UnknownTarget { .. }
            | Self::MissingDefault { .. }
            | Self::RecursiveDefault { .. }
            | Self::TargetMismatch { .. }
            | Self::KindMismatch { .. }
            | Self::SchemaMismatch { .. }
            | Self::UnknownVariant { .. }
            | Self::VariantMismatch { .. }
            | Self::InvalidArity { .. }
            | Self::ValueType { .. }
            | Self::Cycle { .. } => None,
        }
    }
}

struct Name<'a>(&'a type_def::Identity);

impl fmt::Display for Name<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            type_def::Identity::Host(identity) => write!(formatter, "host::{identity:?}"),
            type_def::Identity::Script(key) => key.fmt(formatter),
        }
    }
}
