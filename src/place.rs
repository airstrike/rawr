//! Typed external places (D651): authority to operate on native storage.
//!
//! A handle owns neither a snapshot nor a reference with an erased lifetime.
//! Borrowed storage is reached through the existing scoped loan boundary;
//! owned storage retains its native value until a source operation moves or
//! drops it. Cloning a handle does neither of those things.

mod callback;
mod native;
pub mod ownership;
pub mod reading;
pub mod sequence;
pub mod slice;
#[cfg(test)]
mod tests;
pub mod text;

use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;
use std::fmt;

pub use native::{borrow, borrow_ref, lend, lend_ref, owned, reach, take};

/// The ownership action selected by a checked extension expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtendMode {
    /// Consume owned elements.
    Move,
    /// Read elements with native Copy evidence.
    Copy,
    /// Clone elements through the native slice extension operation.
    Clone,
}

/// An operation on a checked native place. Arguments are evaluated before
/// dispatch; operations that expose a borrow run their continuation inside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Read an explicitly copied value, rather than copy the place handle.
    Copy,
    /// Format with the native Debug implementation.
    Debug,
    /// Format with the native Display implementation.
    Display,
    /// Compare with native PartialEq.
    Equal {
        /// The external place is the original right-hand operand.
        reversed: bool,
    },
    /// Compare with the selected native ordering trait.
    Compare {
        /// Use Ord rather than PartialOrd.
        total: bool,
        /// The external place is the original right-hand operand.
        reversed: bool,
    },
    /// Sort using the native Ord implementation.
    Sort {
        /// Whether to retain the relative order of equal elements.
        stable: bool,
    },
    /// Invoke the referent's explicit `Clone`.
    Clone,
    /// Assign the owned argument to the referent now.
    Store,
    /// Replace the referent and return ownership of its old value.
    Replace,
    /// Replace the referent with its default and return the old value.
    Take,
    /// Observe a collection's length.
    Len,
    /// Observe a collection's allocation capacity.
    Capacity,
    /// Borrow the element at the supplied index for a continuation.
    Element {
        /// Whether the child loan permits mutation.
        mutable: bool,
    },
    /// Borrow an actual native subslice for a continuation.
    Slice {
        /// Whether the child slice permits mutation.
        mutable: bool,
    },
    /// Read an indexed Copy element while holding its shared parent claim.
    CopyElement,
    /// Assign an owned value to the indexed element now.
    Set,
    /// Append an owned element.
    Push,
    /// Remove the last element, returning its ownership.
    Pop,
    /// Insert an owned element at the supplied index.
    Insert,
    /// Remove the indexed element, returning its ownership.
    Remove,
    /// Remove the indexed element by swapping it with the last one.
    SwapRemove,
    /// Swap two indexed elements.
    Swap,
    /// Swap two whole native referents while preserving both allocations.
    SwapValues,
    /// Reverse the existing elements in place.
    Reverse,
    /// Drop every element, preserving the container's allocation.
    Clear,
    /// Drop the elements past the supplied length.
    Truncate,
    /// Move a second vector's elements into this vector.
    Append,
    /// Extend a vector with the source expression's ownership mode.
    Extend(ExtendMode),
    /// Search with native PartialEq, borrowing each compared element.
    Contains,
    /// Split off the tail as an owned native vector.
    SplitOff,
    /// Fill the existing elements using native Clone/clone_from behavior.
    Fill,
    /// Resize using the element's native Clone behavior.
    Resize,
    /// Resize using the native operation and a script element constructor.
    ResizeWith,
    /// Retain the elements accepted by a script predicate.
    Retain,
    /// Sort with a script comparator using the selected native algorithm.
    SortBy {
        /// Whether to retain the relative order of equal elements.
        stable: bool,
    },
    /// Append text to an actual native String.
    PushStr,
    /// Insert text at a native String byte boundary.
    InsertStr,
    /// Remove a native range and move its elements into an owned vector.
    Drain,
    /// Copy equal-length slices using the native element's `Copy` evidence.
    CopyFromSlice,
    /// Remove consecutive equal elements using native `PartialEq`.
    Dedup,
}

