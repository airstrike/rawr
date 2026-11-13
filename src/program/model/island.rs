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
/// A marked value native code lends whole, beside its parts (#1484): the
/// whole's index among the call's lent values, then the value. Native code
/// the call gives the value to by reference is given that lend.
pub(crate) const WHOLE: u8 = 24;
/// A `&str` result in a text the call's receiver copied from native code's
/// value, as `&self.0` of a marked `Id(String)`: the text's index among
/// the receiver's texts in the order native code wrote them, then the
/// start and length of the result in it.
pub(crate) const TEXT_AT: u8 = 25;
/// Before the [`ARRAY`] of a hashed set's keys in native code's order,
/// which the runner sorts by key, as the interpreter keeps a set.
pub(crate) const HASHED_SET: u8 = 26;
/// A hashed map's `[key, value]` entries in native code's order, as
/// [`HASHED_SET`] holds a set's keys.
pub(crate) const HASHED_MAP: u8 = 27;
/// Before the [`STR`] of a `&str` native code gives: the address and
/// length of native code's own text, which a crate call that borrows the
/// program's copy for its output borrows instead (`scope::Pins::pin`).
pub(crate) const KEPT_TEXT: u8 = 28;
/// A program value native code holds as std's trait object, as the
/// `Box<dyn Iterator<Item = T>>` a marked `Header::new` keeps the program's
/// iterator in (#1600): its handle among the kept values. The runner writes
/// it where a member declared so holds the program's own value, and native
/// code holds the value by it; native code that gives it back gives the
/// value back whole, and the runner lets go of the handle.
pub(crate) const KEPT: u8 = 29;
/// A [`KEPT`] value native code lends where it is: the call shares it with
/// native code, which goes on from where the call left it.
pub(crate) const KEPT_LENT: u8 = 30;

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
/// The first byte of a call's header: the function returns `()`, which
/// comes back whatever value its body's last statement left, as the part
/// `self.tabs.remove(index);` removes (#1475).
pub(crate) const UNIT_RESULT: u8 = 16;
/// The first byte of a call's header: the result borrows a marked input,
/// whose key follows the header (#1473). The call runs in a region kept
/// in that input's pins, so the result lives as long as the input is
/// borrowed.
pub(crate) const ANCHORED: u8 = 32;
/// The first byte of a call's header: the call takes marked inputs `&mut`,
/// whose keys follow the header and the anchor's (#1473). What native code
/// borrowed of each is let go of first.
pub(crate) const RELEASES: u8 = 64;
/// The first byte of a call's header: two masks follow the releases' keys.
/// The first names the arguments given for the call alone, as a by-value
/// `View<'a>` of borrows (#1225): what is left of each once the call
/// returns drops then. The second names the lends kept with the call's
/// anchor, as `&'a inbox::State` of `view<'a>(&'a self, inbox: &'a
/// inbox::State) -> Element<'a, Message>` (#1413): what the result
/// borrows of each lives in the anchored region.
pub(crate) const SCOPES: u8 = 128;

/// Nesting deeper than this does not cross.
const DEPTH: usize = 256;

/// How a value is written for native code ([`Model::island_write_in`]):
/// the region a call's result was made in, whose values move to native
/// code, and the call's own lends, a part still lent by one of which goes
/// back as its index among them ([`Model::island_write_back`]).
#[derive(Clone, Copy)]
struct Writing<'a> {
    region: Option<crate::scope::Nonce>,
    own: &'a [crate::scope::Nonce],
}

/// The crate's script types by their path from the crate root, found once
/// per generation.
pub(crate) struct Types {
    paths: BTreeMap<String, type_def::Identity>,
    /// What the native values each member of a script type's value holds
    /// are attested as, by the type and its variant: the `themedef::Theme`
    /// of `Theme::Selected(themedef::Theme)`, which native code gives by
    /// value (#1466), or the `wizard::State` of a field of
    /// `Option<wizard::State>` (#1475).
    parts: Parts,
}

