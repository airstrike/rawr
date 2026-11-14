//! rustc's deny-by-default `arithmetic_overflow` and `unconditional_panic`
//! lints: an operation on integers the body knows when it is compiled
//! that would overflow, divide by zero or index an array past its end is
//! refused, as rustc's const propagation refuses it. The kinds of the
//! literals involved are only settled once the body is read, so each
//! known operation is noted as it is read and checked then.

use super::expr::ParsedExpr;
use super::sequence;
use super::ty::{Int, Ty};
use super::Parser;
use crate::resolve::{Error, Lint};
use crate::tokenizer::Token;
use crate::{Span, INT};

/// An integer expression whose value the body knows when compiled.
#[derive(Clone, Debug)]
pub(super) enum Known {
    /// A literal, a `-` before it included, of the type `ty`.
    Literal { value: i128, ty: Ty },
    /// `-operand`.
    Negation {
        operand: Box<Known>,
        ty: Ty,
        at: Span,
    },
    /// `lhs operator rhs`.
    Binary {
        operator: Operator,
        lhs: Box<Known>,
        rhs: Box<Known>,
        at: Span,
    },
}

/// A binary operator on integers.
#[derive(Clone, Copy, Debug)]
pub(super) enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    ShiftLeft,
    ShiftRight,
    And,
    Or,
    Xor,
}

/// An index known when compiled into an array of a known length.
#[derive(Clone, Debug)]
pub(super) struct KnownIndex {
    length: usize,
    index: Known,
    at: Span,
}

impl Operator {
    fn of(token: &Token) -> Option<Self> {
        Some(match token {
            Token::Plus => Self::Add,
            Token::Minus => Self::Subtract,
            Token::Multiply => Self::Multiply,
            Token::Divide => Self::Divide,
            Token::Modulo => Self::Remainder,
            Token::LeftShift => Self::ShiftLeft,
            Token::RightShift => Self::ShiftRight,
            Token::Ampersand => Self::And,
            Token::Pipe => Self::Or,
            Token::XOr => Self::Xor,
            _ => return None,
        })
    }

    const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
            Self::And => "&",
            Self::Or => "|",
            Self::Xor => "^",
        }
    }
}

impl Known {
    /// The type the value has.
    fn ty(&self) -> &Ty {
        match self {
            Self::Literal { ty, .. } | Self::Negation { ty, .. } => ty,
            Self::Binary { lhs, .. } => lhs.ty(),
        }
    }

    /// The value and its kind, or the lint's error at the first operation
    /// in it that would panic. `None` for a value of a kind the lints
    /// don't follow here: not an integer, or a 128-bit one. `kind` gives a
    /// type's settled kind.
    fn value(&self, kind: &dyn Fn(&Ty) -> Option<Int>) -> Result<Option<(i128, Int)>, Error> {
        let Some(own) = kind(self.ty()) else {
            return Ok(None);
        };
        match self {
            Self::Literal { value, .. } => Ok(Some((*value, own))),
            Self::Negation { operand, at, .. } => {
                let Some((value, _)) = operand.value(kind)? else {
                    return Ok(None);
                };
                if value == own.full_range().0 && own.signed() {
                    return Err(panic(
                        Lint::ArithmeticOverflow,
                        format!(
                            "attempt to negate `{}`, which would overflow",
                            shown(value, own)
                        ),
                        *at,
                    ));
                }
                Ok(Some((-value, own)))
            }
            Self::Binary {
                operator,
                lhs,
                rhs,
                at,
            } => {
                let (Some((left, _)), Some((right, amount))) = (lhs.value(kind)?, rhs.value(kind)?)
                else {
                    return Ok(None);
                };
                operation(*operator, (left, own), (right, amount), *at).map(Some)
            }
        }
    }
}

/// `left operator right`, `left` of the kind `own` and `right` of the kind
/// `amount` (a shift's own), or the lint's error at `at` when it panics.
fn operation(
    operator: Operator,
    (left, own): (i128, Int),
    (right, amount): (i128, Int),
    at: Span,
) -> Result<(i128, Int), Error> {
    let (low, high) = own.full_range();
    let overflow = || {
        panic(
            Lint::ArithmeticOverflow,
            format!(
                "attempt to compute `{} {} {}`, which would overflow",
                shown(left, own),
                operator.symbol(),
                shown(right, own)
            ),
            at,
        )
    };
    let fits = |value: Option<i128>| value.filter(|value| (low..=high).contains(value));
    let value = match operator {
        Operator::Add => fits(left.checked_add(right)).ok_or_else(overflow)?,
        Operator::Subtract => fits(left.checked_sub(right)).ok_or_else(overflow)?,
        Operator::Multiply => fits(left.checked_mul(right)).ok_or_else(overflow)?,
        Operator::Divide | Operator::Remainder if right == 0 => {
            let label = match operator {
                Operator::Divide => format!("attempt to divide `{}` by zero", shown(left, own)),
                _ => format!(
                    "attempt to calculate the remainder of `{}` with a divisor of zero",
                    shown(left, own)
                ),
            };
            return Err(panic(Lint::UnconditionalPanic, label, at));
        }
        Operator::Divide | Operator::Remainder if own.signed() && left == low && right == -1 => {
            let (left, right) = (shown(left, own), shown(right, own));
            let label = match operator {
                Operator::Divide => {
                    format!("attempt to compute `{left} / {right}`, which would overflow")
                }
                _ => format!(
                    "attempt to compute the remainder of `{left} % {right}`, which would overflow"
                ),
            };
            return Err(panic(Lint::UnconditionalPanic, label, at));
        }
        Operator::Divide => left / right,
        Operator::Remainder => left % right,
        Operator::ShiftLeft | Operator::ShiftRight => {
            let bits = own.bits();
            let Some(by) = u32::try_from(right).ok().filter(|by| *by < bits) else {
                let direction = match operator {
                    Operator::ShiftLeft => "left",
                    _ => "right",
                };
                return Err(panic(
                    Lint::ArithmeticOverflow,
                    format!(
                        "attempt to shift {direction} by `{}`, which would overflow",
                        shown(right, amount)
                    ),
                    at,
                ));
            };
            match operator {
                Operator::ShiftLeft => wrapped(left << by, own),
                _ => left >> by,
            }
        }
        Operator::And => left & right,
        Operator::Or => left | right,
        Operator::Xor => wrapped(left ^ right, own),
    };
    Ok((value, own))
}

