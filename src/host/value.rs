//! What Rawr runs on a facade's host values natively: `==` by the type's own
//! `PartialEq` ([`facade::Builder::value_type`](super::facade::Builder::value_type)),
//! ordering, `Default`, `Debug` and `Display`.
//!
//! A host value is an opaque Rust value inside a `Dynamic`, so each is the
//! function compiled for its type, looked up by `TypeId` when used. What a
//! type declared is one row of one table. An imported type's `Default` is
//! looked up by the type's identity in the same table's second map.
//!
//! Registration has no fixed end: a runner registers at its start, facade
//! builders register as bindings load, lowering registers during a compile
//! and some types register at first use. So the table stays behind one
//! `RwLock`, which every host value's `==`, `<`, `Debug`, `Display` and
//! `Default` reads on any thread.

use crate::Dynamic;
use std::any::{Any, TypeId};
use std::{collections::BTreeMap, sync::RwLock};

type Equal = fn(&Dynamic, &Dynamic) -> Option<bool>;
type Compare = fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>;
/// The text of a value of the row's type, `None` for a value of another.
pub(super) type Text = fn(&dyn Any) -> Option<String>;

/// What one host value type declared.
#[derive(Clone, Copy, Default)]
pub(super) struct Declared {
    pub(super) debug: Option<Text>,
    pub(super) display: Option<Text>,
    equal: Option<Equal>,
    compare: Option<Compare>,
    default: Option<fn() -> Dynamic>,
}

type MakeDefault = std::sync::Arc<dyn Fn() -> Dynamic + Send + Sync>;

/// Every host value type's declarations.
struct Table {
    /// One row per host value type.
    declared: BTreeMap<TypeId, Declared>,
    /// Imported types' `Default`, by the type's identity.
    imported_defaults: BTreeMap<[u8; 32], MakeDefault>,
}

static TABLE: RwLock<Table> = RwLock::new(Table {
    declared: BTreeMap::new(),
    imported_defaults: BTreeMap::new(),
});

/// Change the row of `ty` with `declare`.
pub(super) fn declare(ty: TypeId, declare: impl FnOnce(&mut Declared)) {
    if let Ok(mut table) = TABLE.write() {
        declare(table.declared.entry(ty).or_default());
    }
}

/// The row of `ty`, when it declared anything.
pub(super) fn declared(ty: TypeId) -> Option<Declared> {
    TABLE.read().ok()?.declared.get(&ty).copied()
}

/// Let a derived `Default` build a host value field by the type's own
/// `Default`.
pub(crate) fn register_default<T: Default + Clone + crate::func::SendSync + 'static>() {
    declare(TypeId::of::<T>(), |declared| {
        declared.default = Some(|| Dynamic::from(T::default()));
    });
}

/// The `Default` value of the host type `ty`, when it declared one.
pub(crate) fn host_default(ty: TypeId) -> Option<Dynamic> {
    declared(ty)?.default.map(|default| default())
}

/// Let a derived `Default` build a field of the imported type `identity`
/// by the crate's own `Default`, as a generated runner calls it with the
/// `Default` rustc proved for the type.
#[doc(hidden)]
pub fn register_imported_default(
    identity: [u8; 32],
    default: impl Fn() -> Dynamic + Send + Sync + 'static,
) {
    if let Ok(mut table) = TABLE.write() {
        table
            .imported_defaults
            .insert(identity, std::sync::Arc::new(default));
    }
}

/// The `Default` value of the imported type `identity`, when a runner
/// registered one.
pub(crate) fn imported_default(identity: &[u8; 32]) -> Option<Dynamic> {
    // The lock is let go before the default runs, so a default that
    // builds a host value can read the table again.
    let default = std::sync::Arc::clone(TABLE.read().ok()?.imported_defaults.get(identity)?);
    Some(default())
}

fn equal<T: PartialEq + Clone + 'static>(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    Some(*lhs.read_lock::<T>()? == *rhs.read_lock::<T>()?)
}

fn compare<T: PartialOrd + Clone + 'static>(
    lhs: &Dynamic,
    rhs: &Dynamic,
) -> Option<std::cmp::Ordering> {
    lhs.read_lock::<T>()?.partial_cmp(&*rhs.read_lock::<T>()?)
}

pub(crate) fn register_equality<T: PartialEq + Clone + 'static>() {
    declare(TypeId::of::<T>(), |declared| {
        declared.equal = Some(equal::<T>)
    });
}

/// Let the values of the host type `T` order by its own `PartialOrd`, as a
/// facade's ordered std type does: `<` and `Ord::cmp` of them.
pub(crate) fn register_ordering<T: PartialOrd + Clone + 'static>() {
    declare(TypeId::of::<T>(), |declared| {
        declared.compare = Some(compare::<T>)
    });
}

/// Let the values of the host type `T` compare: `==`, and the order a
/// hashed collection keeps them in. A generated bridge registers its
/// handles, which compare by the imported values they stand for.
#[doc(hidden)]
pub fn register_relations<T: 'static>(
    equal: fn(&Dynamic, &Dynamic) -> Option<bool>,
    compare: fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>,
) {
    declare(TypeId::of::<T>(), |declared| {
        declared.equal = Some(equal);
        declared.compare = Some(compare);
    });
}

/// The order of two host values of one type that registered one.
pub(crate) fn host_compare(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
    {
        let ty = lhs.type_id();
        if ty != rhs.type_id() {
            return None;
        }
        declared(ty)?.compare?(lhs, rhs)
    }
}

/// `lhs == rhs` by their type's `PartialEq`, when both are host values of
/// one type that declared it.
pub(crate) fn host_equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<bool> {
    {
        let scoped = |value: &Dynamic| value.is::<crate::scope::Scoped>();
        let lent = |value: &Dynamic| value.is::<crate::scope::Lent>();
        // A value in a scope frame's region compares with one of its type
        // in a runner's arena (D558 tier 1), and so does a value native
        // code lent a script function (D558.3).
        let ty = if scoped(lhs) || scoped(rhs) {
            TypeId::of::<crate::scope::Scoped>()
        } else if lent(lhs) || lent(rhs) {
            TypeId::of::<crate::scope::Lent>()
        } else {
            let ty = lhs.type_id();
            if ty != rhs.type_id() {
                return None;
            }
            ty
        };
        declared(ty)?.equal?(lhs, rhs)
    }
}

/// `lhs == rhs` as a derived `PartialEq` compares them: field by field,
/// host values by their own `PartialEq`.
pub fn structural_equal(lhs: &Dynamic, rhs: &Dynamic) -> bool {
    crate::resolve::lower_nominal::equal_value(lhs, rhs)
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
