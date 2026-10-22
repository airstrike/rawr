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
        }
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
