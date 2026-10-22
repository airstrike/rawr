// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See parser/PROVENANCE.md.

//! Pinned no-base/default URL parser with original-account serialization.

mod authority;
pub(in crate::input::membership::manifest) mod canonical;
mod file;
pub(in crate::input::membership::manifest) use file::base::{Accepted, BaseError};
mod input;
mod output;
mod path;
mod tail;
mod value;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use input::Input;
use output::Output;

use super::host;
use crate::input::membership::manifest::{session, Error, Step, Stop};

/// A completely interpreted owned URL; getters never expose mutation authority.
pub(in crate::input::membership::manifest) struct Url {
    output: Output,
    scheme_end: u32,
    path_start: u32,
    query_start: Option<u32>,
    fragment_start: Option<u32>,
}

/// Only errors reachable from no-base/default parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::input::membership::manifest) enum Invalid {
    #[error("empty host")]
    EmptyHost,
    #[error("invalid international domain name")]
    Idna,
    #[error("invalid port number")]
    Port,
    #[error("invalid IPv4 address")]
    Ipv4,
    #[error("invalid IPv6 address")]
    Ipv6,
    #[error("invalid domain character")]
    Character,
    #[error("relative URL without a base")]
    Relative,
    #[error("URLs more than 4 GB are not supported")]
    Overflow,
}

enum Failure {
    Stop(Stop),
    Invalid(Invalid),
}

type Parse<T> = Result<T, Failure>;

impl From<Stop> for Failure {
    fn from(stop: Stop) -> Self {
        Self::Stop(stop)
    }
}

impl From<host::Invalid> for Invalid {
    fn from(error: host::Invalid) -> Self {
        match error {
            host::Invalid::Empty => Self::EmptyHost,
            host::Invalid::Idna => Self::Idna,
            host::Invalid::Ipv4 => Self::Ipv4,
            host::Invalid::Ipv6 => Self::Ipv6,
            host::Invalid::Character => Self::Character,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Scheme {
    File,
    Special,
    Other,
}

impl Scheme {
    fn of(scheme: &str) -> Self {
        match scheme {
            "file" => Self::File,
            "ftp" | "http" | "https" | "ws" | "wss" => Self::Special,
            _ => Self::Other,
        }
    }

    fn special(self) -> bool {
        self != Self::Other
    }
}

struct Parser<'s, 'a, A: document::Admission> {
    output: Output,
    session: &'s mut session::Session<'a, A>,
}

impl Url {
    pub(in crate::input::membership::manifest) fn parse<A: document::Admission>(
        text: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        let input = Input::new(text, session)?;
        let parser = Parser {
            output: Output::new(),
            session,
        };
        match parser.absolute(input) {
            Ok(url) => Ok(Ok(url)),
            Err(Failure::Invalid(error)) => Ok(Err(error)),
            Err(Failure::Stop(stop)) => Err(stop),
        }
    }
}

impl<A: document::Admission> Parser<'_, '_, A> {
    fn invalid<T>(&mut self, error: Invalid) -> Parse<T> {
        self.session.items(1)?;
        Err(Failure::Invalid(error))
    }

    fn state<T>(&mut self) -> Parse<T> {
        self.session.fail(Error::State).map_err(Failure::Stop)
    }

    fn offset(&mut self) -> Parse<u32> {
        self.output
            .offset(self.session)?
            .map_err(|_| Failure::Invalid(Invalid::Overflow))
    }

    fn absolute(self, input: Input<'_>) -> Parse<Url> {
        self.absolute_in(input, None)
    }

    fn absolute_in(
        mut self,
        mut input: Input<'_>,
        base: Option<&file::base::Native>,
    ) -> Parse<Url> {
        if !input
            .clone()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            return self.invalid(Invalid::Relative);
        }
        loop {
            match input.next() {
                Some(c @ ('a'..='z' | '0'..='9' | '+' | '-' | '.')) => {
                    self.output.push(c, self.session)?;
                }
                Some(c @ 'A'..='Z') => self.output.push(c.to_ascii_lowercase(), self.session)?,
                Some(':') => break,
                _ => {
                    self.output.clear(self.session)?;
                    return self.invalid(Invalid::Relative);
                }
            }
        }
        let scheme_end = self.offset()?;
        let scheme = Scheme::of(self.output.as_str(self.session)?);
        self.output.push(':', self.session)?;
        match scheme {
            Scheme::File => {
                self.output.clear(self.session)?;
                match base {
                    Some(base) => self.file_with_native(input, base),
                    None => self.file(input),
                }
            }
            Scheme::Special => {
                let (_, input) = input.count_matching(|c| matches!(c, '/' | '\\'));
                self.authority(input, scheme, scheme_end)
            }
            Scheme::Other => {
                if let Some(input) = input.split_prefix("//") {
                    return self.authority(input, scheme, scheme_end);
                }
                let path_start = self.offset()?;
                let input = if let Some(input) = input.split_prefix("/") {
                    self.output.push('/', self.session)?;
                    self.path(scheme, &mut false, path_start as usize, input)?
                } else {
                    self.opaque(input)?
                };
                self.finish(scheme, scheme_end, path_start, input, true)
            }
        }
    }

    fn finish(
        mut self,
        scheme: Scheme,
        scheme_end: u32,
        mut path_start: u32,
        input: Input<'_>,
        anarchist: bool,
    ) -> Parse<Url> {
        if anarchist && self.output.as_bytes()[path_start as usize..].starts_with(b"//") {
            self.output
                .insert(path_start as usize, "/.", self.session)?;
            let Some(offset) = path_start.checked_add(2) else {
                return self.invalid(Invalid::Overflow);
            };
            path_start = offset;
        }
        let (query_start, fragment_start) = self.tail(input, scheme.special())?;
        self.session.items(1)?;
        Ok(Url {
            output: self.output,
            scheme_end,
            path_start,
            query_start,
            fragment_start,
        })
    }
}
