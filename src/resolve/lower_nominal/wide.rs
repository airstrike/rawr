//! 128-bit integers, and numeric literals whose type is not known yet.

use super::op::PreludeOp;
use super::ty::{Float, Int, Ty};
use super::{invariant, kind_code, origin, span, ParsedExpr, Parser, INT_METHODS};
use crate::func::FnCallArgs;
use crate::resolve::Error;
use crate::tokenizer::Token;
use crate::{Dynamic, EvalAltResult, ImmutableString, Position, RawrResult, Span, INT};

/// A runtime integer as the number it is: an `i64`, or the boxed `i128`
/// or `u128` an `i128` or a `u128` past `i64` is (`Int`).
#[derive(Clone, Copy, Debug)]
pub(super) enum Wide {
    Signed(i128),
    Unsigned(u128),
}

impl Wide {
    /// The number's place in one order of every `i128` and `u128`.
    pub(super) const fn key(self) -> (bool, u128) {
        match self {
            Self::Signed(value) => (value >= 0, value as u128),
            Self::Unsigned(value) => (true, value),
        }
    }

    /// The number as an `i128`, a `u128` past `i128::MAX` wrapped, as `as`
    /// wraps it.
    pub(super) const fn signed(self) -> i128 {
        match self {
            Self::Signed(value) => value,
            Self::Unsigned(value) => value as i128,
        }
    }

    /// The number as a `u128`, a negative one wrapped.
    pub(super) const fn unsigned(self) -> u128 {
        match self {
            Self::Signed(value) => value as u128,
            Self::Unsigned(value) => value,
        }
    }

    /// The number in `kind`'s bounds, as `TryFrom` asks: `Err(true)` when
    /// it is below them, `Err(false)` above.
    pub(super) fn fits(self, kind: Int) -> Result<(), bool> {
        let (negative, magnitude) = match self {
            Self::Signed(value) => (value < 0, value.unsigned_abs()),
            Self::Unsigned(value) => (false, value),
        };
        let (low, high): (u128, u128) = match kind {
            Int::U128 => (0, u128::MAX),
            Int::I128 => (1 << 127, i128::MAX as u128),
            kind => {
                let (low, high) = kind.full_range();
                (low.unsigned_abs(), high as u128)
            }
        };
        match (negative, magnitude) {
            (true, magnitude) if magnitude > low => Err(true),
            (false, magnitude) if magnitude > high => Err(false),
            _ => Ok(()),
        }
    }

    /// The number as the runtime keeps a value of `kind`, wrapped to it:
    /// an `i128` or a `u128` an `i64` when it fits one, else boxed.
    pub(super) fn store(self, kind: Int) -> Dynamic {
        match kind {
            Int::I128 => wide_i128(self.signed()),
            Int::U128 => wide_u128(self.unsigned()),
            kind => {
                let bits = kind.bits();
                let pattern = self.unsigned() & (u128::MAX >> (128 - bits));
                Dynamic::from_int(kind.store(kind.sign(pattern as i128)))
            }
        }
    }
}

impl std::fmt::Display for Wide {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signed(value) => value.fmt(formatter),
            Self::Unsigned(value) => value.fmt(formatter),
        }
    }
}

/// The number an integer literal's text is (decimal, or `0x`, `0o`, `0b`
/// digits, with an optional `-`); `None` for a float literal's or one past
/// the `i128` and `u128` bounds.
pub(super) fn integer_text(text: &str) -> Option<Wide> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    let magnitude = match digits.get(..2) {
        Some("0x") => u128::from_str_radix(&digits[2..], 16).ok()?,
        Some("0o") => u128::from_str_radix(&digits[2..], 8).ok()?,
        Some("0b") => u128::from_str_radix(&digits[2..], 2).ok()?,
        _ if digits.bytes().all(|byte| byte.is_ascii_digit()) => digits.parse().ok()?,
        _ => return None,
    };
    if !negative {
        return Some(i128::try_from(magnitude).map_or(Wide::Unsigned(magnitude), Wide::Signed));
    }
    (magnitude <= 1 << 127).then(|| Wide::Signed(0i128.wrapping_sub_unsigned(magnitude)))
}

