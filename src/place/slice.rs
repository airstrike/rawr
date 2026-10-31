//! Actual native slice loans. A fat reference lives only inside its scoped
//! family; the interpreter carries a nonce, never an escaped element pointer.

use super::{argument, failure, index, Access, Operation, Place, Policy, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;
use std::marker::PhantomData;
use std::ops::Range;

struct Mutable<E>(PhantomData<E>);
struct ReadOnly<E>(PhantomData<E>);
impl<E: Policy> crate::scope::Family for Mutable<E> {
    type Of<'a> = &'a mut [E::Value];
}
impl<E: Policy> crate::scope::Family for ReadOnly<E> {
    type Of<'a> = &'a [E::Value];
}

struct Source<E> {
    nonce: crate::scope::Nonce,
    mutable: bool,
    policy: PhantomData<fn() -> E>,
}

impl<E: Policy> Source<E> {
    fn access<R>(
        &self,
        mutable: bool,
        visit: impl FnOnce(Access<'_, [E::Value]>) -> RawrResultOf<R>,
    ) -> RawrResultOf<R> {
        match (self.mutable, mutable) {
            (false, true) => Err(failure("the native slice is shared")),
            (false, false) => crate::scope::with_ref::<ReadOnly<E>, _>(self.nonce, |value| {
                visit(Access::Shared(value))
            })
            .map_err(|error| failure(error.to_string()))?,
            (true, false) => crate::scope::with_ref::<Mutable<E>, _>(self.nonce, |value| {
                visit(Access::Shared(value))
            })
            .map_err(|error| failure(error.to_string()))?,
            (true, true) => crate::scope::with::<Mutable<E>, _>(self.nonce, |value| {
                visit(Access::Mutable(value))
            })
            .map_err(|error| failure(error.to_string()))?,
        }
    }
}

impl<E: Policy> Storage for Source<E> {
    fn invoke(
        &self,
        witness: &Witness,
        path: &[usize],
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        self.access(operation.mutable(), |value| {
            let Some((&at, path)) = path.split_first() else {
                return invoke::<E>(value, witness, operation, arguments, rest);
            };
            let element = witness
                .sequence_element()
                .ok_or_else(|| failure("a native slice has no element witness"))?;
            match value {
                Access::Shared(value) => E::project(
                    Access::Shared(value.get(at).ok_or_else(out_of_bounds)?),
                    &element,
                    path,
                    operation,
                    arguments,
                    rest,
                ),
                Access::Mutable(value) => E::project(
                    Access::Mutable(value.get_mut(at).ok_or_else(out_of_bounds)?),
                    &element,
                    path,
                    operation,
                    arguments,
                    rest,
                ),
            }
        })
    }

    fn reach(
        &self,
        _witness: &Witness,
        path: &[usize],
        mutable: bool,
        visit: &mut dyn FnMut(Access<'_, dyn Any>) -> RawrResult,
    ) -> RawrResult {
        let Some((&at, path)) = path.split_first() else {
            return Err(failure("a native slice is reached by its own descriptor"));
        };
        self.access(mutable, |value| match value {
            Access::Shared(value) => E::reach_element(
                Access::Shared(value.get(at).ok_or_else(out_of_bounds)?),
                path,
                visit,
            ),
            Access::Mutable(value) => E::reach_element(
                Access::Mutable(value.get_mut(at).ok_or_else(out_of_bounds)?),
                path,
                visit,
            ),
        })
    }

    fn copies(&self, path: &[usize]) -> bool {
        path.split_first()
            .is_some_and(|(_, path)| E::copies_at(path))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn out_of_bounds() -> Box<crate::EvalAltResult> {
    failure("native slice index out of bounds")
}

fn checked<E: Policy>(nonce: crate::scope::Nonce, mutable: bool, witness: &Witness) -> RawrResult {
    if !witness
        .sequence_element()
        .is_some_and(|element| E::accepts(&element))
    {
        return Err(failure("a native slice has the wrong element witness"));
    }
    Ok(Dynamic::from(Place {
        witness: witness.clone(),
        mutable,
        storage: Shared::new(Source::<E> {
            nonce,
            mutable,
            policy: PhantomData,
        }),
    }))
}

/// Lend the actual mutable native slice for its continuation.
pub fn lend<E: Policy>(
    mut value: &mut [E::Value],
    witness: &Witness,
    rest: &mut crate::scope::Rest<'_>,
) -> RawrResult {
    crate::scope::lend::<Mutable<E>, _>(&mut value, |nonce| {
        rest(checked::<E>(nonce, true, witness)?)
    })
}

/// Lend the actual shared native slice for its continuation.
pub fn lend_ref<E: Policy>(
    value: &[E::Value],
    witness: &Witness,
    rest: &mut crate::scope::Rest<'_>,
) -> RawrResult {
    crate::scope::lend_ref::<ReadOnly<E>, _>(&value, |nonce| {
        rest(checked::<E>(nonce, false, witness)?)
    })
}

/// Reach either a native vector or an already lent slice as a slice, without
/// changing its structural permissions or ownership representation.
pub fn reach<E: Policy, R>(
    place: &Place,
    witness: &Witness,
    mutable: bool,
    visit: impl FnOnce(Access<'_, [E::Value]>) -> RawrResultOf<R>,
) -> RawrResultOf<R> {
    if !place.witness.same_native_type(witness) {
        return Err(failure("a native slice operand has the wrong logical type"));
    }
    if let Some(source) = place.storage.as_any().downcast_ref::<Source<E>>() {
        return source.access(mutable, visit);
    }
    super::reach::<super::sequence::Vector<E>, _>(place, witness, mutable, |value| match value {
        Access::Shared(value) => visit(Access::Shared(value)),
        Access::Mutable(value) => visit(Access::Mutable(value)),
    })
}

pub(super) fn bounds(arguments: &[Dynamic], length: usize) -> RawrResultOf<Range<usize>> {
    let low = index(arguments, 0)?;
    let high = arguments
        .get(1)
        .and_then(|value| value.as_int().ok())
        .ok_or_else(|| failure("a slice has no upper bound"))?;
    let inclusive = arguments
        .get(2)
        .and_then(|value| value.as_bool().ok())
        .ok_or_else(|| failure("a slice has no inclusivity"))?;
    let high = if high == -1 {
        length
    } else {
        usize::try_from(high)
            .ok()
            .and_then(|high| high.checked_add(usize::from(inclusive)))
            .ok_or_else(|| failure("a slice has an invalid upper bound"))?
    };
    if low > high || high > length {
        return Err(failure("native slice range is out of bounds"));
    }
    Ok(low..high)
}

fn invoke<E: Policy>(
    mut value: Access<'_, [E::Value]>,
    witness: &Witness,
    operation: Operation,
    arguments: &mut [Dynamic],
    rest: Option<&mut crate::scope::Rest<'_>>,
) -> RawrResult {
    let element = witness
        .sequence_element()
        .ok_or_else(|| failure("a native slice has no element witness"))?;
    match operation {
        Operation::Debug | Operation::Equal { .. } | Operation::Compare { .. } => {
            super::reading::slice::<E>(value.read(), witness, operation, arguments)
        }
        Operation::Len => Ok((value.read().len() as crate::INT).into()),
        Operation::Element { mutable } => {
            let at = index(arguments, 0)?;
            let rest = rest.ok_or_else(|| failure("an element loan has no continuation"))?;
            if mutable {
                let item = value
                    .write()?
                    .get_mut(at)
                    .ok_or_else(|| failure("native slice index out of bounds"))?;
                E::lend(item, &element, rest)
            } else {
                let item = value
                    .read()
                    .get(at)
                    .ok_or_else(|| failure("native slice index out of bounds"))?;
                E::lend_ref(item, &element, rest)
            }
        }
        Operation::Slice { mutable } => {
            let range = bounds(arguments, value.read().len())?;
            let rest = rest.ok_or_else(|| failure("a slice loan has no continuation"))?;
            if mutable {
                lend::<E>(&mut value.write()?[range], witness, rest)
            } else {
                lend_ref::<E>(&value.read()[range], witness, rest)
            }
        }
        Operation::CopyElement => {
            let item = value
                .read()
                .get(index(arguments, 0)?)
                .ok_or_else(|| failure("native slice index out of bounds"))?;
            E::invoke(
                Access::Shared(item),
                &element,
                Operation::Copy,
                &mut [],
                None,
            )
        }
        Operation::Set => {
            let at = index(arguments, 0)?;
            let replacement = E::from_dynamic(argument(arguments, 1)?, &element)?;
            *value
                .write()?
                .get_mut(at)
                .ok_or_else(|| failure("native slice index out of bounds"))? = replacement;
            Ok(Dynamic::UNIT)
        }
        Operation::Reverse => {
            value.write()?.reverse();
            Ok(Dynamic::UNIT)
        }
        Operation::Swap => {
            let (left, right) = (index(arguments, 0)?, index(arguments, 1)?);
            let value = value.write()?;
            if left >= value.len() || right >= value.len() {
                return Err(failure("native slice swap index out of bounds"));
            }
            value.swap(left, right);
            Ok(Dynamic::UNIT)
        }
        Operation::Sort { stable } => {
            super::reading::sort::<E>(value.write()?, stable)?;
            Ok(Dynamic::UNIT)
        }
        Operation::SortBy { stable } => {
            super::sequence::sort_by::<E>(value.write()?, &element, argument(arguments, 0)?, stable)
        }
        Operation::Clone => {
            let cloning = E::cloning()
                .ok_or_else(|| failure("the native slice element does not implement Clone"))?;
            super::sequence::Vector::<E>::into_dynamic((cloning.slice)(value.read()), witness)
        }
        Operation::Fill => {
            let cloning = E::cloning()
                .ok_or_else(|| failure("the native slice element does not implement Clone"))?;
            let item = E::from_dynamic(argument(arguments, 0)?, &element)?;
            (cloning.fill)(value.write()?, item);
            Ok(Dynamic::UNIT)
        }
        Operation::Contains => {
            let needle = argument(arguments, 0)?;
            for item in value.read() {
                let equal = E::invoke(
                    Access::Shared(item),
                    &element,
                    Operation::Equal { reversed: false },
                    &mut [needle.clone()],
                    None,
                )?;
                if equal
                    .as_bool()
                    .map_err(|_| failure("native equality did not return bool"))?
                {
                    return Ok(true.into());
                }
            }
            Ok(false.into())
        }
        Operation::CopyFromSlice => {
            let copying = E::copying()
                .ok_or_else(|| failure("the native slice element does not implement Copy"))?;
            let source = argument(arguments, 0)?;
            let mut copy = |source: &[E::Value]| {
                let target = value.write()?;
                if target.len() != source.len() {
                    return Err(failure("copy_from_slice requires equal lengths"));
                }
                (copying.slice)(target, source);
                Ok(Dynamic::UNIT)
            };
            if let Some(source) = super::get(&source) {
                reach::<E, _>(&source, witness, false, |source| copy(source.read()))
            } else {
                let source = source
                    .read_lock::<crate::Array>()
                    .ok_or_else(|| failure("copy_from_slice requires a sequence"))?;
                let source = source
                    .iter()
                    .map(|value| E::copy_dynamic(value, &element))
                    .collect::<RawrResultOf<Vec<_>>>()?;
                copy(&source)
            }
        }
        _ => Err(failure(
            "this operation is not defined for the native slice",
        )),
    }
}
