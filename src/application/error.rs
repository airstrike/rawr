//! Structured application-root diagnostics.

use crate::{ImmutableString, Span};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Stable category for an application-root failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    /// Application compilation found no restricted root.
    MissingEntry,
    /// `main` has an unsupported declaration shape.
    InvalidMain,
    /// More than one restricted root was attempted.
    MultipleEntries,
    /// A restricted root was placed behind control flow.
    ConditionalEntry,
    /// A restricted root was nested in another expression or block.
    NestedEntry,
    /// A helper or associated method attempted to hide the root.
    HiddenEntry,
    /// A restricted root was not the complete tail expression.
    NonTailEntry,
    /// An intrinsic root call used an unsupported shape.
    MalformedEntry,
    /// A callback argument was an expression instead of a symbolic item.
    NonSymbolicCallback,
    /// A symbolic callback path did not resolve to a script item.
    UnknownCallback,
    /// A selected callback has the wrong semantic item kind.
    InvalidCallbackKind,
    /// A selected callback has the wrong receiver contract.
    InvalidReceiver,
    /// A selected callback has the wrong positional arity.
    InvalidArity,
    /// State or message types do not unify across callbacks.
    TypeMismatch,
    /// A callback output is outside the normalized application contract.
    InvalidOutput,
    /// The default-backed root's state cannot be produced by Default.
    MissingDefault,
    /// The application message lacks a required Rawr capability.
    MissingCapability,
    /// The installed intrinsic facade lacks required application type metadata.
    InvalidFacadeContract,
    /// Two retained dependencies claim one facade root with unequal proofs.
    FacadeDependencyConflict,
    /// A runtime application role reaches a structural tuple without value semantics.
    UnsupportedRuntimeType,
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEntry => formatter.write_str("missing application entry"),
            Self::InvalidMain => formatter.write_str("invalid application main"),
            Self::MultipleEntries => formatter.write_str("multiple application entries"),
            Self::ConditionalEntry => formatter.write_str("conditional application entry"),
            Self::NestedEntry => formatter.write_str("nested application entry"),
            Self::HiddenEntry => formatter.write_str("hidden application entry"),
            Self::NonTailEntry => formatter.write_str("non-tail application entry"),
            Self::MalformedEntry => formatter.write_str("malformed application entry"),
            Self::NonSymbolicCallback => formatter.write_str("non-symbolic application callback"),
            Self::UnknownCallback => formatter.write_str("unknown application callback"),
            Self::InvalidCallbackKind => formatter.write_str("invalid application callback kind"),
            Self::InvalidReceiver => formatter.write_str("invalid application callback receiver"),
            Self::InvalidArity => formatter.write_str("invalid application callback arity"),
            Self::TypeMismatch => formatter.write_str("application callback type mismatch"),
            Self::InvalidOutput => formatter.write_str("invalid application callback output"),
            Self::MissingDefault => formatter.write_str("application state lacks Default"),
            Self::MissingCapability => {
                formatter.write_str("application message lacks a required capability")
            }
            Self::InvalidFacadeContract => {
                formatter.write_str("invalid application facade contract")
            }
            Self::FacadeDependencyConflict => {
                formatter.write_str("conflicting application facade dependencies")
            }
            Self::UnsupportedRuntimeType => {
                formatter.write_str("unsupported application runtime type")
            }
        }
    }
}

