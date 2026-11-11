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

use super::Held;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The entries of a program's own facades, and what every source reads of
/// them.
pub(crate) struct Outline {
    entries: Box<[Entry]>,
    everywhere: [u8; 32],
}

/// One entry of a facade: a type, the items at one path, a re-export, a
/// trait header or an impl.
pub(crate) struct Entry {
    digest: [u8; 32],
    /// The names that reach it.
    names: Box<[Box<str>]>,
    /// The names it writes, which a source that reaches it reaches too.
    writes: Box<[Box<str>]>,
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
                let header =
                    serde_json::from_slice::<serde_json::Value>(view.trait_header_bytes(index));
                if let Ok(header) = header
                    && let Some(identity) = header.get("identity").and_then(|value| value.as_str())
                {
                    named
                        .entry(identity.to_owned())
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
        let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
        self.walk(&value, writes);
        Some(())
    }

    fn walk(&self, value: &serde_json::Value, writes: &mut BTreeSet<Box<str>>) {
        match value {
            serde_json::Value::String(text) => self.text(text, writes),
            serde_json::Value::Array(values) => {
                for value in values {
                    self.walk(value, writes);
                }
            }
            serde_json::Value::Object(fields) => {
                for value in fields.values() {
                    self.walk(value, writes);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
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
            let blanket = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|header| header.get("self_type").cloned())
                .is_none_or(|ty| ty.get("Parameter").is_some());
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
            unread,
        } = std::mem::take(self);
        let names = match (unread, reached) {
            (true, _) => BTreeSet::new(),
            (false, Reached::Declared) => names,
            (false, Reached::Written) => writes.clone(),
        };
        Entry {
            digest: digest.finalize().into(),
            names: names.into_iter().collect(),
            writes: writes.into_iter().collect(),
        }
    }
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
