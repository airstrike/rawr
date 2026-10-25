//! Rust's `PartialEq`, `PartialOrd` and `Ord` of runtime values, as a
//! derive or a builtin compares them, with a crate type's hand-written
//! impl wherever its values sit: in a tuple, an `Option`, a vector, a map
//! key or a derived type's field (D419, D423). A [`Shape`] says where a
//! `u64` or `usize` sits, whose bits order as an unsigned number (D425).

use super::*;

/// Where a value's static type reads integers as unsigned: a `u64` or a
/// `usize` is its bits in the runtime's `i64`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Shape {
    /// Compared as the values say.
    Plain,
    /// A `u64` or a `usize`.
    Unsigned,
    /// Each element of a vector, array, slice or set.
    Each(Box<Shape>),
    /// A tuple's members; a map's `[key, value]` entry.
    Tuple(Box<[Shape]>),
    Option(Box<Shape>),
    Result(Box<Shape>, Box<Shape>),
}

impl Shape {
    pub(super) fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }

    /// The shape of `ty`, each part read through `resolve`. A crate type
    /// is plain: its fields carry their own shapes (`field_shape`).
    pub(super) fn of(ty: &Ty, resolve: &dyn Fn(&Ty) -> Ty) -> Self {
        let of = |ty: &Ty| Self::of(ty, resolve);
        let shape = match resolve(ty) {
            Ty::Int(Int::U64 | Int::Usize) => Self::Unsigned,
            Ty::Reference { target, .. } | Ty::Boxed(_, target) => of(&target),
            Ty::Option(element) => Self::Option(Box::new(of(&element))),
            Ty::Result(ok, err) => Self::Result(Box::new(of(&ok)), Box::new(of(&err))),
            Ty::Array { element, .. } => Self::Each(Box::new(of(&element))),
            Ty::Tuple(members) => Self::Tuple(members.iter().map(of).collect()),
            Ty::Keyed(kind, arguments) if kind.is_map() && arguments.len() == 2 => {
                Self::Each(Box::new(Self::Tuple(arguments.iter().map(of).collect())))
            }
            Ty::Keyed(_, arguments) if arguments.len() == 1 => {
                Self::Each(Box::new(of(&arguments[0])))
            }
            _ => Self::Plain,
        };
        shape.simplified()
    }

    /// `Plain` where nothing inside is unsigned.
    fn simplified(self) -> Self {
        let plain = match &self {
            Self::Plain | Self::Unsigned => return self,
            Self::Each(inner) | Self::Option(inner) => inner.is_plain(),
            Self::Tuple(members) => members.iter().all(Self::is_plain),
            Self::Result(ok, err) => ok.is_plain() && err.is_plain(),
        };
        if plain {
            Self::Plain
        } else {
            self
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plain => formatter.write_str("p"),
            Self::Unsigned => formatter.write_str("u"),
            Self::Each(inner) => write!(formatter, "e({inner})"),
            Self::Option(inner) => write!(formatter, "o({inner})"),
            Self::Result(ok, err) => write!(formatter, "r({ok},{err})"),
            Self::Tuple(members) => {
                formatter.write_str("t(")?;
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{member}")?;
                }
                formatter.write_str(")")
            }
        }
    }
}

/// The shape of a derived type's field of declared type `ty`.
fn field_shape(ty: &type_ref::Type) -> Shape {
    if mentions_unsigned(ty) {
        Shape::of(&Ty::from_resolved(ty), &Ty::clone)
    } else {
        Shape::Plain
    }
}

fn mentions_unsigned(ty: &type_ref::Type) -> bool {
    match ty {
        type_ref::Type::Unit(_) => false,
        type_ref::Type::Tuple { members, .. } => members.iter().any(mentions_unsigned),
        type_ref::Type::Reference { target, .. } => mentions_unsigned(target),
        type_ref::Type::Path(path) => {
            path.target == type_ref::Target::Builtin
                && matches!(path.name.as_str(), "u64" | "usize")
                || path.arguments.iter().any(|argument| match argument {
                    type_ref::Argument::Type(ty) => mentions_unsigned(ty),
                    type_ref::Argument::Lifetime(_) => false,
                })
        }
    }
}

