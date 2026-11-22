//! std's range types as values: `Range`, `RangeInclusive`, `RangeFrom`,
//! `RangeTo`, `RangeToInclusive` and `RangeFull`. Which std struct a
//! lowered range type is, and the bounds of a range at runtime, which its
//! fields, `contains`, `is_empty`, `==` and the order a set keeps read.

use super::ty::{Float, Ty};
use super::wide::{wide_i128, wide_u128, wide_value};
use super::Parser;
use crate::{capability, Dynamic, INT};
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

/// A range open at one end or both, as `a..`, `..b`, `..=b` and `..` are:
/// std's `RangeFrom`, `RangeTo`, `RangeToInclusive` and `RangeFull`. Each
/// is a `RangeFrom<Dynamic>` (and kin) at runtime, of its bound's value,
/// and `RangeFull` std's own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Open {
    From,
    To,
    ToInclusive,
    Full,
}

impl Open {
    /// std's struct of the range.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::From => "RangeFrom",
            Self::To => "RangeTo",
            Self::ToInclusive => "RangeToInclusive",
            Self::Full => "RangeFull",
        }
    }

    /// std's definition of the range's struct.
    pub(super) const fn definition(self) -> &'static str {
        match self {
            Self::From => "core::ops::range::RangeFrom",
            Self::To => "core::ops::range::RangeTo",
            Self::ToInclusive => "core::ops::range::RangeToInclusive",
            Self::Full => "core::ops::range::RangeFull",
        }
    }

    /// The range rawr's builtin type `name` is.
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "RangeFrom" => Self::From,
            "RangeTo" => Self::To,
            "RangeToInclusive" => Self::ToInclusive,
            "RangeFull" => Self::Full,
            _ => return None,
        })
    }

    /// The `RangeNew` code that makes a value of the range.
    pub(super) const fn code(self) -> INT {
        match self {
            Self::From => 0,
            Self::To => 1,
            Self::ToInclusive => 2,
            Self::Full => 3,
        }
    }
}

/// The runtime type of an `open` range's value.
pub(super) fn type_id(open: Open) -> std::any::TypeId {
    use std::any::TypeId;
    match open {
        Open::From => TypeId::of::<RangeFrom<Dynamic>>(),
        Open::To => TypeId::of::<RangeTo<Dynamic>>(),
        Open::ToInclusive => TypeId::of::<RangeToInclusive<Dynamic>>(),
        Open::Full => TypeId::of::<RangeFull>(),
    }
}

/// The `RangeNew` code of `low..high` of chars, a `Range<char>` at runtime.
pub(super) const CHAR_RANGE: INT = 4;

/// The `RangeNew` code of `low..=high` of chars or a crate's type, a
/// `RangeInclusive<Dynamic>` at runtime.
pub(super) const INCLUSIVE_OF: INT = 5;

/// A range value of `code` (`Open::code`, `CHAR_RANGE`, `INCLUSIVE_OF`) of
/// its bounds.
pub(super) fn new(code: INT, low: &Dynamic, high: &Dynamic) -> Option<Dynamic> {
    Some(match code {
        0 => Dynamic::from(low.clone()..),
        1 => Dynamic::from(..high.clone()),
        2 => Dynamic::from(..=high.clone()),
        3 => Dynamic::from(..),
        CHAR_RANGE => Dynamic::from(low.as_char().ok()?..high.as_char().ok()?),
        INCLUSIVE_OF => Dynamic::from(low.clone()..=high.clone()),
        _ => return None,
    })
}

/// Which of std's range structs a runtime range is.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Kind {
    Range,
    Inclusive,
    From,
    To,
    ToInclusive,
    Full,
}

/// A runtime range's bounds, each as the runtime keeps a value of it.
pub(super) struct Bounds {
    pub(super) kind: Kind,
    pub(super) start: Option<Dynamic>,
    pub(super) end: Option<Dynamic>,
}

impl Bounds {
    fn new(kind: Kind, start: Option<Dynamic>, end: Option<Dynamic>) -> Self {
        Self { kind, start, end }
    }

    /// Whether the end is in the range.
    pub(super) const fn inclusive(&self) -> bool {
        matches!(self.kind, Kind::Inclusive | Kind::ToInclusive)
    }

    /// Whether `value` is in the range, its bounds ordered by `order`.
    pub(super) fn contains(
        &self,
        value: &Dynamic,
        order: impl Fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>,
    ) -> bool {
        use std::cmp::Ordering;
        let above = self.start.as_ref().is_none_or(|start| {
            matches!(order(start, value), Some(Ordering::Less | Ordering::Equal))
        });
        let below = self.end.as_ref().is_none_or(|end| match order(value, end) {
            Some(Ordering::Less) => true,
            Some(Ordering::Equal) => self.inclusive(),
            _ => false,
        });
        above && below
    }

