//! Sealed authority supplied to authenticated native facade callbacks.

use super::invocation;
use super::type_ref::Type;
use super::witness;
use super::Witness;
use crate::receiver;
use crate::{Dynamic, EvalAltResult};
use std::fmt;

/// Rust's `PartialEq` of two values as the calling program compares them:
/// a crate type by its own `eq` where it has one.
pub type Equal<'a> = dyn Fn(&Dynamic, &Dynamic) -> Result<bool, Box<EvalAltResult>> + 'a;

/// Invocation-local authority for issuing exact host instantiation witnesses.
///
/// Values of this type are created only by a launch-bound runtime registry.
/// Native facade code may use the authority to attest a constructor result or
/// derive a projection result, but cannot create an authority itself.
pub struct Call<'a> {
    authority: witness::Authority,
    operation: Operation<'a>,
    invocation: Option<&'a invocation::Proof>,
    event_mapper: Option<&'a [u8; 16]>,
    equal: Option<&'a Equal<'a>>,
}

enum Operation<'a> {
    Item {
        operands: &'a [Type],
        output: &'a Type,
        receiver: Option<receiver::Mode>,
    },
    Projection {
        source: &'a Type,
        target: &'a Type,
    },
    Structural {
        operands: &'a [Type],
        output: &'a Type,
        receiver: Option<receiver::Mode>,
    },
}

/// A callback requested witness authority outside its authenticated operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Only facade constructors and associated functions may attest a fresh output.
    OutputRequiresConstructor,
    /// Only a registered direct projection may derive a target witness.
    TargetRequiresProjection,
    /// The supplied source witness does not match the projection's exact source.
    ProjectionSourceMismatch,
    /// Only an exact owned item invocation may transform a carried witness.
    TransformRequiresOwnedItem,
    /// The supplied source witness does not match the item's exact receiver type.
    TransformSourceMismatch,
    /// Invocation-scoped authority was requested outside a direct mutable item.
    InvocationRequiresMutableReceiver,
    /// No invocation-scoped authority accompanies this evaluator call.
    InvocationUnavailable,
    /// The authority's facade type differs from the exact mutable receiver.
    InvocationReceiverMismatch,
    /// The authority carries a different host-private value type.
    InvocationPayloadMismatch,
}

impl<'a> Call<'a> {
    pub(super) const fn item(
        authority: witness::Authority,
        operands: &'a [Type],
        output: &'a Type,
        receiver: Option<receiver::Mode>,
        invocation: Option<&'a invocation::Proof>,
    ) -> Self {
        Self {
            authority,
            operation: Operation::Item {
                operands,
                output,
                receiver,
            },
            invocation,
            event_mapper: None,
            equal: None,
        }
    }

