//! Match exhaustiveness: the patterns a match covers, specialised by
//! constructor, and the witnesses of what it leaves out.

use super::models::Nominal;
use super::op::ordering_discriminant;
use super::pattern;
use super::pattern::Prelude;
use super::ty::{Int, Ty};
use super::{cow, instantiate, origin, text_end, without_references, Parser};
use crate::resolve::{BindingSite, Error};
use crate::types::nominal;
use crate::{ast, type_def, ImmutableString, Span};

/// One constructor of a matched type: an enum variant, or a prelude
/// constructor (`Some`, `None`, `Ok`, `Err`).
#[derive(Clone, PartialEq)]
pub(super) enum Key {
    Enum {
        identity: type_def::Identity,
        variant: nominal::variant::Id,
    },
    Prelude(Prelude),
    /// `Cow::Owned` (`true`) or `Cow::Borrowed`.
    Cow(bool),
    /// A literal: of `bool`, one of its two values; of an integer or a
    /// `char`, the range of that one value (`Scalar::key_ordinals`);
    /// otherwise one of unboundedly many.
    Literal(ast::NominalLiteral),
    /// A range pattern's bounds: of an integer or a `char`, the values
    /// between them (`Scalar::key_ordinals`); of a float, never a complete
    /// set of values on its own.
    Range {
        low: Option<ast::NominalLiteral>,
        high: Option<ast::NominalLiteral>,
        inclusive: bool,
    },
    /// The integers or `char`s whose ordinals are `low..=high`: one of the
    /// ranges the patterns of a column split their type into, which each
    /// of those patterns matches whole or not at all, as rustc splits them
    /// (`Parser::column_constructors`).
    Ordinals {
        scalar: Scalar,
        low: u128,
        high: u128,
    },
    /// A crate value's part tested at run time, which, as a range, is
    /// never a complete set of values on its own (`host_coverage`).
    Tested,
    /// A tuple's only constructor.
    Tuple,
    /// A slice pattern: its payload is its `before` leading elements, then
    /// the trailing ones, with any number between when `rest` (`..`).
    Slice {
        before: usize,
        rest: bool,
    },
    /// An array's or slice's constructor: exactly `length` elements, or,
    /// when `open`, that many or more (Rust's variable-length slice).
    SliceLength {
        length: usize,
        open: bool,
    },
}

/// The payload of a slice pattern (`before` leading elements, `rest` its
/// `..`) under a `SliceLength` constructor, or `None` where it does not
/// match: a `..` stands for the wildcards that fill the length.
pub(super) fn specialize_slice(
    before: usize,
    rest: bool,
    payload: &[Pattern],
    length: usize,
    open: bool,
) -> Option<Vec<Pattern>> {
    if !rest {
        return (!open && payload.len() == length).then(|| payload.to_vec());
    }
    if payload.len() > length {
        return None;
    }
    let mut specialized = payload[..before].to_vec();
    specialized.extend(std::iter::repeat_n(
        Pattern::Wildcard,
        length - payload.len(),
    ));
    specialized.extend_from_slice(&payload[before..]);
    Some(specialized)
}

/// The `..` of a tuple-like pattern's `members`: its index, with Rust's
/// errors for a second one and for `name @ ..` outside a slice.
pub(super) fn tuple_rest(members: &[pattern::Syntax]) -> Result<Option<usize>, Error> {
    let mut found: Option<(usize, Span)> = None;
    for (index, member) in members.iter().enumerate() {
        let pattern::Syntax::Rest { binding, span } = member else {
            continue;
        };
        if let Some((name, _)) = binding {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`{name} @` in a tuple pattern (`{name} @ ..` is not allowed in a tuple)"
                )
                .into(),
                source: *span,
            });
        }
        if let Some((_, previous)) = found {
            return Err(Error::RepeatedRest {
                slice: false,
                previous,
                rest: *span,
            });
        }
        found = Some((index, *span));
    }
    Ok(found.map(|(index, _)| index))
}

/// `members` of a tuple-like pattern of `arity` elements, its `..` read as
/// the `discard`s that fill the arity: Rust's `(a, .., z)`.
pub(super) fn tuple_members<'a>(
    members: &'a [pattern::Syntax],
    arity: usize,
    discard: &'a pattern::Syntax,
) -> Result<Vec<&'a pattern::Syntax>, Error> {
    let Some(rest) = tuple_rest(members)? else {
        return Ok(members.iter().collect());
    };
    let fill = (arity + 1).saturating_sub(members.len());
    Ok(members[..rest]
        .iter()
        .chain(std::iter::repeat_n(discard, fill))
        .chain(&members[rest + 1..])
        .collect())
}

/// The payload of `pattern` under `constructor`: a slice pattern's
/// elements for one of its lengths, else a matching key's payload.
pub(super) fn specialize_coverage_pattern(
    pattern: &Pattern,
    constructor: &Constructor,
) -> Option<Vec<Pattern>> {
    match (pattern, &constructor.key) {
        (Pattern::Wildcard, _) => Some(vec![Pattern::Wildcard; constructor.payload.len()]),
        (
            Pattern::Constructor {
                key: Key::Slice { before, rest },
                payload,
                ..
            },
            Key::SliceLength { length, open },
        ) => specialize_slice(*before, *rest, payload, *length, *open),
        // A literal or a range matches each range of its column it holds.
        (Pattern::Constructor { key, .. }, Key::Ordinals { scalar, low, high }) => scalar
            .key_ordinals(key)
            .filter(|(from, to)| from <= low && high <= to)
            .map(|_| Vec::new()),
        (Pattern::Constructor { key, payload, .. }, constructor_key) if key == constructor_key => {
            Some(payload.to_vec())
        }
        (Pattern::Constructor { .. }, _) => None,
        (Pattern::Or(_), _) => unreachable!("or-patterns are expanded first"),
    }
}

#[derive(Clone)]
pub(super) enum Pattern {
    Wildcard,
    Constructor {
        key: Key,
        /// `Enum::Variant`, `Some`, `None`, a literal, or `(..)`.
        name: ImmutableString,
        payload: Box<[Pattern]>,
    },
    /// `p | q`, expanded before coverage is checked.
    Or(Box<[Pattern]>),
}

/// For each declared field, the index of the written field pattern that
/// names it, `None` where `..` skips it; Rust's errors for an unknown field
/// (E0026, of `variant` where the pattern is one of `owner`'s variants)
/// and, without `..`, a missing one.
pub(super) fn field_order<'a>(
    owner: &str,
    variant: Option<&str>,
    declared: impl Iterator<Item = &'a str>,
    written: &[(ImmutableString, Span)],
    rest: bool,
    pattern: Span,
) -> Result<Vec<Option<usize>>, Error> {
    let declared = declared.collect::<Vec<_>>();
    if let Some((unknown, reference)) = written
        .iter()
        .find(|(name, _)| !declared.contains(&name.as_str()))
    {
        // rustc offers a similar name only for the one field the pattern
        // leaves unmentioned.
        let unmentioned = declared
            .iter()
            .filter(|field| !written.iter().any(|(name, _)| name.as_str() == **field))
            .map(|field| ImmutableString::from(*field))
            .collect::<Vec<_>>();
        let candidates = if unmentioned.len() == 1 {
            unmentioned
        } else {
            Vec::new()
        };
        return Err(Error::UnknownField {
            owner: crate::resolve::FieldOwner {
                variant: variant.is_some(),
                name: match variant {
                    Some(variant) => format!("{owner}::{variant}").into(),
                    None => owner.into(),
                },
            },
            field: unknown.clone(),
            reference: Span::new(reference.start(), text_end(reference.start(), unknown)),
            pattern: true,
            hint: crate::resolve::FieldHint::new(unknown, candidates, true),
        });
    }
    declared
        .into_iter()
        .map(|field| {
            let index = written.iter().position(|(name, _)| name.as_str() == field);
            if index.is_none() && !rest {
                return Err(Error::InvalidMatch {
                    owner: owner.into(),
                    reason: format!("pattern does not mention field `{field}`").into(),
                    pattern,
                });
            }
            Ok(index)
        })
        .collect()
}

