//! Cloneable custom Rawr value for one open widget descriptor.

use std::fmt;
use std::sync::Arc;

use rawr::Dynamic;

use crate::{runtime, widget};

/// A cloneable durable widget descriptor stored as a custom Rawr value.
#[derive(Clone)]
pub struct Value {
    descriptor: Arc<dyn widget::Descriptor>,
    identity: widget::Identity,
    key: Option<widget::key::Value>,
    provenance: Provenance,
}

#[derive(Clone)]
pub(crate) enum Provenance {
    Unattested,
    Witness(rawr::host::runtime::Witness),
}

/// A Rawr callback returned a value outside the open widget boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    actual: &'static str,
}

impl Value {
    /// Seal one native descriptor behind the open durable widget boundary.
    #[must_use]
    pub fn new(descriptor: impl widget::Descriptor) -> Self {
        let identity = descriptor.identity();
        Self {
            descriptor: Arc::new(descriptor),
            identity,
            key: None,
            provenance: Provenance::Unattested,
        }
    }

    /// Seal a native descriptor as an authenticated facade constructor output.
    pub fn from_factory(
        descriptor: impl widget::Descriptor,
        call: &rawr::host::runtime::Call<'_>,
    ) -> Result<Self, rawr::host::runtime::WitnessError> {
        let identity = descriptor.identity();
        Ok(Self {
            descriptor: Arc::new(descriptor),
            identity,
            key: None,
            provenance: Provenance::Witness(call.output_witness()?),
        })
    }

    /// Seal a native descriptor through one authenticated direct projection.
    pub fn from_projection(
        descriptor: impl widget::Descriptor,
        call: &rawr::host::runtime::Call<'_>,
        source: &rawr::host::runtime::Witness,
    ) -> Result<Self, rawr::host::runtime::WitnessError> {
        let identity = descriptor.identity();
        Ok(Self {
            descriptor: Arc::new(descriptor),
            identity,
            key: None,
            provenance: Provenance::Witness(call.project_witness(source)?),
        })
    }

    /// Seal a descriptor produced by one exact owned item transformation.
    pub fn from_transform(
        descriptor: impl widget::Descriptor,
        call: &rawr::host::runtime::Call<'_>,
        source: &rawr::host::runtime::Witness,
    ) -> Result<Self, rawr::host::runtime::WitnessError> {
        let identity = descriptor.identity();
        Ok(Self {
            descriptor: Arc::new(descriptor),
            identity,
            key: None,
            provenance: Provenance::Witness(call.transform_witness(source)?),
        })
    }

    pub(crate) fn from_activation(
        descriptor: impl widget::Descriptor,
        witness: rawr::host::runtime::Witness,
    ) -> Self {
        let identity = descriptor.identity();
        Self {
            descriptor: Arc::new(descriptor),
            identity,
            key: None,
            provenance: Provenance::Witness(witness),
        }
    }

    /// Return the descriptor-supplied identity snapshotted at construction.
    #[must_use]
    pub fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }

    /// Lower a fresh transient iced element for the current authority context.
    #[must_use]
    pub fn lower<'a>(&self, context: &widget::Context<'a>) -> iced::Element<'a, runtime::Message> {
        self.descriptor.lower(context)
    }

    /// Promote an opaque Rawr value only when it contains this exact widget type.
    pub fn from_dynamic(value: Dynamic) -> Result<Self, Error> {
        value.try_cast_result::<Self>().map_err(|actual| Error {
            actual: actual.type_name(),
        })
    }

    pub(crate) const fn witness(&self) -> Option<&rawr::host::runtime::Witness> {
        match &self.provenance {
            Provenance::Witness(witness) => Some(witness),
            Provenance::Unattested => None,
        }
    }

    pub(crate) const fn key(&self) -> Option<&widget::key::Value> {
        self.key.as_ref()
    }

    pub(crate) fn keyed(mut self, key: widget::key::Value) -> Self {
        self.key = Some(key);
        self
    }
}

impl Error {
    /// Return the rejected Rawr runtime type name.
    #[must_use]
    pub const fn actual(&self) -> &'static str {
        self.actual
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Value")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Rawr view returned `{}`, expected `iced::Element`",
            self.actual
        )
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{runtime, widget};

    use super::{Provenance, Value};

    #[test]
    fn key_metadata_changes_neither_descriptor_identity_nor_witness() {
        let identity = widget::Identity::unique();
        let value = Value::new(Stub {
            identity: identity.clone(),
        });
        let descriptor = value.descriptor.clone();
        let provenance = value.provenance.clone();

        let keyed = value.keyed(widget::key::Value::integer(42));

        assert!(Arc::ptr_eq(&descriptor, &keyed.descriptor));
        assert_eq!(keyed.identity(), identity);
        assert!(matches!(
            (&keyed.provenance, &provenance),
            (Provenance::Unattested, Provenance::Unattested)
        ));
        assert!(matches!(keyed.key(), Some(widget::key::Value::Integer(42))));
    }

    struct Stub {
        identity: widget::Identity,
    }

    impl widget::Descriptor for Stub {
        fn lower<'a>(&self, _context: &widget::Context<'a>) -> iced::Element<'a, runtime::Message> {
            iced::widget::Widget::boxed(iced::widget::text("key carrier"))
        }

        fn identity(&self) -> widget::Identity {
            self.identity.clone()
        }
    }
}
