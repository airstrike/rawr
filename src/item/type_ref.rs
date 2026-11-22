//! Parsed type references in Rawr item headers.

use crate::{item, Span};

/// The builtin path name of a function type: `impl Fn(A, ..) -> B` and
/// `dyn Fn` parse to this one segment, whose type arguments are the inputs
/// and then the output.
pub(crate) const FN_TYPE: &str = "fn";

/// The builtin path name of a `fn(A, ..) -> B` pointer: a function type
/// ([`FN_TYPE`]) that no closure capturing a value is.
pub(crate) const FN_POINTER_TYPE: &str = "fn pointer";

/// The builtin path name of `impl FnMut` and its `dyn` and bound forms: a
/// function type ([`FN_TYPE`]) whose call takes the function `&mut`, which
/// no Rust path can spell.
pub(crate) const FN_MUT_TYPE: &str = "fn mut";

/// The builtin path name of `impl FnOnce` and its `dyn` and bound forms: a
/// function type ([`FN_TYPE`]) whose call takes the function by value,
/// which Rawr's call takes `&mut` as it does [`FN_MUT_TYPE`]'s.
pub(crate) const FN_ONCE_TYPE: &str = "fn once";

/// Whether `name` is a function type's builtin path name.
pub(crate) fn is_fn_type(name: &str) -> bool {
    matches!(name, FN_TYPE | FN_POINTER_TYPE | FN_MUT_TYPE | FN_ONCE_TYPE)
}

/// The builtin path name of the never type `!`, the type of an expression
/// that never finishes (`panic!()`, `return`, a call of a `-> !`
/// function): one segment without arguments.
pub(crate) const NEVER_TYPE: &str = "!";

/// The builtin path names of `impl Iterator<Item = T>` (also written
/// `impl DoubleEndedIterator` or `impl ExactSizeIterator`) and `impl
/// IntoIterator<Item = T>`, and of a generic parameter bounded by one with
/// its `Item` given: one segment whose one type argument is the item,
/// which no Rust path can spell.
pub(crate) const ITER_TYPE: &str = "impl Iterator";
pub(crate) const INTO_ITER_TYPE: &str = "impl IntoIterator";

/// The builtin path name of std's `Peekable<I>`, an iterator that has
/// `peek`, whose one type argument is the item as `ITER_TYPE`'s is.
pub(crate) const PEEKABLE_TYPE: &str = "Peekable<impl Iterator>";

/// The builtin path name of an array type `[T; N]` is this prefix, then
/// the length and `]` (`[_; 9]`), which no Rust path can spell; its one type
/// argument is the element.
pub(crate) const ARRAY_PREFIX: &str = "[_; ";

/// The builtin path name of a slice type `[T]`, which no Rust path can
/// spell; its one type argument is the element.
pub(crate) const SLICE_TYPE: &str = "[_]";

/// The builtin path name of `[T; length]`.
pub(crate) fn array_name(length: usize) -> String {
    format!("{ARRAY_PREFIX}{length}]")
}

/// `[T; N]` whose length names a constant, until the crate's items are
/// parsed and `N`'s value replaces it (`resolve_const_lengths`).
pub(crate) fn named_array_name(length: &str) -> String {
    format!("{ARRAY_PREFIX}{length}]")
}

/// The prefix of the parameter a function's `where` bound on an associated
/// type lifts (`I::Item: Display`). Rust spells no identifier with it.
const PROJECTION_PARAMETER_PREFIX: &str = "__rawr_projection__";

/// The name of the parameter that stands for `<base as trait_name>::name`
/// in a function whose `where` clause bounds it.
pub(crate) fn projection_parameter(base: &str, trait_name: &str, name: &str) -> String {
    format!("{PROJECTION_PARAMETER_PREFIX}{base}__{trait_name}__{name}")
}

/// The prefix of the parameter a function's `where` bound on a type other
/// than its parameters and their associated types lifts (`Vec<T>: Debug`).
/// Rust spells no identifier with it.
const FIXED_PARAMETER_PREFIX: &str = "__rawr_where__";

/// The name of the `index`th generic parameter of a function, lifted for a
/// `where` bound on a type other than its parameters.
pub(crate) fn fixed_parameter(index: usize) -> String {
    format!("{FIXED_PARAMETER_PREFIX}{index}")
}

/// The associated type a parameter of [`projection_parameter`] stands for,
/// as rustc writes it: `<I as Iterator>::Item`.
pub(crate) fn projection_parameter_name(parameter: &str) -> Option<String> {
    let mut parts = parameter
        .strip_prefix(PROJECTION_PARAMETER_PREFIX)?
        .splitn(3, "__");
    let (base, trait_name, name) = (parts.next()?, parts.next()?, parts.next()?);
    Some(format!("<{base} as {trait_name}>::{name}"))
}

