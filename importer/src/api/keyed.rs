//! Values a stored API writes once, in a table, and names by key
//! (#2166, #2169).
//!
//! Many entries of a stored API share one value: every definition of a
//! crate names its compiler unit, and every export through one `pub use`
//! names that re-export hop with its span. Inside [`Keyed::write`], a field
//! that holds such a value as `Arc<T>` and serializes `with` this module
//! writes the value's key; the value takes a key there the first time an
//! encoding names it, and the table that comes back holds it once. Inside
//! [`Keyed::read`], the field reads its key back, and every reference to one
//! value shares one allocation. Outside both, the value is written and read
//! whole.
//!
//! A key is the value's own stable id ([`Entry::id`]), which no other value
//! of a build has, so it does not depend on which other values an encoding
//! names: a demand round that names one more value than the last changes no
//! reference to the others. A value whose id another value of the table
//! already has takes the id with `#` and a count.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::thread::LocalKey;

use serde::de::DeserializeOwned;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "rustc-driver")]
use serde_json::value::RawValue;

use super::identity::Unit;
use super::ledger::Hop;

/// A value a table holds once.
pub(crate) trait Entry: Ord + Serialize + DeserializeOwned + 'static {
    /// The key the value takes when no other value of its table has it.
    fn id(&self) -> &str;

    /// The scope of this type's encoding under way on this thread.
    fn scope() -> &'static LocalKey<RefCell<Option<Mode<Self>>>>;
}

/// What a scope does with the values it meets.
pub(crate) enum Mode<T> {
    /// Values are written by key.
    Naming(Naming<T>),
    /// The values of the encoding being read, by key.
    Named(BTreeMap<Box<str>, Arc<T>>),
}

/// Values by key, each written once beside the encodings that name them.
pub(crate) struct Keyed<T> {
    values: BTreeMap<Box<str>, Arc<T>>,
}

/// The keys an encoding under way names its values by.
pub(crate) struct Naming<T> {
    /// The table the encoding is written beside.
    known: BTreeMap<Box<str>, Arc<T>>,
    /// The key of each value `known` holds.
    keys: BTreeMap<Arc<T>, Box<str>>,
    /// Values the table does not hold, by the keys they took.
    taken: BTreeMap<Box<str>, Arc<T>>,
    /// The key of each value `taken` holds.
    named: BTreeMap<Arc<T>, Box<str>>,
    /// Each allocation already named, with its key.
    by_allocation: HashMap<*const T, Box<str>>,
    /// Each allocation `by_allocation` names, so none is freed and its
    /// address reused while the scope lives.
    held: Vec<Arc<T>>,
}

/// Puts back the scope that was current when it was made, as its own ends,
/// by return or unwind.
struct Scope<T: Entry> {
    /// The scope to put back, while it is not back yet.
    previous: Option<Option<Mode<T>>>,
}

impl<T: Entry> Scope<T> {
    fn enter(mode: Mode<T>) -> Self {
        Self {
            previous: Some(T::scope().with(|current| current.replace(Some(mode)))),
        }
    }

    /// Put the previous scope back and return this one's.
    fn finish(mut self) -> Option<Mode<T>> {
        let previous = self.previous.take().flatten();
        T::scope().with(|current| current.replace(previous))
    }
}

impl<T: Entry> Drop for Scope<T> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            T::scope().with(|current| current.replace(previous));
        }
    }
}

impl<T> Default for Keyed<T> {
    fn default() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }
}

impl<T> Clone for Keyed<T> {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
        }
    }
}

impl<T: Entry> Keyed<T> {
    /// Run `encode`, which serializes, with each value written as its key:
    /// the key this table holds it under, or one it takes. Returns what
    /// `encode` returned and the table of the values that took keys.
    pub(crate) fn write<R>(&self, encode: impl FnOnce() -> R) -> (R, Self) {
        let scope = Scope::enter(Mode::Naming(Naming {
            known: self.values.clone(),
            keys: self
                .values
                .iter()
                .map(|(key, value)| (Arc::clone(value), key.clone()))
                .collect(),
            taken: BTreeMap::new(),
            named: BTreeMap::new(),
            by_allocation: HashMap::new(),
            held: Vec::new(),
        }));
        let encoded = encode();
        let values = match scope.finish() {
            Some(Mode::Naming(naming)) => naming.taken,
            _ => BTreeMap::new(),
        };
        (encoded, Self { values })
    }