/// Rust rejects a range pattern that matches nothing (`9..=1`, `3..3`).
pub(super) fn reject_empty_range(
    low: Option<&(ast::NominalLiteral, Ty)>,
    high: Option<&(ast::NominalLiteral, Ty)>,
    inclusive: bool,
    owner: &str,
    pattern: Span,
) -> Result<(), Error> {
    let (Some((low, _)), Some((high, _))) = (low, high) else {
        return Ok(());
    };
    let ordering = match (low, high) {
        (ast::NominalLiteral::Int(low), ast::NominalLiteral::Int(high)) => low.cmp(high),
        (ast::NominalLiteral::Char(low), ast::NominalLiteral::Char(high)) => low.cmp(high),
        (ast::NominalLiteral::Float(low), ast::NominalLiteral::Float(high)) => {
            low.partial_cmp(high).unwrap_or(std::cmp::Ordering::Greater)
        }
        _ => return Ok(()),
    };
    if ordering == std::cmp::Ordering::Greater || (!inclusive && ordering.is_eq()) {
        return Err(Error::InvalidMatch {
            owner: owner.into(),
            reason: "lower range bound must be less than or equal to upper".into(),
            pattern,
        });
    }
    Ok(())
}

/// A pattern literal as the lowered script spells it.
pub(super) fn scalar_text(literal: &ast::NominalLiteral) -> String {
    match literal {
        ast::NominalLiteral::Int(value) if *value < 0 => format!("({value})"),
        ast::NominalLiteral::Float(value) if **value < 0.0 => format!("({:?})", **value),
        literal => render_literal(literal),
    }
}

/// The `bool` literals a pattern names, in `[false, true]`, when it has
/// no other alternative.
pub(super) fn bool_literals(syntax: &pattern::Syntax, seen: &mut [bool; 2]) {
    match syntax {
        pattern::Syntax::Literal {
            literal: ast::NominalLiteral::Bool(value),
            ..
        } => seen[usize::from(*value)] = true,
        pattern::Syntax::Or { alternatives, .. } => {
            for alternative in alternatives.iter() {
                bool_literals(alternative, seen);
            }
        }
        pattern::Syntax::At { pattern, .. } => bool_literals(pattern, seen),
        _ => {}
    }
}

/// The type of a match on integers or `char`s, whose arms' values rawr
/// reads as ranges to name the values no arm matches as rustc does
/// (E0004). Values are ordinals: an integer's in the order of its kind,
/// a `char`'s its scalar value.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Scalar {
    Int(Int),
    Char,
}

impl Scalar {
    /// The sign bit, which orders a signed integer's ordinals.
    pub(super) const SIGN: u128 = 1 << 127;

    /// The scalar kind of `ty`; an integer whose kind is still open is
    /// rustc's fallback `i32`.
    pub(super) fn of(ty: &Ty) -> Option<Self> {
        match ty {
            Ty::Int(Int::Var(_)) => Some(Self::Int(Int::I32)),
            Ty::Int(kind) => Some(Self::Int(*kind)),
            Ty::Char => Some(Self::Char),
            _ => None,
        }
    }

    /// The lowest and highest ordinal of the type.
    pub(super) fn bounds(self) -> (u128, u128) {
        match self {
            Self::Int(kind) if kind.signed() => (
                (i128::MIN >> (128 - kind.bits())) as u128 ^ Self::SIGN,
                (i128::MAX >> (128 - kind.bits())) as u128 ^ Self::SIGN,
            ),
            Self::Int(kind) => (0, u128::MAX >> (128 - kind.bits())),
            Self::Char => (0, u128::from(u32::from(char::MAX))),
        }
    }

    /// The ordinal of `literal`, if it is one of the type's.
    pub(super) fn ordinal(self, literal: &ast::NominalLiteral) -> Option<u128> {
        match (self, literal) {
            (Self::Int(kind), ast::NominalLiteral::Int(value)) => {
                let value = kind.widen(*value);
                if kind.signed() {
                    Some(value as u128 ^ Self::SIGN)
                } else {
                    u128::try_from(value).ok()
                }
            }
            (Self::Char, ast::NominalLiteral::Char(value)) => Some(u128::from(u32::from(*value))),
            _ => None,
        }
    }

    /// The value of `ordinal` as rustc writes it in a witness: `5_i32`,
    /// `'a'`.
    pub(super) fn value(self, ordinal: u128) -> String {
        match self {
            Self::Int(kind) if kind.signed() => {
                format!("{}_{}", (ordinal ^ Self::SIGN) as i128, kind.name())
            }
            Self::Int(kind) => format!("{ordinal}_{}", kind.name()),
            Self::Char => {
                let value = u32::try_from(ordinal).ok().and_then(char::from_u32);
                format!("{:?}", value.unwrap_or(char::REPLACEMENT_CHARACTER))
            }
        }
    }

    /// rustc's witness for the ordinals `low..=high`: `i32::MIN..=0_i32`,
    /// `2_i32`, `u8::MAX`, `'a'..='z'`. A pointer-sized or 128-bit kind's
    /// top is open, as `1_usize..`, and an `isize`'s bottom, as
    /// `..=-1_isize`.
    pub(super) fn witness(self, low: u128, high: u128) -> String {
        let (min, max) = self.bounds();
        let Self::Int(kind) = self else {
            return if low == high {
                self.value(low)
            } else {
                format!("{}..={}", self.value(low), self.value(high))
            };
        };
        let open_low = low == min && matches!(kind, Int::Isize);
        let open_high =
            high == max && matches!(kind, Int::Isize | Int::Usize | Int::I128 | Int::U128);
        // An end at the kind's bound is its constant, `i32::MIN` or
        // `u8::MAX`; an unsigned kind's bottom is `0`.
        let end = |ordinal: u128| {
            if ordinal == max {
                format!("{}::MAX", kind.name())
            } else if ordinal == min && kind.signed() {
                format!("{}::MIN", kind.name())
            } else {
                self.value(ordinal)
            }
        };
        match (open_low, open_high) {
            (true, true) => "_".to_string(),
            (false, true) => format!("{}..", end(low)),
            (true, false) => format!("..={}", end(high)),
            (false, false) if low == high => end(low),
            (false, false) => format!("{}..={}", end(low), end(high)),
        }
    }

    /// The ordinals `syntax` matches, added to `covered`; `None` for a
    /// pattern whose values rawr doesn't read, as a constant's path.
    pub(super) fn covered(
        self,
        syntax: &pattern::Syntax,
        covered: &mut Vec<(u128, u128)>,
    ) -> Option<()> {
        match syntax {
            pattern::Syntax::Literal { literal, .. } => {
                let ordinal = self.ordinal(literal)?;
                covered.push((ordinal, ordinal));
            }
            pattern::Syntax::Range {
                low,
                high,
                inclusive,
                ..
            } => covered.push(self.range(
                low.as_ref().map(|(low, _)| low),
                high.as_ref().map(|(high, _)| high),
                *inclusive,
            )?),
            pattern::Syntax::Binding { .. } | pattern::Syntax::Discard { .. } => {
                covered.push(self.bounds());
            }
            pattern::Syntax::Or { alternatives, .. } => {
                for alternative in alternatives.iter() {
                    self.covered(alternative, covered)?;
                }
            }
            pattern::Syntax::At { pattern, .. } | pattern::Syntax::Reference { pattern, .. } => {
                self.covered(pattern, covered)?;
            }
            _ => return None,
        }
        Some(())
    }

    /// The ordinals of the range pattern `low..high`, or `low..=high` when
    /// `inclusive`, an open end the type's bound; `None` for one that
    /// matches nothing or whose ends aren't the type's.
    fn range(
        self,
        low: Option<&ast::NominalLiteral>,
        high: Option<&ast::NominalLiteral>,
        inclusive: bool,
    ) -> Option<(u128, u128)> {
        let (min, max) = self.bounds();
        let low = match low {
            Some(low) => self.ordinal(low)?,
            None => min,
        };
        let high = match high {
            Some(high) if inclusive => self.ordinal(high)?,
            Some(high) => self.ordinal(high)?.checked_sub(1)?,
            None => max,
        };
        (low <= high).then_some((low, high))
    }

    /// The ordinals a coverage pattern's `key` matches: a literal's one, a
    /// range pattern's, or a column's range's; `None` for any other key.
    pub(super) fn key_ordinals(self, key: &Key) -> Option<(u128, u128)> {
        match key {
            Key::Literal(literal) => self.ordinal(literal).map(|ordinal| (ordinal, ordinal)),
            Key::Range {
                low,
                high,
                inclusive,
            } => self.range(low.as_ref(), high.as_ref(), *inclusive),
            Key::Ordinals { scalar, low, high } if *scalar == self => Some((*low, *high)),
            _ => None,
        }
    }

