//! Match exhaustiveness: the patterns a match covers, specialised by
//! constructor, and the witnesses of what it leaves out.

use super::models::Nominal;
use super::op::ordering_discriminant;
use super::pattern;
use super::pattern::Prelude;
use super::ty::{Int, Ty};
use super::{cow, instantiate, origin, without_references, Parser};
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
    let mut found = None;
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
            pattern::Syntax::Binding { .. } | pattern::Syntax::Discard { .. } => {
                covered.push(self.bounds());
            }
            pattern::Syntax::Or { alternatives, .. } => {
                for alternative in alternatives.iter() {
                    self.covered(alternative, covered)?;
                }
            }
            pattern::Syntax::At { pattern, .. } => self.covered(pattern, covered)?,
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

/// A crate type's name as a witness writes it: its path's last segment.
fn host_type_name(definition: &crate::host::type_def::Definition) -> String {
    let path = definition.path();
    path.rsplit("::").next().unwrap_or_default().to_owned()
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

    pub(super) fn render(&self) -> String {
        match self {
            Self::Wildcard => "_".to_string(),
            Self::Constructor {
                key: Key::Tuple,
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
                key: Key::Slice { before, rest },
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

pub(super) struct Constructor {
    pub(super) key: Key,
    pub(super) name: ImmutableString,
    pub(super) payload: Box<[Ty]>,
}

#[derive(Clone)]
pub(super) enum Witness {
    Wildcard,
    Constructor {
        name: ImmutableString,
        payload: Box<[Witness]>,
    },
}

impl Witness {
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

pub(super) fn specialize_coverage_types(types: &[Ty], constructor: &Constructor) -> Vec<Ty> {
    let mut specialized = constructor.payload.to_vec();
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
        let mut witnesses = self.uncovered(syntax, coverage, ty);
        let checks = self.host_checks.borrow().clone();
        for (_, check, check_ty, _) in checks {
            if !witnesses.is_empty() {
                break;
            }
            // A facade part's witness in its place in the pattern, the
            // rest of it `_`: `(Style { width: 0_i64, .. }, _)`.
            let at = check.span();
            let prefix = match syntax {
                pattern::Syntax::Reference { .. } => String::new(),
                _ => self.reference_prefix(ty),
            };
            witnesses = self
                .host_uncovered(&check, &check_ty)?
                .into_iter()
                .map(|missed| match syntax.witness_at(at, &missed) {
                    Some(placed) if syntax.span() != at => format!("{prefix}{placed}"),
                    _ => missed,
                })
                .collect();
        }
        if witnesses.is_empty() {
            return Ok(());
        }
        Err(Error::Refutable {
            witnesses: witnesses.into_iter().map(Into::into).collect(),
            ty: self.matched_type(ty),
            site,
            pattern: syntax.span(),
        })
    }

    /// The values of `ty` that `syntax`, a pattern on a facade value, misses,
    /// as rustc writes them: `Style { width: i64::MIN..=0_i64, .. }` for the
    /// first field whose pattern misses some.
    pub(super) fn host_uncovered(
        &mut self,
        syntax: &pattern::Syntax,
        ty: &Ty,
    ) -> Result<Vec<String>, Error> {
        if !syntax.may_refute() {
            return Ok(Vec::new());
        }
        let referenced = self.referenced(ty);
        match syntax {
            pattern::Syntax::At { pattern, .. } | pattern::Syntax::Reference { pattern, .. } => {
                self.host_uncovered(pattern, &referenced)
            }
            pattern::Syntax::Enum(pattern)
                if pattern.host_struct && matches!(referenced, Ty::Host { .. }) =>
            {
                let names = pattern
                    .named
                    .as_ref()
                    .map(|(names, _)| names.as_ref())
                    .unwrap_or_default();
                let owner = pattern
                    .owner
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                let prefix = self.reference_prefix(ty);
                for ((field, at), member) in names.iter().zip(pattern.payload.iter()) {
                    if !member.may_refute() {
                        continue;
                    }
                    let value = origin::Fragment::plain("__rawr_field");
                    let read = self.host_struct_field(value, &referenced, field, *at)?;
                    let missed = self.host_uncovered(member, &read.ty)?;
                    if !missed.is_empty() {
                        return Ok(missed
                            .into_iter()
                            .map(|missed| format!("{prefix}{owner} {{ {field}: {missed}, .. }}"))
                            .collect());
                    }
                }
                Ok(Vec::new())
            }
            // A facade enum's variant, whose siblings the facade names.
            _ if matches!(referenced, Ty::Host { .. }) => {
                Ok(vec![format!("{}_", self.reference_prefix(ty))])
            }
            _ => {
                let checks = self.host_checks.borrow().len();
                let resolved = self.resolve_root_pattern(syntax, ty);
                self.host_checks.borrow_mut().truncate(checks);
                Ok(self.uncovered(syntax, &resolved?.coverage, ty))
            }
        }
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

    pub(super) fn coverage_constructors(&self, ty: &Ty) -> Option<Vec<Constructor>> {
        let resolved = self.inference.resolve(ty);
        let prelude = |variant: Prelude, payload: &[Ty]| Constructor {
            key: Key::Prelude(variant),
            name: variant.name().into(),
            payload: payload.into(),
        };
        match resolved {
            Ty::Reference { target, .. } => return self.coverage_constructors(&target),
            Ty::Bool => {
                return Some(
                    [false, true]
                        .into_iter()
                        .map(|value| Constructor {
                            key: Key::Literal(ast::NominalLiteral::Bool(value)),
                            name: value.to_string().into(),
                            payload: Box::default(),
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
                        })
                        .collect(),
                );
            }
            Ty::Tuple(members) => {
                return Some(vec![Constructor {
                    key: Key::Tuple,
                    name: ImmutableString::new(),
                    payload: members,
                }]);
            }
            // An array's one length; a slice's lengths up to the longest
            // slice pattern seen, then every longer one at once.
            Ty::Array { element, kind } => {
                let constructor = |length, open| Constructor {
                    key: Key::SliceLength { length, open },
                    name: if open { "[..]" } else { "[]" }.into(),
                    payload: vec![(*element).clone(); length].into(),
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
            Ty::Cow(borrowed) => {
                let owned = self.owned_of(&borrowed).unwrap_or(Ty::Unknown);
                return Some(
                    [(false, cow::reference_to(*borrowed)), (true, owned)]
                        .into_iter()
                        .map(|(owned, payload)| Constructor {
                            key: Key::Cow(owned),
                            name: if owned { "Cow::Owned" } else { "Cow::Borrowed" }.into(),
                            payload: Box::new([payload]),
                        })
                        .collect(),
                );
            }
            // A crate enum's variants, whose fields' patterns below a
            // program's pattern are each a binding, `_` or a part tested at
            // run time (`host_coverage`), so their types are not needed.
            Ty::Host { proof, .. } => {
                let crate::host::type_def::Shape::Enum(variants) = proof.definition.shape() else {
                    return None;
                };
                let identity = type_def::Identity::Host(proof.definition.identity().clone());
                let owner = host_type_name(&proof.definition);
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
            key: Key::Range,
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
                let crate::host::type_def::Shape::Enum(variants) = proof.definition.shape() else {
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

        match &candidate[0] {
            // A slice pattern covers each length it matches.
            Pattern::Constructor {
                key: Key::Slice { .. },
                ..
            } => {
                let Some(constructors) = self.coverage_constructors(&types[0]) else {
                    return false;
                };
                constructors.iter().all(|constructor| {
                    specialize_coverage_pattern(&candidate[0], constructor).is_none() || {
                        let specialized_rows = specialize_coverage_rows(rows, constructor);
                        let specialized_candidate =
                            specialize_coverage_candidate(candidate, constructor);
                        let specialized_types = specialize_coverage_types(types, constructor);
                        self.coverage_pattern_is_covered(
                            &specialized_types,
                            &specialized_rows,
                            &specialized_candidate,
                        )
                    }
                })
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
                let Some(constructors) = self.coverage_constructors(&types[0]) else {
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
        let prefixed = |heads: &[Witness], tails: Vec<Vec<Witness>>| {
            tails
                .iter()
                .flat_map(|tail| {
                    heads.iter().map(move |head| {
                        let mut witness = vec![head.clone()];
                        witness.extend_from_slice(tail);
                        witness
                    })
                })
                .collect::<Vec<_>>()
        };
        let Some(constructors) = self.coverage_constructors(&types[0]) else {
            let tails = self.coverage_witnesses(&types[1..], &default_coverage_rows(rows), false);
            return prefixed(&[Witness::Wildcard], tails);
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
                        .map(|constructor| Witness::Constructor {
                            name: constructor.name.clone(),
                            payload: (0..constructor.payload.len())
                                .map(|_| Witness::Wildcard)
                                .collect(),
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
                        let mut combined = vec![Witness::Constructor {
                            name: constructor.name.clone(),
                            payload: witness.into_boxed_slice(),
                        }];
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
    /// pattern for each of `tys` in each row, cover every value, as rustc
    /// checks it: the arms on one variant of a facade enum cover it when
    /// their patterns below it cover its fields together, so `A(B(x))` and
    /// `A(C)` cover `A` when `B` and `C` are its field's variants.
    pub(super) fn host_rows_cover(
        &mut self,
        rows: Vec<Vec<&pattern::Syntax>>,
        tys: &[Ty],
        at: Span,
    ) -> Result<bool, Error> {
        let Some((ty, rest)) = tys.split_first() else {
            return Ok(!rows.is_empty());
        };
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
                    pattern::Syntax::Or { alternatives, .. } => firsts.extend(alternatives.iter()),
                    _ => spread.push((first, tail.to_vec())),
                }
            }
        }
        let irrefutable = |pattern: &pattern::Syntax| {
            matches!(
                pattern,
                pattern::Syntax::Binding { .. }
                    | pattern::Syntax::Ref { .. }
                    | pattern::Syntax::Discard { .. }
                    | pattern::Syntax::Rest { .. }
            )
        };
        let resolved = self.inference.resolve(ty);
        let enumerated = match without_references(&resolved) {
            Ty::Host { proof, arguments } => match proof.definition.shape() {
                crate::host::type_def::Shape::Enum(variants) => {
                    Some((proof.clone(), arguments.clone(), variants.to_vec()))
                }
                _ => None,
            },
            _ => None,
        };
        let Some((proof, arguments, variants)) =
            enumerated.filter(|_| spread.iter().any(|(first, _)| !irrefutable(first)))
        else {
            // Only a pattern that matches every value covers a type whose
            // values are not told apart here.
            let wild = spread
                .into_iter()
                .filter(|(first, _)| irrefutable(first))
                .map(|(_, tail)| tail)
                .collect();
            return self.host_rows_cover(wild, rest, at);
        };
        let definition = proof.definition.clone();
        let path: ImmutableString = definition.path().as_str().into();
        let discard = pattern::Syntax::Discard { span: at };
        for variant in &variants {
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
                    proof.owner.as_ref(),
                    Some(&definition),
                )?);
            }
            let mut specialized = Vec::new();
            for (first, tail) in &spread {
                let members = match first {
                    pattern if irrefutable(pattern) => vec![&discard; fields.len()],
                    pattern::Syntax::Enum(pattern)
                        if pattern.variant.as_str() == variant.name() =>
                    {
                        match &pattern.named {
                            Some((names, _)) => {
                                let mut members = vec![&discard; fields.len()];
                                for ((name, _), member) in names.iter().zip(pattern.payload.iter())
                                {
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
                            None if pattern.payload.is_empty() => vec![&discard; fields.len()],
                            None => tuple_members(&pattern.payload, fields.len(), &discard)?,
                        }
                    }
                    _ => continue,
                };
                specialized.push(members.into_iter().chain(tail.iter().copied()).collect());
            }
            let tys: Vec<Ty> = fields.into_iter().chain(rest.iter().cloned()).collect();
            if !self.host_rows_cover(specialized, &tys, at)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