/// A runtime integer as the number it is, or `None` for another value.
pub(super) fn wide_value(value: &Dynamic) -> Option<Wide> {
    if let Ok(value) = value.as_int() {
        return Some(Wide::Signed(value.into()));
    }
    if let Some(value) = value.read_lock::<i128>() {
        return Some(Wide::Signed(*value));
    }
    value
        .read_lock::<u128>()
        .map(|value| Wide::Unsigned(*value))
}

/// A runtime value of `kind` as the number it is: a `u64`'s or a `usize`'s
/// bits read unsigned.
pub(super) fn kind_value(value: &Dynamic, kind: Int) -> Wide {
    match (kind, value.as_int()) {
        (Int::U64 | Int::Usize, Ok(bits)) => Wide::Unsigned(u128::from(bits as u64)),
        _ => wide_value(value).unwrap_or(Wide::Signed(0)),
    }
}

/// An `i128` as the runtime keeps it: an `i64` while it fits one.
pub(super) fn wide_i128(value: i128) -> Dynamic {
    INT::try_from(value).map_or_else(|_| Dynamic::from(value), Dynamic::from_int)
}

/// A `u128` as the runtime keeps it: an `i64` while it fits one.
pub(super) fn wide_u128(value: u128) -> Dynamic {
    INT::try_from(value).map_or_else(|_| Dynamic::from(value), Dynamic::from_int)
}

/// The order of two runtime integers where one is a boxed `i128` or
/// `u128`; `None` when either is not an integer.
pub(super) fn wide_order(lhs: &Dynamic, rhs: &Dynamic) -> Option<std::cmp::Ordering> {
    Some(wide_value(lhs)?.key().cmp(&wide_value(rhs)?.key()))
}

/// `low..high` or `low..=high` where a bound is a boxed `i128` or `u128`:
/// Rust's range of the kind, whose items the runtime's iterator boxes.
pub(crate) fn wide_range(args: &mut FnCallArgs, inclusive: bool) -> RawrResult {
    let (Some(low), Some(high)) = (wide_value(args[0]), wide_value(args[1])) else {
        return Err(invariant("a 128-bit range bound is not an integer"));
    };
    Ok(match (low, high, inclusive) {
        (Wide::Unsigned(_), ..) | (_, Wide::Unsigned(_), _) if inclusive => {
            Dynamic::from(low.unsigned()..=high.unsigned())
        }
        (Wide::Unsigned(_), ..) | (_, Wide::Unsigned(_), _) => {
            Dynamic::from(low.unsigned()..high.unsigned())
        }
        _ if inclusive => Dynamic::from(low.signed()..=high.signed()),
        _ => Dynamic::from(low.signed()..high.signed()),
    })
}

/// The runtime's operator `symbol` (`INT_METHODS`) where a value is a boxed
/// `i128` or `u128` and no static type says which: a generic body's `+` or
/// `<`, a literal pattern. Computed in `u128` when either is one.
pub(crate) fn wide_operator(symbol: &str, args: &mut FnCallArgs) -> RawrResult {
    let (Some(lhs), Some(rhs)) = (wide_value(args[0]), wide_value(args[1])) else {
        return Err(invariant(
            "a 128-bit operator on a value that is not an integer",
        ));
    };
    match (lhs, rhs) {
        (Wide::Unsigned(_), _) | (_, Wide::Unsigned(_)) => {
            wide_u128_method(symbol, lhs.unsigned(), rhs.unsigned(), 0)
        }
        _ => wide_i128_method(symbol, lhs.signed(), rhs.signed(), 0),
    }
}

