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
//!
//! A stored API names its units by key instead ([`Keyed`], #2166). A key
//! is the unit's stable crate id, which no other unit of a build has, so
//! it does not depend on which other units an encoding names: a demand
//! round that names one more unit than the last changes no reference to
//! the others.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::de::Error as _;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// Units are written by key.
    Naming(Naming),
    /// The units of the encoding being read, by key.
    Named(BTreeMap<Box<str>, Arc<Unit>>),
}

/// Units by key, each written once beside the encodings that name them by
/// key. A unit's key is its stable crate id; a unit whose id another unit
/// of the table already has takes the id with `#` and a count.
#[derive(Clone, Default)]
pub(crate) struct Keyed {
    units: BTreeMap<Box<str>, Arc<Unit>>,
}

/// The keys an encoding under way names its units by.
struct Naming {
    /// The units of the table the encoding is written beside, with their
    /// keys.
    known: BTreeMap<Arc<Unit>, Box<str>>,
    /// Units the table does not hold, by the keys they took.
    taken: BTreeMap<Box<str>, Arc<Unit>>,
    /// The key of each unit `taken` holds.
    named: BTreeMap<Arc<Unit>, Box<str>>,
    /// Each allocation already named, with its key.
    by_allocation: HashMap<*const Unit, Box<str>>,
    /// Each allocation `by_allocation` names, so none is freed and its
    /// address reused while the table lives.
    held: Vec<Arc<Unit>>,
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
    // Writing a place, a key or a whole unit never comes back to the
    // table: a unit holds no unit.
    TABLE.with(|current| match &mut *current.borrow_mut() {
        Some(Table::Writing {
            by_allocation,
            by_value,
            units,
            held,
        }) => serializer.serialize_u32(place(by_allocation, by_value, units, held, unit)),
        Some(Table::Naming(naming)) => serializer.serialize_str(naming.key(unit)),
        _ => unit.as_ref().serialize(serializer),
    })
}

/// The `serde(with)` form of a definition's unit.
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Unit>, D::Error> {
    enum Form {
        Whole,
        Place,
        Key,
    }
    let form = TABLE.with(|current| match &*current.borrow() {
        Some(Table::Reading(_)) => Form::Place,
        Some(Table::Named(_)) => Form::Key,
        _ => Form::Whole,
    });
    match form {
        Form::Whole => return Unit::deserialize(deserializer).map(Arc::new),
        Form::Key => return deserializer.deserialize_any(Key),
        Form::Place => {}
    }
    let place = u32::deserialize(deserializer)?;
    TABLE
        .with(|current| match &*current.borrow() {
            Some(Table::Reading(units)) => units.get(place as usize).cloned(),
            _ => None,
        })
        .ok_or_else(|| D::Error::custom(format!("no unit has place {place} in the table")))
}

/// A unit read by its key, or whole where its encoding wrote it whole.
struct Key;

impl<'de> serde::de::Visitor<'de> for Key {
    type Value = Arc<Unit>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a compiler unit or its key")
    }

    fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<Arc<Unit>, E> {
        TABLE
            .with(|current| match &*current.borrow() {
                Some(Table::Named(units)) => units.get(key).cloned(),
                _ => None,
            })
            .ok_or_else(|| E::custom(format!("no unit has key {key} in the table")))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Arc<Unit>, A::Error> {
        Unit::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Arc::new)
    }
}

impl Keyed {
    /// Run `encode`, which serializes, with each unit written as its key:
    /// the key this table holds it under, or one it takes. Returns what
    /// `encode` returned and the table of the units that took keys.
    pub(crate) fn write<T>(&self, encode: impl FnOnce() -> T) -> (T, Self) {
        let scope = Scope::enter(Table::Naming(Naming {
            known: self
                .units
                .iter()
                .map(|(key, unit)| (Arc::clone(unit), key.clone()))
                .collect(),
            taken: BTreeMap::new(),
            named: BTreeMap::new(),
            by_allocation: HashMap::new(),
            held: Vec::new(),
        }));
        let encoded = encode();
        let units = match scope.finish() {
            Some(Table::Naming(naming)) => naming.taken,
            _ => BTreeMap::new(),
        };
        (encoded, Self { units })
    }

    /// Run `decode`, which deserializes encodings that name units by this
    /// table's keys. Every reference to one unit shares its allocation.
    pub(crate) fn read<T>(&self, decode: impl FnOnce() -> T) -> T {
        let scope = Scope::enter(Table::Named(self.units.clone()));
        let decoded = decode();
        drop(scope);
        decoded
    }

    /// Close `document`, an encoded JSON object, with this table as its
    /// last field, `units`.
    pub(crate) fn close(&self, document: &mut Vec<u8>) -> serde_json::Result<()> {
        if document.pop() != Some(b'}') {
            return Err(serde::ser::Error::custom("a unit table closes an object"));
        }
        if document.len() > 1 {
            document.push(b',');
        }
        document.extend_from_slice(br#""units":"#);
        serde_json::to_writer(&mut *document, self)?;
        document.push(b'}');
        Ok(())
    }

    /// This table and `other`, which took keys this one does not hold:
    /// `None` when they hold one key both.
    pub(crate) fn with(mut self, other: Self) -> Option<Self> {
        for (key, unit) in other.units {
            if self.units.insert(key, unit).is_some() {
                return None;
            }
        }
        Some(self)
    }
}

impl Serialize for Keyed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.units.len()))?;
        for (key, unit) in &self.units {
            map.serialize_entry(key, unit.as_ref())?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Keyed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let units = BTreeMap::<Box<str>, Unit>::deserialize(deserializer)?;
        Ok(Self {
            units: units
                .into_iter()
                .map(|(key, unit)| (key, Arc::new(unit)))
                .collect(),
        })
    }
}

impl Naming {
    /// The key `unit` is written as: the one its allocation or an equal
    /// unit already has, or else its stable crate id, with `#` and a count
    /// when another unit has that.
    fn key(&mut self, unit: &Arc<Unit>) -> &str {
        let pointer = Arc::as_ptr(unit);
        if !self.by_allocation.contains_key(&pointer) {
            let key = match self.known.get(unit).or_else(|| self.named.get(unit)) {
                Some(key) => key.clone(),
                None => self.take(unit),
            };
            self.held.push(Arc::clone(unit));
            self.by_allocation.insert(pointer, key);
        }
        &self.by_allocation[&pointer]
    }

    fn take(&mut self, unit: &Arc<Unit>) -> Box<str> {
        let id = unit.stable_crate_id();
        let mut key: Box<str> = id.into();
        let mut count = 1_usize;
        while self.taken.contains_key(&key) || self.known.values().any(|known| *known == key) {
            key = format!("{id}#{count}").into();
            count += 1;
        }
        self.taken.insert(key.clone(), Arc::clone(unit));
        self.named.insert(Arc::clone(unit), key.clone());
        key
    }
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
