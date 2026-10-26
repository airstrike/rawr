//! Resolved, deliberately non-executable item shells.

use super::type_ref;
use crate::{item, type_def, Span};

/// A generic trait's name and its type arguments: `(Convert, [i64])` of
/// `Convert<i64>`.
pub type TraitArgument = (crate::ImmutableString, Box<[type_ref::Type]>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Shell {
    pub id: item::Id,
    pub source: crate::program::source::Id,
    pub key: Option<item::Key>,
    pub span: Span,
    pub kind: Kind,
    /// The declarations of the facade traits the item names through its
    /// scope (`use iced::widget::Component;`, `component::Component`).
    pub facades: Box<[crate::ImmutableString]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Use(Use),
    Struct(Struct),
    Enum(Enum),
    Impl(Impl),
    Trait(Trait),
    Function(Function),
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
    /// A variant of a crate enum, as `use Shape::Circle;` or one name of
    /// `use Shape::*;` binds it: the name is the variant in patterns and
    /// expressions.
    Variant {
        definition: type_def::Id,
        identity: type_def::Identity,
    },
    /// A variant of `std::cmp::Ordering`: `use std::cmp::Ordering::*;`.
    OrderingVariant,
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
    pub trait_arguments: Box<[TraitArgument]>,
    /// A trait parameter's default type: `Self` of `trait Add<Rhs = Self>`.
    pub default: Option<type_ref::Type>,
    /// The function's signature names this parameter inside a host type
    /// (`Element<'static, Message>`). The function then takes the
    /// parameter's runtime type as a hidden witness, which its host
    /// operations instantiate their proofs with.
    pub host: bool,
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
    pub associated_bound_arguments: Box<[Box<[TraitArgument]>]>,
    /// Supertraits (`trait Named: Shape + Clone`): their capabilities and
    /// user traits. Every impl of this trait needs them, and a bound by it
    /// implies them.
    pub supertraits: (crate::capability::Set, Box<[crate::item::Name]>),
    /// The type arguments of each generic trait among `supertraits`, in
    /// this trait's parameters and `Self`: `T` of `trait Both<T>:
    /// Convert<T>`, `Self` of `trait Ord2: PartialOrd2<Self>`; made
    /// transitive and substituted by elaboration.
    pub supertrait_arguments: Box<[TraitArgument]>,
    /// `trait Space: Sized`: `dyn Space` is not a type (E0038).
    pub sized: bool,
    /// The trait's own type parameters: `T` of `trait Convert<T>`.
    pub generics: Box<[Generic]>,
    /// The declaration of a facade trait (iced's `canvas::Program`), whose
    /// text its default bodies span.
    pub declaration: Option<crate::ImmutableString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Struct {
    pub header: crate::item::Header,
    pub definition: type_def::Id,
    pub key: type_def::Key,
    pub name: crate::item::Name,
    pub generics: Box<[Generic]>,
    /// How many lifetime parameters it declares.
    pub lifetimes: usize,
    pub fields: Box<[Field]>,
    pub form: crate::item::StructForm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Field {
    pub name: crate::item::Name,
    pub ty: type_ref::Type,
    pub span: Span,
    /// The field is written `&str`, which `ty` reads as `String`: `Copy`,
    /// as a shared reference is.
    pub shared_str: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Enum {
    pub header: crate::item::Header,
    pub definition: type_def::Id,
    pub key: type_def::Key,
    pub name: crate::item::Name,
    pub generics: Box<[Generic]>,
    /// How many lifetime parameters it declares.
    pub lifetimes: usize,
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
    /// The owner is written with `'static` (`impl Copy for Foo<'static>`):
    /// what the impl gives needs a value's borrows to be `'static`.
    pub statics: bool,
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
    /// What the borrow checker reads of the signature's lifetimes.
    pub lifetimes: Lifetimes,
    pub receiver: Option<crate::item::Receiver>,
    pub parameters: Box<[Parameter]>,
    pub output: Option<type_ref::Type>,
    pub body: Body,
}

/// What the borrow checker reads of a signature's lifetimes (D572.4): the
/// name of each lifetime written in its types, by where it is written (a
/// resolved type keeps only that), the receiver's, the outlives bounds, and
/// for a method the impl's owner, which `self` is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lifetimes {
    pub names: std::collections::BTreeMap<Span, crate::ImmutableString>,
    /// `'a` of `&'a self`.
    pub receiver: Option<crate::ImmutableString>,
    /// `('b, 'a)` for `'b: 'a`, and `(T, 'a)` for `T: 'a`.
    pub bounds: Box<[(crate::ImmutableString, crate::ImmutableString)]>,
    /// The impl's owner, for a method: `S<'a>` of `impl<'a> S<'a>`.
    pub owner: Option<Box<type_ref::Type>>,
}

impl Lifetimes {
    /// A function's, from its parsed signature.
    pub(crate) fn of(function: &crate::item::Function) -> Self {
        let mut lifetimes = Self {
            receiver: function
                .receiver_lifetime
                .as_ref()
                .map(|name| name.text.clone()),
            ..Self::default()
        };
        for parameter in &function.parameters {
            lifetimes.name(&parameter.ty);
        }
        if let Some(output) = &function.output {
            lifetimes.name(output);
        }
        lifetimes.bound(&function.lifetimes);
        lifetimes
    }

    /// The lifetimes written in `ty`.
    pub(crate) fn of_type(ty: &crate::item::type_ref::Type) -> Self {
        let mut lifetimes = Self::default();
        lifetimes.name(ty);
        lifetimes
    }

    /// A method's, with its impl's lifetimes and owner.
    pub(crate) fn within(
        &mut self,
        lifetimes: &crate::item::Lifetimes,
        written: &crate::item::type_ref::Type,
        owner: &type_ref::Type,
    ) {
        self.name(written);
        self.bound(lifetimes);
        self.owner = Some(Box::new(owner.clone()));
    }

    fn bound(&mut self, lifetimes: &crate::item::Lifetimes) {
        let mut bounds = self.bounds.to_vec();
        bounds.extend(
            lifetimes
                .bounds
                .iter()
                .map(|(longer, shorter)| (longer.text.clone(), shorter.text.clone())),
        );
        self.bounds = bounds.into_boxed_slice();
    }

    /// Each lifetime written in `ty`, by its span.
    fn name(&mut self, ty: &crate::item::type_ref::Type) {
        use crate::item::type_ref::{Argument, Type};
        match ty {
            Type::Unit(_) => {}
            Type::Tuple { members, .. } => members.iter().for_each(|member| self.name(member)),
            Type::Reference {
                lifetime, target, ..
            } => {
                if let Some(lifetime) = lifetime {
                    self.names.insert(lifetime.span, lifetime.name.text.clone());
                }
                self.name(target);
            }
            Type::Path(path) => {
                for argument in path.segments.iter().flat_map(|segment| &segment.arguments) {
                    match argument {
                        Argument::Type(ty) => self.name(ty),
                        Argument::Lifetime(lifetime) => {
                            self.names.insert(lifetime.span, lifetime.name.text.clone());
                        }
                    }
                }
            }
        }
    }
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
    /// `async fn`: the body runs as an `async move` block.
    pub asynchronous: bool,
    /// The text `span` selects when Rawr wrote the body itself; `None`
    /// for a body in its source.
    pub text: Option<crate::ImmutableString>,
    /// Source within `span` the body leaves out, as blanks: the application
    /// entry at the end of `main`, whose statements before it run once.
    pub elide: Option<Span>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lowering {
    Unavailable,
    NominalFunction,
    NominalMethod(crate::method::Id),
}
