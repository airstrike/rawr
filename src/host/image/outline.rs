//! What a program's lowering reads of its own crate's facades, entry by
//! entry (#1725): the facades of the crate rawr builds from the program's
//! marked sources, which a save that changes what native code sees changes.
//!
//! A lowering reads a facade's entry only through a name, as it reads
//! another source's declaration (`resolve::lower_nominal::interface`): a
//! path, a type a signature or a field names, a method of such a type, or
//! an impl of such a type or trait. So each entry is digested on its own,
//! with the names that reach it and the names it writes, which a source
//! that reaches it reaches too. An item is reached by its name, and a
//! method by its type's or its trait's, as a source calls it on a value
//! of a type it reaches. An impl is reached by each name it writes. An
//! entry writes the name of each of these facades' types and traits its
//! body names by identity, and the item's and its owner's names of each
//! path it writes. What no name reaches is every source's: the modules,
//! derives, macros, re-exported modules and type aliases, and each impl of
//! a type parameter.
//!
//! An outline is a function of its facades' images alone, which their
//! digests name (`Key`), so a runner keeps it on disk for the next runner
//! of the same image (#1755). A body is read as it lies: each string it
//! holds, borrowed, never a tree of values.

use super::Held;
use serde::de::{self, DeserializeSeed as _, IgnoredAny};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The entries of a program's own facades, and what every source reads of
/// them.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Outline {
    entries: Box<[Entry]>,
    everywhere: [u8; 32],
}

/// One entry of a facade: a type, the items at one path, a re-export, a
/// trait header or an impl.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Entry {
    digest: [u8; 32],
    /// The names that reach it.
    names: Box<[Box<str>]>,
    /// The names it writes, which a source that reaches it reaches too.
    writes: Box<[Box<str>]>,
    /// The module its paths are in, a method's owner's, when they are all
    /// in one and its paths' names reach it: a source outside that module
    /// names it only through a path or an import (#1886).
    module: Option<Box<str>>,
    /// Whether its paths' names reach it, rather than each name it writes,
    /// as an impl's do.
    declared: bool,
}

impl Entry {
    pub(crate) fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(AsRef::as_ref)
    }

    pub(crate) fn writes(&self) -> impl Iterator<Item = &str> {
        self.writes.iter().map(AsRef::as_ref)
    }

    /// The module its paths are in, when its paths' names reach it and
    /// they are all in one.
    pub(crate) fn module(&self) -> Option<&str> {
        self.module.as_deref()
    }

    /// Whether its paths' names reach it: an impl is reached by each name
    /// it writes.
    pub(crate) const fn declared(&self) -> bool {
        self.declared
    }
}

impl Outline {
    /// The outline of `facades`, a program's own.
    pub(crate) fn new(facades: &[&Held]) -> Self {
        let named = Named::new(facades);
        let mut entries = Vec::new();
        let mut everywhere = Sha256::new();
        for held in facades {
            held.outline(&named, &mut entries, &mut everywhere);
        }
        Self {
            entries: entries.into_boxed_slice(),
            everywhere: everywhere.finalize().into(),
        }
    }

    /// Each entry, in the facades' order.
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What every source reads of the facades.
    pub(crate) fn everywhere(&self) -> &[u8; 32] {
        &self.everywhere
    }
}

/// What an outline is made of: its facades, in order, each by its root
/// and the digest of everything its image holds. Two sets of facades with
/// one key have one outline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key([u8; 32]);

impl Key {
    /// The key of the outline of `facades`.
    pub(crate) fn of(facades: &[&Held]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.image.outline/v1");
        for held in facades {
            let view = held.view();
            framed(&mut digest, view.root().as_bytes());
            digest.update(view.digest());
        }
        Self(digest.finalize().into())
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The facades' types and traits: their names, by their identities as an
/// image writes them, 64 hex digits, and their paths.
struct Named {
    identities: BTreeMap<String, BTreeSet<Box<str>>>,
    /// The paths of the types and traits, which an item's path below one
    /// names as its owner.
    owners: BTreeSet<&'static str>,
}

impl Named {
    fn new(facades: &[&Held]) -> Self {
        let mut named = BTreeMap::<String, BTreeSet<Box<str>>>::new();
        let mut owners = BTreeSet::new();
        for held in facades {
            let view = held.view();
            owners.extend((0..view.types()).map(|ty| view.type_path(ty)));
            owners.extend((0..view.trait_headers()).map(|index| view.trait_header_path(index)));
            for index in 0..view.identities() {
                let identities = view.identity_at(index);
                let names = named.entry(hex(&identities.identity())).or_default();
                names.extend(identities.types().map(|ty| last(view.type_path(ty)).into()));
            }
            for index in 0..view.trait_headers() {
                let header = serde_json::from_slice::<Header<'_>>(view.trait_header_bytes(index));
                if let Ok(Header {
                    identity: Some(identity),
                }) = header
                {
                    named
                        .entry(identity.into_owned())
                        .or_default()
                        .insert(last(view.trait_header_path(index)).into());
                }
            }
        }
        Self {
            identities: named,
            owners,
        }
    }

    /// The names the path `path` writes: its item's, and its owner's when
    /// it is below a type or a trait, as a method's path is. A module on
    /// the way names nothing a lowering reads.
    fn path<'p>(&self, path: &'p str) -> impl Iterator<Item = &'p str> {
        std::iter::once(last(path)).chain(self.owner(path))
    }

