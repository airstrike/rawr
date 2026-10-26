//! The wire format of native islands (D501), on the interpreter's side:
//! values a trampoline sends, rebuilt in this model, and the values it
//! gets back. The shadow crate's generated support speaks the same format
//! (`package::island::SUPPORT`).
//!
//! A value is one tag byte and its payload. Integers are `i64` and floats
//! `f64`, little-endian; a string or a name is its length (`u64`) and its
//! UTF-8 bytes; an array, a struct and an enum variant give their count of
//! members (`u64`) before them. A struct names its type by its path from
//! the crate root (`physics::Ball`) and an enum also its variant's name.

use super::{Model, Shape};
use crate::types::dynamic::Union;
use crate::types::nominal;
use crate::{type_def, Dynamic, ImmutableString};
use std::collections::BTreeMap;

pub(crate) const UNIT: u8 = 0;
pub(crate) const FALSE: u8 = 1;
pub(crate) const TRUE: u8 = 2;
pub(crate) const INT: u8 = 3;
pub(crate) const FLOAT: u8 = 4;
pub(crate) const CHAR: u8 = 5;
pub(crate) const STR: u8 = 6;
pub(crate) const ARRAY: u8 = 7;
pub(crate) const NONE: u8 = 10;
pub(crate) const SOME: u8 = 11;
pub(crate) const STRUCT: u8 = 15;
pub(crate) const ENUM: u8 = 16;
/// A value native code lends for the call (D558.3): its index among the
/// call's lent values.
pub(crate) const LENT: u8 = 20;

/// Nesting deeper than this does not cross.
const DEPTH: usize = 256;

/// The crate's script types by their path from the crate root, found once
/// per generation.
pub(crate) struct Types(BTreeMap<String, type_def::Identity>);

/// Bytes being read.
pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        if self.bytes.len() < length {
            return Err("the arguments ended early".into());
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }

    pub(crate) fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.take(N)?
            .try_into()
            .map_err(|_| "a number is cut short".into())
    }

    pub(crate) fn count(&mut self) -> Result<usize, String> {
        usize::try_from(u64::from_le_bytes(self.array()?))
            .map_err(|_| "a count is too large".into())
    }

    /// A count of members each at least one byte long.
    fn members(&mut self) -> Result<usize, String> {
        let count = self.count()?;
        if count > self.bytes.len() {
            return Err("a count is larger than its members".into());
        }
        Ok(count)
    }

    pub(crate) fn text(&mut self) -> Result<&'a str, String> {
        let length = self.count()?;
        std::str::from_utf8(self.take(length)?).map_err(|_| "a string is not UTF-8".into())
    }
}

fn count(value: usize, out: &mut Vec<u8>) {
    out.extend_from_slice(&(value as u64).to_le_bytes());
}

fn text(value: &str, out: &mut Vec<u8>) {
    count(value.len(), out);
    out.extend_from_slice(value.as_bytes());
}

/// The path from the crate root of a script type: `physics::Ball`.
fn path(identity: &type_def::Identity) -> Option<String> {
    let type_def::Identity::Script(key) = identity else {
        return None;
    };
    Some(
        key.modules()
            .iter()
            .map(ImmutableString::as_str)
            .chain(std::iter::once(key.name()))
            .collect::<Vec<_>>()
            .join("::"),
    )
}

impl Model {
    /// The model's script types by path.
    pub(crate) fn island_types(&self) -> Types {
        Types(
            self.definitions
                .keys()
                .filter_map(|identity| Some((path(identity)?, identity.clone())))
                .collect(),
        )
    }