    /// Run `decode`, which deserializes encodings that name values by this
    /// table's keys. Every reference to one value shares its allocation.
    pub(crate) fn read<R>(&self, decode: impl FnOnce() -> R) -> R {
        let scope = Scope::enter(Mode::Named(self.values.clone()));
        let decoded = decode();
        drop(scope);
        decoded
    }

    /// This table and `other`, which took keys this one does not hold:
    /// `None` when they hold one key both.
    pub(crate) fn with(mut self, other: Self) -> Option<Self> {
        for (key, value) in other.values {
            if self.values.insert(key, value).is_some() {
                return None;
            }
        }
        Some(self)
    }

    /// Whether the table holds no value.
    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl<T: Entry> Serialize for Keyed<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        for (key, value) in &self.values {
            map.serialize_entry(key, value.as_ref())?;
        }
        map.end()
    }
}

impl<'de, T: Entry> Deserialize<'de> for Keyed<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = BTreeMap::<Box<str>, T>::deserialize(deserializer)?;
        Ok(Self {
            values: values
                .into_iter()
                .map(|(key, value)| (key, Arc::new(value)))
                .collect(),
        })
    }
}

impl<T: Entry> Naming<T> {
    /// The key `value` is written as: the one its allocation or an equal
    /// value already has, or else its id, with `#` and a count when another
    /// value has that.
    fn key(&mut self, value: &Arc<T>) -> &str {
        let pointer = Arc::as_ptr(value);
        if !self.by_allocation.contains_key(&pointer) {
            let key = match self.keys.get(value).or_else(|| self.named.get(value)) {
                Some(key) => key.clone(),
                None => self.take(value),
            };
            self.held.push(Arc::clone(value));
            self.by_allocation.insert(pointer, key);
        }
        &self.by_allocation[&pointer]
    }

    fn take(&mut self, value: &Arc<T>) -> Box<str> {
        let id = value.id();
        let mut key: Box<str> = id.into();
        let mut count = 1_usize;
        while self.taken.contains_key(&key) || self.known.contains_key(&key) {
            key = format!("{id}#{count}").into();
            count += 1;
        }
        self.taken.insert(key.clone(), Arc::clone(value));
        self.named.insert(Arc::clone(value), key.clone());
        key
    }
}

/// The `serde(with)` form of a shared value: its key inside a naming
/// scope, else the value whole.
pub(crate) fn serialize<T: Entry, S: Serializer>(
    value: &Arc<T>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    // Writing a key or a whole value never comes back to this scope: a
    // value may hold values of other types, never one of its own.
    T::scope().with(|current| match &mut *current.borrow_mut() {
        Some(Mode::Naming(naming)) => serializer.serialize_str(naming.key(value)),
        _ => value.as_ref().serialize(serializer),
    })
}

/// The `serde(with)` form of a shared value: read by its key inside a
/// named scope, or whole where its encoding wrote it whole.
pub(crate) fn deserialize<'de, T: Entry, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<T>, D::Error> {
    let named = T::scope().with(|current| matches!(&*current.borrow(), Some(Mode::Named(_))));
    if named {
        deserializer.deserialize_any(Key(std::marker::PhantomData))
    } else {
        T::deserialize(deserializer).map(Arc::new)
    }
}

/// A value read by its key, or whole where its encoding wrote it whole.
struct Key<T>(std::marker::PhantomData<T>);

impl<'de, T: Entry> serde::de::Visitor<'de> for Key<T> {
    type Value = Arc<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a shared value or its key")
    }

    fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<Arc<T>, E> {
        T::scope()
            .with(|current| match &*current.borrow() {
                Some(Mode::Named(values)) => values.get(key).cloned(),
                _ => None,
            })
            .ok_or_else(|| E::custom(format!("no value has key {key} in the table")))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Arc<T>, A::Error> {
        T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Arc::new)
    }
}

