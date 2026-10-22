//! Pinned tokenizer/lookahead and cfg grammar, with admitted explicit frames.
use super::super::super::super::Stop;
use super::{storage, target, Atom, Kind, Node, Session, Step};
use rawr_toml::document;

#[derive(Clone, Copy, PartialEq)]
enum Token<'a> {
    Left,
    Right,
    Comma,
    Equals,
    Ident { raw: bool, text: &'a str },
    String(&'a str),
}
impl Token<'_> {
    fn class(self) -> &'static str {
        match self {
            Self::Left => "`(`",
            Self::Right => "`)`",
            Self::Comma => "`,`",
            Self::Equals => "`=`",
            Self::Ident { .. } => "an identifier",
            Self::String(_) => "a string",
        }
    }
}
enum Reason<'a> {
    String,
    Char(char),
    Token {
        expected: &'static str,
        found: &'static str,
    },
    Incomplete(&'static str),
    Rest(&'a str),
}
enum Failure<'a> {
    Resource(Stop),
    Semantic(Reason<'a>),
}
impl From<Stop> for Failure<'_> {
    fn from(stop: Stop) -> Self {
        Self::Resource(stop)
    }
}
type Parse<'a, T> = Result<T, Failure<'a>>;
#[derive(Clone, Copy)]
struct Lexer<'a> {
    text: &'a str,
    at: usize,
}
impl<'a> Lexer<'a> {
    fn character<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, Option<(usize, char)>> {
        let Some(ch) = self.text[self.at..].chars().next() else {
            return Ok(None);
        };
        session.string(ch.len_utf8())?;
        let at = self.at;
        self.at += ch.len_utf8();
        Ok(Some((at, ch)))
    }
    fn peek_character<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, Option<(usize, char)>> {
        self.clone().character(session)
    }
    fn next<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, Option<Token<'a>>> {
        session.items(1)?;
        loop {
            let Some((start, ch)) = self.character(session)? else {
                return Ok(None);
            };
            return Ok(Some(match ch {
                ' ' => continue,
                '(' => Token::Left,
                ')' => Token::Right,
                ',' => Token::Comma,
                '=' => Token::Equals,
                '"' => loop {
                    match self.character(session)? {
                        Some((end, '"')) => break Token::String(&self.text[start + 1..end]),
                        Some(_) => {}
                        None => return Err(Failure::Semantic(Reason::String)),
                    }
                },
                ch if ident_start(ch) => {
                    let (start, raw) = if ch == 'r'
                        && self
                            .peek_character(session)?
                            .is_some_and(|(_, ch)| ch == '#')
                    {
                        self.character(session)?;
                        match self.character(session)? {
                            Some((at, ch)) if ident_start(ch) => (at, true),
                            Some((_, ch)) => return Err(Failure::Semantic(Reason::Char(ch))),
                            None => {
                                return Err(Failure::Semantic(Reason::Incomplete("identifier")))
                            }
                        }
                    } else {
                        (start, false)
                    };
                    while self
                        .peek_character(session)?
                        .is_some_and(|(_, ch)| ident_start(ch) || ch.is_ascii_digit())
                    {
                        self.character(session)?;
                    }
                    Token::Ident {
                        raw,
                        text: &self.text[start..self.at],
                    }
                }
                other => return Err(Failure::Semantic(Reason::Char(other))),
            }));
        }
    }
    fn peek<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, Option<Token<'a>>> {
        self.clone().next(session)
    }
    fn take<A: document::Admission>(
        &mut self,
        token: Token<'a>,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, bool> {
        match self.peek(session) {
            Ok(Some(actual)) if actual == token => {
                self.next(session)?;
                Ok(true)
            }
            Err(Failure::Resource(stop)) => Err(Failure::Resource(stop)),
            _ => Ok(false),
        }
    }
    fn eat<A: document::Admission>(
        &mut self,
        token: Token<'a>,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, ()> {
        match self.next(session)? {
            Some(actual) if actual == token => Ok(()),
            Some(actual) => Err(Failure::Semantic(Reason::Token {
                expected: token.class(),
                found: actual.class(),
            })),
            None => Err(Failure::Semantic(Reason::Incomplete(token.class()))),
        }
    }
    fn rest<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Parse<'a, Option<&'a str>> {
        let mut lexer = *self;
        loop {
            match lexer.character(session)? {
                Some((_, ' ')) => {}
                Some((at, _)) => return Ok(Some(&self.text[at..])),
                None => return Ok(None),
            }
        }
    }
}
fn ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}
struct Frame {
    node: usize,
    last: Option<usize>,
    child: bool,
    not: bool,
}

