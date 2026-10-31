//! Cached native read operations, obtained from Rust's actual trait evidence.

use super::{argument, failure, Access, Operation, Policy};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult};
use std::any::{Any, TypeId};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Native `Debug` evidence for one concrete type.
pub struct Debugging<T> {
    value: fn(&T) -> String,
    slice: fn(&[T]) -> String,
}
/// Native `Display` evidence for one concrete type.
pub struct Displaying<T>(fn(&T) -> String);
/// Native `PartialOrd` evidence for one concrete type.
pub struct Partial<T> {
    value: fn(&T, &T) -> Option<Ordering>,
    slice: fn(&[T], &[T]) -> Option<Ordering>,
}
/// Native `Ord` evidence, including native vector sorting.
pub struct Total<T> {
    compare: fn(&T, &T) -> Ordering,
    slice: fn(&[T], &[T]) -> Ordering,
    sort: fn(&mut [T], bool),
}

/// Obtain native formatting from a real `Debug` implementation.
#[must_use]
pub const fn debugging<T: std::fmt::Debug>() -> Debugging<T> {
    Debugging {
        value: |value| format!("{value:?}"),
        slice: |value| format!("{value:?}"),
    }
}
/// Obtain native formatting from a real `Display` implementation.
#[must_use]
pub const fn displaying<T: std::fmt::Display>() -> Displaying<T> {
    Displaying(|value| format!("{value}"))
}
/// Obtain native partial comparison, preserving unordered results.
#[must_use]
pub const fn partial<T: PartialOrd>() -> Partial<T> {
    Partial {
        value: PartialOrd::partial_cmp,
        slice: PartialOrd::partial_cmp,
    }
}
/// Obtain native total comparison and sorting.
#[must_use]
pub const fn total<T: Ord>() -> Total<T> {
    Total {
        compare: Ord::cmp,
        slice: Ord::cmp,
        sort: |values, stable| {
            if stable {
                values.sort()
            } else {
                values.sort_unstable()
            }
        },
    }
}

/// The read capabilities established for one concrete native value.
pub struct Reading<T> {
    debug: Option<Debugging<T>>,
    display: Option<Displaying<T>>,
    partial: Option<Partial<T>>,
    total: Option<Total<T>>,
}

impl<T> Reading<T> {
    /// Bundle concrete Rust trait evidence. A missing implementation remains
    /// absent rather than becoming a fabricated comparison or formatter.
    #[must_use]
    pub const fn from_traits(
        debug: Option<Debugging<T>>,
        display: Option<Displaying<T>>,
        partial: Option<Partial<T>>,
        total: Option<Total<T>>,
    ) -> Self {
        Self {
            debug,
            display,
            partial,
            total,
        }
    }
}

type Evidence = &'static (dyn Any + Send + Sync);
fn registry() -> &'static Mutex<BTreeMap<TypeId, Evidence>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<TypeId, Evidence>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// Cache a generated type's read capabilities once for this runner process.
pub fn register<T: 'static>(reading: Reading<T>) {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(TypeId::of::<T>())
        .or_insert_with(|| Box::leak(Box::new(reading)));
}

pub(super) fn known<T: 'static>() -> Option<&'static Reading<T>> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&TypeId::of::<T>())
        .and_then(|evidence| evidence.downcast_ref())
}

/// Run one read using the policy's cached native traits and borrowed operands.
pub fn invoke<P: Policy>(
    value: Access<'_, P::Value>,
    witness: &Witness,
    operation: Operation,
    arguments: &mut [Dynamic],
) -> RawrResult {
    match operation {
        Operation::Debug | Operation::Display => {
            let reading =
                P::reading().ok_or_else(|| failure("native formatting evidence is absent"))?;
            let text = match operation {
                Operation::Debug => (reading
                    .debug
                    .as_ref()
                    .ok_or_else(|| failure("the native value does not implement Debug"))?
                    .value)(value.read()),
                _ => (reading
                    .display
                    .as_ref()
                    .ok_or_else(|| failure("the native value does not implement Display"))?
                    .0)(value.read()),
            };
            Ok(text.into())
        }
        Operation::Equal { reversed } => {
            let rhs = argument(arguments, 0)?;
            let comparing = P::comparing()
                .ok_or_else(|| failure("the native value does not implement PartialEq"))?;
            P::with_dynamic(&rhs, witness, |rhs| {
                Ok(if reversed {
                    (comparing.equal)(rhs, value.read())
                } else {
                    (comparing.equal)(value.read(), rhs)
                }
                .into())
            })
        }
        Operation::Compare { total, reversed } => {
            let rhs = argument(arguments, 0)?;
            let reading =
                P::reading().ok_or_else(|| failure("native comparison evidence is absent"))?;
            P::with_dynamic(&rhs, witness, |rhs| {
                let (left, right) = if reversed {
                    (rhs, value.read())
                } else {
                    (value.read(), rhs)
                };
                let ordering = if total {
                    Some((reading
                        .total
                        .as_ref()
                        .ok_or_else(|| failure("the native value does not implement Ord"))?
                        .compare)(left, right))
                } else {
                    (reading
                        .partial
                        .as_ref()
                        .ok_or_else(|| failure("the native value does not implement PartialOrd"))?
                        .value)(left, right)
                };
                Ok(ordering.map_or_else(crate::option::none, |order| {
                    crate::option::some((order as crate::INT).into())
                }))
            })
        }
        _ => Err(failure("this operation is not a native value read")),
    }
}

