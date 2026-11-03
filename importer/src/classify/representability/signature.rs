//! Admitted declared generic and callable signature vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Parameter {
    index: u32,
    name: Box<str>,
    kind: Kind,
    /// A type's defaulted parameter (`Hsl<S = Srgb>`): what a type written
    /// without the argument takes.
    default: Option<super::convert::Type>,
    /// The derivable std traits its bounds name (`T: Clone`): the script
    /// type that fills it must implement them, as rustc requires of the
    /// type it erases to `Value` (review 3).
    bounds: Box<[Box<str>]>,
    /// An argument's `impl Trait` other than a callable, as Freya's
    /// `width(impl Into<Size>)` takes: its name is [`argument_impl_name`],
    /// the call's argument fills it, and no turbofish names it (E0632).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    argument_impl: bool,
    /// A `&str` fills it only at `'static`, as rustc requires of a literal
    /// passed to iced's `title(impl Title<State>)`, which iced implements
    /// for `&'static str`: by its `T: 'static` bound, or because its
    /// trait's impl for a string reference is at `'static`. The string
    /// lives for the program, as a literal does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    static_str: bool,
}

/// The name a signature declares an argument's `impl Trait` by. rustc names
/// it by its bounds, which two arguments can share and no path can spell.
#[must_use]
pub fn argument_impl_name(index: u32) -> Box<str> {
    format!("impl_{index}").into_boxed_str()
}

/// The std traits a type parameter's bound names that a script type
/// implements or not, and an imported type reports: what an erased
/// parameter's bounds keep.
pub const DERIVABLE: [&str; 7] = [
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "PartialEq",
    "Eq",
    "Hash",
];

/// One admitted generic substitution kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Kind {
    Type,
    Constant,
    Callable,
    /// `F: Future`: a script future fills it as `rawr::future::Script`, and
    /// the instance writes it `_` (D527). Its `F::Output` is the synthetic
    /// type parameter [`future_output`] names.
    Future,
}

/// The bit that marks a synthetic type parameter: the `Output` of the
/// future parameter in its low bits (D527).
pub const FUTURE_OUTPUT: u32 = 0x8000_0000;

/// The synthetic parameter that stands for `F::Output` of the future
/// parameter at `index`.
#[must_use]
pub const fn future_output(index: u32) -> u32 {
    FUTURE_OUTPUT | index
}

/// The synthetic parameter that stands for the `ordinal`th associated type
/// a signature projects from the type parameter at `index`, as `P::State`
/// and `P::Message` of iced's `Application<P>::subscription`. The first is
/// [`future_output`]'s.
#[must_use]
pub const fn projection_output(index: u32, ordinal: u32) -> u32 {
    FUTURE_OUTPUT | (ordinal << 20) | index
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Signature {
    parameters: Box<[Parameter]>,
    inputs: Box<[super::convert::Type]>,
    output: super::convert::Type,
    /// What its bounds tie between its type parameters, as rustc's trait
    /// selection reads them.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    relations: Box<[Relation]>,
    /// A method whose Rust output is a borrow guard: `output` is the type
    /// the guard dereferences to, which each call reaches only while the
    /// guard lives inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guard: Option<Guard>,
    /// An item whose output borrows what the call takes, which it runs in
    /// a native scope frame (D558 tier 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<Scope>,
    /// The receiver's type, for a method of an impl that fixes its type's
    /// arguments, as quick-xml's `Reader::read_event` in `impl<'a>
    /// Reader<&'a [u8]>`: the instance the bridge reaches it at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<super::convert::Type>,
    /// The trait a method of a trait impl is called through, at the impl's
    /// arguments, when the impl has parameters its type doesn't name, as
    /// `Renderer` of `impl<.., Renderer> Widget<Message, Theme, Renderer>
    /// for Button<..>`: the method's own type can't fix them, and the trait
    /// path names them, as `<_ as Widget<Part>>::map` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    through: Option<super::convert::Type>,
    /// The associated types each opaque `impl Trait` type the output names
    /// binds, as `application(..)`'s `impl Program<State = S, Message =
    /// M>` binds `State` and `Message`: each is one of the opaque type's
    /// arguments, after the ones it captures.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    opaques: Box<[Opaque]>,
    /// The inputs, by position, whose borrows the output keeps for as long
    /// as it lives, as `view<'a>(items: &'a [Item]) -> impl Show + 'a`
    /// keeps `items`: a shared reference to crate values at a lifetime the
    /// output names.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    lends: Box<[u32]>,
}

/// The associated types an opaque `impl Trait` type binds, each at the
/// position of the type's argument that is its value.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Opaque {
    definition: [u8; 32],
    associated: Box<[(Associated, u32)]>,
    /// Its trait besides auto traits, at the bound's arguments after
    /// `Self`, as `Show` of `impl Show + 'a`: the trait object a value of
    /// it is held as where it borrows what its call takes (#846).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    principal: Option<super::convert::Type>,
    /// Each method of its trait, when a value held as the trait object
    /// implements the trait by calling the object's (#856), as a crate call
    /// that takes the value by value needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forwards: Option<Box<[Forward]>>,
}