impl Operation {
    /// Whether this operation requires an exclusive native claim.
    const fn mutable(self) -> bool {
        !matches!(
            self,
            Self::Copy
                | Self::Debug
                | Self::Display
                | Self::Equal { .. }
                | Self::Compare { .. }
                | Self::Clone
                | Self::Len
                | Self::Capacity
                | Self::CopyElement
                | Self::Contains
                | Self::Element { mutable: false }
                | Self::Slice { mutable: false }
        )
    }
}

/// The native borrow held for one operation; it cannot escape its visitor.
pub enum Access<'a, T: ?Sized> {
    /// A shared borrow.
    Shared(&'a T),
    /// An exclusive borrow.
    Mutable(&'a mut T),
}

impl<T: ?Sized> Access<'_, T> {
    /// Observe the native value.
    pub fn read(&self) -> &T {
        match self {
            Self::Shared(value) => value,
            Self::Mutable(value) => value,
        }
    }

    /// Reach the native value exclusively, if the operation has that right.
    pub fn write(&mut self) -> RawrResultOf<&mut T> {
        match self {
            Self::Mutable(value) => Ok(value),
            Self::Shared(_) => Err(failure("the native place is shared")),
        }
    }
}

/// Concrete generated conversion and operations for one native type.
///
/// A policy is a descriptor, never a stored native value. `accepts` ties its
/// concrete accessors to the exact logical witness supplied by the checked
/// caller. It is checked before either borrowed or owned storage is exposed.
pub trait Policy: crate::func::SendSync + 'static {
    /// The native storage reached by this descriptor.
    type Value: 'static;

    /// How actual owned values are held, independently of borrowed lending.
    type Owner: ownership::Owner<Self::Value>;

    /// Whether the authenticated logical type is this concrete type.
    fn accepts(witness: &Witness) -> bool;

    /// Materialize an actual owned assignment/call operand, or consume an
    /// owned native carrier of this type without reconstructing its value.
    fn from_dynamic(value: Dynamic, witness: &Witness) -> RawrResultOf<Self::Value>;

    /// Move an owned native result into the interpreter.
    fn into_dynamic(value: Self::Value, witness: &Witness) -> RawrResult;

    /// Concrete cloning operations selected from native trait evidence.
    fn cloning() -> Option<&'static sequence::Cloning<Self::Value>> {
        sequence::known_cloning::<Self::Value>()
    }

    /// Concrete copying operations selected from native `Copy` evidence.
    fn copying() -> Option<&'static sequence::Copying<Self::Value>> {
        sequence::known_copying::<Self::Value>()
    }

    /// Concrete equality operations selected from native `PartialEq` evidence.
    fn comparing() -> Option<&'static sequence::Comparing<Self::Value>> {
        sequence::known_comparing::<Self::Value>()
    }

    /// Native formatting and ordering evidence cached by this runner.
    fn reading() -> Option<&'static reading::Reading<Self::Value>> {
        reading::known::<Self::Value>()
    }

    /// Read an operand without consuming its owner or cloning native elements.
    fn with_dynamic<R>(
        value: &Dynamic,
        witness: &Witness,
        visit: impl FnOnce(&Self::Value) -> RawrResultOf<R>,
    ) -> RawrResultOf<R>
    where
        Self: Sized,
    {
        let place = get(value)
            .ok_or_else(|| failure("the native read operand has no compatible borrowed codec"))?;
        reach::<Self, _>(&place, witness, false, |value| visit(value.read()))
    }

    /// Copy an operand while preserving the source's ownership.
    fn copy_dynamic(_value: &Dynamic, _witness: &Witness) -> RawrResultOf<Self::Value> {
        Err(failure("the native element does not implement Copy"))
    }

    /// Explicitly clone one operand through its native trait evidence.
    fn clone_dynamic(value: &Dynamic, witness: &Witness) -> RawrResultOf<Self::Value>
    where
        Self: Sized,
    {
        let cloning = Self::cloning()
            .ok_or_else(|| failure("the native element does not implement Clone"))?;
        Self::with_dynamic(value, witness, |value| Ok((cloning.element)(value)))
    }

    /// Expose an actual mutable element for its continuation. Generated
    /// nominal policies may use the existing native `Lent` representation.
    fn lend(
        value: &mut Self::Value,
        witness: &Witness,
        rest: &mut crate::scope::Rest<'_>,
    ) -> RawrResult
    where
        Self: Sized,
    {
        lend::<Self>(value, witness, rest)
    }

    /// Expose an actual shared element for its continuation.
    fn lend_ref(
        value: &Self::Value,
        witness: &Witness,
        rest: &mut crate::scope::Rest<'_>,
    ) -> RawrResult
    where
        Self: Sized,
    {
        lend_ref::<Self>(value, witness, rest)
    }

    /// Execute an operation under the exact native access claim.
    fn invoke(
        value: Access<'_, Self::Value>,
        witness: &Witness,
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult;
}

