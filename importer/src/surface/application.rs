//! Concrete source-bound call data carried by an authenticated local application.
//!
//! Deserializing this private transport does not construct an applicability
//! proof. Only the importer-owned catalog can produce a new application; a
//! sealed demand and a generated runner replay the one it produced.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::api::identity as api;
use crate::classify::representability::{convert, origin, ownership, signature};

use super::identity;

/// One checked source method call that the ordinary facade cannot settle:
/// what lowering knows before any applicability proof. It names no expected
/// output, and its lexical scope is the source's own import paths.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Call {
    /// The receiver's imported nominal identity.
    pub receiver: identity::Definition,
    /// The receiver's closed type arguments, when it has any.
    pub receiver_arguments: Box<[convert::Type]>,
    /// The source spelling of the method.
    pub name: Box<str>,
    /// Whether the source receiver is a mutable place.
    pub mutable: bool,
    /// The checked argument types, in order.
    pub arguments: Box<[convert::Type]>,
    /// Every Rust import path in lexical scope at the call, sorted and
    /// deduplicated: module-level imports and those of enclosing blocks.
    pub scope: Box<[Box<[Box<str>]>]>,
}

impl Call {
    /// Sort and deduplicate the scope so equal source scopes have one key.
    #[must_use]
    pub fn canonical(mut self) -> Self {
        let mut scope = self.scope.into_vec();
        scope.sort_unstable();
        scope.dedup();
        self.scope = scope.into_boxed_slice();
        self
    }

    /// The digest lowering looks an application up by, on the host and in a
    /// generated runner alike. It is a lookup key, not a proof.
    #[must_use]
    pub fn key(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"rawr.import.application.call/v1");
        hash.update(serde_json::to_vec(self).unwrap_or_default());
        hash.finalize().into()
    }
}

/// A closed local application produced after applicability and ABI normalization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    /// The lowering request this application answers.
    pub(crate) call: Call,
    /// The selected method declaration.
    pub(crate) declaration: identity::Definition,
    /// The receiver type's public export, which the adapter names.
    pub(crate) export: api::Export,
    /// Where the selected declaration is, for diagnostics.
    pub(crate) origin: origin::Set,
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
    pub const fn call(&self) -> &Call {
        &self.invocation.call
    }
    pub const fn declaration(&self) -> identity::Definition {
        self.invocation.declaration
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
    /// The receiver type's public path, `root::path`.
    #[must_use]
    pub fn receiver_path(&self) -> String {
        std::iter::once(self.invocation.export.root_alias())
            .chain(self.invocation.export.path())
            .collect::<Vec<_>>()
            .join("::")
    }
    /// The root alias of the receiver's crate, whose facade owns the call.
    #[must_use]
    pub fn root(&self) -> &str {
        self.invocation.export.root_alias()
    }
    /// The method spelling the source used.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.invocation.call.name
    }

    /// Encode a set of applications for a generated runner.
    #[doc(hidden)]
    pub fn encode_all(applications: &[Self]) -> Result<Vec<u8>, super::Error> {
        serde_json::to_vec(applications).map_err(|_| super::Error::Encoding)
    }

    /// Decode a generated runner's applications. Each must still be bound
    /// to an adapter the sealed demand generated.
    #[doc(hidden)]
    pub fn decode_all(bytes: &[u8]) -> Result<Vec<Self>, super::Error> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_slice(bytes).map_err(|_| super::Error::Encoding)
    }
}