    /// One value from `reader`, built in this model. `lent` holds the
    /// script values of what the call's trampoline lends.
    pub(crate) fn island_read(
        &self,
        types: &Types,
        lent: &[crate::scope::Lent],
        reader: &mut Reader<'_>,
        depth: usize,
    ) -> Result<Dynamic, String> {
        if depth > DEPTH {
            return Err("a value is nested too deeply".into());
        }
        let depth = depth + 1;
        Ok(match reader.byte()? {
            UNIT => Dynamic::UNIT,
            FALSE => false.into(),
            TRUE => true.into(),
            INT => crate::INT::from_le_bytes(reader.array()?).into(),
            FLOAT => crate::FLOAT::from_le_bytes(reader.array()?).into(),
            CHAR => char::from_u32(u32::from_le_bytes(reader.array()?))
                .ok_or("not a `char`")?
                .into(),
            STR => ImmutableString::from(reader.text()?).into(),
            ARRAY => {
                let length = reader.members()?;
                let mut items = crate::Array::with_capacity(length);
                for _ in 0..length {
                    items.push(self.island_read(types, lent, reader, depth)?);
                }
                Dynamic::from_array(items)
            }
            LENT => {
                let index = reader.count()?;
                Dynamic::from(
                    *lent
                        .get(index)
                        .ok_or("a lent value the call has no place for")?,
                )
            }
            NONE => crate::option::none(),
            SOME => crate::option::some(self.island_read(types, lent, reader, depth)?),
            STRUCT => {
                let name = reader.text()?;
                let identity = types
                    .0
                    .get(name)
                    .ok_or_else(|| format!("the running program has no struct `{name}`"))?;
                let definition = &self.definitions[identity];
                let length = reader.members()?;
                let mut fields = Vec::with_capacity(length);
                for _ in 0..length {
                    fields.push(self.island_read(types, lent, reader, depth)?);
                }
                let proof = self
                    .prove_struct(identity, &definition.schema, fields.into_boxed_slice())
                    .map_err(|error| format!("`{name}`: {error}"))?;
                nominal::Struct::construct(self, identity, &definition.schema, proof)
                    .map_err(|error| format!("`{name}`: {error}"))?
                    .into()
            }
            ENUM => {
                let name = reader.text()?;
                let identity = types
                    .0
                    .get(name)
                    .ok_or_else(|| format!("the running program has no enum `{name}`"))?;
                let definition = &self.definitions[identity];
                let variant_name = reader.text()?;
                let Shape::Enum(variants) = &definition.shape else {
                    return Err(format!("`{name}` is not an enum"));
                };
                let variant = variants
                    .keys()
                    .find(|variant| variant.name() == variant_name)
                    .ok_or_else(|| format!("`{name}` has no variant `{variant_name}`"))?
                    .clone();
                let length = reader.members()?;
                let mut payload = Vec::with_capacity(length);
                for _ in 0..length {
                    payload.push(self.island_read(types, lent, reader, depth)?);
                }
                let proof = self
                    .prove_enum(
                        identity,
                        &definition.schema,
                        &variant,
                        payload.into_boxed_slice(),
                    )
                    .map_err(|error| format!("`{name}::{variant_name}`: {error}"))?;
                nominal::Enum::construct(self, identity, &definition.schema, &variant, proof)
                    .map_err(|error| format!("`{name}::{variant_name}`: {error}"))?
                    .into()
            }
            tag => return Err(format!("unknown value tag {tag}")),
        })
    }

    /// Write `value` for native code; `Err` names what can't cross.
    pub(crate) fn island_write(
        &self,
        value: &Dynamic,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > DEPTH {
            return Err("a value is nested too deeply".into());
        }
        let depth = depth + 1;
        match &value.0 {
            Union::Unit(..) => out.push(UNIT),
            Union::Bool(value, ..) => out.push(if *value { TRUE } else { FALSE }),
            Union::Int(value, ..) => {
                out.push(INT);
                out.extend_from_slice(&(*value).to_le_bytes());
            }
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
            Union::Array(items, ..) => {
                out.push(ARRAY);
                count(items.len(), out);
                for item in items.iter() {
                    self.island_write(item, out, depth)?;
                }
            }
            Union::Struct(value, ..) => {
                out.push(STRUCT);
                text(
                    &path(value.identity()).ok_or("a host struct can't cross")?,
                    out,
                );
                count(value.fields().len(), out);
                for field in value.fields() {
                    self.island_write(field, out, depth)?;
                }
            }
            Union::Enum(value, ..) => {
                out.push(ENUM);
                text(
                    &path(value.identity()).ok_or("a host enum can't cross")?,
                    out,
                );
                text(value.variant().name(), out);
                count(value.payload().len(), out);
                for item in value.payload() {
                    self.island_write(item, out, depth)?;
                }
            }
            Union::Shared(..) => self.island_write(&value.flatten_clone(), out, depth)?,
            Union::Variant(..) => {
                // A lent value can't leave the call it was lent for, in its
                // result or in an argument native code takes back.
                if let Some(lent) = value.read_lock::<crate::scope::Lent>() {
                    return Err(format!(
                        "it keeps the `{}` native code lent it, which is borrowed for that call \
                         only and can't be returned or stored where native code takes it back \
                         (E0515, E0521)",
                        lent.name()
                    ));
                }
                let Some(option) = value.read_lock::<crate::option::Value>() else {
                    return Err(format!("a `{}` can't cross yet", value.type_name()));
                };
                match &*option {
                    None => out.push(NONE),
                    Some(inner) => {
                        out.push(SOME);
                        self.island_write(inner, out, depth)?;
                    }
                }
            }
            _ => return Err(format!("a `{}` can't cross yet", value.type_name())),
        }
        Ok(())
    }
}