/// A restricted application root could not be proven.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Error {
    /// Application compilation found no registered restricted root.
    MissingEntry,
    /// `main` has parameters, a receiver, or another unsupported declaration shape.
    InvalidMain {
        /// Stable explanation of the rejected declaration.
        reason: ImmutableString,
        /// Span of the invalid `main` declaration.
        declaration: Span,
    },
    /// More than one registered root intrinsic was attempted.
    MultipleEntries {
        /// Span of the first attempted root.
        first: Span,
        /// Span of the additional attempted root.
        additional: Span,
    },
    /// A registered root intrinsic was placed behind control flow.
    ConditionalEntry {
        /// Span of the attempted intrinsic root.
        entry: Span,
    },
    /// A registered root intrinsic was nested in another expression or block.
    NestedEntry {
        /// Span of the attempted intrinsic root.
        entry: Span,
    },
    /// A helper function or method attempted to hide the root.
    HiddenEntry {
        /// Name of the helper containing the root.
        function: ImmutableString,
        /// Span of the helper declaration.
        declaration: Span,
        /// Span of the attempted intrinsic root.
        entry: Span,
    },
    /// The root call was followed or preceded by another top-level expression.
    NonTailEntry {
        /// Span of the attempted intrinsic root.
        entry: Span,
    },
    /// A registered intrinsic root used an unsupported call shape.
    MalformedEntry {
        /// Stable explanation of the rejected shape.
        reason: ImmutableString,
        /// Span of the attempted intrinsic root.
        entry: Span,
    },
    /// A callback argument was not a symbolic script item path.
    NonSymbolicCallback {
        /// Callback role being resolved.
        role: ImmutableString,
        /// Span of the rejected expression.
        callback: Span,
    },
    /// A symbolic callback path did not resolve to a script item.
    UnknownCallback {
        /// Callback role being resolved.
        role: ImmutableString,
        /// Path written by the script.
        path: ImmutableString,
        /// Span of the unresolved path.
        callback: Span,
    },
    /// A callback's semantic item kind is not accepted for its role.
    InvalidCallbackKind {
        /// Callback role being validated.
        role: &'static str,
        /// Stable selected callback path.
        callback: ImmutableString,
        /// Human-readable expected item kinds.
        expected: &'static str,
        /// Actual semantic item kind.
        actual: crate::item::Kind,
        /// Callback declaration span.
        declaration: Span,
    },
    /// A callback's receiver mode is not accepted for its role.
    InvalidReceiver {
        /// Callback role being validated.
        role: &'static str,
        /// Stable selected callback path.
        callback: ImmutableString,
        /// Human-readable expected receiver.
        expected: &'static str,
        /// Actual receiver mode.
        actual: Option<crate::receiver::Mode>,
        /// Receiver or callback declaration span.
        declaration: Span,
    },
    /// A callback declares the wrong number of positional parameters.
    InvalidArity {
        /// Callback role being validated.
        role: &'static str,
        /// Stable selected callback path.
        callback: ImmutableString,
        /// Required positional parameter count, excluding the receiver.
        expected: usize,
        /// Actual positional parameter count.
        actual: usize,
        /// Callback declaration span.
        declaration: Span,
    },
    /// Callback state or message types do not unify.
    TypeMismatch {
        /// Callback role or type position being validated.
        role: &'static str,
        /// Expected stable structural type.
        expected: ImmutableString,
        /// Actual stable structural type.
        actual: ImmutableString,
        /// Rejected type annotation span.
        signature: Span,
    },
    /// A callback output is outside the normalized contract.
    InvalidOutput {
        /// Callback role being validated.
        role: &'static str,
        /// Human-readable accepted output shape.
        expected: ImmutableString,
        /// Actual stable structural output type.
        actual: ImmutableString,
        /// Rejected output annotation span.
        signature: Span,
    },
    /// `iced::run` selected a state without a Default proof.
    MissingDefault {
        /// Stable structural state type.
        state: ImmutableString,
        /// Update callback declaration span.
        declaration: Span,
    },
    /// The application message lacks one required Rawr capability.
    MissingCapability {
        /// Stable structural message type.
        message: ImmutableString,
        /// Missing capability name.
        capability: &'static str,
        /// Message parameter annotation span.
        declaration: Span,
    },
    /// Required Task or widget target metadata is absent from the root facade.
    InvalidFacadeContract {
        /// Missing or invalid canonical facade type.
        required: &'static str,
        /// Complete application root span.
        entry: Span,
    },
    /// One canonical facade root was retained with unequal exact fingerprints.
    FacadeDependencyConflict {
        /// Conflicting canonical facade root.
        root: ImmutableString,
    },
    /// A State, Message, or callback value reaches structural tuple metadata.
    UnsupportedRuntimeType {
        /// Application contract role being validated.
        role: &'static str,
        /// Stable structural type rejected for runtime use.
        ty: ImmutableString,
        /// Rejected type annotation span.
        signature: Span,
    },
}