fn expression<'a, A: document::Admission>(
    lexer: &mut Lexer<'a>,
    parent: Option<usize>,
    nodes: &mut storage::Buffer<Node<'a>>,
    session: &mut Session<'_, A>,
) -> Parse<'a, (usize, Option<Frame>)> {
    let peek = match lexer.peek(session) {
        Err(Failure::Semantic(_)) => {
            lexer.next(session)?;
            None
        }
        other => other?,
    };
    let operator = match peek {
        Some(Token::Ident {
            raw: false,
            text: "all",
        }) => Some(Kind::All),
        Some(Token::Ident {
            raw: false,
            text: "any",
        }) => Some(Kind::Any),
        Some(Token::Ident {
            raw: false,
            text: "not",
        }) => Some(Kind::Not),
        _ => None,
    };
    if let Some(kind) = operator {
        lexer.next(session)?;
        lexer.eat(Token::Left, session)?;
        let node = nodes.push(
            Node {
                kind,
                child: None,
                next: None,
                parent,
            },
            session,
        )?;
        return Ok((
            node,
            Some(Frame {
                node,
                last: None,
                child: true,
                not: matches!(kind, Kind::Not),
            }),
        ));
    }
    if peek.is_none() {
        return Err(Failure::Semantic(Reason::Incomplete(
            "start of a cfg expression",
        )));
    }
    let (raw, name) = match lexer.next(session)? {
        Some(Token::Ident { raw, text }) => (raw, text),
        Some(token) => {
            return Err(Failure::Semantic(Reason::Token {
                expected: "identifier",
                found: token.class(),
            }))
        }
        None => return Err(Failure::Semantic(Reason::Incomplete("identifier"))),
    };
    let value = if lexer.take(Token::Equals, session)? {
        Some(match lexer.next(session)? {
            Some(Token::String(value)) => value,
            Some(token) => {
                return Err(Failure::Semantic(Reason::Token {
                    expected: "a string",
                    found: token.class(),
                }))
            }
            None => return Err(Failure::Semantic(Reason::Incomplete("a string"))),
        })
    } else {
        None
    };
    let kind = match (name, value) {
        ("true", None) => Kind::True,
        ("false", None) => Kind::False,
        _ => Kind::Value(Atom { name, raw, value }),
    };
    let node = nodes.push(
        Node {
            kind,
            child: None,
            next: None,
            parent,
        },
        session,
    )?;
    Ok((node, None))
}

fn run<'a, A: document::Admission>(
    text: &'a str,
    session: &mut Session<'_, A>,
) -> Parse<'a, storage::Buffer<Node<'a>>> {
    let mut lexer = Lexer { text, at: 0 };
    let mut nodes = storage::Buffer::new();
    let mut frames = storage::Buffer::new();
    let (_, frame) = expression(&mut lexer, None, &mut nodes, session)?;
    if let Some(frame) = frame {
        frames.push(frame, session)?;
    }
    while let Some(mut frame) = frames.pop() {
        session.items(1)?;
        if frame.child {
            if !frame.not && lexer.take(Token::Right, session)? {
                continue;
            }
            let (child, pending) = expression(&mut lexer, Some(frame.node), &mut nodes, session)?;
            if let Some(last) = frame.last {
                nodes.as_mut_slice()[last].next = Some(child);
            } else {
                nodes.as_mut_slice()[frame.node].child = Some(child);
            }
            frame.last = Some(child);
            frame.child = false;
            frames.push(frame, session)?;
            if let Some(pending) = pending {
                frames.push(pending, session)?;
            }
        } else if !frame.not && lexer.take(Token::Comma, session)? {
            frame.child = true;
            frames.push(frame, session)?;
        } else {
            lexer.eat(Token::Right, session)?;
        }
    }
    if let Some(rest) = lexer.rest(session)? {
        return Err(Failure::Semantic(Reason::Rest(rest)));
    }
    Ok(nodes)
}

pub(super) fn parse<'a, A: document::Admission>(
    text: &'a str,
    session: &mut Session<'_, A>,
) -> Step<Result<storage::Buffer<Node<'a>>, String>> {
    match run(text, session) {
        Ok(nodes) => Ok(Ok(nodes)),
        Err(Failure::Resource(stop)) => Err(stop),
        Err(Failure::Semantic(reason)) => {
            struct Cause<'a>(Reason<'a>);
            impl std::fmt::Display for Cause<'_> {
                fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match &self.0 {
                        Reason::String=>out.write_str("unterminated string in cfg"),
                        Reason::Char(ch)=>write!(out,"unexpected character `{ch}` in cfg, expected parens, a comma, an identifier, or a string"),
                        Reason::Token{expected,found}=>write!(out,"expected {expected}, found {found}"),
                        Reason::Incomplete(expected)=>write!(out,"expected {expected}, but cfg expression ended"),
                        Reason::Rest(rest)=>write!(out,"unexpected content `{rest}` found after cfg expression"),
                    }
                }
            }
            target::formatted(
                format_args!(
                    "failed to parse `{text}` as a cfg expression: {}",
                    Cause(reason)
                ),
                session,
            )
            .map(Err)
        }
    }
}
