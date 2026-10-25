//! Parsed type references in Rawr item headers.

use crate::{item, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The builtin path name of a function type: `fn(A, ..) -> B`, `impl Fn`
/// and `dyn Fn` parse to this one segment, whose type arguments are the
/// inputs and then the output.
pub(crate) const FN_TYPE: &str = "fn";

/// The builtin path names of `impl Iterator<Item = T>` (also written
/// `impl DoubleEndedIterator` or `impl ExactSizeIterator`) and `impl
/// IntoIterator<Item = T>`, and of a generic parameter bounded by one with
/// its `Item` given: one segment whose one type argument is the item,
/// which no Rust path can spell.
pub(crate) const ITER_TYPE: &str = "impl Iterator";
pub(crate) const INTO_ITER_TYPE: &str = "impl IntoIterator";

/// The builtin path name of an array type `[T; N]` is this prefix, then
/// the length and `]` (`[_; 9]`), which no Rust path can spell; its one type
/// argument is the element. A slice `[T]` is `Vec<T>`.
pub(crate) const ARRAY_PREFIX: &str = "[_; ";

/// The builtin path name of `[T; length]`.
pub(crate) fn array_name(length: usize) -> String {
    format!("{ARRAY_PREFIX}{length}]")
}

/// `[T; N]` whose length names a constant, until the crate's items are
/// parsed and `N`'s value replaces it (`resolve_const_lengths`).
pub(crate) fn named_array_name(length: &str) -> String {
    format!("{ARRAY_PREFIX}{length}]")
}

/// The constant an array type's builtin path name names as its length.
pub(crate) fn array_length_name(name: &str) -> Option<&str> {
    let length = name.strip_prefix(ARRAY_PREFIX)?.strip_suffix(']')?;
    length
        .starts_with(|c: char| c.is_alphabetic() || c == '_')
        .then_some(length)
}

/// The length of an array type's builtin path name.
pub(crate) fn array_length(name: &str) -> Option<usize> {
    name.strip_prefix(ARRAY_PREFIX)?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// The first segment of a trait object type: `dyn Trait` parses to the
/// path `dyn::Trait`, which no Rust path can spell.
pub(crate) const DYN_SEGMENT: &str = "dyn";

/// The segment between a qualified path's type and its associated type:
/// `<T as Trait>::Name` parses to `T::as<Trait>::Name`, which no Rust path
/// can spell; the segment's one type argument is the trait.
pub(crate) const QUALIFIED_SEGMENT: &str = "as";
/// `impl Bound + Bound` in a header, before a function's parameter lifts
/// it to an anonymous generic parameter: its bounds are the arguments.
pub(crate) const IMPL_TYPE: &str = "impl";

/// The prefix of an associated type binding of a trait object's trait:
/// `dyn Shape<Output = i64>` keeps `Output = i64` as the generic argument
/// `=Output<i64>`, which no Rust path can spell.
pub(crate) const BINDING_PREFIX: &str = "=";

/// A parsed type reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Type {
    /// The unit type `()`.
    Unit(Span),
    /// An ordered structural tuple with at least two members.
    Tuple {
        /// Tuple members in declaration order.
        members: Box<[Type]>,
        /// Complete tuple span, including parentheses.
        span: Span,
    },
    /// A qualified nominal or host type path.
    Path(Path),
    /// A shared or mutable reference type.
    Reference {
        /// Whether the reference is mutable.
        mutable: bool,
        /// Explicit lifetime marker, when written.
        lifetime: Option<Lifetime>,
        /// Referenced type.
        target: Box<Type>,
        /// Complete reference span.
        span: Span,
    },
}

impl Type {
    /// Complete source span occupied by the type.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Unit(span) | Self::Tuple { span, .. } | Self::Reference { span, .. } => *span,
            Self::Path(path) => path.span,
        }
    }
}

/// A qualified type path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path {
    /// Qualified path segments.
    pub segments: Box<[Segment]>,
    /// Complete path span.
    pub span: Span,
    /// Whether the path starts with `::`, which names an outside crate.
    pub global: bool,
}

/// One type path segment and its generic arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Segment {
    /// Segment name.
    pub name: item::Name,
    /// Generic arguments applied to this segment.
    pub arguments: Box<[Argument]>,
    /// Complete segment span.
    pub span: Span,
}

/// A type or lifetime generic argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Argument {
    /// A nested type argument.
    Type(Type),
    /// An erased lifetime marker.
    Lifetime(Lifetime),
}

/// A Rust-shaped lifetime marker retained for interop headers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lifetime {
    /// Lifetime name or underscore placeholder.
    pub name: item::Name,
    /// Complete lifetime span, including the apostrophe.
    pub span: Span,
}