/// A comparison of runtime values, which calls a crate type's own
/// `eq`, `partial_cmp` or `cmp` through `context`. Without a context (a
/// constant folded at compile time) a type compares as derived.
pub(super) struct Comparison<'a, 'c> {
    pub(super) models: &'a Models,
    pub(super) context: Option<&'a NativeCallContext<'c>>,
    /// `Ord` (`cmp`), as a sort or a `BTreeMap` orders, rather than
    /// `PartialOrd` (`partial_cmp`), as `<` does.
    pub(super) total: bool,
}

type Compared<T> = Result<T, crate::RawrError>;

impl Comparison<'_, '_> {
    /// Rust's `PartialOrd` (or `Ord`, when `total`) of `lhs` and `rhs`:
    /// `None` when they are unordered, as a NaN is.
    pub(super) fn compare(
        &self,
        lhs: &Dynamic,
        rhs: &Dynamic,
        shape: &Shape,
    ) -> Compared<Option<std::cmp::Ordering>> {
        use std::cmp::Ordering;
        match shape {
            Shape::Unsigned => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_int(), rhs.as_int()) {
                    return Ok(Some((lhs as u64).cmp(&(rhs as u64))));
                }
            }
            Shape::Option(inner) => {
                if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs))
                {
                    return match (lhs, rhs) {
                        (Some(lhs), Some(rhs)) => self.compare(&lhs, &rhs, inner),
                        (lhs, rhs) => Ok(Some(lhs.is_some().cmp(&rhs.is_some()))),
                    };
                }
            }
            Shape::Result(ok, err) => {
                if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs))
                {
                    return match (lhs, rhs) {
                        (Ok(lhs), Ok(rhs)) => self.compare(&lhs, &rhs, ok),
                        (Err(lhs), Err(rhs)) => self.compare(&lhs, &rhs, err),
                        (Ok(_), Err(_)) => Ok(Some(Ordering::Less)),
                        (Err(_), Ok(_)) => Ok(Some(Ordering::Greater)),
                    };
                }
            }
            #[cfg(not(feature = "no_index"))]
            Shape::Each(inner) => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
                    return self.lexicographic(&lhs, &rhs, |_| inner);
                }
            }
            #[cfg(not(feature = "no_index"))]
            Shape::Tuple(members) => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
                    return self.lexicographic(&lhs, &rhs, |index| {
                        members.get(index).unwrap_or(&Shape::Plain)
                    });
                }
            }
            _ => {}
        }
        self.compare_plain(lhs, rhs)
    }

    fn lexicographic<'s>(
        &self,
        lhs: &[Dynamic],
        rhs: &[Dynamic],
        shape: impl Fn(usize) -> &'s Shape,
    ) -> Compared<Option<std::cmp::Ordering>> {
        for (index, (lhs, rhs)) in lhs.iter().zip(rhs).enumerate() {
            match self.compare(lhs, rhs, shape(index))? {
                Some(std::cmp::Ordering::Equal) => {}
                ordering => return Ok(ordering),
            }
        }
        Ok(Some(lhs.len().cmp(&rhs.len())))
    }

    fn compare_plain(&self, lhs: &Dynamic, rhs: &Dynamic) -> Compared<Option<std::cmp::Ordering>> {
        let identities = match (&lhs.0, &rhs.0) {
            (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
                Some((lhs.identity().clone(), rhs.identity().clone()))
            }
            (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
                Some((lhs.identity().clone(), rhs.identity().clone()))
            }
            _ => None,
        };
        let Some((identity, other)) = identities else {
            return self.compare_builtin(lhs, rhs);
        };
        if identity != other {
            return Ok(None);
        }
        let Some(nominal) = self.models.by_identity.get(&identity) else {
            return Ok(None);
        };
        // A hand-written impl: `cmp` for `Ord`, `partial_cmp` for `<`, or
        // whichever the type has.
        let owner = nominal.definition().id();
        let preferred = if self.total {
            [("Ord", "cmp"), ("PartialOrd", "partial_cmp")]
        } else {
            [("PartialOrd", "partial_cmp"), ("Ord", "cmp")]
        };
        if let Some(context) = self.context
            && let Some((function, definition)) =
                preferred.iter().find_map(|(trait_name, function)| {
                    Some((
                        *function,
                        self.models.std_impl(owner, trait_name, function)?,
                    ))
                })
        {
            let ordering = call(context, definition, lhs, rhs)?;
            let ordering = if function == "cmp" {
                Some(ordering)
            } else {
                crate::option::read(&ordering)
                    .ok_or_else(|| invariant("`partial_cmp` did not give an `Option`"))?
            };
            return Ok(ordering
                .and_then(|ordering| ordering.as_int().ok())
                .map(|code| code.cmp(&0)));
        }
        // A derive: fields in declaration order; an enum's variants by
        // declaration order, then their payloads.
        match (nominal, &lhs.0, &rhs.0) {
            (Nominal::Struct { item, .. }, Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
                let shapes = item
                    .fields
                    .iter()
                    .map(|field| field_shape(&field.ty))
                    .collect::<Vec<_>>();
                self.lexicographic(lhs.fields(), rhs.fields(), |index| {
                    shapes.get(index).unwrap_or(&Shape::Plain)
                })
            }
            (Nominal::Enum { item, .. }, Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
                let index = |name: &str| {
                    item.variants
                        .iter()
                        .position(|variant| variant.name.text == name)
                };
                let (Some(variant), Some(other)) =
                    (index(lhs.variant().name()), index(rhs.variant().name()))
                else {
                    return Ok(None);
                };
                if variant != other {
                    return Ok(Some(variant.cmp(&other)));
                }
                let shapes = item.variants[variant]
                    .payload
                    .iter()
                    .map(field_shape)
                    .collect::<Vec<_>>();
                self.lexicographic(lhs.payload(), rhs.payload(), |index| {
                    shapes.get(index).unwrap_or(&Shape::Plain)
                })
            }
            _ => Ok(None),
        }
    }

    fn compare_builtin(
        &self,
        lhs: &Dynamic,
        rhs: &Dynamic,
    ) -> Compared<Option<std::cmp::Ordering>> {
        if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
            return match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => self.compare_plain(&lhs, &rhs),
                (lhs, rhs) => Ok(Some(lhs.is_some().cmp(&rhs.is_some()))),
            };
        }
        if let (Some(lhs), Some(rhs)) = (
            lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
            rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        ) {
            return self.compare_plain(&rhs.0, &lhs.0);
        }
        #[cfg(not(feature = "no_index"))]
        if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
            return self.lexicographic(&lhs, &rhs, |_| &Shape::Plain);
        }
        Ok(compare_value(lhs, rhs, self.models))
    }

    /// Rust's `PartialEq` of `lhs` and `rhs`.
    pub(super) fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Compared<bool> {
        if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
            return match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => self.equal(&lhs, &rhs),
                (lhs, rhs) => Ok(lhs.is_none() && rhs.is_none()),
            };
        }
        if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
            return match (lhs, rhs) {
                (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => self.equal(&lhs, &rhs),
                _ => Ok(false),
            };
        }
        if let (Some(lhs), Some(rhs)) = (
            lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
            rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        ) {
            return self.equal(&lhs.0, &rhs.0);
        }
        match (&lhs.0, &rhs.0) {
            #[cfg(not(feature = "no_index"))]
            (Union::Array(left, ..), Union::Array(right, ..)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (lhs, rhs) in left.iter().zip(right.iter()) {
                    if !self.equal(lhs, rhs)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Union::Struct(left, ..), Union::Struct(right, ..)) => {
                if left.identity() != right.identity() || left.schema() != right.schema() {
                    return Ok(false);
                }
                if let Some(equal) = self.hand_written_eq(left.identity(), lhs, rhs)? {
                    return Ok(equal);
                }
                self.all_equal(left.fields(), right.fields())
            }
            (Union::Enum(left, ..), Union::Enum(right, ..)) => {
                if left.identity() != right.identity() || left.schema() != right.schema() {
                    return Ok(false);
                }
                if let Some(equal) = self.hand_written_eq(left.identity(), lhs, rhs)? {
                    return Ok(equal);
                }
                if left.variant() != right.variant() {
                    return Ok(false);
                }
                self.all_equal(left.payload(), right.payload())
            }
            _ => Ok(equal_value(lhs, rhs)),
        }
    }

    fn all_equal(&self, lhs: &[Dynamic], rhs: &[Dynamic]) -> Compared<bool> {
        if lhs.len() != rhs.len() {
            return Ok(false);
        }
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            if !self.equal(lhs, rhs)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The type's own `eq` of `lhs` and `rhs`, when it has one.
    fn hand_written_eq(
        &self,
        identity: &type_def::Identity,
        lhs: &Dynamic,
        rhs: &Dynamic,
    ) -> Compared<Option<bool>> {
        let Some(context) = self.context else {
            return Ok(None);
        };
        let Some(nominal) = self.models.by_identity.get(identity) else {
            return Ok(None);
        };
        let Some(definition) = self
            .models
            .std_impl(nominal.definition().id(), "PartialEq", "eq")
        else {
            return Ok(None);
        };
        let equal = call(context, definition, lhs, rhs)?;
        Ok(Some(equal.as_bool().unwrap_or(false)))
    }
}