impl Error {
    /// Classify this application-root failure without parsing its message.
    #[must_use]
    pub const fn category(&self) -> Category {
        match self {
            Self::MissingEntry => Category::MissingEntry,
            Self::InvalidMain { .. } => Category::InvalidMain,
            Self::MultipleEntries { .. } => Category::MultipleEntries,
            Self::ConditionalEntry { .. } => Category::ConditionalEntry,
            Self::NestedEntry { .. } => Category::NestedEntry,
            Self::HiddenEntry { .. } => Category::HiddenEntry,
            Self::NonTailEntry { .. } => Category::NonTailEntry,
            Self::MalformedEntry { .. } => Category::MalformedEntry,
            Self::NonSymbolicCallback { .. } => Category::NonSymbolicCallback,
            Self::UnknownCallback { .. } => Category::UnknownCallback,
            Self::InvalidCallbackKind { .. } => Category::InvalidCallbackKind,
            Self::InvalidReceiver { .. } => Category::InvalidReceiver,
            Self::InvalidArity { .. } => Category::InvalidArity,
            Self::TypeMismatch { .. } => Category::TypeMismatch,
            Self::InvalidOutput { .. } => Category::InvalidOutput,
            Self::MissingDefault { .. } => Category::MissingDefault,
            Self::MissingCapability { .. } => Category::MissingCapability,
            Self::InvalidFacadeContract { .. } => Category::InvalidFacadeContract,
            Self::FacadeDependencyConflict { .. } => Category::FacadeDependencyConflict,
            Self::UnsupportedRuntimeType { .. } => Category::UnsupportedRuntimeType,
        }
    }

    /// Return the primary source span for this diagnostic.
    #[must_use]
    pub const fn primary_span(&self) -> Span {
        match self {
            Self::MissingEntry | Self::FacadeDependencyConflict { .. } => Span::NONE,
            Self::ConditionalEntry { entry }
            | Self::NestedEntry { entry }
            | Self::HiddenEntry { entry, .. }
            | Self::NonTailEntry { entry }
            | Self::MalformedEntry { entry, .. } => *entry,
            Self::InvalidMain { declaration, .. } => *declaration,
            Self::MultipleEntries { additional, .. } => *additional,
            Self::NonSymbolicCallback { callback, .. } | Self::UnknownCallback { callback, .. } => {
                *callback
            }
            Self::InvalidCallbackKind { declaration, .. }
            | Self::InvalidReceiver { declaration, .. }
            | Self::InvalidArity { declaration, .. }
            | Self::MissingDefault { declaration, .. }
            | Self::MissingCapability { declaration, .. } => *declaration,
            Self::TypeMismatch { signature, .. } | Self::InvalidOutput { signature, .. } => {
                *signature
            }
            Self::InvalidFacadeContract { entry, .. } => *entry,
            Self::UnsupportedRuntimeType { signature, .. } => *signature,
        }
    }