/// The text of `impl Bound + Bound` as rustc prints its opaque type:
/// `impl Shape + Debug`, `impl Named<Out = i64>`, `impl Into<String>`.
pub(crate) fn opaque_text(path: &Path) -> String {
    let mut bounds = Vec::<(String, Vec<String>)>::new();
    let mut bindings = Vec::new();
    for argument in path
        .segments
        .iter()
        .flat_map(|segment| segment.arguments.iter())
    {
        let Argument::Type(Type::Path(bound)) = argument else {
            continue;
        };
        // `=Named=Out<i64>`: the binding `Out = i64` of `Named`.
        if let [binding] = &bound.segments[..]
            && let Some(rest) = binding.name.text.strip_prefix(BINDING_PREFIX)
            && let Some((owner, name)) = rest.split_once('=')
        {
            let value = binding
                .arguments
                .iter()
                .map(argument_text)
                .collect::<String>();
            bindings.push((owner.to_owned(), format!("{name} = {value}")));
            continue;
        }
        let Some(last) = bound.segments.last() else {
            continue;
        };
        let arguments = last.arguments.iter().map(argument_text).collect();
        bounds.push((last.name.text.to_string(), arguments));
    }
    for (owner, binding) in bindings {
        if let Some((_, arguments)) = bounds.iter_mut().find(|(name, _)| *name == owner) {
            arguments.push(binding);
        }
    }
    let bounds = bounds
        .into_iter()
        .map(|(name, arguments)| match &arguments[..] {
            [] => name,
            _ => format!("{name}<{}>", arguments.join(", ")),
        })
        .collect::<Vec<_>>();
    format!("{IMPL_TYPE} {}", bounds.join(" + "))
}

fn argument_text(argument: &Argument) -> String {
    match argument {
        Argument::Type(ty) => type_text(ty),
        Argument::Lifetime(lifetime) => lifetime.name.text.to_string(),
    }
}

fn type_text(ty: &Type) -> String {
    match ty {
        Type::Unit(_) => "()".into(),
        Type::Tuple { members, .. } => format!(
            "({})",
            members.iter().map(type_text).collect::<Vec<_>>().join(", ")
        ),
        Type::Reference {
            mutable, target, ..
        } => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            type_text(target)
        ),
        Type::Path(path) if matches!(&path.segments[..], [segment] if segment.name.text == IMPL_TYPE) => {
            opaque_text(path)
        }
        Type::Path(path) => path
            .segments
            .iter()
            .map(|segment| match &segment.arguments[..] {
                [] => segment.name.text.to_string(),
                arguments => format!(
                    "{}<{}>",
                    segment.name.text,
                    arguments
                        .iter()
                        .map(argument_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
            .collect::<Vec<_>>()
            .join("::"),
    }
}

/// The constant an array type's builtin path name names as its length.
pub(crate) fn array_length_name(name: &str) -> Option<&str> {
    let length = name.strip_prefix(ARRAY_PREFIX)?.strip_suffix(']')?;
    length
        .starts_with(|c: char| c.is_alphabetic() || c == '_')
        .then_some(length)
}

/// An array type's length: a number, or the name of a const parameter of
/// the item it is in (`[T; N]` of `const N: usize`), which each use of the
/// item gives a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Length<'a> {
    Fixed(usize),
    Param(&'a str),
}

impl Length<'_> {
    /// The number, when the length is one.
    pub(crate) fn fixed(self) -> Option<usize> {
        match self {
            Self::Fixed(length) => Some(length),
            Self::Param(_) => None,
        }
    }

    /// Whether an array of `length` elements is of this length: any is of
    /// a parameter's, whose value the item's use gave.
    pub(crate) fn admits(self, length: usize) -> bool {
        self.fixed().is_none_or(|fixed| fixed == length)
    }
}

impl std::fmt::Display for Length<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed(length) => write!(f, "{length}"),
            Self::Param(name) => write!(f, "{name}"),
        }
    }
}

/// The length of an array type's builtin path name. Once the crate's
/// items are parsed, a named length is a const parameter's
/// (`resolve_const_lengths`).
pub(crate) fn array_length(name: &str) -> Option<Length<'_>> {
    if let Some(name) = array_length_name(name) {
        return Some(Length::Param(name));
    }
    name.strip_prefix(ARRAY_PREFIX)?
        .strip_suffix(']')?
        .parse()
        .ok()
        .map(Length::Fixed)
}

/// The builtin path name of a const generic argument is this prefix, then
/// its value as a type writes it (`const 4` of `Buf<4>`), which no Rust path
/// can spell. Before the crate's items are all parsed, an argument naming a
/// `const` (`Buf<{ SIZE * 2 }>`) is its encoded expression
/// (`item::constant::encode`), which `resolve_const_lengths` evaluates.
pub(crate) const CONST_PREFIX: &str = "const ";

/// The builtin path name of the const generic argument `value`.
pub(crate) fn const_name(value: &str) -> String {
    format!("{CONST_PREFIX}{value}")
}

/// What a const generic argument's builtin path name holds: a value, or
/// the expression it is while its names are unknown.
pub(crate) fn const_text(name: &str) -> Option<&str> {
    name.strip_prefix(CONST_PREFIX)
}

/// The value of a const generic argument's builtin path name, once known.
pub(crate) fn const_value(name: &str) -> Option<crate::item::constant::Value> {
    crate::item::constant::Value::parse(const_text(name)?)
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
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
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
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
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
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
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
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub enum Argument {
    /// A nested type argument.
    Type(Type),
    /// An erased lifetime marker.
    Lifetime(Lifetime),
}

/// A Rust-shaped lifetime marker retained for interop headers.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub struct Lifetime {
    /// Lifetime name or underscore placeholder.
    pub name: item::Name,
    /// Complete lifetime span, including the apostrophe.
    pub span: Span,
}
