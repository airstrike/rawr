//! Declaration-only generic function item evidence.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Class {
    Owned,
    Borrowed,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Item {
    signature: super::signature::Signature,
    receiver: Option<super::ownership::Receiver>,
    class: Option<crate::surface::callable::Class>,
    binders: Box<[crate::surface::instance::Binder]>,
    predicates: Box<[super::predicate::Bound]>,
    /// The callable parameter's `Fn*` inputs and output, as its bound and
    /// its `Output` projection write them (`F: FnOnce() -> R`).
    #[serde(default)]
    call: Option<Call>,
    /// Each callable parameter's inputs and output, as a script function
    /// fills it by a slot (D594). Empty when one can't.
    #[serde(default)]
    crossings: Box<[Call]>,
    /// The item with each parameter of a crate's trait over `Fn` that the
    /// crate also implements for a type of its own kept at that trait, as
    /// iced's `ThemeFn` is implemented for `Theme`: a value of a type that
    /// implements it fills the parameter, as `.theme(Theme::Dark)` does.
    /// A call with no script closure or function there takes this form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    values: Option<Box<Item>>,
}

/// What a generic item's callable parameter takes and gives: a script
/// closure passed there runs with these (D513.7 b).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Call {
    parameter: u32,
    inputs: Box<[super::convert::Type]>,
    output: super::convert::Type,
    /// The output borrows at the lifetime the bound declares for its
    /// inputs, as `for<'a> Fn(&'a S) -> Element<'a, M>`'s does.
    #[serde(default)]
    borrows: bool,
}

impl Call {
    pub(crate) const fn new(
        parameter: u32,
        inputs: Box<[super::convert::Type]>,
        output: super::convert::Type,
    ) -> Self {
        Self {
            parameter,
            inputs,
            output,
            borrows: false,
        }
    }

    pub(crate) const fn borrowing(mut self, borrows: bool) -> Self {
        self.borrows = borrows;
        self
    }

    /// Whether the output borrows at the lifetime of the inputs.
    #[must_use]
    pub const fn borrows(&self) -> bool {
        self.borrows
    }

    /// The index of the callable type parameter.
    #[must_use]
    pub const fn parameter(&self) -> u32 {
        self.parameter
    }

    /// The callable's inputs, in order.
    #[must_use]
    pub fn inputs(&self) -> &[super::convert::Type] {
        &self.inputs
    }

    /// The callable's output.
    #[must_use]
    pub const fn output(&self) -> &super::convert::Type {
        &self.output
    }
}

/// One finite generic inherent operation declaration.
///
/// Unlike [`Item`], this proof grants only direct checked invocation. It never
/// represents a callable operand or runtime value.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Invocation {
    item: Item,
    container_parameters: usize,
}

impl Item {
    pub(crate) fn new(
        signature: super::signature::Signature,
        receiver: Option<super::ownership::Receiver>,
        class: Option<crate::surface::callable::Class>,
        binders: Box<[crate::surface::instance::Binder]>,
        predicates: Box<[super::predicate::Bound]>,
    ) -> Self {
        Self {
            signature,
            receiver,
            class,
            binders,
            predicates,
            call: None,
            crossings: Box::new([]),
            values: None,
        }
    }

    pub(crate) fn with_values(mut self, values: Self) -> Self {
        self.values = Some(Box::new(values));
        self
    }

    /// The form that a value of a type implementing the trait fills, when
    /// the item has one ([`Self::form`]).
    #[must_use]
    pub fn values(&self) -> Option<&Self> {
        self.values.as_deref()
    }

    /// The form a call takes: the one a value fills when the call passes
    /// no script closure (`callable`), script function (`slotted`) or
    /// crate function there, and the item has one; this item otherwise.
    #[must_use]
    pub fn form(
        &self,
        callable: Option<crate::surface::callable::Class>,
        slotted: bool,
        functions: bool,
    ) -> &Self {
        match self.values() {
            Some(values) if callable.is_none() && !slotted && !functions => values,
            _ => self,
        }
    }

    pub(crate) fn with_call(mut self, call: Option<Call>) -> Self {
        self.call = call;
        self
    }

    pub(crate) fn with_crossings(mut self, crossings: Box<[Call]>) -> Self {
        self.crossings = crossings;
        self
    }

    /// Each callable parameter's inputs and output, as a script function
    /// fills it by a slot (D594).
    #[must_use]
    pub fn crossings(&self) -> &[Call] {
        &self.crossings
    }

    /// The callable parameter's inputs and output, when it has one.
    #[must_use]
    pub const fn call(&self) -> Option<&Call> {
        self.call.as_ref()
    }

    #[must_use]
    pub const fn signature(&self) -> &super::signature::Signature {
        &self.signature
    }

    #[must_use]
    pub const fn receiver(&self) -> Option<super::ownership::Receiver> {
        self.receiver
    }

    #[must_use]
    pub const fn class(&self) -> Option<crate::surface::callable::Class> {
        self.class
    }

    #[must_use]
    pub const fn binders(&self) -> &[crate::surface::instance::Binder] {
        &self.binders
    }

    #[must_use]
    pub const fn predicates(&self) -> &[super::predicate::Bound] {
        &self.predicates
    }
}

impl Item {
    /// This item with the parameters at `indices` inferred at the call
    /// (`Signature::with_inferred`).
    pub(crate) fn with_inferred(mut self, indices: &[u32], container: usize) -> Self {
        self.signature = self.signature.with_inferred(indices, container);
        self.values = self
            .values
            .map(|values| Box::new(values.with_inferred(indices, container)));
        self
    }

    /// This item of a trait impl, called through the trait at `through`.
    pub(crate) fn with_through(mut self, through: Option<super::convert::Type>) -> Self {
        self.signature = self.signature.with_through(through);
        self
    }

    /// This item of an impl that fixes some of its type's arguments, at
    /// that instance.
    pub(crate) fn with_instance(mut self, instance: super::convert::Type) -> Self {
        self.values = self
            .values
            .map(|values| Box::new(values.with_instance(instance.clone())));
        self.signature = self.signature.with_instance(Some(instance));
        self
    }
}

impl Invocation {
    pub(crate) const fn new(item: Item, container_parameters: usize) -> Self {
        Self {
            item,
            container_parameters,
        }
    }

    #[must_use]
    pub const fn signature(&self) -> &super::signature::Signature {
        self.item.signature()
    }

    #[must_use]
    pub const fn receiver(&self) -> Option<super::ownership::Receiver> {
        self.item.receiver()
    }

    #[must_use]
    pub const fn class(&self) -> Option<crate::surface::callable::Class> {
        self.item.class()
    }

    #[must_use]
    pub const fn binders(&self) -> &[crate::surface::instance::Binder] {
        self.item.binders()
    }

    #[must_use]
    pub const fn predicates(&self) -> &[super::predicate::Bound] {
        self.item.predicates()
    }

    /// The script closure the item takes, when Rawr makes it.
    #[must_use]
    pub const fn call(&self) -> Option<&Call> {
        self.item.call()
    }

    /// Each callable parameter's inputs and output, as a script function
    /// fills it by a slot (D594).
    #[must_use]
    pub fn crossings(&self) -> &[Call] {
        self.item.crossings()
    }

    /// The item ([`Item::form`]).
    #[must_use]
    pub const fn item(&self) -> &Item {
        &self.item
    }

    /// Number of leading generic parameters owned by the inherent
    /// implementation rather than the associated item.
    #[must_use]
    pub const fn container_parameters(&self) -> usize {
        self.container_parameters
    }
}