/// `INT_METHODS`' entry of `args[1]` on an `i128` or a `u128` (`kind`), or
/// `try_from` between such a kind and another, as debug Rust computes it.
pub(super) fn wide_method(kind: Int, operand_kind: Int, args: &mut FnCallArgs) -> RawrResult {
    let code = usize::try_from(args[1].as_int().unwrap_or_default()).unwrap_or(0);
    let (name, operand_type, _) = INT_METHODS.get(code).copied().unwrap_or(("", 'n', 't'));
    if name == "try_from" {
        let source = kind_value(args[0], operand_kind);
        // `TryFromIntError`'s `Debug`, its runtime value.
        return Ok(match source.fits(kind) {
            Ok(()) => crate::result::ok(source.store(kind)),
            Err(below) => crate::result::err(
                ImmutableString::from(if below {
                    "TryFromIntError(NegOverflow)"
                } else {
                    "TryFromIntError(PosOverflow)"
                })
                .into(),
            ),
        });
    }
    let value = kind_value(args[0], kind);
    let operand = match operand_type {
        's' => kind_value(args[2], kind),
        'u' => kind_value(args[2], Int::U32),
        _ if matches!(name, "<<" | ">>" | "**") => kind_value(args[2], operand_kind),
        _ => kind_value(args[2], kind),
    };
    // A shift amount or an exponent; one past `i128::MAX` overflows anyway.
    let amount = match operand {
        Wide::Signed(amount) => amount,
        Wide::Unsigned(amount) => i128::try_from(amount).unwrap_or(i128::MAX),
    };
    if kind == Int::U128 {
        wide_u128_method(name, value.unsigned(), operand.unsigned(), amount)
    } else {
        wide_i128_method(name, value.signed(), operand.signed(), amount)
    }
}

