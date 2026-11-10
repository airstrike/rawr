//! The compiler sidecar's wire form: each id written once, and every value
//! read at whatever depth rustc's types reach.
//!
//! A body names the same definitions over and over: every field, argument
//! and bound of every signature names its type's id, and an id spells its
//! crate in full. So a body leads with `"ids"`, each distinct id in full in
//! the order the body first names it, and names an id everywhere else by
//! its index there (protocol 67). The fragment that holds a body holds its
//! ids, so the publication and the shared document splice fragments as
//! they did.
//!
//! Depth is rustc's: a long combinator type nests as deep as its source
//! does. serde_json's default recursion limit of 128 is no bound of the
//! protocol, so a document reads with no limit on a stack that grows.

use std::cell::RefCell;
use std::marker::PhantomData;

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;

std::thread_local! {
    /// The ids of the fragment being read: a fragment's `ids` sets them,
    /// and its end clears them.
    static IDS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

/// An id object, as `model::Id` serializes: the only object whose first key
/// is `authority`.
const ID: &[u8] = b"{\"authority\":";

/// Rewrite a serialized body so it leads with its ids and names each id by
/// its index in them. The meaning is the body's: reading an index gives the
/// id written there.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn intern(body: &[u8]) -> Vec<u8> {
    let mut ids = Vec::<&[u8]>::new();
    let mut indices = std::collections::HashMap::<&[u8], usize>::new();
    let mut fields = Vec::with_capacity(body.len());
    let mut copied = 0;
    let mut cursor = 0;
    while cursor < body.len() {
        match body[cursor] {
            b'"' => cursor = string_end(body, cursor),
            b'{' if body[cursor..].starts_with(ID) => {
                let end = object_end(body, cursor);
                let id = &body[cursor..end];
                let next = ids.len();
                let index = *indices.entry(id).or_insert_with(|| {
                    ids.push(id);
                    next
                });
                fields.extend_from_slice(&body[copied..cursor]);
                fields.extend_from_slice(index.to_string().as_bytes());
                copied = end;
                cursor = end;
            }
            _ => cursor += 1,
        }
    }
    fields.extend_from_slice(&body[copied..]);
    let written = ids
        .iter()
        .map(|id| id.len().saturating_add(1))
        .sum::<usize>();
    let mut interned = Vec::with_capacity(fields.len().saturating_add(written).saturating_add(9));
    interned.extend_from_slice(b"{\"ids\":[");
    for (index, id) in ids.iter().enumerate() {
        if index != 0 {
            interned.push(b',');
        }
        interned.extend_from_slice(id);
    }
    interned.push(b']');
    // The body's own fields, after its opening brace.
    let rest = fields.get(1..).unwrap_or(b"}");
    if rest != b"}" {
        interned.push(b',');
    }
    interned.extend_from_slice(rest);
    interned
}

/// The index just past the string that opens at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start.saturating_add(1);
    while let Some(&byte) = bytes.get(cursor) {
        match byte {
            b'\\' => cursor = cursor.saturating_add(2),
            b'"' => return cursor.saturating_add(1),
            _ => cursor += 1,
        }
    }
    bytes.len()
}

/// The index just past the object or array that opens at `start`.
fn object_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut cursor = start;
    while let Some(&byte) = bytes.get(cursor) {
        match byte {
            b'"' => {
                cursor = string_end(bytes, cursor);
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return cursor.saturating_add(1);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    bytes.len()
}

/// Read a document of fragments whose ids are written once. No fragment's
/// ids outlive the read, whatever it returns.
pub(crate) fn read_fragments<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    IDS.with(|ids| ids.borrow_mut().clear());
    let read = read(bytes);
    IDS.with(|ids| ids.borrow_mut().clear());
    read
}

/// Read one JSON value at any depth.
#[cfg(feature = "rustc-driver")]
pub(crate) fn read<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(serde_stacker::Deserializer::new(&mut deserializer))?;
    deserializer.end()?;
    Ok(value)
}

/// Without the driver no compiler sidecar is read, so serde_json's own
/// limit stands.
#[cfg(not(feature = "rustc-driver"))]
pub(crate) fn read<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    serde_json::from_slice(bytes)
}

/// A fragment's ids, now the ones its later fields name: written after its
/// handshake and before its body's fields. `close` ends them.
#[derive(Debug, Default)]
pub(crate) struct Opened {
    ids: usize,
}

impl Opened {
    /// End the fragment's ids, so the next fragment names none of them.
    pub(crate) fn close(self) -> usize {
        IDS.with(|ids| ids.borrow_mut().clear());
        self.ids
    }
}

impl<'de> Deserialize<'de> for Opened {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let written = Vec::<Value>::deserialize(deserializer)?;
        let ids = written.len();
        IDS.with(|current| *current.borrow_mut() = written);
        Ok(Self { ids })
    }
}

/// Read an id written in full, as `T`'s fields, or by its index in its
/// fragment's ids.
pub(crate) fn id<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(Written(PhantomData))
}