/// One method of an opaque type's trait, which a value held as its trait
/// object forwards to the object's (`Opaque::forwards`).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Forward {
    name: Box<str>,
    mutable: bool,
    inputs: Box<[super::convert::Type]>,
    output: super::convert::Type,
}

impl Forward {
    pub(crate) const fn new(
        name: Box<str>,
        mutable: bool,
        inputs: Box<[super::convert::Type]>,
        output: super::convert::Type,
    ) -> Self {
        Self {
            name,
            mutable,
            inputs,
            output,
        }
    }

    /// The method's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether it takes `&mut self`, else `&self`.
    #[must_use]
    pub const fn mutable(&self) -> bool {
        self.mutable
    }

    /// The types it takes after its receiver.
    #[must_use]
    pub const fn inputs(&self) -> &[super::convert::Type] {
        &self.inputs
    }

    /// The type it gives.
    #[must_use]
    pub const fn output(&self) -> &super::convert::Type {
        &self.output
    }
}

impl Opaque {
    pub(crate) const fn new(
        definition: [u8; 32],
        associated: Box<[(Associated, u32)]>,
        principal: Option<super::convert::Type>,
        forwards: Option<Box<[Forward]>>,
    ) -> Self {
        Self {
            definition,
            associated,
            principal,
            forwards,
        }
    }

    /// Each method of its trait a value held as its trait object forwards,
    /// when it can implement the trait so.
    #[must_use]
    pub fn forwards(&self) -> Option<&[Forward]> {
        self.forwards.as_deref()
    }

    /// Its trait besides auto traits, at the bound's arguments after
    /// `Self`, when it has one Rawr can write.
    #[must_use]
    pub const fn principal(&self) -> Option<&super::convert::Type> {
        self.principal.as_ref()
    }

    /// The opaque type, by its definition identity.
    #[must_use]
    pub const fn definition(&self) -> &[u8; 32] {
        &self.definition
    }

    /// Each associated type it binds, with the position of the argument
    /// that is its value.
    #[must_use]
    pub const fn associated(&self) -> &[(Associated, u32)] {
        &self.associated
    }
}

/// How an item's output borrows (D558 tier 1). The call opens a native
/// scope frame, which holds what the output borrows and puts the output
/// in the frame's region, and the rest of the Rust scope the output is
/// bound in runs inside it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Scope {
    /// The output borrows its `&str` inputs, as `Parser::new(&text)`'s
    /// does, or `&mut Vec<T>` inputs, as `read_event_into(&mut buf)`'s
    /// does: the frame owns a copy of each.
    Opens,
    /// The output borrows the receiver, as `BytesStart::name(&self)`'s
    /// does: the frame holds the receiver borrowed.
    Borrows,
}

/// How a method's guard output is reached: read through `Deref`, as
/// `State::read` gives, or also written through `DerefMut`, as
/// `State::write` and `RefCell::borrow_mut` give.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Guard {
    Shared,
    Mutable,
    /// An `Option<&T>` a method lends from its receiver, as pane_grid's
    /// `State::get(&self, pane)` gives: a copy of the target, or `None`.
    OptionShared,
    /// An `Option<&mut T>` a method lends from its receiver, as pane_grid's
    /// `State::get_mut(&mut self, pane)` gives: `Some` of a place each
    /// read and store reaches by calling the method again with copies of
    /// its inputs, or `None`.
    OptionMutable,
}

/// The bit that marks a pattern variable of a [`Case`]: the impl's own
/// type parameter at its low bits.
pub const IMPL_PARAMETER: u32 = 0x4000_0000;