    /// The type's values split at each end of `ranges`, in order, so that
    /// each of `ranges` holds a part whole or not at all: rustc's
    /// constructors of an integer or `char` column. A `char`'s surrogates
    /// are no values, so no part holds them.
    pub(super) fn split(self, ranges: &[(u128, u128)]) -> Vec<(u128, u128)> {
        let (min, max) = self.bounds();
        let domains: &[(u128, u128)] = match self {
            Self::Char => &[(0, 0xD7FF), (0xE000, max)],
            Self::Int(_) => &[(min, max)],
        };
        // The ordinals a part starts at.
        let mut starts = Vec::with_capacity(ranges.len() * 2);
        for (low, high) in ranges {
            starts.push(*low);
            starts.extend(high.checked_add(1));
        }
        starts.sort_unstable();
        starts.dedup();
        let mut parts = Vec::new();
        for &(from, to) in domains {
            let mut low = from;
            for &start in &starts {
                if low < start && start <= to {
                    parts.push((low, start - 1));
                    low = start;
                }
            }
            parts.push((low, to));
        }
        parts
    }

    /// The ranges of ordinals `covered` misses, in order. A `char`'s
    /// surrogates are no values, so count as covered.
    pub(super) fn uncovered(self, covered: &[(u128, u128)]) -> Vec<(u128, u128)> {
        let (min, max) = self.bounds();
        let mut covered = covered.to_vec();
        if matches!(self, Self::Char) {
            covered.push((0xD800, 0xDFFF));
        }
        covered.sort_unstable();
        let mut uncovered = Vec::new();
        // The lowest ordinal not yet covered; `None` past the top.
        let mut next = Some(min);
        for (low, high) in covered {
            let Some(from) = next else { break };
            if low > from {
                uncovered.push((from, low - 1));
            }
            if high >= from {
                next = high.checked_add(1).filter(|next| *next <= max);
            }
        }
        if let Some(from) = next {
            uncovered.push((from, max));
        }
        uncovered
    }
}

/// A crate type's name as a witness writes it: its path's last segment. A
/// type of Rawr's std is named as rustc names std's own (`std_type_name`).
pub(super) fn host_type_name(
    definition: &crate::host::type_def::Definition,
    models: &super::Models,
) -> String {
    let path = definition.path();
    super::ty::std_type_name(path.as_str(), models)
        .unwrap_or_else(|| path.rsplit("::").next().unwrap_or_default().to_owned())
}

/// A pattern literal as Rust spells it.
pub(super) fn render_literal(literal: &ast::NominalLiteral) -> String {
    match literal {
        ast::NominalLiteral::Int(value) => value.to_string(),
        ast::NominalLiteral::Bool(value) => value.to_string(),
        ast::NominalLiteral::Char(value) => format!("{value:?}"),
        ast::NominalLiteral::Str(value) => format!("{:?}", value.as_str()),
        ast::NominalLiteral::Float(value) => format!("{:?}", **value),
    }
}

pub(super) fn render_constructor<T>(
    name: &str,
    payload: &[T],
    render: impl Fn(&T) -> String,
) -> String {
    if payload.is_empty() {
        return name.to_string();
    }
    format!(
        "{name}({})",
        payload.iter().map(render).collect::<Vec<_>>().join(", ")
    )
}

impl Pattern {
    pub(super) fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    /// Whether the pattern holds a crate part tested at run time, which
    /// covers no whole value (`Parser::host_coverage`).
    pub(super) fn tested(&self) -> bool {
        match self {
            Self::Wildcard => false,
            Self::Constructor {
                key: Key::Tested, ..
            } => true,
            Self::Constructor { payload, .. } | Self::Or(payload) => {
                payload.iter().any(Self::tested)
            }
        }
    }

    /// This pattern with each crate part tested at run time matching
    /// anything, as a pattern's facade checks name what those parts miss.
    fn untested(&self) -> Self {
        match self {
            Self::Wildcard
            | Self::Constructor {
                key: Key::Tested, ..
            } => Self::Wildcard,
            Self::Constructor { key, name, payload } => Self::Constructor {
                key: key.clone(),
                name: name.clone(),
                payload: payload.iter().map(Self::untested).collect(),
            },
            Self::Or(alternatives) => Self::Or(alternatives.iter().map(Self::untested).collect()),
        }
    }

    /// The or-free patterns this one matches the union of.
    pub(super) fn alternatives(&self) -> Vec<Self> {
        match self {
            Self::Wildcard => vec![Self::Wildcard],
            Self::Or(alternatives) => alternatives.iter().flat_map(Self::alternatives).collect(),
            Self::Constructor { key, name, payload } => {
                let mut expanded = vec![Vec::new()];
                for child in payload.iter() {
                    let options = child.alternatives();
                    expanded = expanded
                        .into_iter()
                        .flat_map(|prefix| {
                            options.iter().map(move |option| {
                                let mut next = prefix.clone();
                                next.push(option.clone());
                                next
                            })
                        })
                        .collect();
                }
                expanded
                    .into_iter()
                    .map(|payload| Self::Constructor {
                        key: key.clone(),
                        name: name.clone(),
                        payload: payload.into_boxed_slice(),
                    })
                    .collect()
            }
        }
    }
}

pub(super) struct Constructor {
    pub(super) key: Key,
    pub(super) name: ImmutableString,
    pub(super) payload: Box<[Ty]>,
    /// A struct or struct variant's field names, one per payload type,
    /// which its witness names as rustc does: `E::C { y: false, .. }`.
    pub(super) fields: Option<Box<[ImmutableString]>>,
}

#[derive(Clone)]
pub(super) enum Witness {
    Wildcard,
    /// `_` of a `#[non_exhaustive]` enum, by its name as the witness writes
    /// it, which rustc's note names: `IntErrorKind`.
    Sealed(ImmutableString),
    Constructor {
        name: ImmutableString,
        payload: Box<[Witness]>,
        fields: Option<Box<[ImmutableString]>>,
    },
}

impl Witness {
    /// The witness of `constructor` over `payload`, one per payload type.
    fn of(constructor: &Constructor, payload: Box<[Witness]>) -> Self {
        Self::Constructor {
            name: constructor.name.clone(),
            payload,
            fields: constructor.fields.clone(),
        }
    }