/// [`Types::parts`]: by the type and its variant's name, each member's
/// [`Part`], `None` for a member that holds no crate's type.
type Parts = BTreeMap<(type_def::Identity, Option<ImmutableString>), Box<[Option<Part>]>>;

/// What a native value a value holds is attested as (#1475): its own
/// type's witness, its element's through an `Option` or a `Vec`, and each
/// member's through a tuple. A `Box` is the value it holds.
pub(crate) enum Part {
    /// A native value of the type the witness proves.
    Native(crate::host::runtime::Witness),
    /// An `Option` or a `Vec`, each of whose elements is this part.
    Each(Box<Part>),
    /// A tuple, by its members, `None` for one that holds no crate's type.
    Members(Box<[Option<Part>]>),
    /// A value of a generic script type at the arguments its type gives,
    /// as `Group<'a, grid::Message>` (D869): each member's part, by its
    /// variant, none for a struct's, its parameters' types substituted.
    Script(Variants),
}

/// [`Part::Script`]'s members, by variant.
type Variants = BTreeMap<Option<ImmutableString>, Box<[Option<Part>]>>;

/// How many generic script types deep [`Part::script`] follows a type.
const SCRIPT_DEPTH: usize = 16;

impl Part {
    /// The part of a value of `ty`, whose native values `witness` attests;
    /// `None` when it holds no crate's type. `depth` counts the generic
    /// script types it is inside of.
    fn of(
        ty: &crate::resolve::type_ref::Type,
        model: &Model,
        witness: &impl Fn(&crate::host::runtime::type_ref::Type) -> crate::host::runtime::Witness,
        depth: usize,
    ) -> Option<Self> {
        if let Some(inner) = ty.box_element() {
            return Self::of(inner, model, witness, depth);
        }
        if let Some(inner) = ty
            .option_element()
            .or_else(|| ty.vec_element())
            .or_else(|| ty.iterated_element())
        {
            return Self::of(inner, model, witness, depth).map(|inner| Self::Each(Box::new(inner)));
        }
        match ty {
            crate::resolve::type_ref::Type::Tuple { members, .. } => {
                let members = members
                    .iter()
                    .map(|member| Self::of(member, model, witness, depth))
                    .collect::<Box<[_]>>();
                members
                    .iter()
                    .any(Option::is_some)
                    .then_some(Self::Members(members))
            }
            crate::resolve::type_ref::Type::Path(path)
                if matches!(path.target, crate::resolve::type_ref::Target::Host { .. }) =>
            {
                crate::host::runtime::type_ref::Type::from_resolved_in(ty, model, &[])
                    .ok()
                    .map(|ty| Self::Native(witness(&ty)))
            }
            crate::resolve::type_ref::Type::Path(crate::resolve::type_ref::Path {
                target: crate::resolve::type_ref::Target::Script { identity, .. },
                arguments,
                ..
            }) => Self::script(identity, arguments, model, witness, depth),
            _ => None,
        }
    }

