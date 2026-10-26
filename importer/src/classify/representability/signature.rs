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
}

/// How a method's guard output is reached: read through `Deref`, as
/// `State::read` gives, or also written through `DerefMut`, as
/// `State::write` and `RefCell::borrow_mut` give.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Guard {
    Shared,
    Mutable,
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
        }
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
    pub(crate) fn new(index: u32, name: Box<str>, kind: Kind) -> Self {
        Self {
            index,
            name,
            kind,
            default: None,
            bounds: Box::new([]),
        }
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
        }
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