/// A bound `R: Trait<A..>` on the type parameter `R`, with the trait's
/// implementations whose self types Rawr can name. rand's `random_range`
/// has `R: SampleRange<T>`, and rand implements `SampleRange<T>` for
/// `Range<T>`. A call whose argument fills `R` with one case's self type
/// gives that case's trait arguments to `A..`, so `random_range(0.5..1.0)`
/// gives the `T` of the range's floats, as rustc infers it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Relation {
    parameter: u32,
    arguments: Box<[super::convert::Type]>,
    cases: Box<[Case]>,
    /// The bound's trait, by its definition identity, when its impls'
    /// headers may fix what no case does (D604): `W: Widget<M, T, R>`
    /// where `W` is `Column<Element<M, Theme, R>>` and the impl for
    /// `Column<W>` has its own `W: Widget<M, T, R>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trait_identity: Option<[u8; 32]>,
    /// The associated type a projection's synthetic parameter stands for,
    /// as `P::State` of iced's `Application<P>::subscription`: an opaque
    /// type that fills `P` gives it from its own bound (`State = S`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    associated: Option<Associated>,
}

/// An associated type of a trait, by the trait's definition identity and
/// the associated type's name: `Program::State`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Associated {
    trait_identity: [u8; 32],
    name: Box<str>,
}

impl Associated {
    pub(crate) const fn new(trait_identity: [u8; 32], name: Box<str>) -> Self {
        Self {
            trait_identity,
            name,
        }
    }

    /// The trait, by its definition identity.
    #[must_use]
    pub const fn trait_identity(&self) -> [u8; 32] {
        self.trait_identity
    }

    /// The associated type's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// One implementation of a [`Relation`]'s trait: its self type and its
/// trait arguments, with the impl's parameters as [`IMPL_PARAMETER`]
/// variables.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Case {
    this: super::convert::Type,
    arguments: Box<[super::convert::Type]>,
}

impl Relation {
    pub(crate) const fn new(
        parameter: u32,
        arguments: Box<[super::convert::Type]>,
        cases: Box<[Case]>,
    ) -> Self {
        Self {
            parameter,
            arguments,
            cases,
            trait_identity: None,
            associated: None,
        }
    }

    /// The relation of the trait whose definition identity is `identity`.
    pub(crate) const fn of_trait(mut self, identity: [u8; 32]) -> Self {
        self.trait_identity = Some(identity);
        self
    }

    /// The relation of a projection's synthetic parameter, which stands
    /// for `associated` of the parameter.
    pub(crate) fn of_associated(mut self, associated: Associated) -> Self {
        self.associated = Some(associated);
        self
    }

    /// The associated type a projection's synthetic parameter stands for.
    #[must_use]
    pub const fn associated(&self) -> Option<&Associated> {
        self.associated.as_ref()
    }

    /// The bound's trait, by its definition identity, when known.
    #[must_use]
    pub const fn trait_identity(&self) -> Option<[u8; 32]> {
        self.trait_identity
    }

    /// The type parameter the bound is on.
    #[must_use]
    pub const fn parameter(&self) -> u32 {
        self.parameter
    }

    /// The trait's arguments in the item's terms.
    #[must_use]
    pub const fn arguments(&self) -> &[super::convert::Type] {
        &self.arguments
    }

    /// The trait's implementations.
    #[must_use]
    pub const fn cases(&self) -> &[Case] {
        &self.cases
    }
}

impl Case {
    pub(crate) const fn new(
        this: super::convert::Type,
        arguments: Box<[super::convert::Type]>,
    ) -> Self {
        Self { this, arguments }
    }

    /// The implementation's self type.
    #[must_use]
    pub const fn this(&self) -> &super::convert::Type {
        &self.this
    }

    /// The implementation's trait arguments.
    #[must_use]
    pub const fn arguments(&self) -> &[super::convert::Type] {
        &self.arguments
    }
}

impl Parameter {
    /// A type parameter at `index`, named `name`.
    #[must_use]
    pub fn new(index: u32, name: Box<str>, kind: Kind) -> Self {
        Self {
            index,
            name,
            kind,
            default: None,
            bounds: Box::new([]),
            argument_impl: false,
            static_str: false,
        }
    }

    /// This parameter, which a `&str` fills only at `'static` when
    /// `static_str`.
    pub(crate) const fn with_static_str(mut self, static_str: bool) -> Self {
        self.static_str = static_str;
        self
    }

    /// Whether a `&str` fills it only at `'static`.
    #[must_use]
    pub const fn static_str(&self) -> bool {
        self.static_str
    }

    /// This parameter as an argument's `impl Trait`, renamed so a
    /// signature can declare it.
    pub(crate) fn into_argument_impl(mut self) -> Self {
        self.name = argument_impl_name(self.index);
        self.argument_impl = true;
        self
    }

    /// Whether this is an argument's `impl Trait` other than a callable.
    #[must_use]
    pub const fn is_argument_impl(&self) -> bool {
        self.argument_impl
    }

    pub(crate) fn with_bounds(mut self, bounds: Box<[Box<str>]>) -> Self {
        self.bounds = bounds;
        self
    }

