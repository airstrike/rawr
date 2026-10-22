// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See input/PROVENANCE.md.

//! Borrowed URL input with pinned trimming and filtered/raw cursor semantics.

use rawr_toml::document;

use crate::input::membership::manifest::{session, Step};

/// A cursor over one admitted input. Cloning preserves an allocation-free rewind.
#[derive(Clone, Debug)]
pub(super) struct Input<'i> {
    chars: std::str::Chars<'i>,
}

impl<'i> Input<'i> {
    /// Admit the original spelling before applying absolute-entry trimming.
    pub(super) fn new<A: document::Admission>(
        original: &'i str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.string(original.len())?;
        Ok(Self {
            chars: original.trim_matches(|c| c <= ' ').chars(),
        })
    }

    /// Setter input keeps leading and trailing controls; cursor filtering stays pinned.
    pub(super) fn no_trim(
        original: &'i str,
        session: &mut session::Session<'_, impl document::Admission>,
    ) -> Step<Self> {
        session.string(original.len())?;
        Ok(Self {
            chars: original.chars(),
        })
    }

    /// Raw, unconsumed UTF-8, including any still-unread tab, LF or CR.
    pub(super) fn remaining(&self) -> &'i str {
        self.chars.as_str()
    }

    /// Bulk path/query/fragment scanners bypass filtering one scalar at a time.
    pub(super) fn next_raw(&mut self) -> Option<char> {
        self.chars.next()
    }

    /// The next retained scalar and its exact borrowed UTF-8 spelling.
    pub(super) fn next_utf8(&mut self) -> Option<(char, &'i str)> {
        loop {
            let remaining = self.chars.as_str();
            let character = self.chars.next()?;
            if !ignored(character) {
                return Some((character, &remaining[..character.len_utf8()]));
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.clone().next().is_none()
    }

    pub(super) fn starts_with(&self, prefix: &str) -> bool {
        self.split_prefix(prefix).is_some()
    }

    /// Prefix matching consumes filtered scalars; failure preserves this cursor.
    pub(super) fn split_prefix(&self, prefix: &str) -> Option<Self> {
        let mut remaining = self.clone();
        for expected in prefix.chars() {
            if remaining.next() != Some(expected) {
                return None;
            }
        }
        Some(remaining)
    }

    pub(super) fn split_first(&self) -> (Option<char>, Self) {
        let mut remaining = self.clone();
        (remaining.next(), remaining)
    }

    /// The first nonmatching scalar, and ignored bytes before it, stay unread.
    pub(super) fn count_matching(&self, matches: impl Fn(char) -> bool) -> (usize, Self) {
        let mut count = 0;
        let mut remaining = self.clone();
        loop {
            let mut candidate = remaining.clone();
            if candidate.next().is_some_and(&matches) {
                remaining = candidate;
                // Each increment consumes at least one byte of the admitted str.
                count += 1;
            } else {
                return (count, remaining);
            }
        }
    }
}

impl Iterator for Input<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        self.chars.by_ref().find(|&character| !ignored(character))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.chars.as_str().len()))
    }
}

fn ignored(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r')
}

#[cfg(test)]
mod tests;