/// `wide_method` of one of the 128-bit kinds: `$t`, kept by `$store`.
macro_rules! wide_methods {
    ($function:ident, $t:ty, $store:ident) => {
        fn $function(name: &str, a: $t, b: $t, amount: i128) -> RawrResult {
            let panic = |message: &str| -> RawrResult {
                Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
            };
            let zero: $t = 0;
            let exponent = u32::try_from(amount).unwrap_or(u32::MAX);
            let some = |value: Option<$t>| match value {
                Some(value) => crate::option::some($store(value)),
                None => crate::option::none(),
            };
            let pair = |(value, overflowed): ($t, bool)| {
                Dynamic::from_array(vec![$store(value), overflowed.into()])
            };
            let exact = |value: Option<$t>, verb: &str| match value {
                Some(value) => Ok($store(value)),
                None => panic(&format!("attempt to {verb} with overflow")),
            };
            const DIVIDE: &str = "attempt to divide by zero";
            const REMAINDER: &str = "attempt to calculate the remainder with a divisor of zero";
            let divisor = matches!(
                name,
                "/" | "wrapping_div" | "saturating_div" | "overflowing_div" | "div_euclid"
            );
            let remainder = matches!(
                name,
                "%" | "wrapping_rem" | "overflowing_rem" | "rem_euclid"
            );
            if b == zero && (divisor || remainder) {
                return panic(if divisor { DIVIDE } else { REMAINDER });
            }
            Ok(match name {
                "+" => return exact(a.checked_add(b), "add"),
                "-" => return exact(a.checked_sub(b), "subtract"),
                "*" => return exact(a.checked_mul(b), "multiply"),
                "/" => return exact(a.checked_div(b), "divide"),
                "%" => return exact(a.checked_rem(b), "calculate the remainder"),
                "**" => return exact(a.checked_pow(exponent), "multiply"),
                "neg" => return exact(a.checked_neg(), "negate"),
                "abs" if a < zero => return exact(a.checked_neg(), "negate"),
                "abs" => $store(a),
                "checked_add" => some(a.checked_add(b)),
                "checked_sub" => some(a.checked_sub(b)),
                "checked_mul" => some(a.checked_mul(b)),
                "checked_div" => some(a.checked_div(b)),
                "checked_rem" => some(a.checked_rem(b)),
                "checked_neg" => some(a.checked_neg()),
                "checked_pow" => some(a.checked_pow(exponent)),
                "wrapping_add" => $store(a.wrapping_add(b)),
                "wrapping_sub" => $store(a.wrapping_sub(b)),
                "wrapping_mul" => $store(a.wrapping_mul(b)),
                "wrapping_div" => $store(a.wrapping_div(b)),
                "wrapping_rem" => $store(a.wrapping_rem(b)),
                "wrapping_neg" => $store(a.wrapping_neg()),
                "wrapping_pow" => $store(a.wrapping_pow(exponent)),
                "saturating_add" => $store(a.saturating_add(b)),
                "saturating_sub" => $store(a.saturating_sub(b)),
                "saturating_mul" => $store(a.saturating_mul(b)),
                "saturating_div" => $store(a.saturating_div(b)),
                "saturating_pow" => $store(a.saturating_pow(exponent)),
                "overflowing_add" => pair(a.overflowing_add(b)),
                "overflowing_sub" => pair(a.overflowing_sub(b)),
                "overflowing_mul" => pair(a.overflowing_mul(b)),
                "overflowing_div" => pair(a.overflowing_div(b)),
                "overflowing_rem" => pair(a.overflowing_rem(b)),
                "overflowing_neg" => pair(a.overflowing_neg()),
                "overflowing_pow" => pair(a.overflowing_pow(exponent)),
                "count_ones" => INT::from(a.count_ones()).into(),
                "count_zeros" => INT::from(a.count_zeros()).into(),
                "leading_zeros" => INT::from(a.leading_zeros()).into(),
                "trailing_zeros" => INT::from(a.trailing_zeros()).into(),
                "rotate_left" => $store(a.rotate_left(exponent)),
                "rotate_right" => $store(a.rotate_right(exponent)),
                "rem_euclid" => return exact(a.checked_rem_euclid(b), "calculate the remainder"),
                "div_euclid" => return exact(a.checked_div_euclid(b), "divide"),
                "abs_diff" => wide_u128(a.abs_diff(b)),
                "unsigned_abs" if a < zero => wide_u128((a as u128).wrapping_neg()),
                "unsigned_abs" => wide_u128(a as u128),
                "is_power_of_two" => (a > zero && a & (a - 1) == zero).into(),
                "is_negative" => (a < zero).into(),
                "is_positive" => (a > zero).into(),
                "signum" => INT::from(a > zero).wrapping_sub(INT::from(a < zero)).into(),
                "ilog2" | "ilog10" if a <= zero => {
                    return panic("argument of integer logarithm must be positive")
                }
                "ilog2" => INT::from(a.ilog2()).into(),
                "ilog10" => INT::from(a.ilog10()).into(),
                "<<" | ">>" if !(0..128).contains(&amount) => {
                    return panic(if name == "<<" {
                        "attempt to shift left with overflow"
                    } else {
                        "attempt to shift right with overflow"
                    })
                }
                "<<" => $store(a << exponent),
                ">>" => $store(a >> exponent),
                "!" => $store(!a),
                "&" => $store(a & b),
                "|" => $store(a | b),
                "^" => $store(a ^ b),
                "<" => (a < b).into(),
                ">" => (a > b).into(),
                "<=" => (a <= b).into(),
                ">=" => (a >= b).into(),
                "==" => (a == b).into(),
                "!=" => (a != b).into(),
                _ => return Err(invariant("an unknown integer method at runtime")),
            })
        }
    };
}

wide_methods!(wide_i128_method, i128, wide_i128);
wide_methods!(wide_u128_method, u128, wide_u128);

/// An integer literal past `i64` whose kind inference settles later: its
/// placeholder, written over with its text as an `i128`, a `u128` or a
/// `u64`'s bits (`Parser::wide_constant`).
pub(super) struct PendingWide {
    pub(super) ty: Ty,
    pub(super) value: Wide,
    pub(super) at: Span,
    pub(super) placeholder: String,
    pub(super) texts: [String; 3],
}