    /// The `#[non_exhaustive]` enums whose `_` the witness holds, each once,
    /// in the order it writes them.
    pub(super) fn sealed(&self, names: &mut Vec<ImmutableString>) {
        match self {
            Self::Wildcard => {}
            Self::Sealed(name) => {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            Self::Constructor { payload, .. } => {
                for member in payload {
                    member.sealed(names);
                }
            }
        }
    }

    pub(super) fn render(&self) -> String {
        match self {
            Self::Wildcard | Self::Sealed(_) => "_".to_string(),
            // rustc names a struct's fields that aren't `_`, and `..` for
            // the rest.
            Self::Constructor {
                name,
                payload,
                fields: Some(fields),
            } => {
                let mut members = fields
                    .iter()
                    .zip(payload.iter())
                    .filter(|(_, member)| !matches!(member, Self::Wildcard | Self::Sealed(_)))
                    .map(|(field, member)| format!("{field}: {}", member.render()))
                    .collect::<Vec<_>>();
                if members.len() < fields.len() {
                    members.push("..".into());
                }
                if members.is_empty() {
                    format!("{name} {{  }}")
                } else {
                    format!("{name} {{ {} }}", members.join(", "))
                }
            }
            Self::Constructor { name, payload, .. } if name.is_empty() => format!(
                "({})",
                payload
                    .iter()
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            // An array's or slice's length (`coverage_constructors`).
            Self::Constructor { name, payload, .. } if name.starts_with('[') => {
                let mut elements = payload.iter().map(Self::render).collect::<Vec<_>>();
                if name.as_str() == "[..]" {
                    elements.push("..".into());
                }
                format!("[{}]", elements.join(", "))
            }
            Self::Constructor { name, payload, .. } => {
                render_constructor(name, payload, Self::render)
            }
        }
    }
}

pub(super) fn specialize_coverage_rows(
    rows: &[Vec<Pattern>],
    constructor: &Constructor,
) -> Vec<Vec<Pattern>> {
    rows.iter()
        .filter_map(|row| {
            let mut specialized = specialize_coverage_pattern(&row[0], constructor)?;
            specialized.extend_from_slice(&row[1..]);
            Some(specialized)
        })
        .collect()
}

pub(super) fn specialize_coverage_candidate(
    candidate: &[Pattern],
    constructor: &Constructor,
) -> Vec<Pattern> {
    let mut specialized = specialize_coverage_pattern(&candidate[0], constructor)
        .expect("the candidate matches its constructor");
    specialized.extend_from_slice(&candidate[1..]);
    specialized
}

/// The column types under `constructor` of the first column: its payload's,
/// each behind a reference where the column's value is, as a part of a
/// referenced value is never known to have values (`Parser::uninhabited`).
pub(super) fn specialize_coverage_types(types: &[Ty], constructor: &Constructor) -> Vec<Ty> {
    let mut specialized = match &types[0] {
        Ty::Reference { .. } => constructor
            .payload
            .iter()
            .map(|payload| Ty::Reference {
                mutable: false,
                target: Box::new(payload.clone()),
            })
            .collect(),
        _ => constructor.payload.to_vec(),
    };
    specialized.extend_from_slice(&types[1..]);
    specialized
}

pub(super) fn default_coverage_rows(rows: &[Vec<Pattern>]) -> Vec<Vec<Pattern>> {
    rows.iter()
        .filter(|row| row[0].is_wildcard())
        .map(|row| row[1..].to_vec())
        .collect()
}

impl<'a> Parser<'a> {
    /// rustc's E0005 when `syntax`, whose coverage is `coverage`, on `ty`
    /// misses some of its values, with the patterns of the values it
    /// misses: an integer's or a `char`'s ranges, as a `match` names them,
    /// or a facade struct's field's, which the pattern's facade checks
    /// still to lower test.
    pub(super) fn require_irrefutable(
        &mut self,
        syntax: &pattern::Syntax,
        coverage: &Pattern,
        ty: &Ty,
        site: BindingSite,
    ) -> Result<(), Error> {
        // A facade value's parts are tested at run time, so its pattern as
        // written names what it misses, below its variant too, as
        // `Event::Pressed(1_i64..=i64::MAX)`, and what a facade part of it
        // misses, in its place: rustc's `(Style { height:
        // i64::MIN..=-1_i64, .. }, _)`, never `(_, _)`.
        let facade = matches!(self.referenced(ty), Ty::Host { .. });
        let witnesses = if self.host_checks.borrow().is_empty() {
            if facade {
                Vec::new()
            } else {
                self.uncovered(syntax, &coverage.untested(), ty)
            }
        } else {
            match self.host_rows_witnesses(&[syntax], ty, syntax.span())? {
                Some(witnesses) => witnesses,
                None if facade => vec![format!("{}_", self.reference_prefix(ty))],
                None => self.uncovered(syntax, &coverage.untested(), ty),
            }
        };
        if witnesses.is_empty() {
            return Ok(());
        }
        // The variant each witness names, as `E::B` of `&E::B(_)`.
        let missed = witnesses.iter().filter_map(|witness| {
            let witness = witness
                .trim_start_matches(['&', ' '])
                .trim_start_matches("mut ");
            let head = witness.split(['(', ' ']).next()?;
            head.contains("::")
                .then(|| head.rsplit("::").next())
                .flatten()
        });
        let uncovered = self.uncovered_type(ty, missed);
        Err(Error::Refutable {
            ty: self.matched_type(ty),
            site,
            pattern: syntax.span(),
            uncovered,
            witnesses: witnesses.into_iter().map(Into::into).collect(),
        })
    }

    /// The patterns of the values of `ty` that `syntax`, whose coverage is
    /// `coverage`, misses, as rustc writes them.
    pub(super) fn uncovered(
        &self,
        syntax: &pattern::Syntax,
        coverage: &Pattern,
        ty: &Ty,
    ) -> Vec<String> {
        // The value below the references, whose witnesses read through
        // them, as `&None` (`reference_prefix`).
        let referenced = self.referenced(ty);
        let scalar = Scalar::of(&referenced).and_then(|scalar| {
            let mut covered = Vec::new();
            scalar.covered(syntax, &mut covered)?;
            Some((scalar, covered))
        });
        let witnesses = match scalar {
            Some((scalar, covered)) => scalar
                .uncovered(&covered)
                .into_iter()
                .map(|(low, high)| scalar.witness(low, high))
                .collect::<Vec<_>>(),
            None => {
                let rows = coverage
                    .alternatives()
                    .into_iter()
                    .map(|alternative| vec![alternative])
                    .collect::<Vec<_>>();
                self.coverage_witnesses(std::slice::from_ref(&referenced), &rows, true)
                    .iter()
                    .map(|witness| {
                        witness
                            .first()
                            .map_or_else(|| "_".to_string(), Witness::render)
                    })
                    .collect()
            }
        };
        let prefix = self.reference_prefix(ty);
        witnesses
            .into_iter()
            .map(|witness| format!("{prefix}{witness}"))
            .collect()
    }

    /// rustc's E0005 for `syntax`, a `for` or parameter pattern at `site`
    /// on `ty` that misses some of its values, before it is destructured.
    pub(super) fn refuse_refutable(
        &mut self,
        syntax: &pattern::Syntax,
        ty: &Ty,
        site: BindingSite,
    ) -> Result<(), Error> {
        if !syntax.may_refute() && !self.matches_unsized_slice(syntax, ty) {
            return Ok(());
        }
        // The facade parts the check meets are destructured after it.
        let checks = self.host_checks.borrow().len();
        let resolved = self.resolve_root_pattern(syntax, ty);
        let refused = resolved
            .and_then(|resolved| self.require_irrefutable(syntax, &resolved.coverage, ty, site));
        self.host_checks.borrow_mut().truncate(checks);
        refused
    }

    /// Whether `ty`, not behind a reference, is an enum with variants,
    /// whose empty match rustc refuses by naming them rather than calling
    /// the type non-empty.
    pub(super) fn enumerated(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Option(_) | Ty::Result(..) | Ty::Cow(_) | Ty::Ordering => true,
            ref polled if self.poll_output(polled).is_some() => true,
            Ty::Host { proof, .. } => matches!(
                proof.definition.shape(),
                crate::host::type_def::Shape::Enum { variants, .. } if !variants.is_empty()
            ),
            Ty::Script(id, _) => matches!(
                self.models.definition(id),
                Nominal::Enum { item, .. } if !item.variants.is_empty()
            ),
            _ => false,
        }
    }

    /// Whether `ty` has no values, as rustc's exhaustiveness sees it: `!`,
    /// `Infallible`, an enum with no variants, or a type each of whose
    /// constructors holds a part with none. A reference always has values,
    /// so a part of a referenced value is never empty.
    pub(super) fn uninhabited(&self, ty: &Ty) -> bool {
        self.uninhabited_within(ty, 0)
    }

    fn uninhabited_within(&self, ty: &Ty, depth: usize) -> bool {
        // A type that holds itself only behind a pointer, which has values.
        const DEPTH: usize = 32;
        if depth > DEPTH || matches!(self.inference.resolve(ty), Ty::Reference { .. }) {
            return false;
        }
        self.coverage_constructors(ty).is_some_and(|constructors| {
            constructors
                .iter()
                .all(|constructor| self.empty_constructor(constructor, depth + 1))
        })
    }

    /// Whether `constructor` holds a part of a type with no values, so that
    /// no value is of it and no witness names it, as `Err(_)` of a
    /// `Result<T, Infallible>`.
    fn empty_constructor(&self, constructor: &Constructor, depth: usize) -> bool {
        constructor
            .payload
            .iter()
            .any(|part| self.uninhabited_within(part, depth))
    }

    /// The constructors of a column of type `ty` whose patterns are
    /// `heads`: an integer's or a `char`'s values split at the ends of the
    /// literals and ranges among them (`Scalar::split`), else the type's
    /// own (`coverage_constructors`) but those no value is of, which a part
    /// of a referenced value has none of.
    pub(super) fn column_constructors<'p>(
        &self,
        ty: &Ty,
        heads: impl Iterator<Item = &'p Pattern>,
    ) -> Option<Vec<Constructor>> {
        let Some(scalar) = Scalar::of(&self.referenced(ty)) else {
            let mut constructors = self.coverage_constructors(ty)?;
            if !matches!(self.inference.resolve(ty), Ty::Reference { .. }) {
                constructors.retain(|constructor| !self.empty_constructor(constructor, 0));
            } else if constructors.is_empty() {
                // A referenced value always has values, so of a type with
                // no constructor only a wildcard covers it.
                return None;
            }
            return Some(constructors);
        };
        let ranges = heads
            .filter_map(|head| match head {
                Pattern::Constructor { key, .. } => scalar.key_ordinals(key),
                _ => None,
            })
            .collect::<Vec<_>>();
        Some(
            scalar
                .split(&ranges)
                .into_iter()
                .map(|(low, high)| Constructor {
                    key: Key::Ordinals { scalar, low, high },
                    name: scalar.witness(low, high).into(),
                    payload: Box::default(),
                    fields: None,
                })
                .collect(),
        )
    }

