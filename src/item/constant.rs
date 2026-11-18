//! Constant expressions rustc evaluates while it compiles: a const generic
//! argument (`Buf<4>`, `Buf::<{ 0 + 1 + 1 }>`) and the `const` items it
//! names.

use crate::tokenizer::Token;
use crate::ImmutableString;

/// A constant's value, as a const generic parameter takes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Serialize))]
pub(crate) enum Value {
    Integer(i128),
    Bool(bool),
    Char(char),
}

impl Value {
    /// The value as a type writes it: `4`, `true`, `'a'`.
    pub(crate) fn text(self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Char(value) => format!("{value:?}"),
        }
    }

    /// The value [`Value::text`] or [`encode`] wrote.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "true" => return Some(Self::Bool(true)),
            "false" => return Some(Self::Bool(false)),
            _ => {}
        }
        if let Some(code) = text
            .strip_prefix("'\\u{")
            .and_then(|rest| rest.strip_suffix("}'"))
        {
            return u32::from_str_radix(code, 16)
                .ok()
                .and_then(char::from_u32)
                .map(Self::Char);
        }
        if let Some(inner) = text
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
        {
            let mut chars = inner.chars();
            return match (chars.next(), chars.next()) {
                (Some(value), None) => Some(Self::Char(value)),
                _ => None,
            };
        }
        text.parse().ok().map(Self::Integer)
    }

    /// The value as one word with no space in it, as [`encode`] writes it.
    fn word(self) -> String {
        match self {
            Self::Char(value) => format!("'\\u{{{:x}}}'", u32::from(value)),
            Self::Integer(_) | Self::Bool(_) => self.text(),
        }
    }
}

/// An operator of a constant expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Serialize))]
pub(crate) enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    And,
    Or,
    Xor,
    ShiftLeft,
    ShiftRight,
    Not,
}

impl Operator {
    const SYMBOLS: [(Self, &'static str); 11] = [
        (Self::Add, "+"),
        (Self::Subtract, "-"),
        (Self::Multiply, "*"),
        (Self::Divide, "/"),
        (Self::Remainder, "%"),
        (Self::And, "&"),
        (Self::Or, "|"),
        (Self::Xor, "^"),
        (Self::ShiftLeft, "<<"),
        (Self::ShiftRight, ">>"),
        (Self::Not, "!"),
    ];

    /// The operator's place among all of them, as a parse cell keeps it.
    pub(crate) fn index(self) -> u8 {
        let index = Self::SYMBOLS
            .iter()
            .position(|(operator, _)| *operator == self)
            .expect("every operator has a symbol");
        u8::try_from(index).expect("fewer than 256 operators")
    }

    /// The operator at `index` among all of them.
    pub(crate) fn from_index(index: u8) -> Option<Self> {
        Self::SYMBOLS
            .get(usize::from(index))
            .map(|(operator, _)| *operator)
    }

    fn symbol(self) -> &'static str {
        Self::SYMBOLS
            .iter()
            .find(|(operator, _)| *operator == self)
            .map(|(_, symbol)| *symbol)
            .expect("every operator has a symbol")
    }

    /// How tightly the operator binds between two operands, in Rust's
    /// precedence; `None` for `!`, which only prefixes.
    fn binding(self) -> Option<u8> {
        match self {
            Self::Multiply | Self::Divide | Self::Remainder => Some(5),
            Self::Add | Self::Subtract => Some(4),
            Self::ShiftLeft | Self::ShiftRight => Some(3),
            Self::And => Some(2),
            Self::Xor => Some(1),
            Self::Or => Some(0),
            Self::Not => None,
        }
    }

    /// `left op right`; `None` when Rust's evaluation would fail (an
    /// overflow, a division by zero) or the operands don't take it.
    fn apply(self, left: Value, right: Value) -> Option<Value> {
        use Value::{Bool, Integer};
        Some(match (self, left, right) {
            (Self::Add, Integer(a), Integer(b)) => Integer(a.checked_add(b)?),
            (Self::Subtract, Integer(a), Integer(b)) => Integer(a.checked_sub(b)?),
            (Self::Multiply, Integer(a), Integer(b)) => Integer(a.checked_mul(b)?),
            (Self::Divide, Integer(a), Integer(b)) => Integer(a.checked_div(b)?),
            (Self::Remainder, Integer(a), Integer(b)) => Integer(a.checked_rem(b)?),
            (Self::And, Integer(a), Integer(b)) => Integer(a & b),
            (Self::Or, Integer(a), Integer(b)) => Integer(a | b),
            (Self::Xor, Integer(a), Integer(b)) => Integer(a ^ b),
            (Self::And, Bool(a), Bool(b)) => Bool(a & b),
            (Self::Or, Bool(a), Bool(b)) => Bool(a | b),
            (Self::Xor, Bool(a), Bool(b)) => Bool(a ^ b),
            (Self::ShiftLeft, Integer(a), Integer(b)) => {
                Integer(a.checked_shl(u32::try_from(b).ok()?)?)
            }
            (Self::ShiftRight, Integer(a), Integer(b)) => {
                Integer(a.checked_shr(u32::try_from(b).ok()?)?)
            }
            _ => return None,
        })
    }
}

/// One token of a constant expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Piece {
    Value(Value),
    /// A path naming a constant: `SIZE`, `Self::N`.
    Name(ImmutableString),
    Operator(Operator),
    Open,
    Close,
}

