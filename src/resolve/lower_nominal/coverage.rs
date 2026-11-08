//! Match exhaustiveness: the patterns a match covers, specialised by
//! constructor, and the witnesses of what it leaves out.

use super::ty::{Int, Ty};
use super::{PatternSyntax, Prelude};
use crate::ast;
use crate::resolve::Error;
use crate::types::nominal;
use crate::{type_def, ImmutableString, Span};

/// One constructor of a matched type: an enum variant, or a prelude
/// constructor (`Some`, `None`, `Ok`, `Err`).
#[derive(Clone, PartialEq)]
pub(super) enum CoverageKey {
    Enum {
        identity: type_def::Identity,
        variant: nominal::variant::Id,
    },
    Prelude(Prelude),
    /// `Cow::Owned` (`true`) or `Cow::Borrowed`.
    Cow(bool),
    /// A literal: of `bool`, one of its two values; otherwise one of
    /// unboundedly many.
    Literal(ast::NominalLiteral),
    /// A range: never a complete set of values on its own.
    Range,
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
    payload: &[CoveragePattern],
    length: usize,
    open: bool,
) -> Option<Vec<CoveragePattern>> {
    if !rest {
        return (!open && payload.len() == length).then(|| payload.to_vec());
    }
    if payload.len() > length {
        return None;
    }
    let mut specialized = payload[..before].to_vec();
    specialized.extend(std::iter::repeat_n(
        CoveragePattern::Wildcard,
        length - payload.len(),
    ));
    specialized.extend_from_slice(&payload[before..]);
    Some(specialized)
}

/// The `..` of a tuple-like pattern's `members`: its index, with Rust's
/// errors for a second one and for `name @ ..` outside a slice.
pub(super) fn tuple_rest(members: &[PatternSyntax]) -> Result<Option<usize>, Error> {
    let mut found = None;
    for (index, member) in members.iter().enumerate() {
        let PatternSyntax::Rest { binding, span } = member else {
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
        if found.is_some() {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`..` can only be used once per tuple pattern".into(),
                source: *span,
            });
        }
        found = Some(index);
    }
    Ok(found)
}

/// `members` of a tuple-like pattern of `arity` elements, its `..` read as
/// the `discard`s that fill the arity: Rust's `(a, .., z)`.
pub(super) fn tuple_members<'a>(
    members: &'a [PatternSyntax],
    arity: usize,
    discard: &'a PatternSyntax,
) -> Result<Vec<&'a PatternSyntax>, Error> {
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
    pattern: &CoveragePattern,
    constructor: &CoverageConstructor,
) -> Option<Vec<CoveragePattern>> {
    match (pattern, &constructor.key) {
        (CoveragePattern::Wildcard, _) => {
            Some(vec![CoveragePattern::Wildcard; constructor.payload.len()])
        }
        (
            CoveragePattern::Constructor {
                key: CoverageKey::Slice { before, rest },
                payload,
                ..
            },
            CoverageKey::SliceLength { length, open },
        ) => specialize_slice(*before, *rest, payload, *length, *open),
        (CoveragePattern::Constructor { key, payload, .. }, constructor_key)
            if key == constructor_key =>
        {
            Some(payload.to_vec())
        }
        (CoveragePattern::Constructor { .. }, _) => None,
        (CoveragePattern::Or(_), _) => unreachable!("or-patterns are expanded first"),
    }
}

#[derive(Clone)]
pub(super) enum CoveragePattern {
    Wildcard,
    Constructor {
        key: CoverageKey,
        /// `Enum::Variant`, `Some`, `None`, a literal, or `(..)`.
        name: ImmutableString,
        payload: Box<[CoveragePattern]>,
    },
    /// `p | q`, expanded before coverage is checked.
    Or(Box<[CoveragePattern]>),
}