struct Written<T>(PhantomData<T>);

impl<'de, T: DeserializeOwned> Visitor<'de> for Written<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an id, or its index in its fragment's ids")
    }

    fn visit_u64<E: de::Error>(self, index: u64) -> Result<T, E> {
        IDS.with(|ids| {
            let ids = ids.borrow();
            let id = usize::try_from(index)
                .ok()
                .and_then(|index| ids.get(index))
                .ok_or_else(|| E::custom(format!("id {index} names none of its fragment's ids")))?;
            T::deserialize(id).map_err(E::custom)
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
        T::deserialize(de::value::MapAccessDeserializer::new(map))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::{intern, read_fragments, Opened};

    const ID: &str = r#"{"authority":{"kind":"compiler","crate_name":"core","stable_crate_id":"0123456789abcdef"},"def_path_hash":"efcdab89674523010000000000000001","namespace":"type"}"#;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Fields {
        authority: serde_json::Value,
        def_path_hash: String,
        namespace: String,
    }

    #[derive(Debug, PartialEq)]
    struct Id(Fields);

    impl<'de> Deserialize<'de> for Id {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            super::id(deserializer).map(Id)
        }
    }

    /// A fragment as `protocol::Fragment` reads one: its ids end with it.
    #[derive(Debug, Deserialize)]
    #[serde(from = "Part")]
    struct Fragment {
        types: Vec<Type>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Part {
        #[serde(default)]
        ids: Opened,
        types: Vec<Type>,
    }

    impl From<Part> for Fragment {
        fn from(part: Part) -> Self {
            part.ids.close();
            Self { types: part.types }
        }
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        fragments: Vec<Fragment>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum Type {
        Definition { id: Id, arguments: Vec<Type> },
    }

    /// A type nested `depth` deep, each level naming the same id.
    fn nested(depth: usize) -> String {
        let mut ty = format!(r#"{{"kind":"definition","id":{ID},"arguments":[]}}"#);
        for _ in 0..depth {
            ty = format!(r#"{{"kind":"definition","id":{ID},"arguments":[{ty}]}}"#);
        }
        format!(r#"{{"types":[{ty}]}}"#)
    }

    /// The types a document of one fragment holds.
    fn types(bytes: &[u8]) -> Vec<Type> {
        read_fragments::<Fragment>(bytes).unwrap().types
    }

    /// A repeated id is written once, and the body reads as it did.
    #[test]
    fn a_repeated_id_is_written_once() {
        let body = nested(40);
        let interned = intern(body.as_bytes());
        let text = std::str::from_utf8(&interned).unwrap();
        assert_eq!(text.matches("\"authority\"").count(), 1);
        assert!(text.starts_with(&format!(r#"{{"ids":[{ID}],"types":"#)));
        assert!(
            interned.len() * 4 < body.len(),
            "{} of {}",
            interned.len(),
            body.len()
        );
        assert_eq!(types(&interned), types(body.as_bytes()));
    }

    /// A value nested far past serde_json's 128 levels reads, written in
    /// full and with its ids written once.
    #[test]
    fn a_value_deeper_than_128_levels_round_trips() {
        let body = nested(400);
        let interned = intern(body.as_bytes());
        let read = types(&interned);
        assert_eq!(read, types(body.as_bytes()));
        let mut depth = 0;
        let mut ty = &read[0];
        loop {
            let Type::Definition { arguments, .. } = ty;
            let Some(argument) = arguments.first() else {
                break;
            };
            depth += 1;
            ty = argument;
        }
        assert_eq!(depth, 400);
    }

    /// An index no id of its fragment answers is refused, as is one in a
    /// fragment after the one whose ids it would name.
    #[test]
    fn an_index_outside_its_fragments_ids_is_refused() {
        let unknown = r#"{"ids":[],"types":[{"kind":"definition","id":0,"arguments":[]}]}"#;
        assert!(read_fragments::<Fragment>(unknown.as_bytes()).is_err());
        let first = String::from_utf8(intern(nested(1).as_bytes())).unwrap();
        let both = format!(r#"{{"fragments":[{first},{first}]}}"#);
        let read = read_fragments::<Document>(both.as_bytes()).unwrap();
        assert_eq!(read.fragments.len(), 2);
        let after = r#"{"types":[{"kind":"definition","id":0,"arguments":[]}]}"#;
        let stale = format!(r#"{{"fragments":[{first},{after}]}}"#);
        assert!(read_fragments::<Document>(stale.as_bytes()).is_err());
    }

    /// An `authority` key inside a string is text, not an id.
    #[test]
    fn an_id_spelled_inside_a_string_is_text() {
        let body = r#"{"note":"{\"authority\":1}","types":[]}"#;
        let interned = intern(body.as_bytes());
        assert_eq!(
            std::str::from_utf8(&interned).unwrap(),
            r#"{"ids":[],"note":"{\"authority\":1}","types":[]}"#
        );
    }
}
