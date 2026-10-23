//! Resolved, deliberately non-executable item shells.

use super::type_ref;
use crate::{item, type_def, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Shell {
    pub id: item::Id,
    pub source: crate::program::source::Id,
    pub key: Option<item::Key>,
    pub span: Span,
    pub kind: Kind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Use(Use),
    Struct(Struct),
    Enum(Enum),
    Impl(Impl),
    Trait(Trait),
    Function(Function),
    LegacyFunction(LegacyFunction),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyFunction {
    pub name: crate::item::Name,
    pub parameter_count: usize,
    pub body: Body,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Use {
    pub header: crate::item::Header,
    pub bindings: Box<[Binding]>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub name: crate::item::Name,
    pub qualified: crate::ImmutableString,
    pub target: Import,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "no_std", allow(dead_code))]
pub(crate) enum Import {
    Module,
    ModuleItem {
        definitions: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
    },
    ScriptType {
        definition: type_def::Id,
        identity: type_def::Identity,
    },
    ScriptItem {
        key: item::Key,
    },
    Type {
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
    },
    Item {
        definitions: Box<[crate::host::item::Definition]>,
        owner: Option<crate::host::facade::Dependency>,
    },
    Endpoint {
        contract: crate::host::endpoint::Contract,
        owner: crate::host::facade::Dependency,
    },
    /// A type alias, which the importing module's types name.
    Alias,
}

/// A generic type parameter and its bounds: built-in derivable traits as
/// capabilities, user traits by name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Generic {
    pub name: crate::item::Name,
    pub bounds: crate::capability::Set,
    pub traits: Box<[crate::item::Name]>,
    /// Associated type equalities of the bounds: `Output = i64` of
    /// `T: Shape<Output = i64>`.
    /// Each is `(trait, name, type)`: `(Shape, Output, i64)`.
    pub projections: Box<[(crate::item::Name, crate::item::Name, type_ref::Type)]>,
    /// The type arguments of each generic trait among `traits`: `i64` of
    /// `T: Convert<i64>`, as `(trait, arguments)`.
    pub trait_arguments: Box<[(crate::ImmutableString, Box<[type_ref::Type]>)]>,
    /// A trait parameter's default type: `Self` of `trait Add<Rhs = Self>`.
    pub default: Option<type_ref::Type>,
}

/// A trait declaration. `Self` in its signatures is the generic parameter
/// `Self`; provided functions keep their default bodies, which each impl
/// that does not override them receives as its own method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Trait {
    pub header: crate::item::Header,
    pub name: crate::item::Name,
    pub functions: Box<[Function]>,
    pub provided: Box<[bool]>,
    /// Associated types each impl names: `type Output;`.
    pub associated: Box<[crate::item::Name]>,
    /// The bounds of each of `associated` (`type Output: Clone + Shape;`):
    /// its capabilities and user traits.
    pub associated_bounds: Box<[(crate::capability::Set, Box<[crate::item::Name]>)]>,
    /// The type arguments of each generic trait among each of
    /// `associated_bounds`, in this trait's parameters and `Self`:
    /// `String` of `type Item: Into<String>;`.
    pub associated_bound_arguments: Box<[Box<[(crate::ImmutableString, Box<[type_ref::Type]>)]>]>,
    /// Supertraits (`trait Named: Shape + Clone`): their capabilities and
    /// user traits. Every impl of this trait needs them, and a bound by it
    /// implies them.
    pub supertraits: (crate::capability::Set, Box<[crate::item::Name]>),
    /// The type arguments of each generic trait among `supertraits`, in
    /// this trait's parameters and `Self`: `T` of `trait Both<T>:
    /// Convert<T>`, `Self` of `trait Ord2: PartialOrd2<Self>`; made
    /// transitive and substituted by elaboration.
    pub supertrait_arguments: Box<[(crate::ImmutableString, Box<[type_ref::Type]>)]>,
    /// The trait's own type parameters: `T` of `trait Convert<T>`.
    pub generics: Box<[Generic]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Struct {
    pub header: crate::item::Header,
    pub definition: type_def::Id,
    pub key: type_def::Key,
    pub name: crate::item::Name,
    pub generics: Box<[Generic]>,
    pub fields: Box<[Field]>,
    pub form: crate::item::StructForm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Field {
    pub name: crate::item::Name,
    pub ty: type_ref::Type,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Enum {
    pub header: crate::item::Header,
    pub definition: type_def::Id,
    pub key: type_def::Key,
    pub name: crate::item::Name,
    pub generics: Box<[Generic]>,
    pub variants: Box<[Variant]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Variant {
    pub name: crate::item::Name,
    pub payload: Box<[type_ref::Type]>,
    /// A struct variant's field names, one per payload type.
    pub fields: Option<Box<[crate::item::Name]>>,
    /// `#[default]`: the variant a derived `Default` builds.
    pub default: Option<Span>,
    /// `V = 5`: the explicit discriminant an `as` cast reads.
    pub discriminant: Option<crate::INT>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Impl {
    pub header: crate::item::Header,
    pub generics: Box<[Generic]>,
    pub trait_: Option<TraitImpl>,
    pub owner: type_ref::Type,
    pub functions: Box<[Function]>,
    /// Associated types: `type Output = i64;`.
    pub associated: Box<[(crate::item::Name, type_ref::Type)]>,
}

/// The trait of `impl Trait for Type`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TraitImpl {
    /// `impl Trait for Owner` of a trait declared in the crate or the
    /// prelude (`From<T>`), with the trait's type arguments: `i64`
    /// of `impl Convert<i64> for Owner`.
    User {
        name: crate::item::Name,
        arguments: Box<[type_ref::Type]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Function {
    pub header: crate::item::Header,
    pub name: crate::item::Name,
    /// The function's own generic parameters; an impl's are on the impl.
    pub generics: Box<[Generic]>,
    pub receiver: Option<crate::item::Receiver>,
    pub parameters: Box<[Parameter]>,
    pub output: Option<type_ref::Type>,
    pub body: Body,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Parameter {
    pub name: crate::item::Name,
    pub ty: type_ref::Type,
    pub span: Span,
    pub pattern: Option<crate::item::ParameterPattern>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Body {
    pub span: Span,
    pub lowering: Lowering,
    pub syntax: crate::item::BodySyntax,
    pub constant: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lowering {
    Unavailable,
    NominalFunction,
    NominalMethod(crate::method::Id),
}