    pub(super) fn coverage_constructors(&self, ty: &Ty) -> Option<Vec<Constructor>> {
        let resolved = self.inference.resolve(ty);
        let prelude = |variant: Prelude, payload: &[Ty]| Constructor {
            key: Key::Prelude(variant),
            name: variant.name().into(),
            payload: payload.into(),
            fields: None,
        };
        match resolved {
            Ty::Reference { target, .. } => return self.coverage_constructors(&target),
            // Types with no values: no constructor.
            Ty::Never | Ty::ParseError(super::from_str::ParseError::Infallible) => {
                return Some(Vec::new());
            }
            Ty::Bool => {
                return Some(
                    [false, true]
                        .into_iter()
                        .map(|value| Constructor {
                            key: Key::Literal(ast::NominalLiteral::Bool(value)),
                            name: value.to_string().into(),
                            payload: Box::default(),
                            fields: None,
                        })
                        .collect(),
                );
            }
            Ty::Ordering => {
                return Some(
                    ["Less", "Equal", "Greater"]
                        .into_iter()
                        .map(|name| Constructor {
                            key: Key::Literal(ast::NominalLiteral::Int(
                                ordering_discriminant(name).expect("an `Ordering` variant"),
                            )),
                            name: format!("Ordering::{name}").into(),
                            payload: Box::default(),
                            fields: None,
                        })
                        .collect(),
                );
            }
            Ty::Tuple(members) => {
                return Some(vec![Constructor {
                    key: Key::Tuple,
                    name: ImmutableString::new(),
                    payload: members,
                    fields: None,
                }]);
            }
            // An array's one length; a slice's lengths up to the longest
            // slice pattern seen, then every longer one at once.
            Ty::Array { element, kind } => {
                let constructor = |length, open| Constructor {
                    key: Key::SliceLength { length, open },
                    name: if open { "[..]" } else { "[]" }.into(),
                    payload: vec![(*element).clone(); length].into(),
                    fields: None,
                };
                return Some(match kind.length() {
                    Some(length) => vec![constructor(length, false)],
                    None => {
                        let longest = self.slice_arity.get();
                        (0..=longest)
                            .map(|length| constructor(length, false))
                            .chain([constructor(longest + 1, true)])
                            .collect()
                    }
                });
            }
            Ty::Option(element) => {
                return Some(vec![
                    prelude(Prelude::None, &[]),
                    prelude(Prelude::Some, &[*element]),
                ]);
            }
            Ty::Result(ok, err) => {
                return Some(vec![
                    prelude(Prelude::Ok, &[*ok]),
                    prelude(Prelude::Err, &[*err]),
                ]);
            }
            ref polled if let Some(output) = self.poll_output(polled) => {
                return Some(vec![
                    prelude(Prelude::Ready, &[output]),
                    prelude(Prelude::Pending, &[]),
                ]);
            }
            Ty::Cow(borrowed) => {
                let owned = self.owned_of(&borrowed).unwrap_or(Ty::Unknown);
                return Some(
                    [(false, cow::reference_to(*borrowed)), (true, owned)]
                        .into_iter()
                        .map(|(owned, payload)| Constructor {
                            key: Key::Cow(owned),
                            name: if owned { "Cow::Owned" } else { "Cow::Borrowed" }.into(),
                            payload: Box::new([payload]),
                            fields: None,
                        })
                        .collect(),
                );
            }
            // A crate enum's variants, whose fields' patterns below a
            // program's pattern are each a binding, `_` or a part tested at
            // run time (`host_coverage`), so their types are not needed.
            // A `#[non_exhaustive]` enum has values outside its variants, so
            // it is covered as a type whose values aren't told apart: by a
            // wildcard alone, its witness `_`.
            Ty::Host { proof, .. } => {
                let crate::host::type_def::Shape::Enum {
                    variants,
                    non_exhaustive: false,
                } = proof.definition.shape()
                else {
                    return None;
                };
                let identity = type_def::Identity::Host(proof.definition.identity().clone());
                let owner = host_type_name(&proof.definition, &self.models);
                return Some(
                    variants
                        .iter()
                        .map(|variant| Constructor {
                            key: Key::Enum {
                                identity: identity.clone(),
                                variant: nominal::variant::Id::from_text(variant.name()),
                            },
                            name: format!("{owner}::{}", variant.name()).into(),
                            payload: vec![Ty::Unknown; variant.fields().len()].into(),
                            fields: None,
                        })
                        .collect(),
                );
            }
            _ => {}
        }
        let definition = resolved.script()?;
        let arguments = match &resolved {
            Ty::Script(_, arguments) => arguments.clone(),
            _ => Box::default(),
        };
        let substitution = self.nominal_substitution(definition, &arguments);
        let item = match self.models.definition(definition) {
            Nominal::Enum { definition, item } => (definition, item),
            // A struct's one constructor, as a tuple's.
            Nominal::Struct { item, .. } => {
                return (item.form != crate::item::StructForm::Unit).then(|| {
                    vec![Constructor {
                        key: Key::Tuple,
                        name: item.name.text.clone(),
                        payload: item
                            .fields
                            .iter()
                            .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                            .collect(),
                        fields: (item.form == crate::item::StructForm::Named).then(|| {
                            item.fields
                                .iter()
                                .map(|field| field.name.text.clone())
                                .collect()
                        }),
                    }]
                });
            }
        };
        let (definition, item) = item;
        let identity = type_def::Identity::Script(definition.key().clone());

        Some(
            item.variants
                .iter()
                .map(|variant| Constructor {
                    key: Key::Enum {
                        identity: identity.clone(),
                        variant: nominal::variant::Id::from_name(&variant.name),
                    },
                    name: format!("{}::{}", definition.name(), variant.name.text).into(),
                    payload: variant
                        .payload
                        .iter()
                        .map(|payload| instantiate(&Ty::from_resolved(payload), &substitution))
                        .collect(),
                    fields: variant
                        .fields
                        .as_ref()
                        .map(|fields| fields.iter().map(|field| field.text.clone()).collect()),
                })
                .collect(),
        )
    }

    /// What `syntax`, a pattern on the crate type `ty` that its arm tests at
    /// run time, covers of `ty`'s values: a crate enum's variant, with each
    /// field `_` where its pattern matches anything and else a part that
    /// covers no whole value, as a literal is. Any other refutable pattern
    /// covers no whole value. A program's `match` on `Message::Wizard(..)`
    /// is then exhaustive where rustc's is (#1249).
    pub(super) fn host_coverage(
        &self,
        syntax: &pattern::Syntax,
        ty: &Ty,
    ) -> Result<Pattern, Error> {
        let partial = || Pattern::Constructor {
            key: Key::Tested,
            name: "_".into(),
            payload: Box::default(),
        };
        if !syntax.may_refute() {
            return Ok(Pattern::Wildcard);
        }
        let referenced = self.referenced(ty);
        match syntax {
            pattern::Syntax::At { pattern, .. } | pattern::Syntax::Reference { pattern, .. } => {
                self.host_coverage(pattern, &referenced)
            }
            pattern::Syntax::Or { alternatives, .. } => Ok(Pattern::Or(
                alternatives
                    .iter()
                    .map(|alternative| self.host_coverage(alternative, &referenced))
                    .collect::<Result<_, _>>()?,
            )),
            // std's `Poll::Ready(p)` and `Poll::Pending`.
            pattern::Syntax::Prelude {
                variant: variant @ (Prelude::Ready | Prelude::Pending),
                payload,
                ..
            } if self.poll_output(&referenced).is_some() => Ok(Pattern::Constructor {
                key: Key::Prelude(*variant),
                name: variant.name().into(),
                payload: payload
                    .iter()
                    .map(|payload| {
                        if payload.may_refute() {
                            partial()
                        } else {
                            Pattern::Wildcard
                        }
                    })
                    .collect(),
            }),
            pattern::Syntax::Enum(written) if !written.host_struct => {
                let Some(constructor) = self.coverage_constructors(&referenced).and_then(|all| {
                    all.into_iter().find(|constructor| {
                        matches!(&constructor.key, Key::Enum { variant, .. }
                            if variant.name() == written.variant.as_str())
                    })
                }) else {
                    return Ok(partial());
                };
                let Ty::Host { proof, .. } = &referenced else {
                    return Ok(partial());
                };
                let crate::host::type_def::Shape::Enum { variants, .. } = proof.definition.shape()
                else {
                    return Ok(partial());
                };
                let Some(variant) = variants
                    .iter()
                    .find(|variant| variant.name() == written.variant.as_str())
                else {
                    return Ok(partial());
                };
                let discard = pattern::Syntax::Discard {
                    span: syntax.span(),
                };
                let members = match &written.named {
                    Some((names, _)) => {
                        let mut members = vec![&discard; variant.fields().len()];
                        for ((name, _), member) in names.iter().zip(written.payload.iter()) {
                            if let Some(index) = variant
                                .fields()
                                .iter()
                                .position(|field| field.name() == name.as_str())
                            {
                                members[index] = member;
                            }
                        }
                        members
                    }
                    None if written.payload.is_empty() => vec![&discard; variant.fields().len()],
                    None => tuple_members(&written.payload, variant.fields().len(), &discard)?,
                };
                Ok(Pattern::Constructor {
                    key: constructor.key,
                    name: constructor.name,
                    payload: members
                        .into_iter()
                        .map(|member| {
                            if member.may_refute() {
                                partial()
                            } else {
                                Pattern::Wildcard
                            }
                        })
                        .collect(),
                })
            }
            _ => Ok(partial()),
        }
    }

