//! Exact facade-scoped activation of compiler-proven widget descriptors.

use crate::func::SendSync;
use crate::{host, Dynamic, EvalAltResult, Shared};
use std::any::TypeId;

use super::{type_ref, Witness};

trait Callback: SendSync {
    fn activate(
        &self,
        descriptor: crate::widget::Descriptor,
        witness: Witness,
    ) -> Result<Dynamic, Box<EvalAltResult>>;
}

impl<F> Callback for F
where
    F: Fn(crate::widget::Descriptor, Witness) -> Result<Dynamic, Box<EvalAltResult>> + SendSync,
{
    fn activate(
        &self,
        descriptor: crate::widget::Descriptor,
        witness: Witness,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        self(descriptor, witness)
    }
}

pub(super) struct Binding {
    target: host::type_def::Export,
    callback: Shared<dyn Callback>,
}

pub(super) enum Error {
    Unknown,
    Ambiguous,
}

/// One exact open-widget materialization required by compiler-private lowering.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Requirement {
    owner: host::facade::Dependency,
    target: type_ref::Type,
}

impl Binding {
    pub(super) fn bind<T: 'static>(
        definition: &host::facade::Definition,
        callback: impl Fn(crate::widget::Descriptor, Witness) -> Result<Dynamic, Box<EvalAltResult>>
            + SendSync
            + 'static,
    ) -> Result<Self, Error> {
        let mut targets = definition.types().filter(|definition| {
            definition.native_identity() == Some(TypeId::of::<T>())
                && definition.parameters().len() == 1
                && definition.storage() == host::type_def::Storage::Owned
        });
        let target = targets.next().ok_or(Error::Unknown)?;
        if targets.next().is_some() {
            return Err(Error::Ambiguous);
        }
        let target = host::type_def::Export::new(
            definition.root_path().clone(),
            definition.fingerprint().clone(),
            target.contract(),
        );
        Ok(Self {
            target,
            callback: Shared::new(callback),
        })
    }

    pub(super) fn accepts(&self, requirement: &Requirement) -> bool {
        requirement.owner == self.target.dependency()
            && requirement.target.instantiates_export(&self.target)
    }

    pub(super) fn activate(
        &self,
        descriptor: crate::widget::Descriptor,
        witness: Witness,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        self.callback.activate(descriptor, witness)
    }
}

impl Requirement {
    pub(crate) fn bind(owner: host::facade::Dependency, target: type_ref::Type) -> Self {
        Self { owner, target }
    }

    pub(super) const fn owner(&self) -> &host::facade::Dependency {
        &self.owner
    }

    pub(super) const fn target(&self) -> &type_ref::Type {
        &self.target
    }
}
