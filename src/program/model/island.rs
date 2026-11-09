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
//! A native value, such as the `iced::Error` in an `iced::Result`, moves
//! itself to native code: its tag names its place among the reply's
//! [`Natives`]. One native code gives by value moves to the interpreter
//! the same way, its tag naming its place among the call's given values.

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
pub(crate) const OK: u8 = 12;
pub(crate) const ERR: u8 = 13;
pub(crate) const STRUCT: u8 = 15;
pub(crate) const ENUM: u8 = 16;
/// A value native code lends for the call (D558.3): its index among the
/// call's lent values.
pub(crate) const LENT: u8 = 20;
/// A native value the reply moves to native code: its index among the
/// reply's [`Natives`]; or one the call gives the interpreter, its index
/// among the call's given values.
pub(crate) const NATIVE: u8 = 21;
/// The receiver of a call of a native trait object's stand-in (#1264):
/// the handle of the program value it holds, which the call takes from the
/// kept values and puts back after (`island::object`).
pub(crate) const HANDLE: u8 = 22;
/// A stand-in's `&str` result that borrows its receiver (#1322): the
/// address and length of the program's own text, pinned with the program
/// value till it is next taken `&mut` (`island::Held::pins`).
pub(crate) const PINNED: u8 = 23;

/// The first byte of a call's header: the function is a member of the
/// type before its last segment.
pub(crate) const OWNED: u8 = 1;
/// The first byte of a call's header: the result is a `&str` that borrows
/// the stand-in's receiver, which comes back [`PINNED`].
pub(crate) const BORROWS: u8 = 2;
/// The first byte of a call's header: the result is a reference to a
/// native value, which comes back where the value is (#1442).
pub(crate) const IN_PLACE: u8 = 4;
/// With [`IN_PLACE`]: the reference is a `&'static`, which only a value
/// that lives as long as the program fills.
pub(crate) const LASTING: u8 = 8;

/// Nesting deeper than this does not cross.
const DEPTH: usize = 256;

/// How a value is written for native code ([`Model::island_write_in`]):
/// the region a call's result was made in, whose values move to native
/// code, and the call's own lends, a part still lent by one of which goes
/// back as `()` ([`Model::island_write_back`]).
#[derive(Clone, Copy)]
struct Writing<'a> {
    region: Option<crate::scope::Nonce>,
    own: &'a [crate::scope::Nonce],
}

/// The crate's script types by their path from the crate root, found once
/// per generation.
pub(crate) struct Types {
    paths: BTreeMap<String, type_def::Identity>,
    /// What each member of a script type's value that is a crate's type is
    /// attested as, by the type and its variant: the `themedef::Theme` of
    /// `Theme::Selected(themedef::Theme)`, which native code gives by value
    /// (#1466).
    parts: Parts,
}

/// [`Types::parts`]: by the type and its variant's name, each member's
/// witness, `None` for a member of no crate's type.
type Parts = BTreeMap<
    (type_def::Identity, Option<ImmutableString>),
    Box<[Option<crate::host::runtime::Witness>]>,
>;

impl Types {
    /// The witness of the `index`th member of `identity`'s value, of its
    /// `variant` for an enum.
    fn part(
        &self,
        identity: &type_def::Identity,
        variant: Option<&str>,
        index: usize,
    ) -> Option<&crate::host::runtime::Witness> {
        self.parts
            .get(&(identity.clone(), variant.map(ImmutableString::from)))?
            .get(index)?
            .as_ref()
    }
}