/// The tables a stored API's canonical body and selected ledger close
/// with: its re-export hops and its compiler units. A hop names its unit by
/// key, so the unit table holds every unit the hop table names.
#[derive(Clone, Default)]
pub(crate) struct Tables {
    reexports: Keyed<Hop>,
    units: Keyed<Unit>,
}

impl Tables {
    /// Run `encode`, which serializes, with each hop and unit written as
    /// its key. Returns what `encode` returned and the tables of the values
    /// that took keys.
    pub(crate) fn write<R>(&self, encode: impl FnOnce() -> R) -> (R, Self) {
        let ((encoded, reexports), units) = self.units.write(|| {
            let (encoded, reexports) = self.reexports.write(encode);
            // The hops that took keys name their units while the unit
            // scope lives, so the unit table that comes back holds them.
            // An error here recurs in `close`, which reports it.
            let _ = serde_json::to_writer(std::io::sink(), &reexports);
            (encoded, reexports)
        });
        (encoded, Self { reexports, units })
    }

    /// Run `decode`, which deserializes encodings that name hops and units
    /// by these tables' keys.
    pub(crate) fn read<R>(&self, decode: impl FnOnce() -> R) -> R {
        self.units.read(|| self.reexports.read(decode))
    }

    /// Close `document`, an encoded JSON object, with these tables as its
    /// last fields, `reexports` and `units`. The encoding was written
    /// beside the tables `beside`, which hold the units of this hop table
    /// that this unit table does not.
    pub(crate) fn close(&self, beside: &Self, document: &mut Vec<u8>) -> serde_json::Result<()> {
        if document.pop() != Some(b'}') {
            return Err(serde::ser::Error::custom("a table closes an object"));
        }
        if document.len() > 1 {
            document.push(b',');
        }
        document.extend_from_slice(br#""reexports":"#);
        let known = beside
            .units
            .clone()
            .with(self.units.clone())
            .ok_or_else(|| serde::ser::Error::custom("two unit tables hold one key"))?;
        let (written, taken) =
            known.write(|| serde_json::to_writer(&mut *document, &self.reexports));
        written?;
        if !taken.is_empty() {
            return Err(serde::ser::Error::custom(
                "a re-export hop names a unit no table holds",
            ));
        }
        document.extend_from_slice(br#","units":"#);
        serde_json::to_writer(&mut *document, &self.units)?;
        document.push(b'}');
        Ok(())
    }

    /// The tables a body and its selected ledger close with, `parts`, each
    /// its encoded `reexports` and `units`, as one: `None` when two hold
    /// one key. Hops are read once every unit table is, since a hop names
    /// its unit by key.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn of<'a>(
        parts: impl IntoIterator<Item = (Option<&'a RawValue>, Option<&'a RawValue>)>,
    ) -> serde_json::Result<Option<Self>> {
        fn read<T: Entry>(value: &RawValue) -> serde_json::Result<Keyed<T>> {
            crate::extract::sidecar::read(value.get().as_bytes())
        }
        let mut units = Keyed::default();
        let mut hops = Vec::new();
        for (reexports, table) in parts {
            if let Some(table) = table {
                let Some(merged) = units.with(read(table)?) else {
                    return Ok(None);
                };
                units = merged;
            }
            hops.extend(reexports);
        }
        let mut reexports = Keyed::default();
        for table in hops {
            let Some(merged) = reexports.with(units.read(|| read(table))?) else {
                return Ok(None);
            };
            reexports = merged;
        }
        Ok(Some(Self { reexports, units }))
    }
}

impl Entry for Unit {
    fn id(&self) -> &str {
        self.stable_crate_id()
    }

    fn scope() -> &'static LocalKey<RefCell<Option<Mode<Self>>>> {
        &UNITS
    }
}

impl Entry for Hop {
    fn id(&self) -> &str {
        self.item().def_path_hash()
    }

    fn scope() -> &'static LocalKey<RefCell<Option<Mode<Self>>>> {
        &HOPS
    }
}

std::thread_local! {
    static UNITS: RefCell<Option<Mode<Unit>>> = const { RefCell::new(None) };
    static HOPS: RefCell<Option<Mode<Hop>>> = const { RefCell::new(None) };
}
