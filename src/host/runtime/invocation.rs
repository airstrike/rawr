//! Facade-authenticated values scoped to one evaluator invocation.

use crate::func::SendSync;
use crate::{host, Shared};
use std::any::Any;
use std::fmt;

trait Payload: SendSync {
    fn export(&self) -> &host::type_def::Export;

    fn value(&self) -> &dyn Any;
}

struct Value<T> {
    export: host::type_def::Export,
    value: T,
}

impl<T: SendSync + 'static> Payload for Value<T> {
    fn export(&self) -> &host::type_def::Export {
        &self.export
    }

    fn value(&self) -> &dyn Any {
        &self.value
    }
}

/// One immutable host proof available only during an exact evaluator call.
///
/// Construction requires an exported facade type whose checked storage class
/// is [`host::type_def::Storage::Invocation`]. Native callbacks can recover the
/// carried value only when their direct mutable receiver is that exact facade
/// type.
pub struct Invocation<T> {
    proof: Shared<Value<T>>,
}

/// An exported facade type cannot issue invocation-scoped authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The exported type permits ordinary owned storage.
    OwnedStorage,
}

#[derive(Clone)]
pub(crate) struct Proof {
    proof: Shared<dyn Payload>,
}

impl<T: SendSync + 'static> Invocation<T> {
    /// Bind one value to an exact invocation-scoped facade type.
    pub fn bind(export: host::type_def::Export, value: T) -> Result<Self, Error> {
        if export.storage() != host::type_def::Storage::Invocation {
            return Err(Error::OwnedStorage);
        }

        Ok(Self {
            proof: Shared::new(Value { export, value }),
        })
    }

    pub(crate) fn proof(&self) -> Proof {
        Proof {
            proof: self.proof.clone(),
        }
    }
}

impl<T> Clone for Invocation<T> {
    fn clone(&self) -> Self {
        Self {
            proof: self.proof.clone(),
        }
    }
}

impl<T> fmt::Debug for Invocation<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Invocation")
            .field("type", &self.proof.export.path())
            .finish_non_exhaustive()
    }
}

impl Proof {
    pub(super) fn export(&self) -> &host::type_def::Export {
        self.proof.export()
    }

    pub(super) fn value<T: 'static>(&self) -> Option<&T> {
        self.proof.value().downcast_ref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnedStorage => formatter.write_str(
                "invocation authority requires a facade type with invocation-scoped storage",
            ),
        }
    }
}

impl std::error::Error for Error {}
