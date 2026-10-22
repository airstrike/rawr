// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See output/PROVENANCE.md.

//! Charged UTF-8 serialization storage and pinned URL component encoding sets.

use std::ops::Range;

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Step};

/// Mutable transformation data; no complete-URL interpretation follows.
pub(super) struct Output {
    bytes: storage::Buffer<u8>,
}

/// Sets reached by absolute parsing and native file-path construction.
#[derive(Clone, Copy, Debug)]
pub(super) enum Set {
    Controls,
    Userinfo,
    Path,
    SpecialPathSegment,
    PathSegment,
    Query,
    SpecialQuery,
    Fragment,
}

/// Pinned semantic component-offset overflow, distinct from resource refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("URLs more than 4 GB are not supported")]
pub(super) struct Overflow;

impl Output {
    pub(super) const fn new() -> Self {
        Self {
            bytes: storage::Buffer::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub(super) fn as_str<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<&str> {
        session.check()?;
        match std::str::from_utf8(self.as_bytes()) {
            Ok(text) => Ok(text),
            Err(_) => session.fail(Error::State),
        }
    }

    pub(super) fn offset<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<u32, Overflow>> {
        offset(self.len(), session)
    }

    pub(super) fn append<A: document::Admission>(
        &mut self,
        text: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.grow(text.len(), session)?;
        self.extend(text.bytes(), session)
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        character: char,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let mut bytes = [0; 4];
        self.append(character.encode_utf8(&mut bytes), session)
    }

    pub(super) fn encode<A: document::Admission>(
        &mut self,
        text: &str,
        set: Set,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.encode_bytes(text.as_bytes(), set, session)
    }

    pub(super) fn encode_bytes<A: document::Admission>(
        &mut self,
        text: &[u8],
        set: Set,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.string(text.len())?;
        let Some(length) = text.iter().copied().try_fold(0usize, |length, byte| {
            length.checked_add(if set.encoded(byte) { 3 } else { 1 })
        }) else {
            return session.fail(Error::Overflow);
        };
        self.grow(length, session)?;
        // Each fixed fragment borrows local iterator state; no intermediate
        // encoded string or collection is retained.
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let bytes = text.iter().copied().flat_map(|byte| {
            let (bytes, length) = if set.encoded(byte) {
                (
                    [
                        b'%',
                        HEX[usize::from(byte >> 4)],
                        HEX[usize::from(byte & 15)],
                    ],
                    3,
                )
            } else {
                ([byte, 0, 0], 1)
            };
            bytes.into_iter().take(length)
        });
        self.extend(bytes, session)
    }

    pub(super) fn clear<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        self.bytes.clear();
        Ok(())
    }

    pub(super) fn truncate<A: document::Admission>(
        &mut self,
        length: usize,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        // String::truncate leaves an already shorter string unchanged.
        if length < self.len() {
            if !self.boundary(length) {
                return session.fail(Error::State);
            }
            self.bytes.truncate(length);
        }
        Ok(())
    }

    pub(super) fn pop<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<char>> {
        session.check()?;
        if self.len() == 0 {
            return Ok(None);
        }
        let mut start = self.len() - 1;
        while !self.boundary(start) {
            start -= 1;
        }
        let character = match std::str::from_utf8(&self.as_bytes()[start..]) {
            Ok(text) => text.chars().next(),
            Err(_) => return session.fail(Error::State),
        };
        self.bytes.truncate(start);
        Ok(character)
    }

    pub(super) fn remove<A: document::Admission>(
        &mut self,
        range: Range<usize>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if range.start > range.end || !self.boundary(range.start) || !self.boundary(range.end) {
            return session.fail(Error::State);
        }
        let length = self.len() - (range.end - range.start);
        self.bytes
            .as_mut_slice()
            .copy_within(range.end.., range.start);
        self.bytes.truncate(length);
        Ok(())
    }

    pub(super) fn insert<A: document::Admission>(
        &mut self,
        index: usize,
        text: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if !self.boundary(index) {
            return session.fail(Error::State);
        }
        let old = self.len();
        self.grow(text.len(), session)?;
        self.extend(std::iter::repeat_n(0, text.len()), session)?;
        let bytes = self.bytes.as_mut_slice();
        bytes.copy_within(index..old, index + text.len());
        bytes[index..index + text.len()].copy_from_slice(text.as_bytes());
        Ok(())
    }

    fn boundary(&self, index: usize) -> bool {
        index <= self.len()
            && self
                .as_bytes()
                .get(index)
                .is_none_or(|byte| byte & 0xc0 != 0x80)
    }

    fn grow<A: document::Admission>(
        &self,
        additional: usize,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        let Some(length) = self.len().checked_add(additional) else {
            return session.fail(Error::Overflow);
        };
        session.string(length)
    }

    fn extend<A: document::Admission>(
        &mut self,
        bytes: impl Iterator<Item = u8>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let old = self.len();
        for byte in bytes {
            if let Err(stop) = self.bytes.push(byte, session) {
                // A refused multibyte append must leave valid UTF-8. Capacity
                // and all spent admissions remain; only content rolls back.
                self.bytes.truncate(old);
                return Err(stop);
            }
        }
        Ok(())
    }
}

fn offset<A: document::Admission>(
    length: usize,
    session: &mut session::Session<'_, A>,
) -> Step<Result<u32, Overflow>> {
    session.check()?;
    match u32::try_from(length) {
        Ok(offset) => Ok(Ok(offset)),
        Err(_) => {
            session.items(1)?;
            Ok(Err(Overflow))
        }
    }
}

impl Set {
    fn encoded(self, byte: u8) -> bool {
        if !(0x20..0x7f).contains(&byte) {
            return true;
        }
        let fragment = matches!(byte, b' ' | b'"' | b'<' | b'>' | b'`');
        let path = fragment || matches!(byte, b'#' | b'?' | b'{' | b'}');
        let query = matches!(byte, b' ' | b'"' | b'#' | b'<' | b'>');
        match self {
            Self::Controls => false,
            Self::Fragment => fragment,
            Self::Path => path,
            Self::PathSegment => path || matches!(byte, b'/' | b'%'),
            Self::SpecialPathSegment => path || matches!(byte, b'/' | b'%' | b'\\'),
            Self::Userinfo => {
                path || matches!(
                    byte,
                    b'/' | b':' | b';' | b'=' | b'@' | b'[' | b'\\' | b']' | b'^' | b'|'
                )
            }
            Self::Query => query,
            Self::SpecialQuery => query || byte == b'\'',
        }
    }
}

#[cfg(test)]
mod tests;