    /// The name of the type or trait the path `path` is below.
    fn owner<'p>(&self, path: &'p str) -> Option<&'p str> {
        path.rsplit_once("::")
            .map(|(owner, _)| owner)
            .filter(|owner| self.owners.contains(owner))
            .map(last)
    }

    /// The names a stored body writes into `writes`: the names of the
    /// types and traits it names by identity, as a body names a crate's
    /// type, and those of each path it writes (`Named::path`). Its own
    /// names, its fields', its variants' and its parameters', reach
    /// nothing. `None` when the body isn't JSON, which every source reads.
    fn written(&self, bytes: &[u8], writes: &mut BTreeSet<Box<str>>) -> Option<()> {
        let mut found = BTreeSet::new();
        let mut json = serde_json::Deserializer::from_slice(bytes);
        Walk {
            named: self,
            writes: &mut found,
        }
        .deserialize(&mut json)
        .and_then(|()| json.end())
        .ok()?;
        writes.append(&mut found);
        Some(())
    }

    fn text(&self, text: &str, writes: &mut BTreeSet<Box<str>>) {
        if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            if let Some(names) = self.identities.get(text) {
                writes.extend(names.iter().cloned());
            }
        } else if text.contains("::") {
            writes.extend(self.path(text).map(Into::into));
        }
    }
}

impl Held {
    /// Each entry of this facade into `entries`, and what no name reaches
    /// into `everywhere`.
    fn outline(&self, named: &Named, entries: &mut Vec<Entry>, everywhere: &mut Sha256) {
        let view = self.view();
        let image = view.image();
        framed(everywhere, view.root().as_bytes());
        for index in 0..view.modules() {
            framed(everywhere, view.module(index).as_bytes());
        }
        for index in 0..view.derives() {
            framed(everywhere, view.derive(index).as_bytes());
        }
        for table in view.tables_bytes() {
            framed(everywhere, table);
        }
        let mut made = Made::default();
        let first = entries.len();
        // Each crate type identity, with each of its paths.
        let mut covered = BTreeSet::new();
        for index in 0..view.identities() {
            let identities = view.identity_at(index);
            made.begin();
            made.digest.update(identities.identity());
            made.digest.update(identities.parameters().to_le_bytes());
            for ty in identities.types() {
                covered.insert(ty);
                made.at(named, view.type_path(ty));
                made.body(named, image.type_body_bytes(view.type_body(ty)));
            }
            entries.push(made.finish(Reached::Declared));
        }
        for ty in (0..view.types()).filter(|ty| !covered.contains(ty)) {
            made.begin();
            made.at(named, view.type_path(ty));
            made.body(named, image.type_body_bytes(view.type_body(ty)));
            entries.push(made.finish(Reached::Declared));
        }
        for index in 0..view.groups() {
            let group = view.group(index);
            made.begin();
            made.at(named, group.path());
            made.owned(named, group.path());
            for body in group.bodies() {
                made.body(named, image.item_body_bytes(body));
            }
            for macro_index in group.macros() {
                made.digest.update(macro_index.to_le_bytes());
            }
            entries.push(made.finish(Reached::Declared));
        }
        for index in 0..view.aliases() {
            let alias = view.alias(index);
            made.begin();
            made.at(named, alias.path);
            made.refers(named, alias.target);
            made.digest.update([alias.kind as u8]);
            entries.push(made.finish(Reached::Declared));
        }
        for index in 0..view.trait_headers() {
            made.begin();
            made.at(named, view.trait_header_path(index));
            made.body(named, view.trait_header_bytes(index));
            entries.push(made.finish(Reached::Declared));
        }
        // An impl is reached by each name it writes: its type's and its
        // trait's. An impl of a type parameter is every source's.
        for index in 0..view.impls() {
            let bytes = view.impl_bytes(index);
            let blanket = serde_json::from_slice::<Impl>(bytes)
                .map_or(true, |header| header.self_type.blanket());
            if blanket {
                framed(everywhere, bytes);
                continue;
            }
            made.begin();
            made.body(named, bytes);
            entries.push(made.finish(Reached::Written));
        }
        // An entry no name reaches is every source's.
        for entry in entries[first..]
            .iter()
            .filter(|entry| entry.names.is_empty())
        {
            everywhere.update(entry.digest);
        }
    }
}

