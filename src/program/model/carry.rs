//! A lossless encoding of plain script data, so application State outlives
//! the runner process that holds it.
//!
//! Primitives, strings, arrays, maps, blobs, `Option`, `Result`, `Reverse`
//! and script structs and enums are encoded; a struct or enum carries its type
//! key, its variant and the digest of its schema. A host value is encoded when
//! its facade made its type `Carry` (its bytes and its Rust type name).
//! Anything else (a native handle, an imported Rust value, a closure, a
//! timestamp) is not plain data, and the value is not carried. Decoding rebuilds each script value in the new
//! program's model, and only when its schema digest is unchanged.

use super::{Model, Shape};
use crate::types::dynamic::Union;
use crate::types::nominal;
use crate::{type_def, Dynamic, ImmutableString};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

const UNIT: u8 = 0;
const FALSE: u8 = 1;
const TRUE: u8 = 2;
const INT: u8 = 3;
const FLOAT: u8 = 4;
const CHAR: u8 = 5;
const STR: u8 = 6;
const ARRAY: u8 = 7;
const MAP: u8 = 8;
const BLOB: u8 = 9;
const NONE: u8 = 10;
const SOME: u8 = 11;
const OK: u8 = 12;
const ERR: u8 = 13;
const REVERSE: u8 = 14;
const STRUCT: u8 = 15;
const ENUM: u8 = 16;
const HOST: u8 = 17;

/// Nesting deeper than this is not carried (it would be a deeply recursive
/// structure no State needs, and decoding must not overflow the stack).
const DEPTH: usize = 256;

/// The bytes of `value`, or `None` when it holds anything but plain data.
pub(crate) fn encode(value: &Dynamic) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    write(value, &mut out, 0)?;
    Some(out)
}

fn schema_digest(schema: &super::schema::Fingerprint) -> Option<[u8; 16]> {
    use crate::preview::digest::Write as _;
    let mut writer = crate::preview::digest::Writer::with_version(b"rawr-carry-schema-v1");
    schema.write(&mut writer).ok()?;
    Some(writer.finish().bytes())
}

fn script_key(identity: &type_def::Identity) -> Option<String> {
    match identity {
        type_def::Identity::Script(key) => Some(key.to_string()),
        type_def::Identity::Host(_) => None,
    }
}