    pub(super) fn coverage_pattern_is_covered(
        &self,
        types: &[Ty],
        rows: &[Vec<Pattern>],
        candidate: &[Pattern],
    ) -> bool {
        debug_assert!(rows.iter().all(|row| row.len() == types.len()));
        debug_assert_eq!(candidate.len(), types.len());

        if candidate.is_empty() {
            return rows.iter().any(Vec::is_empty);
        }
        if rows.is_empty() {
            return false;
        }
        if rows.iter().any(|row| row.iter().all(Pattern::is_wildcard)) {
            return true;
        }

        let scalar = Scalar::of(&self.referenced(&types[0])).is_some();
        match &candidate[0] {
            // A slice pattern covers each length it matches, and an
            // integer's or a `char`'s literal or range each range of its
            // column it holds.
            Pattern::Constructor { key, .. } if scalar || matches!(key, Key::Slice { .. }) => {
                let heads = rows.iter().map(|row| &row[0]).chain([&candidate[0]]);
                let Some(constructors) = self.column_constructors(&types[0], heads) else {
                    return false;
                };
                let mut matched = constructors.iter().filter(|constructor| {
                    specialize_coverage_pattern(&candidate[0], constructor).is_some()
                });
                let mut covers = |constructor: &Constructor| {
                    let specialized_rows = specialize_coverage_rows(rows, constructor);
                    let specialized_candidate =
                        specialize_coverage_candidate(candidate, constructor);
                    let specialized_types = specialize_coverage_types(types, constructor);
                    self.coverage_pattern_is_covered(
                        &specialized_types,
                        &specialized_rows,
                        &specialized_candidate,
                    )
                };
                // A pattern that matches none of the column's values isn't
                // one rawr reads, so it is never covered.
                matched.next().is_some_and(&mut covers) && matched.all(covers)
            }
            Pattern::Constructor { key, .. } => {
                let Some(constructor) =
                    self.coverage_constructors(&types[0])
                        .and_then(|constructors| {
                            constructors
                                .into_iter()
                                .find(|constructor| &constructor.key == key)
                        })
                else {
                    return false;
                };
                let specialized_rows = specialize_coverage_rows(rows, &constructor);
                let specialized_candidate = specialize_coverage_candidate(candidate, &constructor);
                let specialized_types = specialize_coverage_types(types, &constructor);
                self.coverage_pattern_is_covered(
                    &specialized_types,
                    &specialized_rows,
                    &specialized_candidate,
                )
            }
            Pattern::Or(_) => unreachable!("or-patterns are expanded first"),
            Pattern::Wildcard if rows.iter().all(|row| row[0].is_wildcard()) => {
                let default_rows = default_coverage_rows(rows);
                self.coverage_pattern_is_covered(&types[1..], &default_rows, &candidate[1..])
            }
            Pattern::Wildcard => {
                let heads = rows.iter().map(|row| &row[0]);
                let Some(constructors) = self.column_constructors(&types[0], heads) else {
                    let default_rows = default_coverage_rows(rows);
                    return self.coverage_pattern_is_covered(
                        &types[1..],
                        &default_rows,
                        &candidate[1..],
                    );
                };

                constructors.iter().all(|constructor| {
                    let specialized_rows = specialize_coverage_rows(rows, constructor);
                    let specialized_candidate =
                        specialize_coverage_candidate(candidate, constructor);
                    let specialized_types = specialize_coverage_types(types, constructor);
                    self.coverage_pattern_is_covered(
                        &specialized_types,
                        &specialized_rows,
                        &specialized_candidate,
                    )
                })
            }
        }
    }