/// The native values a reply moves to native code, in the order its
/// [`NATIVE`] tags name them.
pub(crate) type Natives = Vec<Box<dyn std::any::Any>>;

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
    /// `lhs` and `rhs` ordered as a derived `PartialOrd` orders them, for a
    /// native trait object's stand-in (#1301): numbers, strings, chars,
    /// bools and `()`, `Option` and `Result`, arrays and tuples element by
    /// element, a struct's fields in order and an enum's variants in
    /// declaration order, then their payloads. `None` when unordered.
    pub(crate) fn island_order(&self, lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        let lexicographic = |lhs: &[Dynamic], rhs: &[Dynamic]| {
            for (lhs, rhs) in lhs.iter().zip(rhs) {
                match self.island_order(lhs, rhs)? {
                    Ordering::Equal => {}
                    ordering => return Some(ordering),
                }
            }
            Some(lhs.len().cmp(&rhs.len()))
        };
        if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
            return match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => self.island_order(&lhs, &rhs),
                (lhs, rhs) => Some(lhs.is_some().cmp(&rhs.is_some())),
            };
        }
        if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
            return match (lhs, rhs) {
                (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => self.island_order(&lhs, &rhs),
                (lhs, rhs) => Some(lhs.is_err().cmp(&rhs.is_err())),
            };
        }
        match (&lhs.0, &rhs.0) {
            (Union::Shared(..), _) | (_, Union::Shared(..)) => {
                self.island_order(&lhs.flatten_clone(), &rhs.flatten_clone())
            }
            (Union::Unit(..), Union::Unit(..)) => Some(Ordering::Equal),
            (Union::Bool(lhs, ..), Union::Bool(rhs, ..)) => Some(lhs.cmp(rhs)),
            (Union::Char(lhs, ..), Union::Char(rhs, ..)) => Some(lhs.cmp(rhs)),
            (Union::Int(lhs, tag, ..), Union::Int(rhs, other, ..))
                if *tag == crate::types::dynamic::UNSIGNED_TAG
                    || *other == crate::types::dynamic::UNSIGNED_TAG =>
            {
                Some((*lhs as u64).cmp(&(*rhs as u64)))
            }
            (Union::Int(lhs, ..), Union::Int(rhs, ..)) => Some(lhs.cmp(rhs)),
            (Union::Str(lhs, ..), Union::Str(rhs, ..)) => Some(lhs.as_str().cmp(rhs.as_str())),
            (Union::Float(lhs, ..), Union::Float(rhs, ..)) => (**lhs).partial_cmp(&**rhs),
            (Union::Array(lhs, ..), Union::Array(rhs, ..)) => lexicographic(lhs, rhs),
            (Union::Variant(..), Union::Variant(..)) => crate::host::value::host_compare(lhs, rhs),
            (Union::Struct(lhs, ..), Union::Struct(rhs, ..))
                if lhs.identity() == rhs.identity() =>
            {
                lexicographic(lhs.fields(), rhs.fields())
            }
            (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) if lhs.identity() == rhs.identity() => {
                let order = &self.definitions.get(lhs.identity())?.names.order;
                let index = |variant: &nominal::variant::Id| {
                    order.iter().position(|declared| declared == variant)
                };
                match index(lhs.variant())?.cmp(&index(rhs.variant())?) {
                    Ordering::Equal => lexicographic(lhs.payload(), rhs.payload()),
                    ordering => Some(ordering),
                }
            }
            _ => None,
        }
    }

    /// Feed `value` to `state` as a derived `Hash` does, for a native trait
    /// object's stand-in (#1301): each field or element in order, an enum's
    /// variant first. `Err` names what can't be hashed.
    pub(crate) fn island_hash(
        &self,
        value: &Dynamic,
        state: &mut impl std::hash::Hasher,
    ) -> Result<(), String> {
        use std::hash::Hash;
        match &value.0 {
            Union::Shared(..) => self.island_hash(&value.flatten_clone(), state),
            Union::Unit(..)
            | Union::Bool(..)
            | Union::Str(..)
            | Union::Char(..)
            | Union::Int(..)
            | Union::Float(..) => {
                value.hash(state);
                Ok(())
            }
            Union::Array(elements, ..) => {
                elements.len().hash(state);
                elements
                    .iter()
                    .try_for_each(|element| self.island_hash(element, state))
            }
            Union::Struct(nominal, ..) => nominal
                .fields()
                .iter()
                .try_for_each(|field| self.island_hash(field, state)),
            Union::Enum(nominal, ..) => {
                nominal.variant().hash(state);
                nominal
                    .payload()
                    .iter()
                    .try_for_each(|member| self.island_hash(member, state))
            }
            _ => match (crate::option::read(value), crate::result::read(value)) {
                (Some(option), _) => {
                    option.is_some().hash(state);
                    option.map_or(Ok(()), |inner| self.island_hash(&inner, state))
                }
                (_, Some(result)) => {
                    result.is_ok().hash(state);
                    let (Ok(inner) | Err(inner)) = result;
                    self.island_hash(&inner, state)
                }
                _ => Err(format!("a `{}` can't be hashed yet", value.type_name())),
            },
        }
    }

    /// The model's script types by path, with what `witness` gives each
    /// member of a crate's type.
    pub(crate) fn island_types(
        &self,
        witness: impl Fn(&crate::host::runtime::type_ref::Type) -> crate::host::runtime::Witness,
    ) -> Types {
        let members = |members: &[crate::resolve::type_ref::Type]| {
            members
                .iter()
                .map(|ty| {
                    let crate::resolve::type_ref::Type::Path(path) = ty else {
                        return None;
                    };
                    if !matches!(path.target, crate::resolve::type_ref::Target::Host { .. }) {
                        return None;
                    }
                    crate::host::runtime::type_ref::Type::from_resolved_in(ty, self, &[])
                        .ok()
                        .map(|ty| witness(&ty))
                })
                .collect::<Box<[_]>>()
        };
        let mut parts = Parts::new();
        for (identity, definition) in &self.definitions {
            match &definition.shape {
                Shape::Struct(fields) => {
                    parts.insert((identity.clone(), None), members(fields));
                }
                Shape::Enum(variants) => {
                    for (variant, payload) in variants {
                        parts.insert(
                            (identity.clone(), Some(variant.name().into())),
                            members(payload),
                        );
                    }
                }
            }
        }
        parts.retain(|_, members| members.iter().any(Option::is_some));
        Types {
            paths: self
                .definitions
                .keys()
                .filter_map(|identity| Some((path(identity)?, identity.clone())))
                .collect(),
            parts,
        }
    }

    /// One value from `reader`, built in this model. `lent` holds the
    /// script values of what the call's trampoline lends, and `given` the
    /// native values it gives, each taken once; `witness` proves the type a
    /// given value is, which an argument is.
    pub(crate) fn island_read(
        &self,
        types: &Types,
        lent: &[crate::scope::Lent],
        given: &mut [Option<crate::island::Given>],
        witness: Option<&crate::host::runtime::Witness>,
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
                    items.push(self.island_read(types, lent, given, None, reader, depth)?);
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
            NATIVE => {
                let index = reader.count()?;
                let value = given
                    .get_mut(index)
                    .and_then(Option::take)
                    .ok_or("a native value the call has no place for")?;
                crate::island::lodged(value, witness)?
            }
            NONE => crate::option::none(),
            SOME => crate::option::some(self.island_read(types, lent, given, None, reader, depth)?),
            OK => crate::result::ok(self.island_read(types, lent, given, None, reader, depth)?),
            ERR => crate::result::err(self.island_read(types, lent, given, None, reader, depth)?),
            STRUCT => {
                let name = reader.text()?;
                let identity = types
                    .paths
                    .get(name)
                    .ok_or_else(|| format!("the running program has no struct `{name}`"))?;
                let definition = &self.definitions[identity];
                let length = reader.members()?;
                let mut fields = Vec::with_capacity(length);
                for index in 0..length {
                    let witness = types.part(identity, None, index);
                    fields.push(self.island_read(types, lent, given, witness, reader, depth)?);
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
                    .paths
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
                for index in 0..length {
                    let witness = types.part(identity, Some(variant_name), index);
                    payload.push(self.island_read(types, lent, given, witness, reader, depth)?);
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

    /// Write `value` for native code, moving each native value it holds
    /// to `natives`; `Err` names what can't cross. A call's result made in
    /// the region `region`, which the caller keeps for as long as native
    /// code holds what it takes (`island::Calls::call`): a value of that
    /// region, as a `view(&self)`'s `Element<'_, Message>`, moves to native
    /// code as a native value does (#1321). A value of any other region
    /// ends with it.
    pub(crate) fn island_write_in(
        &self,
        value: &Dynamic,
        region: Option<crate::scope::Nonce>,
        out: &mut Vec<u8>,
        natives: &mut Natives,
        depth: usize,
    ) -> Result<(), String> {
        let writing = Writing { region, own: &[] };
        self.island_write_with(value, writing, out, natives, depth)
    }

    /// [`Model::island_write_in`] of a `&mut` argument native code takes back
    /// after the call, whose parts it lent the call where they are (#1469):
    /// a part still lent by one of `own`, the call's own lends, is still
    /// where it was, and goes back as `()`, as an unchanged lent `&mut`
    /// does.
    pub(crate) fn island_write_back(
        &self,
        value: &Dynamic,
        own: &[crate::scope::Nonce],
        out: &mut Vec<u8>,
        natives: &mut Natives,
    ) -> Result<(), String> {
        let writing = Writing { region: None, own };
        self.island_write_with(value, writing, out, natives, 0)
    }

    fn island_write_with(
        &self,
        value: &Dynamic,
        writing: Writing<'_>,
        out: &mut Vec<u8>,
        natives: &mut Natives,
        depth: usize,
    ) -> Result<(), String> {
        let region = writing.region;
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
                    self.island_write_with(item, writing, out, natives, depth)?;
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
                    self.island_write_with(field, writing, out, natives, depth)?;
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
                    self.island_write_with(item, writing, out, natives, depth)?;
                }
            }
            Union::Shared(..) => {
                self.island_write_with(&value.flatten_clone(), writing, out, natives, depth)?
            }
            Union::Variant(..) => {
                // A lent value can't leave the call it was lent for, in its
                // result or in an argument native code takes back.
                if let Some(lent) = value.read_lock::<crate::scope::Lent>() {
                    if writing.own.contains(&lent.nonce()) {
                        out.push(UNIT);
                        return Ok(());
                    }
                    return Err(format!(
                        "it keeps the `{}` native code lent it, which is borrowed for that call \
                         only and can't be returned or stored where native code takes it back \
                         (E0515, E0521)",
                        lent.name()
                    ));
                }
                if let Some(scoped) = value.read_lock::<crate::scope::Scoped>().map(|held| *held) {
                    if region != Some(scoped.nonce()) {
                        return Err(format!(
                            "it is a `{}` that borrows what ends with the call (E0515)",
                            scoped.name()
                        ));
                    }
                    // SAFETY: the region is the call's, which its caller
                    // keeps for as long as native code holds what it takes.
                    let native = crate::scope::with::<crate::scope::RegionF, _>(
                        scoped.nonce(),
                        |region| unsafe { region.take_native(scoped) },
                    )
                    .and_then(|taken| taken)
                    .map_err(|error| error.to_string())?;
                    out.push(NATIVE);
                    count(natives.len(), out);
                    natives.push(native);
                } else if let Some(option) = value.read_lock::<crate::option::Value>() {
                    match &*option {
                        None => out.push(NONE),
                        Some(inner) => {
                            out.push(SOME);
                            self.island_write_with(inner, writing, out, natives, depth)?;
                        }
                    }
                } else if let Some(result) = value.read_lock::<crate::result::Value>() {
                    let (tag, inner) = match &*result {
                        Ok(inner) => (OK, inner),
                        Err(inner) => (ERR, inner),
                    };
                    out.push(tag);
                    self.island_write_with(inner, writing, out, natives, depth)?;
                } else {
                    let native = crate::island::moved(value)
                        .ok_or_else(|| format!("a `{}` can't cross yet", value.type_name()))??;
                    out.push(NATIVE);
                    count(natives.len(), out);
                    natives.push(native);
                }
            }
            _ => return Err(format!("a `{}` can't cross yet", value.type_name())),
        }
        Ok(())
    }
}