pub(super) fn slice<E: Policy>(
    value: &[E::Value],
    witness: &Witness,
    operation: Operation,
    arguments: &mut [Dynamic],
) -> RawrResult {
    if operation == Operation::Debug {
        let reading =
            E::reading().ok_or_else(|| failure("native formatting evidence is absent"))?;
        let debug = reading
            .debug
            .as_ref()
            .ok_or_else(|| failure("the native slice element does not implement Debug"))?;
        return Ok((debug.slice)(value).into());
    }
    let rhs = argument(arguments, 0)?;
    if let Some(rhs) = rhs.read_lock::<crate::Array>() {
        let element = witness
            .sequence_element()
            .ok_or_else(|| failure("a native sequence has no element witness"))?;
        if matches!(operation, Operation::Equal { .. }) && value.len() != rhs.len() {
            return Ok(false.into());
        }
        for (left, right) in value.iter().zip(rhs.iter()) {
            let compared = E::invoke(
                Access::Shared(left),
                &element,
                operation,
                &mut [right.clone()],
                None,
            )?;
            if matches!(operation, Operation::Equal { .. }) {
                if !compared
                    .as_bool()
                    .map_err(|_| failure("native equality did not return bool"))?
                {
                    return Ok(false.into());
                }
            } else {
                let ordering = crate::option::read(&compared)
                    .ok_or_else(|| failure("native comparison did not return Option<Ordering>"))?;
                if !ordering.is_some_and(|ordering| ordering.as_int() == Ok(0)) {
                    return Ok(compared);
                }
            }
        }
        return Ok(match operation {
            Operation::Equal { .. } => true.into(),
            Operation::Compare { reversed, .. } => {
                let ordering = if reversed {
                    rhs.len().cmp(&value.len())
                } else {
                    value.len().cmp(&rhs.len())
                };
                crate::option::some((ordering as crate::INT).into())
            }
            _ => {
                return Err(failure(
                    "this operation is not a native sequence comparison",
                ))
            }
        });
    }
    let rhs = super::get(&rhs)
        .ok_or_else(|| failure("a native slice comparison requires a sequence place"))?;
    super::slice::reach::<E, _>(&rhs, witness, false, |rhs| match operation {
        Operation::Equal { reversed } => {
            let comparing = E::comparing()
                .ok_or_else(|| failure("the native slice element does not implement PartialEq"))?;
            let (left, right) = if reversed {
                (rhs.read(), value)
            } else {
                (value, rhs.read())
            };
            Ok((comparing.slice)(left, right).into())
        }
        Operation::Compare { total, reversed } => {
            let reading =
                E::reading().ok_or_else(|| failure("native comparison evidence is absent"))?;
            let (left, right) = if reversed {
                (rhs.read(), value)
            } else {
                (value, rhs.read())
            };
            let ordering = if total {
                Some((reading
                    .total
                    .as_ref()
                    .ok_or_else(|| failure("the native slice element does not implement Ord"))?
                    .slice)(left, right))
            } else {
                (reading
                    .partial
                    .as_ref()
                    .ok_or_else(|| {
                        failure("the native slice element does not implement PartialOrd")
                    })?
                    .slice)(left, right)
            };
            Ok(ordering.map_or_else(crate::option::none, |order| {
                crate::option::some((order as crate::INT).into())
            }))
        }
        _ => Err(failure("this operation is not a native slice read")),
    })
}

pub(super) fn sort<P: Policy>(values: &mut [P::Value], stable: bool) -> crate::RawrResultOf<()> {
    let reading = P::reading().ok_or_else(|| failure("native ordering evidence is absent"))?;
    let total = reading
        .total
        .as_ref()
        .ok_or_else(|| failure("the native element does not implement Ord"))?;
    (total.sort)(values, stable);
    Ok(())
}