    /// How many items a range of integers or chars walks, as std's
    /// `ExactSizeIterator::len` counts them.
    pub(super) fn len(&self) -> Option<INT> {
        let (start, end) = (self.start.as_ref()?, self.end.as_ref()?);
        let (start, end) = match (start.as_char(), end.as_char()) {
            (Ok(start), Ok(end)) => (i128::from(u32::from(start)), i128::from(u32::from(end))),
            _ => (wide_value(start)?.signed(), wide_value(end)?.signed()),
        };
        let length = end
            .saturating_sub(start)
            .saturating_add(i128::from(self.inclusive()));
        Some(INT::try_from(length.max(0)).unwrap_or(INT::MAX))
    }

    /// Whether the range holds no value, as std's `is_empty` of a `Range`
    /// or a `RangeInclusive` says: `!(start < end)`, `!(start <= end)`.
    pub(super) fn is_empty(
        &self,
        order: impl Fn(&Dynamic, &Dynamic) -> Option<std::cmp::Ordering>,
    ) -> bool {
        use std::cmp::Ordering;
        let (Some(start), Some(end)) = (&self.start, &self.end) else {
            return false;
        };
        match order(start, end) {
            Some(Ordering::Less) => false,
            Some(Ordering::Equal) => !self.inclusive(),
            _ => true,
        }
    }
}

/// `value..value`, the `Range` a derived `Default` makes of its element's
/// default: a `Range` of the runtime's own kind of it.
pub(crate) fn empty(value: &Dynamic) -> Option<Dynamic> {
    if let Ok(number) = value.as_int() {
        return Some(Dynamic::from(number..number));
    }
    if let Ok(number) = value.as_float() {
        return Some(Dynamic::from(number..number));
    }
    value.as_char().ok().map(|code| Dynamic::from(code..code))
}

/// The items of a range the runtime's own iterators don't walk, as std's
/// `Step` gives them: a `Range<char>`, and a `RangeInclusive` or a
/// `RangeFrom` of chars or integers.
pub(crate) fn steps(value: Dynamic) -> Box<dyn Iterator<Item = Dynamic>> {
    let Some(bounds) = read(&value) else {
        return Box::new(std::iter::empty());
    };
    let Some(start) = bounds.start else {
        return Box::new(std::iter::empty());
    };
    let inclusive = bounds.kind == Kind::Inclusive;
    match (start.as_char(), start.as_int(), bounds.end) {
        (Ok(start), _, Some(end)) => {
            let end = end.as_char().unwrap_or(start);
            match inclusive {
                true => Box::new((start..=end).map(Dynamic::from_char)),
                false => Box::new((start..end).map(Dynamic::from_char)),
            }
        }
        (Ok(start), _, None) => Box::new((start..=char::MAX).map(Dynamic::from_char)),
        (_, Ok(start), Some(end)) => {
            let end = end.as_int().unwrap_or(start);
            match inclusive {
                true => Box::new((start..=end).map(Dynamic::from_int)),
                false => Box::new((start..end).map(Dynamic::from_int)),
            }
        }
        (_, Ok(start), None) => Box::new((start..).map(Dynamic::from_int)),
        _ => Box::new(std::iter::empty()),
    }
}

/// `Iterator::next` of a range at runtime: its next item, if any, and the
/// range after it, which the place that holds it keeps.
pub(super) fn next(value: &Dynamic) -> Option<(Option<Dynamic>, Dynamic)> {
    macro_rules! own {
        ($($range:ty => $store:expr),*) => {
            $(
                if let Some(range) = value.read_lock::<$range>() {
                    let mut range = range.clone();
                    let item = range.next().map($store);
                    return Some((item, Dynamic::from(range)));
                }
            )*
        };
    }
    own!(
        Range<INT> => Dynamic::from_int,
        RangeInclusive<INT> => Dynamic::from_int,
        Range<char> => Dynamic::from_char,
        Range<i128> => wide_i128,
        Range<u128> => wide_u128,
        RangeInclusive<i128> => wide_i128,
        RangeInclusive<u128> => wide_u128
    );
    // A `RangeFrom` or a `RangeInclusive` of a program's chars or integers.
    let bounds = read(value)?;
    let start = bounds.start?;
    let after = match (start.as_char(), start.as_int()) {
        (Ok(code), _) => {
            // Over the surrogates, as `Step` for `char` steps.
            let next = match u32::from(code) + 1 {
                0xD800 => 0xE000,
                next => next,
            };
            Dynamic::from_char(char::from_u32(next)?)
        }
        (_, Ok(number)) => Dynamic::from_int(number.checked_add(1)?),
        _ => return None,
    };
    match (bounds.kind, bounds.end) {
        (Kind::From, _) => Some((Some(start), Dynamic::from(after..))),
        (Kind::Inclusive, Some(end)) => {
            let ended = match (start.as_char(), end.as_char(), start.as_int(), end.as_int()) {
                (Ok(start), Ok(end), ..) => start > end,
                (.., Ok(start), Ok(end)) => start > end,
                _ => true,
            };
            if ended {
                return Some((None, value.clone()));
            }
            Some((Some(start), Dynamic::from(after..=end)))
        }
        _ => None,
    }
}