    /// The values `rows` miss, as rustc's witnesses: at the `scrutinee`
    /// each missing constructor, as `Message::B`; below it, `_` where the
    /// rows name none of the type's constructors. Constructors no row names
    /// are reported before the ones some row names, and alone, as rustc
    /// reports `None` for `Some(1)`. Empty when the rows cover every value.
    pub(super) fn coverage_witnesses(
        &self,
        types: &[Ty],
        rows: &[Vec<Pattern>],
        scrutinee: bool,
    ) -> Vec<Vec<Witness>> {
        debug_assert!(rows.iter().all(|row| row.len() == types.len()));

        if types.is_empty() {
            return if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            };
        }
        if rows.iter().any(|row| row.iter().all(Pattern::is_wildcard)) {
            return Vec::new();
        }
        // Each head before each tail, the heads outermost, as rustc lists
        // `(A, x)`, `(A, y)`, `(B, x)`.
        let prefixed = |heads: &[Witness], tails: Vec<Vec<Witness>>| {
            heads
                .iter()
                .flat_map(|head| {
                    tails.iter().map(move |tail| {
                        let mut witness = vec![head.clone()];
                        witness.extend_from_slice(tail);
                        witness
                    })
                })
                .collect::<Vec<_>>()
        };
        let heads = rows.iter().map(|row| &row[0]);
        let Some(constructors) = self.column_constructors(&types[0], heads) else {
            let tails = self.coverage_witnesses(&types[1..], &default_coverage_rows(rows), false);
            let head = match without_references(&self.inference.resolve(&types[0])) {
                Ty::Host { proof, .. }
                    if matches!(
                        proof.definition.shape(),
                        crate::host::type_def::Shape::Enum {
                            non_exhaustive: true,
                            ..
                        }
                    ) =>
                {
                    Witness::Sealed(host_type_name(&proof.definition, &self.models).into())
                }
                _ => Witness::Wildcard,
            };
            return prefixed(&[head], tails);
        };
        let (present, missing): (Vec<_>, Vec<_>) = constructors.iter().partition(|constructor| {
            rows.iter().any(|row| {
                !row[0].is_wildcard() && specialize_coverage_pattern(&row[0], constructor).is_some()
            })
        });
        if !missing.is_empty() {
            let tails = self.coverage_witnesses(&types[1..], &default_coverage_rows(rows), false);
            if !tails.is_empty() {
                let heads = if scrutinee || !present.is_empty() {
                    missing
                        .iter()
                        .map(|constructor| {
                            Witness::of(
                                constructor,
                                (0..constructor.payload.len())
                                    .map(|_| Witness::Wildcard)
                                    .collect(),
                            )
                        })
                        .collect()
                } else {
                    vec![Witness::Wildcard]
                };
                return prefixed(&heads, tails);
            }
        }
        present
            .into_iter()
            .flat_map(|constructor| {
                let specialized_rows = specialize_coverage_rows(rows, constructor);
                let specialized_types = specialize_coverage_types(types, constructor);
                self.coverage_witnesses(&specialized_types, &specialized_rows, false)
                    .into_iter()
                    .map(|mut witness| {
                        let tail = witness.split_off(constructor.payload.len());
                        let mut combined =
                            vec![Witness::of(constructor, witness.into_boxed_slice())];
                        combined.extend(tail);
                        combined
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The variants an arm's pattern covers whole: a variant whose fields
    /// it matches with bindings and `_` only.
    pub(super) fn host_covers(syntax: &pattern::Syntax, covered: &mut Vec<ImmutableString>) {
        let irrefutable = |member: &pattern::Syntax| {
            let mut member = member;
            loop {
                match member {
                    pattern::Syntax::At { pattern, .. } => member = pattern,
                    pattern::Syntax::Reference { pattern, .. } => member = pattern,
                    other => {
                        return matches!(
                            other,
                            pattern::Syntax::Binding { .. }
                                | pattern::Syntax::Ref { .. }
                                | pattern::Syntax::Discard { .. }
                                | pattern::Syntax::Rest { .. }
                        )
                    }
                }
            }
        };
        match syntax {
            pattern::Syntax::Or { alternatives, .. } => {
                for alternative in alternatives.iter() {
                    Self::host_covers(alternative, covered);
                }
            }
            pattern::Syntax::At { pattern, .. } | pattern::Syntax::Reference { pattern, .. } => {
                Self::host_covers(pattern, covered);
            }
            pattern::Syntax::Enum(pattern) if pattern.payload.iter().all(irrefutable) => {
                covered.push(pattern.variant.clone());
            }
            _ => {}
        }
    }

    /// Whether the unguarded arms `rows` of a match on facade values, one
    /// pattern for each of `tys` in each row, cover every value
    /// (`host_rows_witnesses`).
    pub(super) fn host_rows_cover(
        &mut self,
        rows: Vec<Vec<&pattern::Syntax>>,
        tys: &[Ty],
        at: Span,
    ) -> Result<bool, Error> {
        let mut opaque = false;
        Ok(self
            .rows_witnesses(rows, tys, at, true, &mut opaque)?
            .is_empty())
    }

    /// rustc's witnesses of the values of `ty` the patterns `arms` miss,
    /// read from the patterns as written (`rows_witnesses`). `None` where
    /// a part of a type they don't tell apart is refutable, which the
    /// program's coverage, if it names any, names better.
    pub(super) fn host_rows_witnesses(
        &mut self,
        arms: &[&pattern::Syntax],
        ty: &Ty,
        at: Span,
    ) -> Result<Option<Vec<String>>, Error> {
        let mut opaque = false;
        let rows = arms.iter().map(|arm| vec![*arm]).collect();
        let witnesses =
            self.rows_witnesses(rows, std::slice::from_ref(ty), at, true, &mut opaque)?;
        let prefix = self.reference_prefix(ty);
        Ok((!opaque || witnesses.is_empty()).then(|| {
            witnesses
                .into_iter()
                .map(|mut witness| format!("{prefix}{}", witness.swap_remove(0)))
                .collect()
        }))
    }

    /// rustc's witnesses of the values the patterns `rows`, one for each of
    /// `tys` in each row, miss, read from the patterns as written where a
    /// facade value's parts are tested at run time: the arms on one
    /// constructor of a column cover it when their patterns below it cover
    /// its fields together, so `A(B(x))` and `A(C)` cover `A` when `B` and
    /// `C` are its field's variants, and `A(true)` and `A(false)` cover `A`
    /// of a `bool`. As `coverage_witnesses` reports them: at the
    /// `scrutinee` each missing constructor, below it `_` where the rows
    /// name none, and the constructors no row names before, and without,
    /// the ones some row names. `opaque` is set where a refutable pattern
    /// is on a part of a type the patterns don't tell apart here.
    fn rows_witnesses(
        &mut self,
        rows: Vec<Vec<&pattern::Syntax>>,
        tys: &[Ty],
        at: Span,
        scrutinee: bool,
        opaque: &mut bool,
    ) -> Result<Vec<Vec<String>>, Error> {
        let Some((ty, rest)) = tys.split_first() else {
            return Ok(if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            });
        };
        if rows
            .iter()
            .any(|row| row.iter().all(|pattern| matches_anything(pattern)))
        {
            return Ok(Vec::new());
        }
        // Each row by its first pattern, past `name @` and `&`, with each
        // alternative of a `|` a row of its own.
        let mut spread = Vec::new();
        for row in rows {
            let Some((&first, tail)) = row.split_first() else {
                continue;
            };
            let mut firsts = vec![first];
            while let Some(first) = firsts.pop() {
                match first {
                    pattern::Syntax::At { pattern, .. }
                    | pattern::Syntax::Reference { pattern, .. } => {
                        firsts.push(pattern);
                    }
                    pattern::Syntax::Or { alternatives, .. } => {
                        firsts.extend(alternatives.iter().rev());
                    }
                    _ => spread.push((first, tail.to_vec())),
                }
            }
        }
        // Each head before each tail, the heads outermost.
        let prefixed = |heads: &[String], tails: Vec<Vec<String>>| {
            heads
                .iter()
                .flat_map(|head| {
                    tails.iter().map(move |tail| {
                        let mut witness = vec![head.clone()];
                        witness.extend_from_slice(tail);
                        witness
                    })
                })
                .collect::<Vec<_>>()
        };
        // The rows past a first pattern that matches anything.
        let wild: Vec<Vec<&pattern::Syntax>> = spread
            .iter()
            .filter(|(first, _)| irrefutable_leaf(first))
            .map(|(_, tail)| tail.clone())
            .collect();
        let splits = if spread.iter().any(|(first, _)| !irrefutable_leaf(first)) {
            self.row_splits(ty, &spread, at)?
        } else {
            None
        };
        // Only a pattern that matches every value covers a type whose
        // values are not told apart here.
        let Some(splits) = splits else {
            *opaque |= spread.iter().any(|(first, _)| !irrefutable_leaf(first));
            let tails = self.rows_witnesses(wild, rest, at, false, opaque)?;
            return Ok(prefixed(&["_".to_string()], tails));
        };
        let discard = pattern::Syntax::Discard { span: at };
        let mut present = Vec::new();
        let mut missing = Vec::new();
        for split in splits {
            let mut named = false;
            for (first, _) in &spread {
                named |= !irrefutable_leaf(first)
                    && split
                        .head
                        .members(first, split.fields.len(), &discard)?
                        .is_some();
            }
            if named {
                present.push(split);
            } else {
                missing.push(split);
            }
        }
        if !missing.is_empty() {
            let tails = self.rows_witnesses(wild, rest, at, false, opaque)?;
            if !tails.is_empty() {
                let heads = if scrutinee || !present.is_empty() {
                    missing
                        .iter()
                        .map(|split| {
                            split
                                .head
                                .render(&vec!["_".to_string(); split.fields.len()])
                        })
                        .collect()
                } else {
                    vec!["_".to_string()]
                };
                return Ok(prefixed(&heads, tails));
            }
        }
        let mut witnesses = Vec::new();
        for Split { head, fields } in &present {
            let mut specialized = Vec::new();
            for (first, tail) in &spread {
                let Some(members) = head.members(first, fields.len(), &discard)? else {
                    continue;
                };
                specialized.push(members.into_iter().chain(tail.iter().copied()).collect());
            }
            let tys: Vec<Ty> = fields.iter().cloned().chain(rest.iter().cloned()).collect();
            for mut witness in self.rows_witnesses(specialized, &tys, at, false, opaque)? {
                let tail = witness.split_off(fields.len());
                let mut combined = vec![head.render(&witness)];
                combined.extend(tail);
                witnesses.push(combined);
            }
        }
        Ok(witnesses)
    }

    /// The constructors `host_rows_cover` splits a column of `ty` by, each
    /// with its fields' types, where the column's first patterns `spread`
    /// tell its values apart: a facade enum's variants, a facade struct
    /// by the fields the patterns name, a tuple, `Option`'s and `Result`'s
    /// variants, and a `bool`'s, an integer's or a `char`'s values its
    /// literals and ranges split. `None` for any other type, or a
    /// `#[non_exhaustive]` enum, whose values only a wildcard covers.
    fn row_splits(
        &mut self,
        ty: &Ty,
        spread: &[(&pattern::Syntax, Vec<&pattern::Syntax>)],
        at: Span,
    ) -> Result<Option<Vec<Split>>, Error> {
        let column = self.referenced(ty);
        let values = |constructors: Vec<Constructor>| {
            constructors
                .into_iter()
                .map(|constructor| Split::of(Head::Values(constructor), Vec::new()))
                .collect()
        };
        Ok(Some(match &column {
            Ty::Host { proof, arguments } => match proof.definition.shape() {
                crate::host::type_def::Shape::Enum {
                    variants,
                    non_exhaustive: false,
                } => {
                    let definition = proof.definition.clone();
                    let owner = proof.owner.clone();
                    let owner_name = host_type_name(&definition, &self.models);
                    let path: ImmutableString = definition.path().as_str().into();
                    let mut splits = Vec::with_capacity(variants.len());
                    for variant in variants.iter() {
                        let mut parameters = definition
                            .parameters()
                            .iter()
                            .cloned()
                            .zip(arguments.iter().cloned())
                            .collect();
                        let mut fields = Vec::with_capacity(variant.fields().len());
                        for field in variant.fields() {
                            fields.push(self.instantiate_host_type(
                                field.ty(),
                                &mut parameters,
                                &path,
                                at,
                                owner.as_ref(),
                                Some(&definition),
                            )?);
                        }
                        let names = variant
                            .fields()
                            .iter()
                            .map(|field| ImmutableString::from(field.name()))
                            .collect();
                        splits.push(Split {
                            head: Head::Variant {
                                name: variant.name().into(),
                                written: format!("{owner_name}::{}", variant.name()).into(),
                                fields: names,
                            },
                            fields,
                        });
                    }
                    splits
                }
                crate::host::type_def::Shape::Enum { .. } => return Ok(None),
                // A struct's declared fields, or, where the facade reads
                // them by methods, the fields some pattern names; the
                // others are `_` in each.
                shape => {
                    let written =
                        ImmutableString::from(host_type_name(&proof.definition, &self.models));
                    let mut names = match shape {
                        crate::host::type_def::Shape::Struct { fields, .. } => fields
                            .iter()
                            .map(|field| (ImmutableString::from(field.name()), at))
                            .collect(),
                        _ => Vec::<(ImmutableString, Span)>::new(),
                    };
                    let complete = !names.is_empty();
                    for (first, _) in spread {
                        let pattern::Syntax::Enum(written) = first else {
                            continue;
                        };
                        let Some((written, _)) =
                            written.named.as_ref().filter(|_| written.host_struct)
                        else {
                            continue;
                        };
                        for (name, name_span) in written.iter() {
                            if !names.iter().any(|(known, _)| known == name) {
                                names.push((name.clone(), *name_span));
                            }
                        }
                    }
                    if names.is_empty() {
                        return Ok(None);
                    }
                    let mut fields = Vec::with_capacity(names.len());
                    for (name, name_span) in &names {
                        let value = origin::Fragment::plain("__rawr_field");
                        fields.push(self.host_struct_field(value, &column, name, *name_span)?.ty);
                    }
                    vec![Split {
                        head: Head::Struct {
                            written,
                            fields: names.into_iter().map(|(name, _)| name).collect(),
                            complete,
                        },
                        fields,
                    }]
                }
            },
            Ty::Tuple(members) => vec![Split::of(Head::Tuple, members.to_vec())],
            Ty::Option(element) => vec![
                Split::of(Head::Prelude(Prelude::None), Vec::new()),
                Split::of(Head::Prelude(Prelude::Some), vec![(**element).clone()]),
            ],
            Ty::Result(ok, err) => vec![
                Split::of(Head::Prelude(Prelude::Ok), vec![(**ok).clone()]),
                Split::of(Head::Prelude(Prelude::Err), vec![(**err).clone()]),
            ],
            Ty::Bool => values(self.coverage_constructors(&column).unwrap_or_default()),
            _ if Scalar::of(&column).is_some() => {
                let heads = spread
                    .iter()
                    .filter_map(|(first, _)| Head::value_pattern(first))
                    .collect::<Vec<_>>();
                values(
                    self.column_constructors(&column, heads.iter())
                        .unwrap_or_default(),
                )
            }
            _ => return Ok(None),
        }))
    }
}

/// Whether a pattern matches any value with no test: a binding, `ref`,
/// `_` or `..`.
fn irrefutable_leaf(pattern: &pattern::Syntax) -> bool {
    matches!(
        pattern,
        pattern::Syntax::Binding { .. }
            | pattern::Syntax::Ref { .. }
            | pattern::Syntax::Discard { .. }
            | pattern::Syntax::Rest { .. }
    )
}

/// A constructor of a column of facade values' parts that
/// `Parser::host_rows_cover` splits its rows by, with its fields' types.
pub(super) struct Split {
    head: Head,
    fields: Vec<Ty>,
}

impl Split {
    fn of(head: Head, fields: Vec<Ty>) -> Self {
        Self { head, fields }
    }
}

/// What a column's first patterns are tested against: a constructor of
/// its type (`Split`).
pub(super) enum Head {
    /// A facade enum's variant, written `written` (`Event::Pressed`), with
    /// its fields' names.
    Variant {
        name: ImmutableString,
        written: ImmutableString,
        fields: Box<[ImmutableString]>,
    },
    /// A facade struct written `written`, by its fields, which are its
    /// declared ones when `complete`, else those some pattern names.
    Struct {
        written: ImmutableString,
        fields: Box<[ImmutableString]>,
        complete: bool,
    },
    Tuple,
    Prelude(Prelude),
    /// The values of a `bool`, an integer or a `char` a column's literals
    /// and ranges split it into (`Parser::column_constructors`).
    Values(Constructor),
}

impl Head {
    /// The coverage of a literal or a range, which a column of `bool`s,
    /// integers or `char`s splits by.
    fn value_pattern(syntax: &pattern::Syntax) -> Option<Pattern> {
        let key = match syntax {
            pattern::Syntax::Literal { literal, .. } => Key::Literal(literal.clone()),
            pattern::Syntax::Range {
                low,
                high,
                inclusive,
                ..
            } => Key::Range {
                low: low.as_ref().map(|(low, _)| low.clone()),
                high: high.as_ref().map(|(high, _)| high.clone()),
                inclusive: *inclusive,
            },
            _ => return None,
        };
        Some(Pattern::Constructor {
            key,
            name: ImmutableString::new(),
            payload: Box::default(),
        })
    }

    /// The patterns below this constructor of `first`, one for each of its
    /// `arity` fields, `discard` where `first` names none; `None` where
    /// `first` matches none of its values.
    fn members<'s>(
        &self,
        first: &'s pattern::Syntax,
        arity: usize,
        discard: &'s pattern::Syntax,
    ) -> Result<Option<Vec<&'s pattern::Syntax>>, Error> {
        if irrefutable_leaf(first) {
            return Ok(Some(vec![discard; arity]));
        }
        // The named fields of a struct or a struct variant in declaration
        // order.
        let named = |declared: &[ImmutableString],
                     written: &'s [(ImmutableString, Span)],
                     payload: &'s [pattern::Syntax]| {
            let mut members = vec![discard; declared.len()];
            for ((name, _), member) in written.iter().zip(payload) {
                if let Some(index) = declared.iter().position(|field| field == name) {
                    members[index] = member;
                }
            }
            members
        };
        Ok(match (self, first) {
            (Self::Variant { name, fields, .. }, pattern::Syntax::Enum(written))
                if !written.host_struct && written.variant == *name =>
            {
                Some(match &written.named {
                    Some((names, _)) => named(fields, names, &written.payload),
                    None if written.payload.is_empty() => vec![discard; arity],
                    None => tuple_members(&written.payload, arity, discard)?,
                })
            }
            (Self::Struct { fields, .. }, pattern::Syntax::Enum(written))
                if written.host_struct =>
            {
                let Some((names, _)) = &written.named else {
                    return Ok(None);
                };
                Some(named(fields, names, &written.payload))
            }
            (Self::Tuple, pattern::Syntax::Tuple { members, .. }) => {
                Some(tuple_members(members, arity, discard)?)
            }
            (
                Self::Prelude(variant),
                pattern::Syntax::Prelude {
                    variant: written,
                    payload,
                    ..
                },
            ) if written == variant => Some(payload.iter().map(|payload| &**payload).collect()),
            (Self::Values(constructor), first) => Self::value_pattern(first)
                .and_then(|head| specialize_coverage_pattern(&head, constructor))
                .map(|_| Vec::new()),
            _ => None,
        })
    }