    /// The part of a value of the generic script type `identity` at
    /// `arguments`: each member's, its type's parameters those types.
    /// `None` for a type that isn't generic, whose members' parts are its
    /// own ([`Types::part`]), or that holds no crate's type. A type that
    /// holds itself, as `Tree<T> { children: Vec<Tree<T>> }`, is followed
    /// [`SCRIPT_DEPTH`] deep: a value nested deeper is attested by its
    /// type's own parts.
    fn script(
        identity: &type_def::Identity,
        arguments: &[crate::resolve::type_ref::Argument],
        model: &Model,
        witness: &impl Fn(&crate::host::runtime::type_ref::Type) -> crate::host::runtime::Witness,
        depth: usize,
    ) -> Option<Self> {
        let definition = model.definitions.get(identity)?;
        if definition.generics.is_empty() || depth >= SCRIPT_DEPTH {
            return None;
        }
        let substituted = definition
            .generics
            .iter()
            .cloned()
            .zip(arguments.iter().filter_map(|argument| match argument {
                crate::resolve::type_ref::Argument::Type(ty) => Some(ty.clone()),
                crate::resolve::type_ref::Argument::Lifetime(_) => None,
            }))
            .collect::<Vec<_>>();
        let members = |members: &[crate::resolve::type_ref::Type]| {
            members
                .iter()
                .map(|member| {
                    let member =
                        crate::resolve::type_ref::substitute_parameters(member, &substituted);
                    Self::of(&member, model, witness, depth + 1)
                })
                .collect::<Box<[_]>>()
        };
        let variants = match &definition.shape {
            Shape::Struct(fields) => Variants::from([(None, members(fields))]),
            Shape::Enum(variants) => variants
                .iter()
                .map(|(variant, payload)| (Some(variant.name().into()), members(payload)))
                .collect(),
        };
        variants
            .values()
            .any(|members| members.iter().any(Option::is_some))
            .then_some(Self::Script(variants))
    }

    /// The part of the `index`th member of a value `part` is, of its
    /// `variant` for an enum, when `part` is a generic script type's.
    fn script_member<'p>(
        part: Option<&'p Self>,
        variant: Option<&str>,
        index: usize,
    ) -> Option<Option<&'p Self>> {
        let Self::Script(variants) = part? else {
            return None;
        };
        Some(
            variants
                .get(&variant.map(ImmutableString::from))
                .and_then(|members| members.get(index)?.as_ref()),
        )
    }

    /// The witness of the native value `part` is.
    fn witness(part: Option<&Self>) -> Option<&crate::host::runtime::Witness> {
        match part? {
            Self::Native(witness) => Some(witness),
            Self::Each(_) | Self::Members(_) | Self::Script(_) => None,
        }
    }

    /// The part of the `index`th member of a value `part` is: an
    /// element's, or a tuple member's.
    fn member(part: Option<&Self>, index: usize) -> Option<&Self> {
        match part? {
            Self::Each(inner) => Some(inner),
            Self::Members(members) => members.get(index)?.as_ref(),
            Self::Native(_) | Self::Script(_) => None,
        }
    }
}