/// The strings a stored body holds, each as `Named::text` reads it: a
/// value's, never a field's name.
struct Walk<'n, 'w> {
    named: &'n Named,
    writes: &'w mut BTreeSet<Box<str>>,
}

impl Walk<'_, '_> {
    /// The walk of a value inside this one.
    fn inner(&mut self) -> Walk<'_, '_> {
        Walk {
            named: self.named,
            writes: self.writes,
        }
    }
}

impl<'de> de::DeserializeSeed<'de> for Walk<'_, '_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> de::Visitor<'de> for Walk<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a stored body")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, text: &str) -> Result<(), E> {
        self.named.text(text, self.writes);
        Ok(())
    }

    fn visit_seq<A: de::SeqAccess<'de>>(mut self, mut values: A) -> Result<(), A::Error> {
        while values.next_element_seed(self.inner())?.is_some() {}
        Ok(())
    }

    fn visit_map<A: de::MapAccess<'de>>(mut self, mut fields: A) -> Result<(), A::Error> {
        while fields.next_key::<IgnoredAny>()?.is_some() {
            fields.next_value_seed(self.inner())?;
        }
        Ok(())
    }
}

/// What a trait header's names read of it: its identity, as text.
#[derive(serde::Deserialize)]
struct Header<'a> {
    #[serde(default, borrow)]
    identity: Option<std::borrow::Cow<'a, str>>,
}

/// What an outline reads of an impl's header: whether its self type is a
/// type parameter.
#[derive(serde::Deserialize)]
struct Impl {
    #[serde(default)]
    self_type: SelfType,
}

/// An impl header's self type, as far as an outline reads it.
#[derive(Default)]
enum SelfType {
    /// The header has none.
    #[default]
    Missing,
    /// A type parameter: an object with a `Parameter` field.
    Parameter,
    /// Any other value.
    Other,
}

impl SelfType {
    /// Whether an impl of it is every source's.
    const fn blanket(&self) -> bool {
        matches!(self, Self::Missing | Self::Parameter)
    }
}

impl<'de> serde::Deserialize<'de> for SelfType {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SelfTypeVisitor)
    }
}

/// Reads a `SelfType` from any value.
struct SelfTypeVisitor;

impl<'de> de::Visitor<'de> for SelfTypeVisitor {
    type Value = SelfType;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an impl's self type")
    }

    fn visit_bool<E>(self, _: bool) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_i64<E>(self, _: i64) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_u64<E>(self, _: u64) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_f64<E>(self, _: f64) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_str<E>(self, _: &str) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_unit<E>(self) -> Result<SelfType, E> {
        Ok(SelfType::Other)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut values: A) -> Result<SelfType, A::Error> {
        while values.next_element::<IgnoredAny>()?.is_some() {}
        Ok(SelfType::Other)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut fields: A) -> Result<SelfType, A::Error> {
        let mut ty = SelfType::Other;
        while let Some(field) = fields.next_key::<std::borrow::Cow<'de, str>>()? {
            fields.next_value::<IgnoredAny>()?;
            if field == "Parameter" {
                ty = SelfType::Parameter;
            }
        }
        Ok(ty)
    }
}

/// How a source reaches an entry.
#[derive(Clone, Copy)]
enum Reached {
    /// By the last segment of each of its paths.
    Declared,
    /// By each name it writes.
    Written,
}

/// An entry being made.
#[derive(Default)]
struct Made {
    digest: Sha256,
    names: BTreeSet<Box<str>>,
    writes: BTreeSet<Box<str>>,
    /// The modules its paths are in (`Entry::module`).
    modules: BTreeSet<Box<str>>,
    /// Whether a body wasn't JSON.
    unread: bool,
}

impl Made {
    fn begin(&mut self) {
        *self = Self::default();
    }

    /// The entry is at `path`: its last segment is a name that reaches it,
    /// and the type or trait it is below a name it writes.
    fn at(&mut self, named: &Named, path: &str) {
        framed(&mut self.digest, path.as_bytes());
        self.names.insert(last(path).into());
        self.writes.extend(named.owner(path).map(Into::into));
        self.modules.insert(module(path).into());
    }

    /// The entry writes `path`: the names the path writes (`Named::path`)
    /// are names it writes.
    fn refers(&mut self, named: &Named, path: &str) {
        framed(&mut self.digest, path.as_bytes());
        self.writes.extend(named.path(path).map(Into::into));
    }

    /// The entry is at `path`, below a type or a trait when it is a
    /// method: the owner's name reaches it, and the method's doesn't, as a
    /// source reaches a method through the type of the value it calls it
    /// on.
    fn owned(&mut self, named: &Named, path: &str) {
        if let Some(owner) = named.owner(path) {
            self.names.clear();
            self.names.insert(owner.into());
            self.modules.clear();
            self.modules.insert(module(module(path)).into());
        }
    }