/// A float literal or arithmetic result whose kind inference settles
/// later (`Lowering::pending_float_kinds`).
pub(super) struct PendingFloat {
    pub(super) placeholder: String,
    pub(super) ty: Ty,
    pub(super) kind: PendingFloatKind,
}

pub(super) enum PendingFloatKind {
    /// The literal's value.
    Literal(f64),
    /// The name of the `RoundF32` helper the placeholder calls.
    Round(String),
}

impl<'a> Parser<'a> {
    /// `lhs symbol rhs` of the `i128` or `u128` type `ty` through
    /// `INT_METHODS`, of the type `output`.
    pub(super) fn wide_binary(
        &mut self,
        symbol: &str,
        lhs: ParsedExpr,
        rhs: ParsedExpr,
        ty: Ty,
        output: Ty,
    ) -> ParsedExpr {
        let expression = span(lhs.span.start(), rhs.span.end());
        let source = self.int_op(
            symbol,
            lhs.rendered_source(),
            rhs.rendered_source(),
            &ty,
            &ty,
        );
        self.operator_expr(source, output, expression)
    }

    /// An integer literal past `i64` (the tokenizer's float of its text
    /// `text`): of its suffix's kind or the one inference settles on, which
    /// must hold it. `None` for a float literal.
    pub(super) fn wide_literal(
        &mut self,
        text: &str,
        at: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some(value) = integer_text(text) else {
            return Ok(None);
        };
        let ty = match self.peek() {
            Token::Identifier(name) if Float::named(name.as_str()).is_some() => return Ok(None),
            Token::Identifier(name) if let Some(kind) = Int::named(name.as_str()) => {
                self.next();
                Ty::Int(kind)
            }
            _ => {
                let id = self.inference.fresh_id();
                self.literal_kinds.push((id, true));
                Ty::Int(Int::Var(id))
            }
        };
        self.wide_constant(value, ty, at).map(Some)
    }

    /// The integer `value` of the integer type `ty` as the runtime keeps
    /// it (`PreludeOp::WideLiteral`); a kind inference has not settled is
    /// checked once it has.
    pub(super) fn wide_constant(
        &mut self,
        value: Wide,
        ty: Ty,
        at: Span,
    ) -> Result<ParsedExpr, Error> {
        let digits = format!("{:?}", value.to_string());
        let mut boxed = |kind: Int| {
            let code = kind_code(Ty::Int(kind)).to_string();
            let call = self.prelude_op(
                PreludeOp::WideLiteral,
                [digits.as_str().into(), code.into()],
            );
            call.as_str().to_string()
        };
        // A `u64`'s or a `usize`'s bits are an `i64` literal.
        let bits = format!("({})", value.unsigned() as u64 as INT);
        let source = match ty {
            Ty::Int(Int::Var(_)) => {
                let texts = [boxed(Int::I128), boxed(Int::U128), bits];
                let width = texts.iter().map(String::len).max().unwrap_or_default();
                let placeholder = format!(
                    "{:<width$}",
                    format!(
                        "__rawr_wide_literal_{:08}",
                        self.pending_wide_literals.len()
                    )
                );
                self.pending_wide_literals.push(PendingWide {
                    ty: ty.clone(),
                    value,
                    at,
                    placeholder: placeholder.clone(),
                    texts,
                });
                placeholder
            }
            Ty::Int(kind) if value.fits(kind).is_err() => {
                return Err(self.overflowing(kind, value, at));
            }
            Ty::Int(Int::U64 | Int::Usize) => bits,
            Ty::Int(kind) => boxed(kind),
            _ => {
                return Err(self.unsupported("an integer literal of a type that is not an integer"))
            }
        };
        Ok(self.operator_expr(origin::Fragment::plain(source), ty, at))
    }
}