impl Types {
    /// The part of the `index`th member of `identity`'s value, of its
    /// `variant` for an enum.
    fn part(
        &self,
        identity: &type_def::Identity,
        variant: Option<&str>,
        index: usize,
    ) -> Option<&Part> {
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

/// Whether `value` is of a struct native code holds by the handle of the
/// program value (D961).
fn held_by_handle(value: &Dynamic) -> bool {
    match &value.0 {
        Union::Struct(structure, ..) => {
            path(structure.identity()).is_some_and(|path| crate::island::handled(&path))
        }
        _ => false,
    }
}

/// `value`, of a struct native code holds by its handle, kept for native
/// code under a handle, which goes out in its place.
fn kept_handle(value: &Dynamic, out: &mut Vec<u8>) {
    out.push(KEPT);
    out.extend_from_slice(&crate::island::kept_handle(value).to_le_bytes());
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

/// The path from the crate root of the script type of `value`, a struct
/// or an enum: what its impls' functions are found by.
pub(crate) fn owner_path(value: &Dynamic) -> Option<String> {
    match &value.0 {
        Union::Struct(nominal, ..) => path(nominal.identity()),
        Union::Enum(nominal, ..) => path(nominal.identity()),
        Union::Shared(..) => owner_path(&value.flatten_clone()),
        _ => None,
    }
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
                .map(|ty| Part::of(ty, self, &witness, 0))
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

    /// What the native values each marked function's by-value parameters
    /// hold are attested as, by whether it belongs to a type and its path:
    /// a bare native one's own type, or each one inside an `Option`, a
    /// `Vec`, a `Box` or a tuple, as `home_grid: Option<Element<'a,
    /// grid::Message>>` holds one (#1224).
    pub(crate) fn island_input_parts(
        &self,
        witness: impl Fn(&crate::host::runtime::type_ref::Type) -> crate::host::runtime::Witness,
    ) -> std::collections::HashMap<(bool, String), Box<[Option<Part>]>> {
        self.island_inputs()
            .iter()
            .map(|(key, types)| {
                let parts = types
                    .iter()
                    .map(|ty| ty.as_ref().and_then(|ty| Part::of(ty, self, &witness, 0)))
                    .collect();
                (key.clone(), parts)
            })
            .collect()
    }

    /// One value from `reader`, built in this model. `lent` holds the
    /// script values of what the call's trampoline lends, and `given` the
    /// native values it gives, each taken once; `part` proves the type of
    /// each given value the value holds, as an argument's does.
    pub(crate) fn island_read(
        &self,
        types: &Types,
        lent: &[crate::scope::Lent],
        given: &mut [Option<crate::island::Given>],
        part: Option<&Part>,
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
            STR => {
                let text = ImmutableString::from(reader.text()?);
                crate::island::read_text(&text);
                text.into()
            }
            KEPT_TEXT => {
                let address = reader.count()?;
                let length = reader.count()?;
                let value = self.island_read(types, lent, given, part, reader, depth)?;
                let text = value
                    .clone()
                    .into_immutable_string()
                    .map_err(|actual| format!("a kept `&str` is a `{actual}`"))?;
                crate::scope::keep_native_text(text, address, length);
                // It outlives what the call makes of it, as rustc says, so a
                // crate call that borrows it makes a value of its own, never
                // one of a region that ends with the call.
                crate::scope::lend_static(value)
            }
            ARRAY => {
                let length = reader.members()?;
                let mut items = crate::Array::with_capacity(length);
                for index in 0..length {
                    let part = Part::member(part, index);
                    items.push(self.island_read(types, lent, given, part, reader, depth)?);
                }
                Dynamic::from_array(items)
            }
            // A hashed set's keys or map's `[key, value]` entries in native
            // code's order, sorted by key as the interpreter keeps them.
            tag @ (HASHED_SET | HASHED_MAP) => {
                let entries = self.island_read(types, lent, given, part, reader, depth)?;
                let mut entries = entries
                    .into_array()
                    .map_err(|actual| format!("a set or a map is a `{actual}`"))?;
                let map = tag == HASHED_MAP;
                let key = |entry: &Dynamic| match entry.as_array_ref() {
                    Ok(pair) if map => pair.first().cloned().unwrap_or_default(),
                    _ => entry.clone(),
                };
                entries.sort_by(|lhs, rhs| {
                    self.island_order(&key(lhs), &key(rhs))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                Dynamic::from_array(entries)
            }
            LENT => {
                let index = reader.count()?;
                Dynamic::from(
                    *lent
                        .get(index)
                        .ok_or("a lent value the call has no place for")?,
                )
            }
            KEPT => crate::island::unkeep(u64::from_le_bytes(reader.array()?))?,
            KEPT_LENT => crate::island::kept_shared(u64::from_le_bytes(reader.array()?))?,
            WHOLE => {
                let index = reader.count()?;
                let whole = *lent
                    .get(index)
                    .ok_or("a lent value the call has no place for")?;
                let value = self.island_read(types, lent, given, part, reader, depth)?;
                crate::island::lent_whole(&value, whole);
                value
            }
            NATIVE => {
                let index = reader.count()?;
                let value = given
                    .get_mut(index)
                    .and_then(Option::take)
                    .ok_or("a native value the call has no place for")?;
                crate::island::lodged(value, Part::witness(part))?
            }
            NONE => crate::option::none(),
            SOME => {
                let part = Part::member(part, 0);
                crate::option::some(self.island_read(types, lent, given, part, reader, depth)?)
            }
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
                    let part = Part::script_member(part, None, index)
                        .unwrap_or_else(|| types.part(identity, None, index));
                    fields.push(self.island_read(types, lent, given, part, reader, depth)?);
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
                    let part = Part::script_member(part, Some(variant_name), index)
                        .unwrap_or_else(|| types.part(identity, Some(variant_name), index));
                    payload.push(self.island_read(types, lent, given, part, reader, depth)?);
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
    /// a part still lent by one of `own`, the call's own lends, goes back
    /// as its index among them, by which native code finds its place, where
    /// the call left it (#1475).
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
                // The program's copy of a `&str` native code gave the call
                // goes back with where native code's text is, which a
                // `&'a str` native code takes back is.
                if let Some((address, length)) = crate::scope::native_text(value) {
                    out.push(KEPT_TEXT);
                    count(address, out);
                    count(length, out);
                }
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
            Union::Struct(..) if held_by_handle(value) => kept_handle(value, out),
            Union::Struct(value, ..) => {
                out.push(STRUCT);
                text(
                    &path(value.identity()).ok_or("a host struct can't cross")?,
                    out,
                );
                count(value.fields().len(), out);
                let declared = match self
                    .definitions
                    .get(value.identity())
                    .map(|definition| &definition.shape)
                {
                    Some(Shape::Struct(fields)) => &fields[..],
                    _ => &[],
                };
                for (index, field) in value.fields().iter().enumerate() {
                    let ty = declared.get(index);
                    self.island_write_member(field, ty, writing, out, natives, depth)?;
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
                let declared = match self
                    .definitions
                    .get(value.identity())
                    .map(|definition| &definition.shape)
                {
                    Some(Shape::Enum(variants)) => variants
                        .get(value.variant())
                        .map_or(&[][..], |payload| &payload[..]),
                    _ => &[],
                };
                for (index, item) in value.payload().iter().enumerate() {
                    let ty = declared.get(index);
                    self.island_write_member(item, ty, writing, out, natives, depth)?;
                }
            }
            // A shared value native code holds by its handle is the one the
            // handle keeps, when it keeps one.
            Union::Shared(..) => {
                let flat = value.flatten_clone();
                if held_by_handle(&flat) {
                    kept_handle(value, out);
                } else {
                    self.island_write_with(&flat, writing, out, natives, depth)?;
                }
            }
            Union::Variant(..) => {
                // A lent value can't leave the call it was lent for, in its
                // result or in an argument native code takes back.
                if let Some(lent) = value.read_lock::<crate::scope::Lent>() {
                    // Native code finds the part's place by its index among
                    // the call's lends, where it moved to (#1475).
                    if let Some(index) = writing.own.iter().position(|own| *own == lent.nonce()) {
                        out.push(LENT);
                        count(index, out);
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

    /// [`Model::island_write_with`] of a member of a struct or a variant,
    /// declared `ty`: a `Box<dyn Iterator<Item = T>>` that holds the
    /// program's own iterator, as inboard's `Header::actions` holds the
    /// `Box::new(actions.into_iter())` its marked `new` made, is kept under
    /// a handle, which native code pulls from (#1600). A native one moves
    /// as any native value does.
    fn island_write_member(
        &self,
        value: &Dynamic,
        ty: Option<&crate::resolve::type_ref::Type>,
        writing: Writing<'_>,
        out: &mut Vec<u8>,
        natives: &mut Natives,
        depth: usize,
    ) -> Result<(), String> {
        let pulled = ty
            .and_then(crate::resolve::type_ref::Type::box_element)
            .and_then(crate::resolve::type_ref::Type::iterator_item)
            .is_some_and(|(_, into)| !into);
        if pulled && !crate::island::native(value) {
            out.push(KEPT);
            out.extend_from_slice(&crate::island::kept_handle(value).to_le_bytes());
            return Ok(());
        }
        self.island_write_with(value, writing, out, natives, depth)
    }
}