    fn body(&mut self, named: &Named, bytes: &[u8]) {
        framed(&mut self.digest, bytes);
        self.unread |= named.written(bytes, &mut self.writes).is_none();
    }

    /// The entry made, which a source reaches as `reached` says. An entry
    /// with a body that wasn't JSON is reached by no name: every source
    /// reads it.
    fn finish(&mut self, reached: Reached) -> Entry {
        let Self {
            digest,
            names,
            writes,
            modules,
            unread,
        } = std::mem::take(self);
        let names = match (unread, reached) {
            (true, _) => BTreeSet::new(),
            (false, Reached::Declared) => names,
            (false, Reached::Written) => writes.clone(),
        };
        let module = match reached {
            Reached::Declared if modules.len() == 1 => modules.into_iter().next(),
            _ => None,
        };
        Entry {
            digest: digest.finalize().into(),
            names: names.into_iter().collect(),
            writes: writes.into_iter().collect(),
            module,
            declared: matches!(reached, Reached::Declared),
        }
    }
}

/// The path `path` is below: its module, or its owner's for a method's.
fn module(path: &str) -> &str {
    path.rsplit_once("::").map_or("", |(module, _)| module)
}

/// The last segment of the path `path`.
fn last(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// `identity` as an image writes it: 64 lowercase hex digits.
fn hex(identity: &[u8; 32]) -> String {
    identity
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// `bytes`, after their length, so two never run together.
fn framed(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(bytes.len().to_le_bytes());
    digest.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::{Key, Named, Outline};
    use crate::host::image::{Held, Plan};
    use std::collections::BTreeSet;

    /// The facades of a runner's image with crate types, items and
    /// headers.
    fn image() -> Vec<Held> {
        let image = Box::leak(rawr_import::test_fixture::runner_foreign_facade_plan());
        let plan = Plan::read(image, &[]).unwrap();
        Plan::held(&plan).collect()
    }

    /// What a body writes, read as a tree of values: what `Named::written`
    /// read before it read a body as it lies.
    fn walked(named: &Named, value: &serde_json::Value, writes: &mut BTreeSet<Box<str>>) {
        match value {
            serde_json::Value::String(text) => named.text(text, writes),
            serde_json::Value::Array(values) => {
                for value in values {
                    walked(named, value, writes);
                }
            }
            serde_json::Value::Object(fields) => {
                for value in fields.values() {
                    walked(named, value, writes);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }

    /// A body read as it lies writes what it writes read as a tree of
    /// values, and a body that isn't JSON writes nothing (#1755).
    #[test]
    fn a_body_read_as_it_lies_writes_what_its_values_write() {
        let held = image();
        let facades = held.iter().collect::<Vec<_>>();
        let named = Named::new(&facades);
        let mut bodies = Vec::new();
        for held in &held {
            let view = held.view();
            let image = view.image();
            bodies.extend((0..view.types()).map(|ty| image.type_body_bytes(view.type_body(ty))));
            for index in 0..view.groups() {
                bodies.extend(
                    view.group(index)
                        .bodies()
                        .map(|body| image.item_body_bytes(body)),
                );
            }
            bodies.extend((0..view.trait_headers()).map(|index| view.trait_header_bytes(index)));
            bodies.extend((0..view.impls()).map(|index| view.impl_bytes(index)));
        }
        assert!(!bodies.is_empty());
        for bytes in bodies {
            let mut lying = BTreeSet::new();
            named.written(bytes, &mut lying).unwrap();
            let mut values = BTreeSet::new();
            walked(&named, &serde_json::from_slice(bytes).unwrap(), &mut values);
            assert_eq!(lying, values);
        }
        let mut writes = BTreeSet::new();
        assert!(named
            .written(b"{\"a\": \"x::y\"} trailing", &mut writes)
            .is_none());
        assert!(writes.is_empty());
    }

    /// The outline a runner keeps for the next, decoded as a file holds
    /// it, is the one made fresh of the same facades, at the same key.
    #[test]
    fn a_kept_outline_is_the_one_made_fresh() {
        let held = image();
        let facades = held.iter().collect::<Vec<_>>();
        let made = Outline::new(&facades);
        assert!(!made.entries().is_empty());
        let kept = rmp_serde::from_slice::<Outline>(&rmp_serde::to_vec(&made).unwrap()).unwrap();
        let again = image();
        let fresh = again.iter().collect::<Vec<_>>();
        assert_eq!(kept, Outline::new(&fresh));
        assert!(Key::of(&facades) == Key::of(&fresh));
        assert!(Key::of(&facades) != Key::of(&facades[1..]));
    }
}