    /// The derivable std traits its bounds name.
    #[must_use]
    pub fn bounds(&self) -> &[Box<str>] {
        &self.bounds
    }

    pub(crate) fn with_default(mut self, default: Option<super::convert::Type>) -> Self {
        self.default = default;
        self
    }

    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The type a type written without this argument takes.
    #[must_use]
    pub const fn default(&self) -> Option<&super::convert::Type> {
        self.default.as_ref()
    }

    pub(crate) fn mark_callable(&mut self) {
        self.kind = Kind::Callable;
    }

    pub(crate) fn mark_future(&mut self) {
        self.kind = Kind::Future;
    }

    /// Whether this is the synthetic `F::Output` of a future parameter.
    #[must_use]
    pub const fn is_future_output(&self) -> bool {
        self.index & FUTURE_OUTPUT != 0
    }
}

impl Signature {
    pub(crate) fn new(
        parameters: Box<[Parameter]>,
        inputs: Box<[super::convert::Type]>,
        output: super::convert::Type,
    ) -> Self {
        Self {
            parameters,
            inputs,
            output,
            relations: Box::new([]),
            guard: None,
            scope: None,
            instance: None,
            through: None,
            opaques: Box::new([]),
            lends: Box::new([]),
        }
    }

    pub(crate) fn with_opaques(mut self, opaques: Box<[Opaque]>) -> Self {
        self.opaques = opaques;
        self
    }

    /// The associated types each opaque type the output names binds.
    #[must_use]
    pub const fn opaques(&self) -> &[Opaque] {
        &self.opaques
    }

    pub(crate) fn with_through(mut self, through: Option<super::convert::Type>) -> Self {
        self.through = through;
        self
    }

    /// The trait the method is called through, as a nominal type of the
    /// trait's identity and its arguments other than `Self`, when its impl
    /// has parameters its type doesn't name.
    #[must_use]
    pub const fn through(&self) -> Option<&super::convert::Type> {
        self.through.as_ref()
    }

    pub(crate) fn with_instance(mut self, instance: Option<super::convert::Type>) -> Self {
        self.instance = instance;
        self
    }

    /// The receiver's type, when the method's impl fixes its type's
    /// arguments.
    #[must_use]
    pub const fn instance(&self) -> Option<&super::convert::Type> {
        self.instance.as_ref()
    }

    /// This signature with the parameters at `indices` inferred at the
    /// call, as an implementation's parameter its type doesn't name is
    /// (`Renderer` of `impl<.., Renderer> Widget<Message, Theme, Renderer>
    /// for Button<..>`): no turbofish names one, as none names an
    /// argument's `impl Trait`. Among the first `container` parameters, the
    /// implementation's, they follow the ones its type names, which a path
    /// to the type gives.
    pub(crate) fn with_inferred(mut self, indices: &[u32], container: usize) -> Self {
        let mut parameters = std::mem::take(&mut self.parameters).into_vec();
        let container = container.min(parameters.len());
        parameters[..container].sort_by_key(|parameter| indices.contains(&parameter.index()));
        self.parameters = parameters
            .into_iter()
            .map(|parameter| {
                if indices.contains(&parameter.index()) {
                    parameter.into_argument_impl()
                } else {
                    parameter
                }
            })
            .collect();
        self
    }

    pub(crate) fn with_scope(mut self, scope: Option<Scope>) -> Self {
        self.scope = scope;
        self
    }

    /// How the output borrows what the call takes, when it does.
    #[must_use]
    pub const fn scope(&self) -> Option<Scope> {
        self.scope
    }

    pub(crate) fn with_lends(mut self, lends: Box<[u32]>) -> Self {
        self.lends = lends;
        self
    }

    /// The inputs, by position, whose borrows the output keeps.
    #[must_use]
    pub fn lends(&self) -> &[u32] {
        &self.lends
    }

    pub(crate) fn with_guard(mut self, guard: Option<Guard>) -> Self {
        self.guard = guard;
        self
    }

    /// How the output is reached when the Rust output is a borrow guard.
    #[must_use]
    pub const fn guard(&self) -> Option<Guard> {
        self.guard
    }

    pub(crate) fn with_relations(mut self, relations: Box<[Relation]>) -> Self {
        self.relations = relations;
        self
    }

    /// What its bounds tie between its type parameters.
    #[must_use]
    pub const fn relations(&self) -> &[Relation] {
        &self.relations
    }

    #[must_use]
    pub const fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn inputs(&self) -> &[super::convert::Type] {
        &self.inputs
    }

    #[must_use]
    pub const fn output(&self) -> &super::convert::Type {
        &self.output
    }
}
