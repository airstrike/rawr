//! Compiler-proven scripted-widget lifecycle protocols.

pub mod binding;
pub mod hook;
pub mod message;
pub mod state;

mod descriptor;
mod error;
mod promotion;
mod resolve;
mod site;

pub use binding::Binding;
pub use descriptor::Descriptor;
pub use error::Error;
pub use promotion::Promotion;
pub(crate) use resolve::resolve;
pub use site::Site;

use crate::{host, item, program, type_def, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// One opaque source-qualified lifecycle location.
#[derive(Clone, Copy, Debug)]
pub struct Location {
    source: program::source::Key,
    span: Span,
}

/// One exact scripted-widget protocol fingerprint.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint {
    owner: type_def::Identity,
    schema: program::schema::Fingerprint,
    element: host::facade::Dependency,
    lifecycle: host::facade::Dependency,
    state: state::Type,
    message: message::Type,
    hooks: Box<[(hook::Kind, item::Value)]>,
}

/// One nominal value with a compiler-validated widget lifecycle.
#[derive(Clone, Debug)]
pub struct Definition {
    owner: type_def::Identity,
    schema: program::schema::Fingerprint,
    element: host::facade::Dependency,
    lifecycle: host::facade::Dependency,
    state: state::Type,
    message: message::Type,
    hooks: Box<[hook::Definition]>,
    declaration: Location,
    fingerprint: Fingerprint,
}

impl Location {
    pub(crate) const fn new(source: program::source::Key, span: Span) -> Self {
        Self { source, span }
    }

    /// Return the exact source span.
    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }

    /// Return whether another location belongs to the same compiled source.
    #[must_use]
    pub fn same_source(self, other: Self) -> bool {
        self.source == other.source
    }
}

impl Definition {
    pub(crate) fn new(
        owner: type_def::Identity,
        schema: program::schema::Fingerprint,
        element: host::facade::Dependency,
        lifecycle: host::facade::Dependency,
        state: state::Type,
        message: message::Type,
        hooks: Box<[hook::Definition]>,
        declaration: Location,
    ) -> Self {
        let fingerprint = Fingerprint {
            owner: owner.clone(),
            schema: schema.clone(),
            element: element.clone(),
            lifecycle: lifecycle.clone(),
            state: state.clone(),
            message: message.clone(),
            hooks: hooks
                .iter()
                .map(|hook| (hook.kind(), hook.item().clone()))
                .collect(),
        };
        Self {
            owner,
            schema,
            element,
            lifecycle,
            state,
            message,
            hooks,
            declaration,
            fingerprint,
        }
    }

    /// Return the widget value's stable nominal identity.
    #[must_use]
    pub const fn owner(&self) -> &type_def::Identity {
        &self.owner
    }

    /// Return the widget value's exact nominal schema.
    #[must_use]
    pub const fn schema(&self) -> &program::schema::Fingerprint {
        &self.schema
    }

    /// Return the exact installed `iced` facade dependency for this protocol.
    #[must_use]
    pub const fn dependency(&self) -> &host::facade::Dependency {
        &self.element
    }

    /// Return the exact installed `rawr_iced` lifecycle facade dependency.
    #[must_use]
    pub const fn lifecycle(&self) -> &host::facade::Dependency {
        &self.lifecycle
    }

    /// Return the exact iced tree-local state contract.
    #[must_use]
    pub const fn state(&self) -> &state::Type {
        &self.state
    }

    /// Return the protocol's single inferred message contract.
    #[must_use]
    pub const fn message(&self) -> &message::Type {
        &self.message
    }

    /// Return declared hooks in canonical lifecycle order.
    #[must_use]
    pub const fn hooks(&self) -> &[hook::Definition] {
        &self.hooks
    }

    /// Return a declared hook, or `None` when its protocol default applies.
    #[must_use]
    pub fn hook(&self, kind: hook::Kind) -> Option<&hook::Definition> {
        self.hooks.iter().find(|hook| hook.kind() == kind)
    }

    /// Return the marked impl declaration location.
    #[must_use]
    pub const fn declaration(&self) -> Location {
        self.declaration
    }

    /// Return the exact structural lifecycle fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }
}
