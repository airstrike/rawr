//! Native vectors reached by the shared place operation protocol.

use super::{argument, failure, get, index, Access, ExtendMode, Operation, Policy};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::{Mutex, OnceLock};

type Capability = &'static (dyn std::any::Any + Send + Sync);

#[derive(Default)]
struct Capabilities {
    cloning: Option<Capability>,
    copying: Option<Capability>,
    comparing: Option<Capability>,
}

fn capabilities() -> &'static Mutex<BTreeMap<std::any::TypeId, Capabilities>> {
    static CAPABILITIES: OnceLock<Mutex<BTreeMap<std::any::TypeId, Capabilities>>> =
        OnceLock::new();
    CAPABILITIES.get_or_init(Mutex::default)
}

/// Cache concrete trait evidence emitted once by this runner. Descriptor
/// storage is process-long, like generated function pointers, and contains no
/// native values. Existing entries are never replaced or leaked again.
pub fn register_capabilities<T: 'static>(
    cloning: Option<Cloning<T>>,
    copying: Option<Copying<T>>,
    comparing: Option<Comparing<T>>,
) {
    fn keep<T: Send + Sync + 'static>(value: Option<T>) -> Option<Capability> {
        value.map(|value| Box::leak(Box::new(value)) as Capability)
    }
    capabilities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(std::any::TypeId::of::<T>())
        .or_insert_with(|| Capabilities {
            cloning: keep(cloning),
            copying: keep(copying),
            comparing: keep(comparing),
        });
}

pub(super) fn known_cloning<T: 'static>() -> Option<&'static Cloning<T>> {
    capabilities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&std::any::TypeId::of::<T>())
        .and_then(|value| value.cloning)?
        .downcast_ref()
}

pub(super) fn known_copying<T: 'static>() -> Option<&'static Copying<T>> {
    capabilities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&std::any::TypeId::of::<T>())
        .and_then(|value| value.copying)?
        .downcast_ref()
}

pub(super) fn known_comparing<T: 'static>() -> Option<&'static Comparing<T>> {
    capabilities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&std::any::TypeId::of::<T>())
        .and_then(|value| value.comparing)?
        .downcast_ref()
}

type Construct = fn(&Witness, usize) -> RawrResult;

struct Factory {
    policy: std::any::TypeId,
    construct: Construct,
}

fn factories() -> &'static Mutex<BTreeMap<Vec<u8>, Factory>> {
    static FACTORIES: OnceLock<Mutex<BTreeMap<Vec<u8>, Factory>>> = OnceLock::new();
    FACTORIES.get_or_init(Mutex::default)
}

/// Install a concrete native vector factory selected by a generated canonical
/// conversion type. The key includes concrete integer widths and imported
/// definition identities; the runtime logical witness is checked separately.
pub fn register<E: Policy>(key: &[u8]) -> RawrResultOf<()> {
    let policy = std::any::TypeId::of::<Vector<E>>();
    let mut factories = factories()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = factories.get(key) {
        return if existing.policy == policy {
            Ok(())
        } else {
            Err(failure(
                "a native sequence factory key has conflicting concrete policies",
            ))
        };
    }
    factories.insert(
        key.to_vec(),
        Factory {
            policy,
            construct: |witness, capacity| {
                super::owned::<Vector<E>>(Vec::with_capacity(capacity), witness)
            },
        },
    );
    Ok(())
}

/// Construct native ownership at the source constructor, before its allocation
/// history begins. `None` means this runner has no concrete native factory for
/// the checked source type; ordinary owned Array construction remains available.
pub fn construct(key: &[u8], witness: &Witness, capacity: usize) -> RawrResultOf<Option<Dynamic>> {
    let construct = factories()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(key)
        .map(|factory| factory.construct);
    // Never retain the registry lock while executing concrete native code.
    construct
        .map(|construct| construct(witness, capacity))
        .transpose()
}

/// Operations justified by a concrete native element's `Clone` bound.
/// The generator selects this descriptor once; generic sequence dispatch
/// does not impose Clone on lending, moving or dropping elements.
pub struct Cloning<T> {
    vector: fn(&Vec<T>) -> Vec<T>,
    pub(super) fill: fn(&mut [T], T),
    resize: fn(&mut Vec<T>, usize, T),
    extend: fn(&mut Vec<T>, &[T]),
    pub(super) element: fn(&T) -> T,
    pub(super) slice: fn(&[T]) -> Vec<T>,
}

/// Operations justified by a concrete native element's `Copy` bound.
pub struct Copying<T> {
    pub(super) slice: fn(&mut [T], &[T]),
    extend: fn(&mut Vec<T>, &[T]),
}

