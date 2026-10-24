//! Borrowed rustc dep-info records and decoded dependency-token events.

use std::borrow::Cow;
use std::iter::Peekable;
use std::str;

/// One raw environment record; uniqueness belongs to its retaining caller.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::input) struct Record<'text> {
    pub(in crate::input) key: &'text str,
    pub(in crate::input) value: Option<&'text str>,
}

/// An environment rejection with no owned diagnostic storage.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(in crate::input) enum Error<'text> {
    #[error("malformed environment record: {0}")]
    Malformed(&'text str),
}

/// Raw lines are read before any continuation removal. The key and value
/// lengths are accounting (`string_bytes`) and are measured by each caller's
/// account, which records a crossing and continues.
pub(in crate::input) struct Environment<'text> {
    lines: str::Lines<'text>,
    failed: bool,
}

impl<'text> Environment<'text> {
    /// Borrow raw records in physical line order.
    pub(in crate::input) fn read(text: &'text str) -> Self {
        Self {
            lines: text.lines(),
            failed: false,
        }
    }
}

impl<'text> Iterator for Environment<'text> {
    type Item = Result<Record<'text>, Error<'text>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        for line in self.lines.by_ref() {
            let record = environment_record(line);
            match record {
                Ok(None) => continue,
                Ok(Some(record)) => return Some(Ok(record)),
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            }
        }
        None
    }
}

fn environment_record(line: &str) -> Result<Option<Record<'_>>, Error<'_>> {
    if !line.starts_with("# env-dep") {
        return Ok(None);
    }
    let record = line
        .strip_prefix("# env-dep:")
        .ok_or(Error::Malformed(line))?;
    let (key, value) = match record.split_once('=') {
        Some((key, value)) => (key, Some(value)),
        None => (record, None),
    };
    if key.is_empty() || key.contains('=') || key.contains('\0') || key.contains('\r') {
        return Err(Error::Malformed(line));
    }
    Ok(Some(Record { key, value }))
}

/// A dependency-rule rejection, independent of its diagnostic file path.
#[derive(Clone, Copy, Debug, thiserror::Error, Eq, PartialEq)]
pub(in crate::input) enum Syntax {
    #[error("trailing line escape")]
    TrailingLineEscape,
    #[error("missing first dependency rule")]
    MissingRule,
    #[error("trailing escape")]
    TrailingEscape,
}

/// Reject the terminal continuation before environment-record interpretation.
pub(in crate::input) fn complete(text: &str) -> Result<(), Syntax> {
    if text.ends_with("\\\n") {
        Err(Syntax::TrailingLineEscape)
    } else {
        Ok(())
    }
}

/// rustc's spelling of a path in a dependency rule (`escape_dep_filename`):
/// only spaces are escaped. `#`, `$` and `:` are written as they are, and
/// `Tokens` reads them back literally.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn escape_path(path: &str) -> Cow<'_, str> {
    if path.contains(' ') {
        Cow::Owned(path.replace(' ', "\\ "))
    } else {
        Cow::Borrowed(path)
    }
}

/// rustc's spelling of an `# env-dep` value (`escape_dep_env`): a backslash,
/// a newline and a carriage return are escaped, and nothing else.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn escape_environment(value: &str) -> Cow<'_, str> {
    if !value.contains(['\\', '\n', '\r']) {
        return Cow::Borrowed(value);
    }
    let mut escaped = String::with_capacity(value.len() + 8);
    for character in value.chars() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\\' => escaped.push_str("\\\\"),
            _ => escaped.push(character),
        }
    }
    Cow::Owned(escaped)
}

/// Materialize the exact continuation replacement after caller admission.
pub(in crate::input) fn normalize(text: &str) -> String {
    text.replace("\\\n", "")
}

/// A decoded character or the end of a nonempty token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input) enum Event {
    Character(char),
    Boundary,
}

enum Characters<'text> {
    Raw(str::Chars<'text>),
    Normalized(str::Chars<'text>),
}

impl Iterator for Characters<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        match self {
            Self::Normalized(characters) => characters.next(),
            Self::Raw(characters) => loop {
                let character = characters.next()?;
                if character == '\\' && characters.as_str().starts_with('\n') {
                    characters.next();
                } else {
                    return Some(character);
                }
            },
        }
    }
}

struct Line<'text> {
    characters: Peekable<Characters<'text>>,
    finished: bool,
}

impl Iterator for Line<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if self.finished {
            return None;
        }
        let character = self.characters.next()?;
        if character == '\n' || (character == '\r' && self.characters.peek() == Some(&'\n')) {
            self.finished = true;
            None
        } else {
            Some(character)
        }
    }
}

/// The first rule's decoded tokens, retaining no token or normalized string.
pub(in crate::input) struct Tokens<'text> {
    line: Line<'text>,
    escaped: bool,
    token: bool,
    finished: bool,
}

impl<'text> Tokens<'text> {
    /// Scan raw text with the same literal continuation removal as `normalize`.
    pub(in crate::input) fn raw(text: &'text str) -> Result<Self, Syntax> {
        complete(text)?;
        Self::read(Characters::Raw(text.chars()))
    }

    /// Scan a caller-owned result of `normalize` without another copy.
    pub(in crate::input) fn normalized(text: &'text str) -> Result<Self, Syntax> {
        Self::read(Characters::Normalized(text.chars()))
    }

    fn read(characters: Characters<'text>) -> Result<Self, Syntax> {
        let mut line = Line {
            characters: characters.peekable(),
            finished: false,
        };
        // The target ends at a colon before whitespace or the line's end, as
        // Cargo reads it (`": "`): rustc writes `:` in a path as it is, so a
        // Windows drive or a directory named `a:b` is part of the target.
        let mut colon = false;
        let mut rule = false;
        for character in line.by_ref() {
            if colon && character.is_whitespace() {
                rule = true;
                break;
            }
            colon = character == ':';
        }
        if !(rule || colon) {
            return Err(Syntax::MissingRule);
        }
        Ok(Self {
            line,
            escaped: false,
            token: false,
            finished: false,
        })
    }
}

impl Iterator for Tokens<'_> {
    type Item = Result<Event, Syntax>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        for character in self.line.by_ref() {
            if self.escaped {
                self.escaped = false;
                self.token = true;
                return Some(Ok(Event::Character(character)));
            }
            if character == '\\' {
                self.escaped = true;
            } else if character.is_whitespace() {
                if self.token {
                    self.token = false;
                    return Some(Ok(Event::Boundary));
                }
            } else {
                self.token = true;
                return Some(Ok(Event::Character(character)));
            }
        }
        self.finished = true;
        if self.escaped {
            Some(Err(Syntax::TrailingEscape))
        } else if self.token {
            Some(Ok(Event::Boundary))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