    pub(super) const fn projection(
        authority: witness::Authority,
        source: &'a Type,
        target: &'a Type,
    ) -> Self {
        Self {
            authority,
            operation: Operation::Projection { source, target },
            invocation: None,
            event_mapper: None,
            equal: None,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(super) const fn event(
        authority: witness::Authority,
        source: &'a Type,
        target: &'a Type,
        mapper: &'a [u8; 16],
    ) -> Self {
        Self {
            authority,
            operation: Operation::Projection { source, target },
            invocation: None,
            event_mapper: Some(mapper),
            equal: None,
        }
    }

    /// Return the mapper required by this private contextual replay conversion.
    /// Ordinary item and facade projection calls carry no event mapper authority.
    #[doc(hidden)]
    pub const fn event_mapper(&self) -> Option<&[u8; 16]> {
        self.event_mapper
    }

    pub(super) const fn structural(
        authority: witness::Authority,
        operands: &'a [Type],
        output: &'a Type,
        receiver: Option<receiver::Mode>,
    ) -> Self {
        Self {
            authority,
            operation: Operation::Structural {
                operands,
                output,
                receiver,
            },
            invocation: None,
            event_mapper: None,
            equal: None,
        }
    }

    /// This call, comparing values with `equal`: the calling program's
    /// `PartialEq`.
    pub(super) fn with_equal(mut self, equal: Option<&'a Equal<'a>>) -> Self {
        self.equal = equal;
        self
    }

    /// Whether `lhs == rhs`, as Rust's `PartialEq` of the calling program
    /// finds it: a crate type with a hand-written `impl PartialEq` is
    /// compared by its own `eq`, called as `lhs.eq(&rhs)`. A widget whose
    /// Rust counterpart compares values (a radio's value with the
    /// selection) compares them through this. Without the calling program
    /// (a call from Rust) the values compare as a derived `PartialEq` does.
    ///
    /// # Errors
    ///
    /// The error of a hand-written `eq` that failed, as a panic in it.
    pub fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Result<bool, Box<EvalAltResult>> {
        match self.equal {
            Some(equal) => equal(lhs, rhs),
            None => Ok(crate::host::value::structural_equal(lhs, rhs)),
        }
    }

    /// Issue the exact output witness for an authenticated constructor call.
    ///
    /// Owned builders must preserve the witness already carried by their
    /// receiver and therefore cannot mint a replacement through this method.
    pub fn output_witness(&self) -> Result<Witness, Error> {
        let (output, receiver) = match &self.operation {
            Operation::Item {
                output, receiver, ..
            }
            | Operation::Structural {
                output, receiver, ..
            } => (*output, *receiver),
            Operation::Projection { .. } => return Err(Error::OutputRequiresConstructor),
        };
        if receiver.is_some() {
            return Err(Error::OutputRequiresConstructor);
        }
        Ok(Witness::issue(self.authority, output.clone()))
    }

    /// The witness of this call's operand at `index`, as the checked call
    /// typed it: a closure's, whose parameters native code fills, checks
    /// what it gives (review 2).
    #[doc(hidden)]
    pub fn operand_witness(&self, index: usize) -> Result<Witness, Error> {
        let operands = match &self.operation {
            Operation::Item { operands, .. } | Operation::Structural { operands, .. } => *operands,
            Operation::Projection { .. } => return Err(Error::OutputRequiresConstructor),
        };
        let operand = operands
            .get(index)
            .ok_or(Error::OutputRequiresConstructor)?;
        Ok(Witness::issue(self.authority, operand.clone()))
    }

    /// The output witness of a method that borrows its receiver, whose
    /// output is a new value, as an async method's future is (D513.6). An
    /// owned builder's output carries its receiver's witness instead.
    #[doc(hidden)]
    pub fn borrowed_output_witness(&self) -> Result<Witness, Error> {
        let (output, receiver) = match &self.operation {
            Operation::Item {
                output, receiver, ..
            }
            | Operation::Structural {
                output, receiver, ..
            } => (*output, *receiver),
            Operation::Projection { .. } => return Err(Error::OutputRequiresConstructor),
        };
        if !matches!(
            receiver,
            Some(receiver::Mode::Shared | receiver::Mode::Mutable)
        ) {
            return Err(Error::OutputRequiresConstructor);
        }
        Ok(Witness::issue(self.authority, output.clone()))
    }

    /// Transform a carried receiver witness into this exact item's output witness.
    ///
    /// The authority exists only for the current launch-bound invocation. The
    /// receiver and every operand have already passed their exact runtime proofs;
    /// this final check ties the carried witness to the proven owned receiver.
    pub fn transform_witness(&self, source: &Witness) -> Result<Witness, Error> {
        self.transform(source, Witness::matches)
    }

    /// [`Call::transform_witness`] for a native Rust receiver, whose witness
    /// may come from an earlier script generation of this runner.
    #[doc(hidden)]
    pub fn transform_native_witness(&self, source: &Witness) -> Result<Witness, Error> {
        self.transform(source, Witness::matches_native)
    }

    fn transform(
        &self,
        source: &Witness,
        matches: fn(&Witness, witness::Authority, &Type) -> bool,
    ) -> Result<Witness, Error> {
        let (operands, output) = match &self.operation {
            Operation::Item {
                operands,
                output,
                receiver: Some(receiver::Mode::Owned),
            }
            | Operation::Structural {
                operands,
                output,
                receiver: Some(receiver::Mode::Owned),
            } => (*operands, *output),
            Operation::Item { .. }
            | Operation::Structural { .. }
            | Operation::Projection { .. } => return Err(Error::TransformRequiresOwnedItem),
        };
        let Some(receiver) = operands.first() else {
            return Err(Error::TransformRequiresOwnedItem);
        };
        if !matches(source, self.authority, receiver) {
            return Err(Error::TransformSourceMismatch);
        }
        Ok(Witness::issue(self.authority, output.clone()))
    }

    /// Derive the exact mapped target witness for an authenticated projection.
    pub fn project_witness(&self, source: &Witness) -> Result<Witness, Error> {
        let Operation::Projection {
            source: expected,
            target,
        } = &self.operation
        else {
            return Err(Error::TargetRequiresProjection);
        };
        if !source.matches(self.authority, expected) {
            return Err(Error::ProjectionSourceMismatch);
        }
        Ok(Witness::issue(self.authority, (*target).clone()))
    }

    /// Borrow the host-private value scoped to this exact mutable receiver.
    ///
    /// The payload is available only when the evaluator call carries authority
    /// for the receiver's exact facade export and the requested Rust value type
    /// matches. A mismatch is reported without exposing or consuming the value.
    pub fn invocation<T: 'static>(&self) -> Result<&T, Error> {
        let Operation::Item {
            operands,
            receiver: Some(receiver::Mode::Mutable),
            ..
        } = &self.operation
        else {
            return Err(Error::InvocationRequiresMutableReceiver);
        };
        let Some(receiver) = operands.first() else {
            return Err(Error::InvocationRequiresMutableReceiver);
        };
        let Some(invocation) = self.invocation else {
            return Err(Error::InvocationUnavailable);
        };
        if !receiver.matches_export(invocation.export()) {
            return Err(Error::InvocationReceiverMismatch);
        }
        invocation.value().ok_or(Error::InvocationPayloadMismatch)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputRequiresConstructor => formatter.write_str(
                "fresh runtime type witnesses require an authenticated constructor output",
            ),
            Self::TargetRequiresProjection => formatter
                .write_str("target runtime type witnesses require an authenticated projection"),
            Self::ProjectionSourceMismatch => formatter.write_str(
                "projection source witness does not match its exact authenticated source type",
            ),
            Self::TransformRequiresOwnedItem => formatter.write_str(
                "runtime type witness transformation requires an authenticated owned item",
            ),
            Self::TransformSourceMismatch => formatter.write_str(
                "transformed source witness does not match the exact authenticated receiver type",
            ),
            Self::InvocationRequiresMutableReceiver => formatter.write_str(
                "invocation authority requires an authenticated direct mutable receiver",
            ),
            Self::InvocationUnavailable => {
                formatter.write_str("no invocation authority accompanies this evaluator call")
            }
            Self::InvocationReceiverMismatch => formatter.write_str(
                "invocation authority does not match the exact authenticated receiver type",
            ),
            Self::InvocationPayloadMismatch => formatter
                .write_str("invocation authority carries a different host-private value type"),
        }
    }
}

impl std::error::Error for Error {}