/// The pieces of the constant expression `tokens` spell, a path's names
/// joined by `::`. `None` at a token no constant expression rawr evaluates
/// has.
pub(crate) fn pieces<'t>(tokens: impl IntoIterator<Item = &'t Token>) -> Option<Vec<Piece>> {
    let mut pieces = Vec::new();
    let mut path = false;
    for token in tokens {
        let piece = match token {
            Token::IntegerConstant(value) => Piece::Value(Value::Integer(i128::from(*value))),
            Token::CharConstant(value) => Piece::Value(Value::Char(*value)),
            Token::True => Piece::Value(Value::Bool(true)),
            Token::False => Piece::Value(Value::Bool(false)),
            Token::Identifier(name) | Token::Reserved(name) => {
                if path && let Some(Piece::Name(head)) = pieces.last_mut() {
                    *head = format!("{head}::{name}").into();
                    path = false;
                    continue;
                }
                Piece::Name(name.as_str().into())
            }
            Token::DoubleColon if matches!(pieces.last(), Some(Piece::Name(_))) => {
                path = true;
                continue;
            }
            Token::LeftParen => Piece::Open,
            Token::RightParen => Piece::Close,
            Token::Plus | Token::UnaryPlus => Piece::Operator(Operator::Add),
            Token::Minus | Token::UnaryMinus => Piece::Operator(Operator::Subtract),
            Token::Multiply => Piece::Operator(Operator::Multiply),
            Token::Divide => Piece::Operator(Operator::Divide),
            Token::Modulo => Piece::Operator(Operator::Remainder),
            Token::Ampersand => Piece::Operator(Operator::And),
            Token::Pipe => Piece::Operator(Operator::Or),
            Token::XOr => Piece::Operator(Operator::Xor),
            Token::LeftShift => Piece::Operator(Operator::ShiftLeft),
            Token::RightShift => Piece::Operator(Operator::ShiftRight),
            Token::Bang => Piece::Operator(Operator::Not),
            _ => return None,
        };
        pieces.push(piece);
    }
    (!path && !pieces.is_empty()).then_some(pieces)
}

/// `pieces` as one line of words, which [`decode`] reads back: how an item
/// header keeps a const argument whose names are known only once every
/// item is parsed.
pub(crate) fn encode(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Value(value) => value.word(),
            Piece::Name(name) => name.to_string(),
            Piece::Operator(operator) => operator.symbol().to_owned(),
            Piece::Open => "(".to_owned(),
            Piece::Close => ")".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pieces [`encode`] wrote.
pub(crate) fn decode(text: &str) -> Option<Vec<Piece>> {
    text.split(' ')
        .map(|word| {
            Some(match word {
                "(" => Piece::Open,
                ")" => Piece::Close,
                _ => {
                    if let Some((operator, _)) =
                        Operator::SYMBOLS.iter().find(|(_, symbol)| *symbol == word)
                    {
                        Piece::Operator(*operator)
                    } else if let Some(value) = Value::parse(word) {
                        Piece::Value(value)
                    } else if word.starts_with(|c: char| c.is_alphabetic() || c == '_') {
                        Piece::Name(word.into())
                    } else {
                        return None;
                    }
                }
            })
        })
        .collect()
}

/// The value of `pieces`, each name's value found by `lookup`. `None` when
/// they are no constant expression rawr evaluates, a name has no value, or
/// Rust's evaluation would fail.
pub(crate) fn evaluate(
    pieces: &[Piece],
    lookup: &mut dyn FnMut(&str) -> Option<Value>,
) -> Option<Value> {
    let mut rest = pieces;
    let value = expression(&mut rest, 0, lookup)?;
    rest.is_empty().then_some(value)
}

/// The operations from the front of `rest` binding at least as tightly as
/// `floor`.
fn expression(
    rest: &mut &[Piece],
    floor: u8,
    lookup: &mut dyn FnMut(&str) -> Option<Value>,
) -> Option<Value> {
    let mut left = operand(rest, lookup)?;
    while let [Piece::Operator(operator), after @ ..] = *rest
        && let Some(binding) = operator.binding()
        && binding >= floor
    {
        *rest = after;
        let right = expression(rest, binding + 1, lookup)?;
        left = operator.apply(left, right)?;
    }
    Some(left)
}

/// One operand with its prefix operators.
fn operand(rest: &mut &[Piece], lookup: &mut dyn FnMut(&str) -> Option<Value>) -> Option<Value> {
    let (first, after) = rest.split_first()?;
    *rest = after;
    match first {
        Piece::Value(value) => Some(*value),
        Piece::Name(name) => lookup(name),
        Piece::Open => {
            let value = expression(rest, 0, lookup)?;
            let (Piece::Close, after) = rest.split_first()? else {
                return None;
            };
            *rest = after;
            Some(value)
        }
        Piece::Operator(Operator::Subtract) => match operand(rest, lookup)? {
            Value::Integer(value) => Some(Value::Integer(value.checked_neg()?)),
            _ => None,
        },
        Piece::Operator(Operator::Not) => match operand(rest, lookup)? {
            Value::Integer(value) => Some(Value::Integer(!value)),
            Value::Bool(value) => Some(Value::Bool(!value)),
            Value::Char(_) => None,
        },
        Piece::Operator(_) | Piece::Close => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_expression_evaluates_in_rusts_precedence() {
        let tokens = [
            Token::IntegerConstant(1),
            Token::Plus,
            Token::Identifier(Box::new("SIZE".into())),
            Token::Multiply,
            Token::LeftParen,
            Token::IntegerConstant(3),
            Token::Minus,
            Token::IntegerConstant(1),
            Token::RightParen,
        ];
        let pieces = pieces(&tokens).expect("a constant expression");
        let round = decode(&encode(&pieces)).expect("encoded pieces decode");
        let mut lookup = |name: &str| (name == "SIZE").then_some(Value::Integer(4));
        assert_eq!(evaluate(&round, &mut lookup), Some(Value::Integer(9)));
    }
}