/// Private erased dispatch. Downcasting can recover only the concrete
/// descriptor that originally checked and stored the native value.
trait Storage: Any + crate::func::SendSync {
    fn invoke(
        &self,
        witness: &Witness,
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult;

    fn as_any(&self) -> &dyn Any;
}

/// A checked native place, produced by lending or transferring native storage
/// under its logical witness (`proof-bundle`).
#[derive(Clone)]
pub struct Place {
    witness: Witness,
    storage: Shared<dyn Storage>,
    mutable: bool,
}

impl fmt::Debug for Place {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<native place>")
    }
}

impl Place {
    /// The exact logical type this native place proves.
    #[must_use]
    pub fn witness(&self) -> &Witness {
        &self.witness
    }

    /// Whether this capability was issued with exclusive access rights.
    /// A current conflicting loan can still refuse an operation.
    #[must_use]
    pub const fn is_mutable(&self) -> bool {
        self.mutable
    }

    /// Whether two handles carry the same checked native storage authority.
    #[must_use]
    pub fn same_storage(&self, other: &Self) -> bool {
        self.witness == other.witness && Shared::ptr_eq(&self.storage, &other.storage)
    }

    /// Perform an operation immediately. No callback-exit reconciliation is
    /// involved, including when an operation or a later statement fails.
    pub fn invoke(&self, operation: Operation, arguments: &mut [Dynamic]) -> RawrResult {
        self.storage
            .invoke(&self.witness, operation, arguments, None)
    }

    /// Invoke within an interpreter call, allowing native collection callbacks
    /// to call an ordinary script function pointer in its current context.
    pub fn invoke_in(
        &self,
        context: &crate::NativeCallContext<'_>,
        operation: Operation,
        arguments: &mut [Dynamic],
    ) -> RawrResult {
        callback::within(context, || self.invoke(operation, arguments))
    }

    /// Run a continuation while an operation's child loan is live.
    pub fn with(
        &self,
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: &mut crate::scope::Rest<'_>,
    ) -> RawrResult {
        self.storage
            .invoke(&self.witness, operation, arguments, Some(rest))
    }
}

/// A native place carried by a Dynamic, copied as authority, not contents.
#[must_use]
pub fn get(value: &Dynamic) -> Option<Place> {
    value.read_lock::<Place>().map(|place| place.clone())
}

pub(crate) fn failure(message: impl Into<String>) -> Box<crate::EvalAltResult> {
    crate::EvalAltResult::ErrorRuntime(message.into().into(), crate::Position::NONE).into()
}

fn argument(arguments: &mut [Dynamic], at: usize) -> RawrResult {
    arguments
        .get_mut(at)
        .map(Dynamic::take)
        .ok_or_else(|| failure("a native place operation is missing an argument"))
}

fn index(arguments: &[Dynamic], at: usize) -> RawrResultOf<usize> {
    arguments
        .get(at)
        .and_then(|value| value.as_int().ok())
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| failure("a native place operation has an invalid index"))
}