    /// Return a related declaration or first-entry span when available.
    #[must_use]
    pub const fn secondary_span(&self) -> Option<Span> {
        match self {
            Self::HiddenEntry { declaration, .. } => Some(*declaration),
            Self::MultipleEntries { first, .. } => Some(*first),
            Self::MissingEntry
            | Self::InvalidMain { .. }
            | Self::ConditionalEntry { .. }
            | Self::NestedEntry { .. }
            | Self::NonTailEntry { .. }
            | Self::MalformedEntry { .. }
            | Self::NonSymbolicCallback { .. }
            | Self::UnknownCallback { .. } => None,
            Self::InvalidCallbackKind { .. }
            | Self::InvalidReceiver { .. }
            | Self::InvalidArity { .. }
            | Self::TypeMismatch { .. }
            | Self::InvalidOutput { .. }
            | Self::MissingDefault { .. }
            | Self::MissingCapability { .. }
            | Self::InvalidFacadeContract { .. }
            | Self::FacadeDependencyConflict { .. } => None,
            Self::UnsupportedRuntimeType { .. } => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEntry => formatter.write_str(
                "application compilation requires one public `main` tail entry using registered `iced` intrinsics",
            ),
            Self::InvalidMain {
                reason,
                declaration,
            } => write!(
                formatter,
                "invalid application `main` at {declaration}: {reason}"
            ),
            Self::MultipleEntries { first, additional } => write!(
                formatter,
                "multiple application entries: first at {first}, additional entry at {additional}"
            ),
            Self::ConditionalEntry { entry } => write!(
                formatter,
                "application entry at {entry} cannot be conditional"
            ),
            Self::NestedEntry { entry } => write!(
                formatter,
                "application entry at {entry} cannot be nested in another expression or block"
            ),
            Self::HiddenEntry {
                function, entry, ..
            } => write!(
                formatter,
                "application entry at {entry} cannot be hidden in helper `{function}`"
            ),
            Self::NonTailEntry { entry } => write!(
                formatter,
                "application entry at {entry} must be the complete tail expression of `pub fn main`"
            ),
            Self::MalformedEntry { reason, entry } => write!(
                formatter,
                "malformed application entry at {entry}: {reason}"
            ),
            Self::NonSymbolicCallback { role, callback } => write!(
                formatter,
                "application {role} callback at {callback} must be a symbolic script item"
            ),
            Self::UnknownCallback {
                role,
                path,
                callback,
            } => write!(
                formatter,
                "unknown application {role} callback `{path}` at {callback}"
            ),
            Self::InvalidCallbackKind {
                role,
                callback,
                expected,
                actual,
                declaration,
            } => write!(
                formatter,
                "application {role} callback `{callback}` at {declaration} must be {expected}, found {actual:?}"
            ),
            Self::InvalidReceiver {
                role,
                callback,
                expected,
                actual,
                declaration,
            } => write!(
                formatter,
                "application {role} callback `{callback}` at {declaration} requires {expected}, found {actual:?}"
            ),
            Self::InvalidArity {
                role,
                callback,
                expected,
                actual,
                declaration,
            } => write!(
                formatter,
                "application {role} callback `{callback}` at {declaration} expects {expected} parameter(s), found {actual}"
            ),
            Self::TypeMismatch {
                role,
                expected,
                actual,
                signature,
            } => write!(
                formatter,
                "application {role} type at {signature} must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidOutput {
                role,
                expected,
                actual,
                signature,
            } => write!(
                formatter,
                "application {role} output at {signature} must be {expected}, found `{actual}`"
            ),
            Self::MissingDefault { state, declaration } => write!(
                formatter,
                "application state `{state}` selected at {declaration} does not provide `Default`"
            ),
            Self::MissingCapability {
                message,
                capability,
                declaration,
            } => write!(
                formatter,
                "application message `{message}` at {declaration} does not provide `{capability}`"
            ),
            Self::InvalidFacadeContract { required, entry } => write!(
                formatter,
                "application entry at {entry} requires canonical facade type `{required}`"
            ),
            Self::FacadeDependencyConflict { root } => write!(
                formatter,
                "application dependency root `{root}` has conflicting exact facade fingerprints"
            ),
            Self::UnsupportedRuntimeType {
                role,
                ty,
                signature,
            } => write!(
                formatter,
                "application {role} type `{ty}` at {signature} reaches a structural tuple, which has no runtime value semantics"
            ),
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {}