/// `value` with each integer its shape says is unsigned tagged as one
/// (`UNSIGNED_TAG`), for a generic body that reads it without its type.
pub(super) fn tag_unsigned(mut value: Dynamic, shape: &Shape) -> Dynamic {
    match shape {
        Shape::Plain => {}
        Shape::Unsigned => {
            if value.is_int() {
                value.set_tag(crate::types::dynamic::UNSIGNED_TAG);
            }
        }
        Shape::Option(inner) => {
            if let Some(Some(element)) = crate::option::read(&value) {
                return crate::option::some(tag_unsigned(element, inner));
            }
        }
        Shape::Result(ok, err) => match crate::result::read(&value) {
            Some(Ok(element)) => return crate::result::ok(tag_unsigned(element, ok)),
            Some(Err(element)) => return crate::result::err(tag_unsigned(element, err)),
            None => {}
        },
        #[cfg(not(feature = "no_index"))]
        Shape::Each(_) | Shape::Tuple(_) => {
            if let Ok(mut array) = value.as_array_mut() {
                for (index, element) in array.iter_mut().enumerate() {
                    let inner = match shape {
                        Shape::Each(inner) => &**inner,
                        Shape::Tuple(members) => members.get(index).unwrap_or(&Shape::Plain),
                        _ => &Shape::Plain,
                    };
                    *element = tag_unsigned(std::mem::take(element), inner);
                }
            }
        }
        #[cfg(feature = "no_index")]
        Shape::Each(_) | Shape::Tuple(_) => {}
    }
    value
}

/// The crate's method `definition` on `lhs` with `rhs` as its argument.
fn call(
    context: &NativeCallContext<'_>,
    definition: &method::Definition,
    lhs: &Dynamic,
    rhs: &Dynamic,
) -> RawrResult {
    let (mut receiver, mut argument) = (lhs.clone(), rhs.clone());
    context.call_script_method(
        definition.symbol().as_str(),
        &mut [&mut receiver, &mut argument],
    )
}
