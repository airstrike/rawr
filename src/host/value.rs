//! What Rawr runs on a facade's host values natively: `==` by the type's own
//! `PartialEq` ([`facade::Builder::value_type`](super::facade::Builder::value_type)),
//! and the bytes that carry one into the next runner process
//! ([`facade::Builder::carried`](super::facade::Builder::carried)).
//!
//! A host value is an opaque Rust value inside a `Dynamic`, so both are the
//! functions compiled for its type, looked up by `TypeId` (or, for carried
//! bytes, by the type's name) when used.

use crate::Dynamic;
use std::{any::TypeId, collections::BTreeMap, sync::RwLock};

/// How a host value type whose values are plain data is carried into the
/// next runner process: the bytes of a value, and the value they encode.
/// A facade implements it on a type of its own, so it can carry a type of
/// another crate's.
pub trait Carry: 'static {
    /// The carried host value type.
    type Value: Clone + crate::func::SendSync + 'static;
    /// Append the bytes of `value`.
    fn encode(value: &Self::Value, out: &mut Vec<u8>);
    /// The value `bytes` encode, or `None` when they are malformed.
    fn decode(bytes: &[u8]) -> Option<Self::Value>;
}

type Equal = fn(&Dynamic, &Dynamic) -> Option<bool>;
type Encode = fn(&Dynamic, &mut Vec<u8>) -> Option<()>;
type Decode = fn(&[u8]) -> Option<Dynamic>;

static EQUALITIES: RwLock<BTreeMap<TypeId, Equal>> = RwLock::new(BTreeMap::new());
static ENCODERS: RwLock<BTreeMap<TypeId, (&'static str, Encode)>> = RwLock::new(BTreeMap::new());
static DECODERS: RwLock<BTreeMap<&'static str, Decode>> = RwLock::new(BTreeMap::new());

type Compare = fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>;

static ORDERINGS: RwLock<BTreeMap<TypeId, Compare>> = RwLock::new(BTreeMap::new());

static DEFAULTS: RwLock<BTreeMap<TypeId, fn() -> Dynamic>> = RwLock::new(BTreeMap::new());

/// Let a derived `Default` build a host value field by the type's own
/// `Default`.
pub(crate) fn register_default<T: Default + Clone + crate::func::SendSync + 'static>() {
    if let Ok(mut defaults) = DEFAULTS.write() {
        defaults.insert(TypeId::of::<T>(), || Dynamic::from(T::default()));
    }
}

/// The `Default` value of the host type `ty`, when it declared one.
pub(crate) fn host_default(ty: TypeId) -> Option<Dynamic> {
    DEFAULTS.read().ok()?.get(&ty).map(|default| default())
}

fn equal<T: PartialEq + Clone + 'static>(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    Some(*lhs.read_lock::<T>()? == *rhs.read_lock::<T>()?)
}

fn encode<T: Carry>(value: &Dynamic, out: &mut Vec<u8>) -> Option<()> {
    T::encode(&*value.read_lock::<T::Value>()?, out);
    Some(())
}

fn decode<T: Carry>(bytes: &[u8]) -> Option<Dynamic> {
    T::decode(bytes).map(Dynamic::from)
}

pub(crate) fn register_equality<T: PartialEq + Clone + 'static>() {
    if let Ok(mut equalities) = EQUALITIES.write() {
        equalities.insert(TypeId::of::<T>(), equal::<T>);
    }
}

/// Let the values of the host type `T` compare: `==`, and the order a
/// hashed collection keeps them in. A generated bridge registers its
/// handles, which compare by the imported values they stand for.
#[doc(hidden)]
pub fn register_relations<T: 'static>(
    equal: fn(&Dynamic, &Dynamic) -> Option<bool>,
    compare: fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>,
) {
    if let (Ok(mut equalities), Ok(mut orderings)) = (EQUALITIES.write(), ORDERINGS.write()) {
        equalities.insert(TypeId::of::<T>(), equal);
        orderings.insert(TypeId::of::<T>(), compare);
    }
}

/// The order of two host values of one type that registered one.
pub(crate) fn host_compare(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
    {
        let ty = lhs.type_id();
        if ty != rhs.type_id() {
            return None;
        }
        let compare = *ORDERINGS.read().ok()?.get(&ty)?;
        compare(lhs, rhs)
    }
}

pub(crate) fn register_carry<T: Carry>() {
    if let (Ok(mut encoders), Ok(mut decoders)) = (ENCODERS.write(), DECODERS.write()) {
        let name = std::any::type_name::<T::Value>();
        encoders.insert(TypeId::of::<T::Value>(), (name, encode::<T>));
        decoders.insert(name, decode::<T>);
    }
}

/// `lhs == rhs` by their type's `PartialEq`, when both are host values of
/// one type that declared it.
pub(crate) fn host_equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    {
        let ty = lhs.type_id();
        if ty != rhs.type_id() {
            return None;
        }
        let equal = *EQUALITIES.read().ok()?.get(&ty)?;
        equal(lhs, rhs)
    }
}

/// `lhs == rhs` as a derived `PartialEq` compares them: field by field,
/// host values by their own `PartialEq`.
pub fn structural_equal(lhs: &Dynamic, rhs: &Dynamic) -> bool {
    crate::resolve::lower_nominal::equal_value(lhs, rhs)
}

/// The carried type's name and the bytes of `value`, when it is a host
/// value whose type has a [`Carry`].
pub(crate) fn encode_host(value: &Dynamic, out: &mut Vec<u8>) -> Option<&'static str> {
    {
        let (name, encode) = *ENCODERS.read().ok()?.get(&value.type_id())?;
        encode(value, out)?;
        Some(name)
    }
}

/// The host value of the type named `name` that `bytes` encode.
pub(crate) fn decode_host(name: &str, bytes: &[u8]) -> Option<Dynamic> {
    {
        let decode = *DECODERS.read().ok()?.get(name)?;
        decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{host_compare, host_equal, register_relations};
    use crate::Dynamic;

    /// A bridge's handle, which compares by what it stands for.
    #[derive(Clone, Copy)]
    struct Handle(u8);

    #[test]
    fn imported_values_compare_by_the_relations_their_bridge_registered() {
        fn equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
            Some(lhs.read_lock::<Handle>()?.0 == rhs.read_lock::<Handle>()?.0)
        }
        fn compare(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
            Some(
                lhs.read_lock::<Handle>()?
                    .0
                    .cmp(&rhs.read_lock::<Handle>()?.0),
            )
        }
        register_relations::<Handle>(equal, compare);
        let (one, two) = (Dynamic::from(Handle(1)), Dynamic::from(Handle(2)));
        assert_eq!(host_equal(&one, &one.clone()), Some(true));
        assert_eq!(host_equal(&one, &two), Some(false));
        assert_eq!(host_compare(&two, &one), Some(std::cmp::Ordering::Greater));
        assert_eq!(host_compare(&one, &Dynamic::from(1_u8)), None);
    }
}