/// Derive copying operations from Rust's actual `Copy` evidence.
#[must_use]
pub const fn copying<T: Copy>() -> Copying<T> {
    Copying {
        slice: <[T]>::copy_from_slice,
        extend: |target, source| target.extend(source.iter().copied()),
    }
}

/// Operations justified by a concrete native element's `PartialEq` bound.
pub struct Comparing<T> {
    dedup: fn(&mut Vec<T>),
    pub(super) equal: fn(&T, &T) -> bool,
    pub(super) slice: fn(&[T], &[T]) -> bool,
}

/// Derive equality operations from Rust's actual `PartialEq` evidence.
#[must_use]
pub const fn comparing<T: PartialEq>() -> Comparing<T> {
    Comparing {
        dedup: Vec::dedup,
        equal: PartialEq::eq,
        slice: PartialEq::eq,
    }
}

/// Derive a descriptor from Rust's actual `Clone` evidence.
#[must_use]
pub const fn cloning<T: Clone>() -> Cloning<T> {
    Cloning {
        vector: Vec::clone,
        fill: <[T]>::fill,
        resize: Vec::resize,
        extend: Vec::extend_from_slice,
        element: Clone::clone,
        slice: <[T]>::to_vec,
    }
}

/// A vector descriptor whose element's concrete operations are `E`.
pub struct Vector<E>(PhantomData<E>);

impl<E: Policy> Policy for Vector<E> {
    type Value = Vec<E::Value>;
    type Owner = <E::Owner as super::ownership::Owner<E::Value>>::Vector;

    fn accepts(witness: &Witness) -> bool {
        witness
            .sequence_element()
            .is_some_and(|element| E::accepts(&element))
    }

    fn from_dynamic(value: Dynamic, witness: &Witness) -> RawrResultOf<Self::Value> {
        if let Some(place) = get(&value) {
            return super::take::<Self>(&place, witness);
        }
        let element = witness
            .sequence_element()
            .ok_or_else(|| failure("a vector operand has no sequence witness"))?;
        value
            .into_array()
            .map_err(|_| failure("a vector operand is neither an array nor native ownership"))?
            .into_iter()
            .map(|value| E::from_dynamic(value, &element))
            .collect()
    }

    fn into_dynamic(value: Self::Value, witness: &Witness) -> RawrResult {
        super::owned::<Self>(value, witness)
    }