/// `value` cut to `kind`'s width, as a shift that pushes bits out leaves it.
fn wrapped(value: i128, kind: Int) -> i128 {
    let shift = 128 - kind.bits();
    if kind.signed() {
        (value << shift) >> shift
    } else {
        ((value as u128) << shift >> shift) as i128
    }
}

/// `value` of `kind` as rustc's lint shows it: `u8::MAX`, `i32::MIN`,
/// `1_i32`.
fn shown(value: i128, kind: Int) -> String {
    let (low, high) = kind.full_range();
    let name = kind.name();
    if value == high {
        format!("{name}::MAX")
    } else if value == low && kind.signed() {
        format!("{name}::MIN")
    } else {
        format!("{value}_{name}")
    }
}

fn panic(lint: Lint, label: String, at: Span) -> Error {
    Error::KnownPanic {
        lint,
        label: label.into(),
        at,
    }
}

impl Parser<'_> {
    /// The integer literal `literal`, of the number `value`: known.
    pub(super) fn note_known_literal(&mut self, literal: &ParsedExpr, value: INT) {
        self.known_values.insert(
            literal.span,
            Known::Literal {
                value: i128::from(value),
                ty: literal.ty.clone(),
            },
        );
    }

    /// `negation`, `-operand`: known when its operand is. `-` before a
    /// literal is a literal of its own, as rustc reads `-128i8`.
    pub(super) fn note_known_negation(&mut self, operand: Span, negation: &ParsedExpr) {
        let Some(known) = self.known_values.get(&operand).cloned() else {
            return;
        };
        let known = match known {
            Known::Literal { value, .. } => Known::Literal {
                value: -value,
                ty: negation.ty.clone(),
            },
            operand => Known::Negation {
                operand: Box::new(operand),
                ty: negation.ty.clone(),
                at: self.expression_span(negation.span),
            },
        };
        self.known_values.insert(negation.span, known);
    }

    /// `value`, `lhs operator rhs` of the operands spanning `lhs` and
    /// `rhs`: known when both are.
    pub(super) fn note_known_binary(
        &mut self,
        operator: &Token,
        (lhs, rhs): (Span, Span),
        value: &ParsedExpr,
    ) {
        let (Some(operator), Some(lhs), Some(rhs)) = (
            Operator::of(operator),
            self.known_values.get(&lhs),
            self.known_values.get(&rhs),
        ) else {
            return;
        };
        let known = Known::Binary {
            operator,
            lhs: Box::new(lhs.clone()),
            rhs: Box::new(rhs.clone()),
            at: self.expression_span(value.span),
        };
        self.known_values.insert(value.span, known);
    }

    /// `(value)`, spanning `whole`, of the value spanning `inner`.
    pub(super) fn note_known_parenthesized(&mut self, inner: Span, whole: Span) {
        if let Some(known) = self.known_values.get(&inner).cloned() {
            self.known_values.insert(whole, known);
        }
    }

    /// `array[index]`, spanning `at`, where `array` has the type `array`:
    /// an index known into an array of a known length.
    pub(super) fn note_known_index(&mut self, array: &Ty, index: Span, at: Span) {
        let Ty::Array {
            kind: sequence::Kind::Array(length),
            ..
        } = self.inference.resolve(array)
        else {
            return;
        };
        if let Some(index) = self.known_values.get(&index).cloned() {
            self.known_indices.push(KnownIndex {
                length,
                index,
                at: self.expression_span(at),
            });
        }
    }

    /// The body's first known operation that would panic, now that its
    /// literals' kinds are settled: rustc's lint error.
    pub(super) fn check_known_panics(&mut self) -> Result<(), Error> {
        let values = std::mem::take(&mut self.known_values);
        let indices = std::mem::take(&mut self.known_indices);
        let kind = |ty: &Ty| match self.inference.resolve(ty) {
            Ty::Int(Int::Var(_)) => Some(Int::I32),
            Ty::Int(kind) if !kind.wide() => Some(kind),
            _ => None,
        };
        let mut first: Option<Error> = None;
        let mut keep = |error: Error| {
            let earlier = first.as_ref().is_none_or(|known| {
                let (at, known) = (error.primary_span(), known.primary_span());
                (at.start(), at.end()) < (known.start(), known.end())
            });
            if earlier {
                first = Some(error);
            }
        };
        for known in values.values() {
            if let Err(error) = known.value(&kind) {
                keep(error);
            }
        }
        for KnownIndex { length, index, at } in indices {
            match index.value(&kind) {
                Err(error) => keep(error),
                Ok(Some((index, _))) if index >= length as i128 => keep(panic(
                    Lint::UnconditionalPanic,
                    format!("index out of bounds: the length is {length} but the index is {index}"),
                    at,
                )),
                Ok(_) => {}
            }
        }
        first.map_or(Ok(()), Err)
    }
}