    /// rustc's witness of this constructor over `payload`, one per field:
    /// `Event::Pressed(_)`, `Style { width: 0_i64, .. }`, `(a, b)`.
    fn render(&self, payload: &[String]) -> String {
        // rustc names a struct's fields that aren't `_`, and `..` for the
        // rest.
        let named = |written: &str, fields: &[ImmutableString], complete: bool| {
            let mut members = fields
                .iter()
                .zip(payload)
                .filter(|(_, member)| member.as_str() != "_")
                .map(|(field, member)| format!("{field}: {member}"))
                .collect::<Vec<_>>();
            if !complete || members.len() < fields.len() {
                members.push("..".into());
            }
            format!("{written} {{ {} }}", members.join(", "))
        };
        match self {
            Self::Variant {
                written, fields, ..
            } if fields.is_empty() => written.to_string(),
            // A tuple variant's fields are named by their positions.
            Self::Variant {
                written, fields, ..
            } if fields[0].parse::<usize>().is_ok() => {
                format!("{written}({})", payload.join(", "))
            }
            Self::Variant {
                written, fields, ..
            } => named(written, fields, true),
            Self::Struct {
                written,
                fields,
                complete,
            } => named(written, fields, *complete),
            Self::Tuple if payload.len() == 1 => format!("({},)", payload[0]),
            Self::Tuple => format!("({})", payload.join(", ")),
            Self::Prelude(variant) => render_constructor(variant.name(), payload, String::clone),
            Self::Values(constructor) => constructor.name.to_string(),
        }
    }
}

/// Whether a pattern matches any value with no test, past `name @` and
/// `&`.
fn matches_anything(pattern: &pattern::Syntax) -> bool {
    match pattern {
        pattern::Syntax::At { pattern, .. } | pattern::Syntax::Reference { pattern, .. } => {
            matches_anything(pattern)
        }
        pattern => irrefutable_leaf(pattern),
    }
}
