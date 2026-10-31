//! Concrete source-bound call data carried by an authenticated local application.
//!
//! Deserializing this private transport does not construct an applicability
//! proof. Only the importer-owned catalog can produce a new application.

use serde::{Deserialize, Serialize};

use crate::classify::representability::{convert, ownership, signature};

/// A closed local application produced after applicability and ABI normalization.
#[derive(Clone, Debug)]
pub struct Application {
    pub(crate) invocation: Invocation,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Invocation {
    pub(crate) identity: [u8; 32],
    pub(crate) proof: [u8; 32],
    pub(crate) catalog: [u8; 32],
    pub(crate) source: [u8; 32],
    pub(crate) scope: [u8; 32],
    pub(crate) receiver: convert::Type,
    pub(crate) ownership: ownership::Receiver,
    pub(crate) signature: signature::Signature,
    pub(crate) native_receiver: Type,
    pub(crate) native_inputs: Box<[Type]>,
    pub(crate) native_output: Type,
    pub(crate) expression: Expression,
    pub(crate) prelude: Option<Path>,
    pub(crate) traits: Box<[Path]>,
}

pub(crate) type Path = Box<[Box<str>]>;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Expression {
    /// The source value is a place of T, reached through the adapter's &T.
    Method { name: Box<str> },
    /// The source explicitly passed &T to its chosen trait's method.
    Ufcs { path: Path, name: Box<str> },
}

/// A public native spelling proved to denote the exact closed semantic type.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Type {
    Primitive(Box<str>),
    Export { path: Path, arguments: Box<[Type]> },
    Tuple(Box<[Type]>),
    Reference { mutable: bool, ty: Box<Type> },
}

impl Application {
    pub const fn identity(&self) -> &[u8; 32] {
        &self.invocation.identity
    }
    pub const fn catalog(&self) -> &[u8; 32] {
        &self.invocation.catalog
    }
    pub const fn receiver(&self) -> &convert::Type {
        &self.invocation.receiver
    }
    pub const fn ownership(&self) -> ownership::Receiver {
        self.invocation.ownership
    }
    pub const fn signature(&self) -> &signature::Signature {
        &self.invocation.signature
    }
    pub const fn source(&self) -> &[u8; 32] {
        &self.invocation.source
    }
    pub const fn scope(&self) -> &[u8; 32] {
        &self.invocation.scope
    }
}
