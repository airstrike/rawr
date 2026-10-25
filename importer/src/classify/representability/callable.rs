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
}

/// What a generic item's callable parameter takes and gives: a script
/// closure passed there runs with these (D513.7 b).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Call {
    parameter: u32,
    inputs: Box<[super::convert::Type]>,
    output: super::convert::Type,
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
        }
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
    pub(crate) const fn new(
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
        }
    }

    pub(crate) fn with_call(mut self, call: Option<Call>) -> Self {
        self.call = call;
        self
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

    /// Number of leading generic parameters owned by the inherent
    /// implementation rather than the associated item.
    #[must_use]
    pub const fn container_parameters(&self) -> usize {
        self.container_parameters
    }
}