/// The bounds of `value` when it is one of std's ranges at runtime.
pub(super) fn read(value: &Dynamic) -> Option<Bounds> {
    macro_rules! of {
        ($($element:ty => $store:expr),*) => {
            $(
                if let Some(range) = value.read_lock::<Range<$element>>() {
                    return Some(Bounds::new(
                        Kind::Range,
                        Some($store(range.start.clone())),
                        Some($store(range.end.clone())),
                    ));
                }
                if let Some(range) = value.read_lock::<RangeInclusive<$element>>() {
                    return Some(Bounds::new(
                        Kind::Inclusive,
                        Some($store(range.start().clone())),
                        Some($store(range.end().clone())),
                    ));
                }
            )*
        };
    }
    of!(
        INT => Dynamic::from_int,
        crate::FLOAT => Dynamic::from_float,
        char => Dynamic::from_char,
        i128 => wide_i128,
        u128 => wide_u128,
        Dynamic => std::convert::identity
    );
    if let Some(range) = value.read_lock::<RangeFrom<Dynamic>>() {
        return Some(Bounds::new(Kind::From, Some(range.start.clone()), None));
    }
    if let Some(range) = value.read_lock::<RangeTo<Dynamic>>() {
        return Some(Bounds::new(Kind::To, None, Some(range.end.clone())));
    }
    if let Some(range) = value.read_lock::<RangeToInclusive<Dynamic>>() {
        return Some(Bounds::new(
            Kind::ToInclusive,
            None,
            Some(range.end.clone()),
        ));
    }
    value
        .is::<RangeFull>()
        .then(|| Bounds::new(Kind::Full, None, None))
}

/// Which of std's range structs `ty` is, and its element (`()` for a
/// `RangeFull`): `None` for another type.
pub(super) fn parts(ty: &Ty) -> Option<(Kind, Ty)> {
    Some(match ty {
        Ty::Range(element) => (Kind::Range, (**element).clone()),
        Ty::InclusiveRange(kind) => (Kind::Inclusive, Ty::Int(*kind)),
        Ty::FloatRange => (Kind::Inclusive, Ty::Float(Float::F64)),
        Ty::InclusiveOf(element) => (Kind::Inclusive, (**element).clone()),
        Ty::OpenRange(open, element) => (
            match open {
                Open::From => Kind::From,
                Open::To => Kind::To,
                Open::ToInclusive => Kind::ToInclusive,
                Open::Full => Kind::Full,
            },
            (**element).clone(),
        ),
        _ => return None,
    })
}

impl Parser<'_> {
    /// The item of `ty` when it is a range std iterates: a `Range`, a
    /// `RangeInclusive` or a `RangeFrom` of a `Step` type, an integer or a
    /// `char`.
    pub(super) fn stepped_item(&self, ty: &Ty) -> Option<Ty> {
        let (kind, element) = parts(&self.inference.resolve(ty))?;
        let element = self.inference.resolve(&element);
        (matches!(kind, Kind::Range | Kind::Inclusive | Kind::From)
            && matches!(element, Ty::Int(_) | Ty::Char))
        .then_some(element)
    }

    /// What a range type provides, as std derives and implements it for
    /// each range struct: `None` for another type.
    pub(super) fn range_provides(&self, ty: &Ty, capability: capability::Kind) -> Option<bool> {
        let (kind, element) = parts(ty)?;
        let element_provides = || self.type_provides(&element, capability);
        Some(match capability {
            capability::Kind::Iterator => self.stepped_item(ty).is_some(),
            capability::Kind::Copy => {
                matches!(kind, Kind::To | Kind::ToInclusive | Kind::Full) && element_provides()
            }
            capability::Kind::Default => {
                matches!(kind, Kind::Range | Kind::Full) && element_provides()
            }
            capability::Kind::PartialOrd | capability::Kind::Ord | capability::Kind::Error => false,
            capability::Kind::Clone
            | capability::Kind::Debug
            | capability::Kind::PartialEq
            | capability::Kind::Eq
            | capability::Kind::Hash
            | capability::Kind::Send
            | capability::Kind::Static => element_provides(),
        })
    }
}