/// For each declared field, the index of the written field pattern that
/// names it, `None` where `..` skips it; Rust's errors for an unknown field
/// and, without `..`, a missing one.
pub(super) fn field_order<'a>(
    owner: &str,
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
        return Err(Error::UnknownMember {
            owner: owner.into(),
            member: unknown.clone(),
            reference: *reference,
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
pub(super) fn bool_literals(syntax: &PatternSyntax, seen: &mut [bool; 2]) {
    match syntax {
        PatternSyntax::Literal {
            literal: ast::NominalLiteral::Bool(value),
            ..
        } => seen[usize::from(*value)] = true,
        PatternSyntax::Or { alternatives, .. } => {
            for alternative in alternatives.iter() {
                bool_literals(alternative, seen);
            }
        }
        PatternSyntax::At { pattern, .. } => bool_literals(pattern, seen),
        _ => {}
    }
}

/// The type of a match on integers or `char`s, whose arms' values rawr
/// reads as ranges to name the values no arm matches as rustc does
/// (E0004). Values are ordinals: an integer's in the order of its kind,
/// a `char`'s its scalar value.
#[derive(Clone, Copy)]
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
        syntax: &PatternSyntax,
        covered: &mut Vec<(u128, u128)>,
    ) -> Option<()> {
        match syntax {
            PatternSyntax::Literal { literal, .. } => {
                let ordinal = self.ordinal(literal)?;
                covered.push((ordinal, ordinal));
            }
            PatternSyntax::Range {
                low,
                high,
                inclusive,
                ..
            } => {
                let (min, max) = self.bounds();
                let low = match low {
                    Some((low, _)) => self.ordinal(low)?,
                    None => min,
                };
                let high = match high {
                    Some((high, _)) if *inclusive => self.ordinal(high)?,
                    Some((high, _)) => self.ordinal(high)?.checked_sub(1)?,
                    None => max,
                };
                if low <= high {
                    covered.push((low, high));
                }
            }
            PatternSyntax::Binding { .. } | PatternSyntax::Discard { .. } => {
                covered.push(self.bounds());
            }
            PatternSyntax::Or { alternatives, .. } => {
                for alternative in alternatives.iter() {
                    self.covered(alternative, covered)?;
                }
            }
            PatternSyntax::At { pattern, .. } => self.covered(pattern, covered)?,
            _ => return None,
        }
        Some(())
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

impl CoveragePattern {
    pub(super) fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    pub(super) fn render(&self) -> String {
        match self {
            Self::Wildcard => "_".to_string(),
            Self::Constructor {
                key: CoverageKey::Tuple,
                payload,
                ..
            } => format!(
                "({})",
                payload
                    .iter()
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Constructor {
                key: CoverageKey::Slice { before, rest },
                payload,
                ..
            } => {
                let mut elements = payload.iter().map(Self::render).collect::<Vec<_>>();
                if *rest {
                    elements.insert(*before, "..".into());
                }
                format!("[{}]", elements.join(", "))
            }
            Self::Constructor { name, payload, .. } => {
                render_constructor(name, payload, Self::render)
            }
            Self::Or(alternatives) => alternatives
                .iter()
                .map(Self::render)
                .collect::<Vec<_>>()
                .join(" | "),
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

pub(super) struct CoverageConstructor {
    pub(super) key: CoverageKey,
    pub(super) name: ImmutableString,
    pub(super) payload: Box<[Ty]>,
}

#[derive(Clone)]
pub(super) enum CoverageWitness {
    Wildcard,
    Constructor {
        name: ImmutableString,
        payload: Box<[CoverageWitness]>,
    },
}

impl CoverageWitness {
    pub(super) fn render(&self) -> String {
        match self {
            Self::Wildcard => "_".to_string(),
            Self::Constructor { name, payload } if name.is_empty() => format!(
                "({})",
                payload
                    .iter()
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            // An array's or slice's length (`coverage_constructors`).
            Self::Constructor { name, payload } if name.starts_with('[') => {
                let mut elements = payload.iter().map(Self::render).collect::<Vec<_>>();
                if name.as_str() == "[..]" {
                    elements.push("..".into());
                }
                format!("[{}]", elements.join(", "))
            }
            Self::Constructor { name, payload } => render_constructor(name, payload, Self::render),
        }
    }
}

pub(super) fn specialize_coverage_rows(
    rows: &[Vec<CoveragePattern>],
    constructor: &CoverageConstructor,
) -> Vec<Vec<CoveragePattern>> {
    rows.iter()
        .filter_map(|row| {
            let mut specialized = specialize_coverage_pattern(&row[0], constructor)?;
            specialized.extend_from_slice(&row[1..]);
            Some(specialized)
        })
        .collect()
}

pub(super) fn specialize_coverage_candidate(
    candidate: &[CoveragePattern],
    constructor: &CoverageConstructor,
) -> Vec<CoveragePattern> {
    let mut specialized = specialize_coverage_pattern(&candidate[0], constructor)
        .expect("the candidate matches its constructor");
    specialized.extend_from_slice(&candidate[1..]);
    specialized
}

pub(super) fn specialize_coverage_types(
    types: &[Ty],
    constructor: &CoverageConstructor,
) -> Vec<Ty> {
    let mut specialized = constructor.payload.to_vec();
    specialized.extend_from_slice(&types[1..]);
    specialized
}

pub(super) fn default_coverage_rows(rows: &[Vec<CoveragePattern>]) -> Vec<Vec<CoveragePattern>> {
    rows.iter()
        .filter(|row| row[0].is_wildcard())
        .map(|row| row[1..].to_vec())
        .collect()
}
