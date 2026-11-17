//! Compiler units an encoding writes once (#2153).
//!
//! Every definition names its unit, and a unit carries its package, its
//! target's path and every feature it was compiled with: thousands of
//! bytes for a crate with many features. Written whole beside each
//! definition, a crate's units are nearly all of its encoding. Inside
//! [`write`], a definition writes its unit as its place in a table the
//! closure's caller writes once beside the encoding; inside [`read`], it
//! reads that place, and every definition of one unit shares one
//! allocation. Outside both, a unit is written and read whole.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize as _, Deserializer, Serialize as _, Serializer};

use super::identity::Unit;

/// The table of the encoding under way on this thread.
enum Table {
    /// Units take places as an encoding first names them. A unit is found
    /// by its allocation, and an equal unit in another allocation takes
    /// the same place.
    Writing {
        by_allocation: HashMap<*const Unit, u32>,
        by_value: BTreeMap<Arc<Unit>, u32>,
        units: Vec<Arc<Unit>>,
        /// Each allocation `by_allocation` names, so none is freed and its
        /// address reused while the table lives.
        held: Vec<Arc<Unit>>,
    },
    /// The units of the encoding being read, by place.
    Reading(Box<[Arc<Unit>]>),
}

std::thread_local! {
    static TABLE: RefCell<Option<Table>> = const { RefCell::new(None) };
}

/// Puts back the table that was current when it was made, as its scope
/// ends, by return or unwind.
struct Scope {
    /// The table to put back, while it is not back yet.
    previous: Option<Option<Table>>,
}

impl Scope {
    fn enter(table: Table) -> Self {
        Self {
            previous: Some(TABLE.with(|current| current.replace(Some(table)))),
        }
    }

    /// Put the previous table back and return this scope's.
    fn finish(mut self) -> Option<Table> {
        let previous = self.previous.take().flatten();
        TABLE.with(|current| current.replace(previous))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            TABLE.with(|current| current.replace(previous));
        }
    }
}

/// Run `encode`, which serializes, with each unit written as its place in
/// a table. Returns what it returned and the table, by place.
pub(crate) fn write<T>(encode: impl FnOnce() -> T) -> (T, Box<[Arc<Unit>]>) {
    let scope = Scope::enter(Table::Writing {
        by_allocation: HashMap::new(),
        by_value: BTreeMap::new(),
        units: Vec::new(),
        held: Vec::new(),
    });
    let encoded = encode();
    let units = match scope.finish() {
        Some(Table::Writing { units, .. }) => units.into_boxed_slice(),
        _ => Box::default(),
    };
    (encoded, units)
}

/// Run `decode`, which deserializes an encoding [`write`] made, with each
/// unit read as its place in `units`.
pub(crate) fn read<T>(units: Box<[Arc<Unit>]>, decode: impl FnOnce() -> T) -> T {
    let scope = Scope::enter(Table::Reading(units));
    let decoded = decode();
    drop(scope);
    decoded
}

/// The `serde(with)` form of a definition's unit.
pub(super) fn serialize<S: Serializer>(unit: &Arc<Unit>, serializer: S) -> Result<S::Ok, S::Error> {
    let place = TABLE.with(|current| match &mut *current.borrow_mut() {
        Some(Table::Writing {
            by_allocation,
            by_value,
            units,
            held,
        }) => Some(place(by_allocation, by_value, units, held, unit)),
        _ => None,
    });
    match place {
        Some(place) => serializer.serialize_u32(place),
        None => unit.as_ref().serialize(serializer),
    }
}

/// The `serde(with)` form of a definition's unit.
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Unit>, D::Error> {
    let reading = TABLE.with(|current| matches!(&*current.borrow(), Some(Table::Reading(_))));
    if !reading {
        return Unit::deserialize(deserializer).map(Arc::new);
    }
    let place = u32::deserialize(deserializer)?;
    TABLE
        .with(|current| match &*current.borrow() {
            Some(Table::Reading(units)) => units.get(place as usize).cloned(),
            _ => None,
        })
        .ok_or_else(|| D::Error::custom(format!("no unit has place {place} in the table")))
}

fn place(
    by_allocation: &mut HashMap<*const Unit, u32>,
    by_value: &mut BTreeMap<Arc<Unit>, u32>,
    units: &mut Vec<Arc<Unit>>,
    held: &mut Vec<Arc<Unit>>,
    unit: &Arc<Unit>,
) -> u32 {
    if let Some(place) = by_allocation.get(&Arc::as_ptr(unit)) {
        return *place;
    }
    let place = *by_value.entry(Arc::clone(unit)).or_insert_with(|| {
        units.push(Arc::clone(unit));
        u32::try_from(units.len() - 1).expect("an encoding names fewer than 2^32 units")
    });
    held.push(Arc::clone(unit));
    by_allocation.insert(Arc::as_ptr(unit), place);
    place
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::identity::{Authority, Compilation, Definition, Namespace, Package, Unit};

    fn unit(name: &str) -> Unit {
        Unit::new(
            Authority::Cargo(Package::new(
                format!("registry+index#{name}@1.0.0").into(),
                name.into(),
                "1.0.0".into(),
                "registry+index".into(),
            )),
            format!("/src/{name}/lib.rs").into(),
            "lib".into(),
            name.into(),
            Compilation::Target,
            Box::new([format!("feature_of_{name}").into()]),
            format!("{name}-id").into(),
        )
    }

    /// Definitions of one unit, even from two allocations of it, write the
    /// unit once and read back sharing one allocation; outside a table each
    /// writes its unit whole.
    #[test]
    fn an_encoding_writes_each_unit_once_and_reads_it_shared() {
        let definitions = [
            Definition::new(unit("alpha"), "1".into(), Namespace::Type),
            Definition::new(unit("alpha"), "2".into(), Namespace::Value),
            Definition::new(unit("beta"), "3".into(), Namespace::Type),
        ];
        let (encoded, units) = super::write(|| serde_json::to_string(&definitions));
        let encoded = encoded.unwrap();
        assert_eq!(units.len(), 2, "alpha and beta each take one place");
        assert!(
            !encoded.contains("feature_of_"),
            "no unit is written inline"
        );
        let read: Vec<Definition> = super::read(units, || serde_json::from_str(&encoded)).unwrap();
        assert_eq!(read, definitions);
        assert!(Arc::ptr_eq(read[0].shared_unit(), read[1].shared_unit()));

        let whole = serde_json::to_string(&definitions).unwrap();
        assert_eq!(whole.matches("feature_of_alpha").count(), 2);
        let read: Vec<Definition> = serde_json::from_str(&whole).unwrap();
        assert_eq!(read, definitions);
    }
}
