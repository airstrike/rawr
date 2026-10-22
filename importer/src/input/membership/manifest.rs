//! Pinned typed interpretation over borrowed, admitted manifest documents.
//!
//! Flat converted data and explicit frames preserve the distinct adapter modes.
//! Resource and classified source failure are terminal across semantic trials;
//! output carries no
//! package, filesystem, workspace, or current-source authority.

mod datetime;
pub(super) mod io;
mod load;
mod scalar;
mod schema;
mod session;
mod spec;
mod storage;
mod value;
mod walk;

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

use std::collections::TryReserveError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Span {
    start: usize,
    end: usize,
}

impl From<std::ops::Range<usize>> for Span {
    fn from(value: std::ops::Range<usize>) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tag {
    IntegerOverflow,
    FloatOverflow,
    Integer128,
    Integer64,
    Type,
    DateTime,
    Missing,
    Length,
    Variant,
    WorkspaceFalse,
    Scalar(scalar::Error),
    Spec(spec::Invalid),
    CargoFeatures,
    Duplicate,
    WorkspaceConflict,
    WorkspaceOptional,
    WorkspacePublic,
    DependencyWorkspaceMissing,
    DependencyInheritedMissing,
    DependencyDefaultFeatures,
    DependencyDefaultFeaturesUnderscore,
    DependencyPublicKind,
    DependencyTableUnderscore,
    LintsOverride,
    LintsInheritedMissing,
    DependencyPathRelative,
    DependencyPathUtf8,
    LintName,
    Feature(load::features::Invalid),
    Frontmatter(load::embedded::Invalid),
    ScriptRequired,
    ConfigValue(load::config::values::Unsupported),
    ConfigMerge,
    ConfigLookup,
    ConfigMissing,
    PatchBaseWithoutPath,
    PatchBaseUndefined,
    PackageMissingName,
    PackageInherited(schema::package::Field),
    PackagePathRelative(schema::package::Field),
    PackagePathUtf8(schema::package::Field),
    LibraryMissingName,
    LibraryEmptyName,
    LibraryHyphen,
    LibraryMissingPath,
    LibraryProcMacroUnderscore,
    LibraryCrateTypeUnderscore,
    TargetMissingName,
    TargetEmptyName,
    BinaryConflictingName,
    TargetProcMacroUnderscore,
    TargetCrateTypeUnderscore,
    ConfigDefinition,
    ConfigInclude,
    ConfigCycle,
    /// Pinned dotted-key parser rejects the leading '/' of a governed file arg.
    ConfigCliParse,
    State,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Diagnostic {
    tag: Tag,
    span: Option<Span>,
    expected: Option<&'static str>,
}

impl Diagnostic {
    fn at(tag: Tag, span: Option<Span>) -> Self {
        Self {
            tag,
            span,
            expected: None,
        }
    }
}

/// Only these verified pinned operations are classified.
/// This is not permission to translate arbitrary upstream panics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Source {
    SpecReparse,
    FrontmatterPartialAdvance { bytes: u8 },
    ConfigDefinitionSecondParent,
    JoinedDependencyPathUtf8,
    BuildScriptStem,
    CompileTargetStem,
}

#[derive(Debug, thiserror::Error)]
enum Error<E> {
    #[error("configuration file path contains an interior NUL")]
    ConfigPathNul,
    #[error("manifest recorded input failed: {0}")]
    Host(E),
    #[error("typed manifest admission refused: {0}")]
    Admission(E),
    #[error("typed manifest allocation failed: {0}")]
    Allocation(TryReserveError),
    #[error("typed Git configuration inline-vector allocation failed: {0:?}")]
    InlineAllocation(smallvec::CollectionAllocErr),
    #[error("typed manifest layout overflowed")]
    Overflow,
    #[error("typed manifest rejected: {0:?}")]
    Invalid(Diagnostic),
    #[error("pinned manifest source execution failed: {cause:?} at {span:?}")]
    Source { cause: Source, span: Option<Span> },
    #[error("typed manifest stopped without a retained failure")]
    State,
}

/// Semantic trial rejection is distinct from latched resource/source failure.
#[derive(Clone, Copy, Debug)]
enum Stop {
    Rejected(Diagnostic),
    Refused,
}

type Step<T> = std::result::Result<T, Stop>;