fn write(value: &Dynamic, out: &mut Vec<u8>, depth: usize) -> Option<()> {
    if depth > DEPTH {
        return None;
    }
    let depth = depth + 1;
    match &value.0 {
        Union::Unit(..) => out.push(UNIT),
        Union::Bool(value, ..) => out.push(if *value { TRUE } else { FALSE }),
        Union::Int(value, ..) => {
            out.push(INT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        #[cfg(not(feature = "no_float"))]
        Union::Float(value, ..) => {
            out.push(FLOAT);
            out.extend_from_slice(&(**value).to_le_bytes());
        }
        Union::Char(value, ..) => {
            out.push(CHAR);
            out.extend_from_slice(&u32::from(*value).to_le_bytes());
        }
        Union::Str(value, ..) => {
            out.push(STR);
            text(value, out);
        }
        #[cfg(not(feature = "no_index"))]
        Union::Array(items, ..) => {
            out.push(ARRAY);
            count(items.len(), out);
            for item in items.iter() {
                write(item, out, depth)?;
            }
        }
        #[cfg(not(feature = "no_index"))]
        Union::Blob(bytes, ..) => {
            out.push(BLOB);
            count(bytes.len(), out);
            out.extend_from_slice(bytes);
        }
        #[cfg(not(feature = "no_object"))]
        Union::Map(map, ..) => {
            out.push(MAP);
            count(map.len(), out);
            for (key, item) in map.iter() {
                text(key, out);
                write(item, out, depth)?;
            }
        }
        Union::Struct(value, ..) => {
            out.push(STRUCT);
            text(&script_key(value.identity())?, out);
            out.extend_from_slice(&schema_digest(value.schema())?);
            count(value.fields().len(), out);
            for field in value.fields() {
                write(field, out, depth)?;
            }
        }
        Union::Enum(value, ..) => {
            out.push(ENUM);
            text(&script_key(value.identity())?, out);
            out.extend_from_slice(&schema_digest(value.schema())?);
            text(value.variant().name(), out);
            count(value.payload().len(), out);
            for item in value.payload() {
                write(item, out, depth)?;
            }
        }
        #[cfg(not(feature = "no_closure"))]
        Union::Shared(..) => write(&value.flatten_clone(), out, depth)?,
        Union::Variant(..) => {
            if let Some(option) = value.read_lock::<crate::option::Value>() {
                match &*option {
                    None => out.push(NONE),
                    Some(inner) => {
                        out.push(SOME);
                        write(inner, out, depth)?;
                    }
                }
            } else if let Some(result) = value.read_lock::<crate::result::Value>() {
                let (tag, inner) = match &*result {
                    Ok(inner) => (OK, inner),
                    Err(inner) => (ERR, inner),
                };
                out.push(tag);
                write(inner, out, depth)?;
            } else if let Some(reverse) = value.read_lock::<std::cmp::Reverse<Dynamic>>() {
                out.push(REVERSE);
                write(&reverse.0, out, depth)?;
            } else {
                // A host value whose type is `Carry` (an iced `Color` or
                // `Theme`) is its bytes; any other lives in one process.
                let mut bytes = Vec::new();
                let name = crate::host::value::encode_host(value, &mut bytes)?;
                out.push(HOST);
                text(name, out);
                count(bytes.len(), out);
                out.extend_from_slice(&bytes);
            }
        }
        _ => return None,
    }
    Some(())
}

fn count(value: usize, out: &mut Vec<u8>) {
    out.extend_from_slice(&(value as u64).to_le_bytes());
}

fn text(value: &str, out: &mut Vec<u8>) {
    count(value.len(), out);
    out.extend_from_slice(value.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, length: usize) -> Option<&[u8]> {
        if self.bytes.len() < length {
            return None;
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Some(head)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn count(&mut self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.array()?)).ok()
    }

    /// A count of items each at least one byte long, so a corrupt count
    /// cannot ask for more than the input holds.
    fn items(&mut self) -> Option<usize> {
        let count = self.count()?;
        (count <= self.bytes.len()).then_some(count)
    }

    fn text(&mut self) -> Option<&str> {
        let length = self.count()?;
        std::str::from_utf8(self.take(length)?).ok()
    }
}

impl Model {
    /// The declared type of the first field in `value` whose value cannot be
    /// carried, such as an imported Rust value. It looks inside script structs
    /// and enums, and `None` means every field carries.
    pub(crate) fn uncarried_field(&self, value: &Dynamic) -> Option<String> {
        self.uncarried(value, 0)
    }

    fn uncarried(&self, value: &Dynamic, depth: usize) -> Option<String> {
        if depth > DEPTH {
            return None;
        }
        let value = value.flatten_clone();
        let (identity, schema, variant, members) = match &value.0 {
            Union::Struct(nominal, ..) => (nominal.identity(), nominal.schema(), None, nominal.fields()),
            Union::Enum(nominal, ..) => (
                nominal.identity(),
                nominal.schema(),
                Some(nominal.variant()),
                nominal.payload(),
            ),
            _ => return None,
        };
        let definition = self.definition(identity, schema).ok()?;
        let types = match (&definition.shape, variant) {
            (Shape::Struct(fields), None) => fields.as_ref(),
            (Shape::Enum(variants), Some(variant)) => variants.get(variant)?.as_ref(),
            _ => return None,
        };
        members.iter().zip(types).find_map(|(member, ty)| {
            use crate::resolve::type_ref::{Target, Type};
            if encode(member).is_some() {
                return None;
            }
            match ty {
                Type::Path(path) => match &path.target {
                    Target::Script { .. } => self.uncarried(member, depth + 1),
                    Target::Host { definition, .. } => Some(definition.path()),
                    _ => Some(path.name.to_string()),
                },
                _ => None,
            }
        })
    }

    /// The value `bytes` encode, rebuilt in this model; `None` when they are
    /// malformed or name a script type whose schema changed.
    pub(crate) fn decode_carried(&self, bytes: &[u8]) -> Option<Dynamic> {
        let mut reader = Reader { bytes };
        let value = self.read(&mut reader, 0)?;
        reader.bytes.is_empty().then_some(value)
    }

    fn read(&self, reader: &mut Reader<'_>, depth: usize) -> Option<Dynamic> {
        if depth > DEPTH {
            return None;
        }
        let depth = depth + 1;
        Some(match reader.byte()? {
            UNIT => Dynamic::UNIT,
            FALSE => false.into(),
            TRUE => true.into(),
            INT => crate::INT::from_le_bytes(reader.array()?).into(),
            #[cfg(not(feature = "no_float"))]
            FLOAT => crate::FLOAT::from_le_bytes(reader.array()?).into(),
            CHAR => char::from_u32(u32::from_le_bytes(reader.array()?))?.into(),
            STR => ImmutableString::from(reader.text()?).into(),
            #[cfg(not(feature = "no_index"))]
            ARRAY => {
                let length = reader.items()?;
                let mut items = crate::Array::with_capacity(length);
                for _ in 0..length {
                    items.push(self.read(reader, depth)?);
                }
                Dynamic::from_array(items)
            }
            #[cfg(not(feature = "no_index"))]
            BLOB => {
                let length = reader.count()?;
                Dynamic::from_blob(reader.take(length)?.to_vec())
            }
            #[cfg(not(feature = "no_object"))]
            MAP => {
                let length = reader.items()?;
                let mut map = crate::Map::new();
                for _ in 0..length {
                    let key = reader.text()?.into();
                    map.insert(key, self.read(reader, depth)?);
                }
                Dynamic::from_map(map)
            }
            NONE => crate::option::none(),
            SOME => crate::option::some(self.read(reader, depth)?),
            OK => crate::result::ok(self.read(reader, depth)?),
            ERR => crate::result::err(self.read(reader, depth)?),
            REVERSE => Dynamic::from(std::cmp::Reverse(self.read(reader, depth)?)),
            HOST => {
                let name = reader.text()?.to_owned();
                let length = reader.count()?;
                crate::host::value::decode_host(&name, reader.take(length)?)?
            }
            STRUCT => {
                let (identity, schema) = self.carried_type(reader)?;
                let length = reader.items()?;
                let mut fields = Vec::with_capacity(length);
                for _ in 0..length {
                    fields.push(self.read(reader, depth)?);
                }
                let proof = self
                    .prove_struct(&identity, &schema, fields.into_boxed_slice())
                    .ok()?;
                nominal::Struct::construct(self, &identity, &schema, proof)
                    .ok()?
                    .into()
            }
            ENUM => {
                let (identity, schema) = self.carried_type(reader)?;
                let name = reader.text()?;
                let Shape::Enum(variants) = &self.definitions.get(&identity)?.shape else {
                    return None;
                };
                let variant = variants.keys().find(|variant| variant.name() == name)?.clone();
                let length = reader.items()?;
                let mut payload = Vec::with_capacity(length);
                for _ in 0..length {
                    payload.push(self.read(reader, depth)?);
                }
                let proof = self
                    .prove_enum(&identity, &schema, &variant, payload.into_boxed_slice())
                    .ok()?;
                nominal::Enum::construct(self, &identity, &schema, &variant, proof)
                    .ok()?
                    .into()
            }
            _ => return None,
        })
    }

    /// The script type a carried key names in this model, when its schema
    /// digest is the carried one.
    fn carried_type(
        &self,
        reader: &mut Reader<'_>,
    ) -> Option<(type_def::Identity, super::schema::Fingerprint)> {
        let key = reader.text()?.to_owned();
        let digest: [u8; 16] = reader.array()?;
        let (identity, definition) = self
            .definitions
            .iter()
            .find(|(identity, _)| script_key(identity).as_deref() == Some(key.as_str()))?;
        (schema_digest(&definition.schema)? == digest)
            .then(|| (identity.clone(), definition.schema.clone()))
    }
}