    fn invoke(
        mut value: Access<'_, Self::Value>,
        witness: &Witness,
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        let element = witness
            .sequence_element()
            .ok_or_else(|| failure("a native vector has no element witness"))?;
        match operation {
            Operation::Debug | Operation::Equal { .. } | Operation::Compare { .. } => {
                super::reading::slice::<E>(value.read(), witness, operation, arguments)
            }
            Operation::Len => Ok(Dynamic::from_int(value.read().len() as crate::INT)),
            Operation::Capacity => Ok(Dynamic::from_int(value.read().capacity() as crate::INT)),
            Operation::Store | Operation::Replace => {
                let replacement = Self::from_dynamic(argument(arguments, 0)?, witness)?;
                let place = value.write()?;
                if operation == Operation::Replace {
                    return Self::into_dynamic(std::mem::replace(place, replacement), witness);
                }
                *place = replacement;
                Ok(Dynamic::UNIT)
            }
            Operation::Take => Self::into_dynamic(std::mem::take(value.write()?), witness),
            Operation::Clone => {
                let cloning = E::cloning()
                    .ok_or_else(|| failure("the native vector element does not implement Clone"))?;
                Self::into_dynamic((cloning.vector)(value.read()), witness)
            }
            Operation::Fill => {
                let cloning = E::cloning()
                    .ok_or_else(|| failure("the native vector element does not implement Clone"))?;
                let item = E::from_dynamic(argument(arguments, 0)?, &element)?;
                (cloning.fill)(value.write()?, item);
                Ok(Dynamic::UNIT)
            }
            Operation::Resize => {
                let cloning = E::cloning()
                    .ok_or_else(|| failure("the native vector element does not implement Clone"))?;
                let length = index(arguments, 0)?;
                let item = E::from_dynamic(argument(arguments, 1)?, &element)?;
                (cloning.resize)(value.write()?, length, item);
                Ok(Dynamic::UNIT)
            }
            Operation::Element { mutable } => {
                let at = index(arguments, 0)?;
                let rest = rest.ok_or_else(|| failure("an element loan has no continuation"))?;
                if mutable {
                    let item = value
                        .write()?
                        .get_mut(at)
                        .ok_or_else(|| failure("native sequence index out of bounds"))?;
                    E::lend(item, &element, rest)
                } else {
                    let item = value
                        .read()
                        .get(at)
                        .ok_or_else(|| failure("native sequence index out of bounds"))?;
                    E::lend_ref(item, &element, rest)
                }
            }
            Operation::Slice { mutable } => {
                let range = super::slice::bounds(arguments, value.read().len())?;
                let rest = rest.ok_or_else(|| failure("a slice loan has no continuation"))?;
                if mutable {
                    super::slice::lend::<E>(&mut value.write()?[range], witness, rest)
                } else {
                    super::slice::lend_ref::<E>(&value.read()[range], witness, rest)
                }
            }
            Operation::CopyElement => {
                let at = index(arguments, 0)?;
                let item = value
                    .read()
                    .get(at)
                    .ok_or_else(|| failure("native sequence index out of bounds"))?;
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
                let item = value
                    .write()?
                    .get_mut(at)
                    .ok_or_else(|| failure("native sequence index out of bounds"))?;
                *item = replacement;
                Ok(Dynamic::UNIT)
            }
            Operation::Push => {
                let item = E::from_dynamic(argument(arguments, 0)?, &element)?;
                value.write()?.push(item);
                Ok(Dynamic::UNIT)
            }
            Operation::Pop => match value.write()?.pop() {
                Some(item) => E::into_dynamic(item, &element).map(crate::option::some),
                None => Ok(crate::option::none()),
            },
            Operation::Insert => {
                let at = index(arguments, 0)?;
                let item = E::from_dynamic(argument(arguments, 1)?, &element)?;
                let value = value.write()?;
                if at > value.len() {
                    return Err(failure("native sequence insertion index out of bounds"));
                }
                value.insert(at, item);
                Ok(Dynamic::UNIT)
            }
            Operation::Remove | Operation::SwapRemove => {
                let at = index(arguments, 0)?;
                let value = value.write()?;
                if at >= value.len() {
                    return Err(failure("native sequence removal index out of bounds"));
                }
                E::into_dynamic(
                    if operation == Operation::Remove {
                        value.remove(at)
                    } else {
                        value.swap_remove(at)
                    },
                    &element,
                )
            }
            Operation::Swap => {
                let (left, right) = (index(arguments, 0)?, index(arguments, 1)?);
                let value = value.write()?;
                if left >= value.len() || right >= value.len() {
                    return Err(failure("native sequence swap index out of bounds"));
                }
                value.swap(left, right);
                Ok(Dynamic::UNIT)
            }
            Operation::Reverse => {
                value.write()?.reverse();
                Ok(Dynamic::UNIT)
            }
            Operation::Clear => {
                value.write()?.clear();
                Ok(Dynamic::UNIT)
            }
            Operation::Truncate => {
                let length = index(arguments, 0)?;
                value.write()?.truncate(length);
                Ok(Dynamic::UNIT)
            }
            Operation::Append => {
                let donor = argument(arguments, 0)?;
                let donor = get(&donor)
                    .ok_or_else(|| failure("a native append donor must be a checked place"))?;
                super::native::access::<Self, _>(&donor, witness, |donor| {
                    value.write()?.append(donor);
                    Ok(Dynamic::UNIT)
                })
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
            Operation::Extend(mode) => {
                let source = argument(arguments, 0)?;
                if let Some(source) = get(&source) {
                    if mode == ExtendMode::Move {
                        let source = super::take::<Self>(&source, witness)?;
                        value.write()?.extend(source);
                    } else {
                        super::slice::reach::<E, _>(&source, witness, false, |source| {
                            match mode {
                                ExtendMode::Clone => {
                                    let cloning = E::cloning().ok_or_else(|| {
                                        failure("the native element does not implement Clone")
                                    })?;
                                    (cloning.extend)(value.write()?, source.read());
                                }
                                ExtendMode::Copy => {
                                    let copying = E::copying().ok_or_else(|| {
                                        failure("the native element does not implement Copy")
                                    })?;
                                    (copying.extend)(value.write()?, source.read());
                                }
                                ExtendMode::Move => unreachable!(),
                            }
                            Ok(())
                        })?;
                    }
                } else if mode == ExtendMode::Move {
                    let source = source
                        .into_array()
                        .map_err(|_| failure("native extend requires a sequence"))?;
                    let target = value.write()?;
                    super::callback::run(|abort| {
                        target.extend(
                            source
                                .into_iter()
                                .map(|item| abort.value(E::from_dynamic(item, &element))),
                        );
                    })?;
                } else {
                    let source = source
                        .read_lock::<crate::Array>()
                        .ok_or_else(|| failure("native extend requires a sequence"))?;
                    let target = value.write()?;
                    super::callback::run(|abort| {
                        target.extend(source.iter().map(|item| {
                            abort.value(match mode {
                                ExtendMode::Copy => E::copy_dynamic(item, &element),
                                ExtendMode::Clone => E::clone_dynamic(item, &element),
                                ExtendMode::Move => unreachable!(),
                            })
                        }));
                    })?;
                }
                Ok(Dynamic::UNIT)
            }
            Operation::SplitOff => {
                let at = index(arguments, 0)?;
                let value = value.write()?;
                if at > value.len() {
                    return Err(failure("native sequence split index out of bounds"));
                }
                Self::into_dynamic(value.split_off(at), witness)
            }
            Operation::Drain => {
                let low = index(arguments, 0)?;
                let high = arguments
                    .get(1)
                    .and_then(|value| value.as_int().ok())
                    .ok_or_else(|| failure("a drain range has no upper bound"))?;
                let inclusive = arguments
                    .get(2)
                    .and_then(|value| value.as_bool().ok())
                    .ok_or_else(|| failure("a drain range has no inclusivity"))?;
                let value = value.write()?;
                let high = if high == -1 {
                    value.len()
                } else {
                    usize::try_from(high)
                        .ok()
                        .and_then(|high| high.checked_add(usize::from(inclusive)))
                        .ok_or_else(|| failure("a drain range has an invalid upper bound"))?
                };
                if low > high || high > value.len() {
                    return Err(failure("native drain range is out of bounds"));
                }
                Self::into_dynamic(value.drain(low..high).collect(), witness)
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
                if let Some(source) = get(&source) {
                    super::slice::reach::<E, _>(&source, witness, false, |source| {
                        copy(source.read())
                    })
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
            Operation::Dedup => {
                let comparing = E::comparing().ok_or_else(|| {
                    failure("the native vector element does not implement PartialEq")
                })?;
                (comparing.dedup)(value.write()?);
                Ok(Dynamic::UNIT)
            }
            Operation::Sort { stable } => {
                super::reading::sort::<E>(value.write()?, stable)?;
                Ok(Dynamic::UNIT)
            }
            Operation::ResizeWith => {
                let length = index(arguments, 0)?;
                let callable = super::callback::Callable::new(argument(arguments, 1)?)?;
                let value = value.write()?;
                super::callback::run(|abort| {
                    value.resize_with(length, || {
                        abort.value(
                            callable
                                .call(&mut [])
                                .and_then(|value| E::from_dynamic(value, &element)),
                        )
                    });
                })?;
                Ok(Dynamic::UNIT)
            }
            Operation::Retain => {
                let callable = super::callback::Callable::new(argument(arguments, 0)?)?;
                let value = value.write()?;
                super::callback::run(|abort| {
                    value.retain(|item| {
                        let accepted =
                            E::lend_ref(item, &element, &mut |item| callable.call(&mut [item]));
                        abort.value(accepted.and_then(|value| {
                            value
                                .as_bool()
                                .map_err(|_| failure("a retain predicate did not return bool"))
                        }))
                    });
                })?;
                Ok(Dynamic::UNIT)
            }
            Operation::SortBy { stable } => {
                let callback = argument(arguments, 0)?;
                sort_by::<E>(value.write()?, &element, callback, stable)
            }
            _ => super::reading::invoke::<Self>(value, witness, operation, arguments),
        }
    }
}

pub(super) fn sort_by<E: Policy>(
    value: &mut [E::Value],
    element: &Witness,
    callback: Dynamic,
    stable: bool,
) -> RawrResult {
    let callable = super::callback::Callable::new(callback)?;
    super::callback::run(|abort| {
        let compare = |left: &E::Value, right: &E::Value| {
            let compared = E::lend_ref(left, element, &mut |left| {
                E::lend_ref(right, element, &mut |right| {
                    callable.call(&mut [left.clone(), right])
                })
            });
            abort.value(compared.and_then(|value| {
                value
                    .as_int()
                    .map(|ordering| ordering.cmp(&0))
                    .map_err(|_| failure("a sort comparator did not return Ordering"))
            }))
        };
        if stable {
            value.sort_by(compare);
        } else {
            value.sort_unstable_by(compare);
        }
    })?;
    Ok(Dynamic::UNIT)
}
