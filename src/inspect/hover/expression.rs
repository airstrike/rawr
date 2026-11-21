//! The expressions a hover may read, found in rawr's tokens of the code it
//! is written in: the smallest one around a character of a line, or one
//! code given whole.
//!
//! The parse is tolerant, since a line is rarely a whole expression: it
//! reads each expression it can along the line and steps over the tokens
//! between them (`let`, `=`, `{`, `;`). Calls, macros, `?` and `.await`
//! are kept as nodes too, with their operands and arguments inside, so a
//! hover on a call says it is one, and a hover on an argument reads that
//! argument.

use std::ops::Range;

use crate::tokenizer::Token;
#[cfg(test)]
use crate::Engine;
use crate::Position;

/// A code's tokens, each with the bytes it is written over.
pub(in crate::inspect) struct Tokens<'t> {
    text: &'t str,
    pieces: Vec<Piece>,
}

struct Piece {
    token: Token,
    span: Range<usize>,
}

/// One expression, with the bytes it is written over.
#[derive(Debug, PartialEq)]
pub(in crate::inspect) struct Node {
    pub(super) kind: Kind,
    pub(super) span: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub(in crate::inspect) enum Kind {
    /// A name, which reads as the binding it names; `self` among them.
    Binding(String),
    /// `.name`, or `.0` on a tuple.
    Field(Box<Node>, String),
    /// `*operand`.
    Deref(Box<Node>),
    /// `&operand` or `&mut operand`.
    Reference(Box<Node>),
    /// `base[index]`.
    Index(Box<Node>, Box<Node>),
    /// A number, string, char or bool, as written.
    Literal(Literal),
    /// `operand as type`.
    Cast(Box<Node>, String),
    Unary(Unary, Box<Node>),
    Binary(Box<Node>, Binary, Box<Node>),
    /// `(operand)`.
    Paren(Box<Node>),
    /// `receiver.len()` or `receiver.is_empty()`.
    Length(Box<Node>, Length),
    /// Code that runs code: a call, a macro, `?` or `.await`, with the
    /// expressions written in it.
    Runs(Runs, Vec<Node>),
    /// Code that is no one value to read: a tuple, an array or a path,
    /// with the expressions written in it.
    Other(Vec<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Literal {
    Number,
    Text,
    Char,
    Bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Unary {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Binary {
    /// `+`, `-`, `*`, `/`, `%`, `<<`, `>>` and `^`.
    Arithmetic,
    /// `==`, `!=`, `<`, `>`, `<=` and `>=`.
    Comparison,
    /// `&&` and `||`.
    Logic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Length {
    Len,
    IsEmpty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Runs {
    Call,
    Macro,
    Try,
    Await,
}

/// How tightly `as` binds: past the binary operators, under the unary
/// ones, which an operand's parse takes first.
const CAST: u8 = 12;

impl<'t> Tokens<'t> {
    /// The tokens of `text` as rawr's lexer reads Rust. A comment is none.
    #[cfg(test)]
    pub(in crate::inspect) fn of(engine: &Engine, text: &'t str) -> Self {
        let lexed = crate::resolve::lower_nominal::lex_rust(engine, text);
        Self::on(text, &lexed, None)
    }

    /// The tokens `lexed`, rawr's lexer's of `text`, on line `line` (from
    /// 1) alone, or on every line when `None`. A comment is none.
    pub(in crate::inspect) fn on(
        text: &'t str,
        lexed: &[(Token, Position)],
        line: Option<usize>,
    ) -> Self {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        let pieces = lexed
            .iter()
            .take_while(|(token, _)| *token != Token::EOF)
            .filter(|(token, position)| {
                !matches!(token, Token::Comment(..))
                    && line.is_none_or(|line| position.line() == Some(line))
            })
            .filter_map(|(token, position)| {
                let start = *starts.get(position.line()?.checked_sub(1)?)?;
                let within = text[start..]
                    .char_indices()
                    .nth(position.position()?.checked_sub(1)?)
                    .map(|(at, _)| at)?;
                let start = start + within;
                let length = crate::resolve::lower_nominal::token_length(&text[start..]);
                Some(Piece {
                    token: token.clone(),
                    span: start..start + length.max(1),
                })
            })
            .collect();
        Self { text, pieces }
    }

    /// Every expression the tokens write, outermost first, in order.
    pub(in crate::inspect) fn nodes(&self) -> Vec<Node> {
        let mut parser = Parser {
            tokens: self,
            at: 0,
            inside: Vec::new(),
        };
        parser.nodes_until(None)
    }

    /// The one expression the tokens write whole, when they write one.
    pub(in crate::inspect) fn whole(&self) -> Option<Node> {
        let mut pieces = self.pieces.iter().map(|piece| &piece.span);
        let first = pieces.next()?.start;
        let last = self
            .pieces
            .iter()
            .rev()
            .find(|piece| piece.token != Token::SemiColon)?
            .span
            .end;
        let mut nodes = self.nodes();
        (nodes.len() == 1 && nodes[0].span == (first..last)).then(|| nodes.remove(0))
    }
}

impl Node {
    /// The smallest expression among `nodes` and the expressions in them
    /// whose text holds the byte `at`.
    pub(in crate::inspect) fn around(nodes: &[Self], at: usize) -> Option<&Self> {
        nodes.iter().find_map(|node| node.smallest(at))
    }

    /// The expressions written inside this one, by reference.
    pub(super) fn children(&self) -> Vec<&Self> {
        match &self.kind {
            Kind::Binding(_) | Kind::Literal(_) => Vec::new(),
            Kind::Field(operand, _)
            | Kind::Deref(operand)
            | Kind::Reference(operand)
            | Kind::Cast(operand, _)
            | Kind::Unary(_, operand)
            | Kind::Paren(operand)
            | Kind::Length(operand, _) => vec![operand],
            Kind::Index(left, right) | Kind::Binary(left, _, right) => vec![left, right],
            Kind::Runs(_, parts) | Kind::Other(parts) => parts.iter().collect(),
        }
    }

    /// The smallest expression in this one, or this one, whose text holds
    /// the byte `at`.
    pub(in crate::inspect) fn smallest(&self, at: usize) -> Option<&Self> {
        if !self.span.contains(&at) {
            return None;
        }
        Some(
            self.children()
                .into_iter()
                .find_map(|child| child.smallest(at))
                .unwrap_or(self),
        )
    }
}

struct Parser<'p, 't> {
    tokens: &'p Tokens<'t>,
    at: usize,
    /// The brackets open around the parse, the innermost last, as the
    /// token that closes each.
    inside: Vec<Token>,
}

impl Parser<'_, '_> {
    fn peek(&self) -> Option<&Token> {
        self.peek_at(0)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Token> {
        self.tokens
            .pieces
            .get(self.at + ahead)
            .map(|piece| &piece.token)
    }

    fn span(&self, index: usize) -> Range<usize> {
        self.tokens.pieces[index].span.clone()
    }

    fn text(&self, index: usize) -> &str {
        &self.tokens.text[self.span(index)]
    }

    /// The end of the last token taken.
    fn end(&self) -> usize {
        self.span(self.at - 1).end
    }

    /// The expressions up to `close`, which is taken, or to the end: each
    /// one it can read, stepping over the tokens between them.
    fn nodes_until(&mut self, close: Option<Token>) -> Vec<Node> {
        let mut nodes = Vec::new();
        while let Some(token) = self.peek() {
            if close.as_ref() == Some(token) {
                self.at += 1;
                break;
            }
            if let Some(node) = self.expression(0) {
                nodes.push(node);
                continue;
            }
            let token = self.peek().cloned();
            self.at += 1;
            if let Some(closing) = token.as_ref().and_then(closer) {
                self.inside.push(closing.clone());
                nodes.extend(self.nodes_until(Some(closing)));
                self.inside.pop();
            }
        }
        nodes
    }

    fn expression(&mut self, least: u8) -> Option<Node> {
        let mut left = self.operand()?;
        while let Some(token) = self.peek() {
            let start = left.span.start;
            if *token == Token::As {
                if CAST < least {
                    break;
                }
                let before = self.at;
                self.at += 1;
                let Some(name) = self.type_name() else {
                    self.at = before;
                    break;
                };
                left = Node {
                    span: start..self.end(),
                    kind: Kind::Cast(Box::new(left), name),
                };
                continue;
            }
            let Some((operator, power)) = binary(token) else {
                break;
            };
            if power < least {
                break;
            }
            let before = self.at;
            self.at += 1;
            let Some(right) = self.expression(power + 1) else {
                self.at = before;
                break;
            };
            left = Node {
                span: start..right.span.end,
                kind: Kind::Binary(Box::new(left), operator, Box::new(right)),
            };
        }
        Some(left)
    }

    /// An operand: its prefix operators, then what it starts with and the
    /// fields, indices, calls and `?` after it.
    fn operand(&mut self) -> Option<Node> {
        let before = self.at;
        let start = self.span(self.at).start;
        let token = self.peek()?.clone();
        let wrap = |kind: fn(Box<Node>) -> Kind| {
            move |parser: &mut Self| {
                parser.at += 1;
                let operand = parser.operand()?;
                Some(Node {
                    span: start..operand.span.end,
                    kind: kind(Box::new(operand)),
                })
            }
        };
        let found = match token {
            Token::Multiply => wrap(Kind::Deref)(self),
            Token::Minus | Token::UnaryMinus => {
                wrap(|operand| Kind::Unary(Unary::Negate, operand))(self)
            }
            Token::Bang => wrap(|operand| Kind::Unary(Unary::Not, operand))(self),
            Token::Ampersand => {
                if self.peek_at(1) == Some(&Token::Mut) {
                    self.at += 1;
                }
                wrap(Kind::Reference)(self)
            }
            Token::And => wrap(|operand| {
                let span = operand.span.clone();
                Kind::Reference(Box::new(Node {
                    span,
                    kind: Kind::Reference(operand),
                }))
            })(self),
            _ => self.primary().map(|primary| self.postfix(primary)),
        };
        if found.is_none() {
            self.at = before;
        }
        found
    }

    fn primary(&mut self) -> Option<Node> {
        let index = self.at;
        let span = self.span(index);
        let literal = |kind| Node {
            span: span.clone(),
            kind: Kind::Literal(kind),
        };
        let found = match self.peek()? {
            Token::Identifier(_) | Token::SelfValue => return self.named(),
            Token::IntegerConstant(_) | Token::FloatConstant(_) | Token::ByteConstant(_) => {
                literal(Literal::Number)
            }
            Token::StringConstant(_) | Token::ByteStringConstant(_) => literal(Literal::Text),
            Token::CharConstant(_) => literal(Literal::Char),
            Token::True | Token::False => literal(Literal::Bool),
            Token::LeftParen => {
                self.at += 1;
                return self.parenthesized(span.start);
            }
            Token::LeftBracket => {
                self.at += 1;
                self.inside.push(Token::RightBracket);
                let parts = self.nodes_until(Some(Token::RightBracket));
                self.inside.pop();
                return Some(Node {
                    span: span.start..self.end(),
                    kind: Kind::Other(parts),
                });
            }
            _ => return None,
        };
        self.at += 1;
        Some(found)
    }

    /// What a name starts: a binding, or a call, a macro or a path.
    fn named(&mut self) -> Option<Node> {
        let start = self.span(self.at).start;
        // The name an item is declared by is no value.
        if self.at > 0
            && matches!(
                self.tokens.pieces[self.at - 1].token,
                Token::Fn | Token::Struct | Token::Enum | Token::Mod | Token::Use | Token::Impl
            )
        {
            return None;
        }
        // A field's label in a struct's literal or pattern.
        if self.peek_at(1) == Some(&Token::Colon) && self.inside.last() == Some(&Token::RightBrace)
        {
            return None;
        }
        let name = self.text(self.at).to_owned();
        self.at += 1;
        match self.peek() {
            Some(Token::Bang) if self.peek_at(1).and_then(closer).is_some() => {
                self.at += 1;
                let closing = self.peek().and_then(closer)?;
                self.at += 1;
                self.inside.push(closing.clone());
                let parts = self.nodes_until(Some(closing));
                self.inside.pop();
                Some(Node {
                    span: start..self.end(),
                    kind: Kind::Runs(Runs::Macro, parts),
                })
            }
            Some(Token::DoubleColon) => {
                while self.peek() == Some(&Token::DoubleColon) {
                    self.at += 1;
                    match self.peek() {
                        Some(Token::Identifier(_)) => self.at += 1,
                        Some(Token::LessThan) => self.generics(),
                        _ => break,
                    }
                }
                if self.calls() {
                    let parts = self.arguments();
                    return Some(Node {
                        span: start..self.end(),
                        kind: Kind::Runs(Runs::Call, parts),
                    });
                }
                Some(Node {
                    span: start..self.end(),
                    kind: Kind::Other(Vec::new()),
                })
            }
            Some(Token::LeftParen | Token::Unit) => {
                let parts = self.arguments();
                Some(Node {
                    span: start..self.end(),
                    kind: Kind::Runs(Runs::Call, parts),
                })
            }
            _ => Some(Node {
                span: start..self.end(),
                kind: Kind::Binding(name),
            }),
        }
    }

    /// After `(`: a parenthesized expression, a tuple or `()`.
    fn parenthesized(&mut self, start: usize) -> Option<Node> {
        let before = self.at;
        self.inside.push(Token::RightParen);
        if let Some(inner) = self.expression(0)
            && self.peek() == Some(&Token::RightParen)
        {
            self.at += 1;
            self.inside.pop();
            return Some(Node {
                span: start..self.end(),
                kind: Kind::Paren(Box::new(inner)),
            });
        }
        self.at = before;
        let parts = self.nodes_until(Some(Token::RightParen));
        self.inside.pop();
        Some(Node {
            span: start..self.end(),
            kind: Kind::Other(parts),
        })
    }

    /// Whether a call's arguments come next: `(`, or `()`, which lexes as
    /// one token.
    fn calls(&self) -> bool {
        matches!(self.peek(), Some(Token::LeftParen | Token::Unit))
    }

    /// At `(` or `()`: the expressions among a call's arguments, to its
    /// `)`.
    fn arguments(&mut self) -> Vec<Node> {
        let opened = self.peek() == Some(&Token::LeftParen);
        self.at += 1;
        if !opened {
            return Vec::new();
        }
        self.inside.push(Token::RightParen);
        let parts = self.nodes_until(Some(Token::RightParen));
        self.inside.pop();
        parts
    }

    /// At `<`: a path's generic arguments, to their `>`.
    fn generics(&mut self) {
        let mut depth = 0usize;
        while let Some(token) = self.peek() {
            match token {
                Token::LessThan => depth += 1,
                Token::GreaterThan => depth = depth.saturating_sub(1),
                Token::RightShift => depth = depth.saturating_sub(2),
                _ => {}
            }
            self.at += 1;
            if depth == 0 {
                break;
            }
        }
    }

    /// The fields, indices, calls, `?` and `.await` after `node`.
    fn postfix(&mut self, mut node: Node) -> Node {
        let start = node.span.start;
        loop {
            let before = self.at;
            let next = match self.peek() {
                Some(Token::Period) => {
                    self.at += 1;
                    self.member(node, start)
                }
                Some(Token::LeftBracket) => {
                    self.at += 1;
                    self.inside.push(Token::RightBracket);
                    let index = self.expression(0);
                    self.inside.pop();
                    match index {
                        Some(index) if self.peek() == Some(&Token::RightBracket) => {
                            self.at += 1;
                            Ok(Node {
                                span: start..self.end(),
                                kind: Kind::Index(Box::new(node), Box::new(index)),
                            })
                        }
                        _ => Err(node),
                    }
                }
                Some(Token::LeftParen | Token::Unit) => {
                    let mut parts = vec![node];
                    parts.extend(self.arguments());
                    Ok(Node {
                        span: start..self.end(),
                        kind: Kind::Runs(Runs::Call, parts),
                    })
                }
                Some(Token::Reserved(word)) if word.as_str() == "?" => {
                    self.at += 1;
                    Ok(Node {
                        span: start..self.end(),
                        kind: Kind::Runs(Runs::Try, vec![node]),
                    })
                }
                _ => Err(node),
            };
            match next {
                Ok(next) => node = next,
                Err(back) => {
                    self.at = before;
                    return back;
                }
            }
        }
    }

    /// After `.`: a field, a tuple's item, a method's call or `.await`.
    /// `Err` gives `node` back when nothing a member is follows.
    fn member(&mut self, node: Node, start: usize) -> Result<Node, Node> {
        let index = self.at;
        match self.peek() {
            Some(Token::Identifier(_)) => {
                let name = self.text(index).to_owned();
                self.at += 1;
                if name == "await" {
                    return Ok(Node {
                        span: start..self.end(),
                        kind: Kind::Runs(Runs::Await, vec![node]),
                    });
                }
                if self.peek() == Some(&Token::DoubleColon) {
                    self.at += 1;
                    if self.peek() == Some(&Token::LessThan) {
                        self.generics();
                    }
                }
                if !self.calls() {
                    return Ok(Node {
                        span: start..self.end(),
                        kind: Kind::Field(Box::new(node), name),
                    });
                }
                let arguments = self.arguments();
                let length = match name.as_str() {
                    "len" => Some(Length::Len),
                    "is_empty" => Some(Length::IsEmpty),
                    _ => None,
                };
                let span = start..self.end();
                Ok(match length {
                    Some(length) if arguments.is_empty() && self.called_bare(index) => Node {
                        span,
                        kind: Kind::Length(Box::new(node), length),
                    },
                    _ => {
                        let mut parts = vec![node];
                        parts.extend(arguments);
                        Node {
                            span,
                            kind: Kind::Runs(Runs::Call, parts),
                        }
                    }
                })
            }
            Some(Token::IntegerConstant(_)) => {
                let name = self.text(index).to_owned();
                self.at += 1;
                Ok(Node {
                    span: start..self.end(),
                    kind: Kind::Field(Box::new(node), name),
                })
            }
            // `.0.1` lexes as a number with a point.
            Some(Token::FloatConstant(_)) => {
                let written = self.text(index).to_owned();
                let Some((first, second)) = written.split_once('.') else {
                    return Err(node);
                };
                if !first
                    .bytes()
                    .chain(second.bytes())
                    .all(|byte| byte.is_ascii_digit())
                {
                    return Err(node);
                }
                let middle = self.span(index).start + first.len();
                self.at += 1;
                let inner = Node {
                    span: start..middle,
                    kind: Kind::Field(Box::new(node), first.to_owned()),
                };
                Ok(Node {
                    span: start..self.end(),
                    kind: Kind::Field(Box::new(inner), second.to_owned()),
                })
            }
            _ => Err(node),
        }
    }

    /// Whether the method named at `index` is called with nothing between
    /// its name and `(`, as `len()` is: a turbofish makes it another call.
    fn called_bare(&self, index: usize) -> bool {
        matches!(
            self.tokens.pieces.get(index + 1).map(|piece| &piece.token),
            Some(Token::LeftParen | Token::Unit)
        )
    }

    /// A type's name after `as`, by its path's last segment.
    fn type_name(&mut self) -> Option<String> {
        let mut name = None;
        while let Some(Token::Identifier(_)) = self.peek() {
            name = Some(self.text(self.at).to_owned());
            self.at += 1;
            if self.peek() != Some(&Token::DoubleColon) {
                break;
            }
            self.at += 1;
        }
        name
    }
}

/// The token that closes the bracket `token` opens.
fn closer(token: &Token) -> Option<Token> {
    match token {
        Token::LeftParen => Some(Token::RightParen),
        Token::LeftBracket => Some(Token::RightBracket),
        Token::LeftBrace | Token::MapStart => Some(Token::RightBrace),
        _ => None,
    }
}

/// The binary operator `token` is and how tightly it binds, as Rust's
/// precedence has them. `&` and `|` are left out: on a line they are more
/// often a reference or a closure's than a bitwise operator.
fn binary(token: &Token) -> Option<(Binary, u8)> {
    Some(match token {
        Token::Multiply | Token::Divide | Token::Modulo => (Binary::Arithmetic, 10),
        Token::Plus | Token::Minus => (Binary::Arithmetic, 9),
        Token::LeftShift | Token::RightShift => (Binary::Arithmetic, 8),
        Token::XOr => (Binary::Arithmetic, 6),
        Token::EqualsTo
        | Token::NotEqualsTo
        | Token::LessThan
        | Token::GreaterThan
        | Token::LessThanEqualsTo
        | Token::GreaterThanEqualsTo => (Binary::Comparison, 4),
        Token::And => (Binary::Logic, 3),
        Token::Or => (Binary::Logic, 2),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Binary, Kind, Length, Literal, Node, Runs, Tokens};

    /// The text of the smallest expression around the first `^` under
    /// `line`'s text, and its kind's name.
    fn at(line: &str, marker: &str) -> Option<(String, &'static str)> {
        let engine = crate::Engine::new();
        let tokens = Tokens::of(&engine, line);
        let column = marker.find('^').expect("the marker has a caret");
        let nodes = tokens.nodes();
        let node = Node::around(&nodes, column)?;
        Some((line[node.span.clone()].to_owned(), name(&node.kind)))
    }

    fn name(kind: &Kind) -> &'static str {
        match kind {
            Kind::Binding(_) => "binding",
            Kind::Field(..) => "field",
            Kind::Deref(_) => "deref",
            Kind::Reference(_) => "reference",
            Kind::Index(..) => "index",
            Kind::Literal(_) => "literal",
            Kind::Cast(..) => "cast",
            Kind::Unary(..) => "unary",
            Kind::Binary(..) => "binary",
            Kind::Paren(_) => "paren",
            Kind::Length(..) => "length",
            Kind::Runs(Runs::Call, _) => "call",
            Kind::Runs(Runs::Macro, _) => "macro",
            Kind::Runs(..) => "runs",
            Kind::Other(_) => "other",
        }
    }

    #[test]
    fn a_hover_takes_the_smallest_expression_around_its_character() {
        let line = "    let shown = player.score + names[turn as usize % names.len()].len();";
        assert_eq!(
            at(line, "                ^"),
            Some(("player".to_owned(), "binding"))
        );
        assert_eq!(
            at(line, "                       ^"),
            Some(("player.score".to_owned(), "field")),
            "a field's name reads the whole place"
        );
        assert_eq!(
            at(line, "                                       ^"),
            Some(("turn".to_owned(), "binding"))
        );
        assert_eq!(
            at(line, "                                    ^"),
            Some(("names[turn as usize % names.len()]".to_owned(), "index"))
        );
        assert_eq!(
            at(line, "                                            ^"),
            Some(("turn as usize".to_owned(), "cast"))
        );
        assert_eq!(
            at(
                line,
                "                                                          ^"
            ),
            Some(("names.len()".to_owned(), "length"))
        );
        assert_eq!(at(line, "    ^"), None, "`let` is no expression");
        assert_eq!(at(line, "        ^"), Some(("shown".to_owned(), "binding")));
    }

    #[test]
    fn places_parse_through_derefs_tuple_items_and_references() {
        assert_eq!(
            at("total += *self.items[0].1;", "               ^"),
            Some(("self.items".to_owned(), "field"))
        );
        assert_eq!(
            at("total += *self.items[0].1;", "                       ^"),
            Some(("self.items[0].1".to_owned(), "field"))
        );
        assert_eq!(
            at("total += *self.items[0].1;", "         ^"),
            Some(("*self.items[0].1".to_owned(), "deref"))
        );
        assert_eq!(
            at("let pair = grid.0.1;", "                 ^"),
            Some(("grid.0.1".to_owned(), "field")),
            "`.0.1` lexes as one number and reads as two items"
        );
        assert_eq!(
            at("scores[&name]", "        ^"),
            Some(("name".to_owned(), "binding"))
        );
        assert_eq!(
            at("scores[&name]", "      ^"),
            Some(("scores[&name]".to_owned(), "index"))
        );
    }

    #[test]
    fn calls_macros_and_item_names_are_no_places() {
        assert_eq!(
            at("    player.reset();", "            ^"),
            Some(("player.reset()".to_owned(), "call"))
        );
        assert_eq!(
            at("    player.reset();", "     ^"),
            Some(("player".to_owned(), "binding")),
            "a call's receiver is a place"
        );
        assert_eq!(
            at("    tick(count);", "     ^"),
            Some(("tick(count)".to_owned(), "call"))
        );
        assert_eq!(
            at("    tick(count);", "          ^"),
            Some(("count".to_owned(), "binding"))
        );
        assert_eq!(
            at("println!(\"{}\", total);", "  ^"),
            Some(("println!(\"{}\", total)".to_owned(), "macro"))
        );
        assert_eq!(
            at("fn tick(count: u32) -> u32 {", "     ^"),
            None,
            "a function's own name is no place"
        );
        assert_eq!(
            at("fn tick(count: u32) -> u32 {", "         ^"),
            Some(("count".to_owned(), "binding"))
        );
        assert_eq!(
            at("let p = Point { x: other, y: 2 };", "                ^"),
            None,
            "a struct literal's label names a field"
        );
        assert_eq!(
            at("let p = Point { x: other, y: 2 };", "                    ^"),
            Some(("other".to_owned(), "binding"))
        );
        assert_eq!(
            at("let n = items.iter().count();", "              ^"),
            Some(("items.iter()".to_owned(), "call"))
        );
    }

    #[test]
    fn a_code_given_whole_is_one_expression_or_none() {
        let engine = crate::Engine::new();
        let whole = |code: &str| Tokens::of(&engine, code).whole().map(|node| node.kind);
        assert!(matches!(whole("player.score"), Some(Kind::Field(..))));
        assert!(matches!(
            whole("names.len()"),
            Some(Kind::Length(_, Length::Len))
        ));
        assert!(matches!(
            whole("count * 2"),
            Some(Kind::Binary(_, Binary::Arithmetic, _))
        ));
        assert!(matches!(whole("\"a\""), Some(Kind::Literal(Literal::Text))));
        assert!(matches!(whole("tick()"), Some(Kind::Runs(Runs::Call, _))));
        assert!(matches!(whole("player.score;"), Some(Kind::Field(..))));
        assert_eq!(whole("let x = 1"), None);
        assert_eq!(whole("a b"), None);
    }
}
