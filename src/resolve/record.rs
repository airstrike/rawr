//! The instances native code calls marked generic functions at, as the
//! generated crate's last check recorded them (D866, D869): each is read
//! as a type the crate's root writes, and compiled as an instance of its
//! template beside those marked callers need (D629).

use crate::item::type_ref::{Argument, Lifetime, Path, Segment, Type};
use crate::item::Name;
use crate::{ImmutableString, Span};

/// One recorded instance of a template: the path native code's call names
/// it by, the types its named generic parameters take, as the crate's root
/// names them, and the key its instance is named by, which native code's
/// trampoline calls it by.
#[derive(Clone, Debug)]
pub(crate) struct Native {
    pub(crate) path: ImmutableString,
    pub(crate) types: Box<[super::type_ref::Type]>,
    pub(crate) key: ImmutableString,
}

/// The type `text` writes, as rustc prints a type from a crate's root:
/// paths with generic arguments, references, tuples and `()`, a path from
/// `crate::` spelled through the crate's name `krate`, as rawr spells a
/// marked module's path to native code. `None` for any other, which no
/// recorded instance is compiled for.
pub(crate) fn written(text: &str, krate: &str) -> Option<Type> {
    let tokens = tokens(text)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        krate,
    };
    let ty = parser.ty()?;
    (parser.at == parser.tokens.len()).then_some(ty)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    Lifetime(String),
    Punct(&'static str),
}

fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&character) = characters.peek() {
        if character.is_whitespace() {
            characters.next();
        } else if character == '\'' {
            characters.next();
            let mut name = String::new();
            while let Some(&next) = characters.peek().filter(|next| is_word(**next)) {
                name.push(next);
                characters.next();
            }
            tokens.push(Token::Lifetime(name));
        } else if is_word(character) {
            let mut word = String::new();
            while let Some(&next) = characters.peek().filter(|next| is_word(**next)) {
                word.push(next);
                characters.next();
            }
            tokens.push(Token::Word(word));
        } else {
            characters.next();
            let punct = match character {
                ':' if characters.next_if_eq(&':').is_some() => "::",
                '<' => "<",
                '>' => ">",
                ',' => ",",
                '&' => "&",
                '(' => "(",
                ')' => ")",
                _ => return None,
            };
            tokens.push(Token::Punct(punct));
        }
    }
    Some(tokens)
}

fn is_word(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct Parser<'k> {
    tokens: Vec<Token>,
    at: usize,
    krate: &'k str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, punct: &str) -> bool {
        if matches!(self.peek(), Some(Token::Punct(found)) if *found == punct) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn ty(&mut self) -> Option<Type> {
        if self.eat("&") {
            let lifetime = match self.peek() {
                Some(Token::Lifetime(name)) => {
                    let name = name.clone();
                    self.at += 1;
                    Some(lifetime(&name))
                }
                _ => None,
            };
            let mutable = matches!(self.peek(), Some(Token::Word(word)) if word == "mut");
            if mutable {
                self.at += 1;
            }
            let target = self.ty()?;
            return Some(Type::Reference {
                mutable,
                lifetime,
                target: Box::new(target),
                span: Span::NONE,
            });
        }
        if self.eat("(") {
            let mut members = Vec::new();
            while !self.eat(")") {
                members.push(self.ty()?);
                if !self.eat(",") && !matches!(self.peek(), Some(Token::Punct(")"))) {
                    return None;
                }
            }
            return Some(match members.len() {
                0 => Type::Unit(Span::NONE),
                1 => members.pop()?,
                _ => Type::Tuple {
                    members: members.into_boxed_slice(),
                    span: Span::NONE,
                },
            });
        }
        let mut segments = Vec::new();
        loop {
            let Some(Token::Word(word)) = self.peek().cloned() else {
                return None;
            };
            self.at += 1;
            let word = if segments.is_empty() && word == "crate" {
                self.krate.to_owned()
            } else {
                word
            };
            let mut arguments = Vec::new();
            if self.eat("<") {
                while !self.eat(">") {
                    match self.peek().cloned() {
                        Some(Token::Lifetime(name)) => {
                            self.at += 1;
                            arguments.push(Argument::Lifetime(lifetime(&name)));
                        }
                        _ => arguments.push(Argument::Type(self.ty()?)),
                    }
                    if !self.eat(",") && !matches!(self.peek(), Some(Token::Punct(">"))) {
                        return None;
                    }
                }
            }
            segments.push(Segment {
                name: name(&word),
                arguments: arguments.into_boxed_slice(),
                span: Span::NONE,
            });
            if !self.eat("::") {
                break;
            }
        }
        Some(Type::Path(Path {
            segments: segments.into_boxed_slice(),
            span: Span::NONE,
            global: false,
        }))
    }
}

fn name(text: &str) -> Name {
    Name {
        text: text.into(),
        span: Span::NONE,
    }
}

fn lifetime(text: &str) -> Lifetime {
    Lifetime {
        name: name(text),
        span: Span::NONE,
    }
}

#[cfg(test)]
mod tests {
    use super::written;

    #[test]
    fn a_recorded_type_reads_as_the_crate_root_writes_it() {
        assert!(written("crate::Message", "app").is_some());
        assert!(written("std::option::Option<std::vec::Vec<u8>>", "app").is_some());
        assert!(written("&'_ str", "app").is_some());
        assert!(written("(u8, crate::grid::Message)", "app").is_some());
        assert!(written("[u8; 2]", "app").is_none());
    }
}
