// Copyright (c) 2014 The Rust Project Developers. MIT OR Apache-2.0.
//! Safe borrowed views of the fixed little-endian serialization. Every scalar,
//! slice and copied inline slot is admitted before decoding it.

use super::{document, Error, Session, Step};

pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// Same two-word borrowed representation as a source &[u32], without casts.
pub(super) struct Words<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    pub(super) fn header<A: document::Admission>(
        bytes: &'a [u8],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut cursor = Self { bytes, at: 0 };
        while cursor.at < 7 && cursor.at < bytes.len() {
            session.items(1)?;
            if bytes[cursor.at] != 0 {
                break;
            }
            cursor.at += 1;
        }
        cursor.aligned(session)?;
        let label = b"rust-regex-automata-dfa-dense";
        let remaining = &bytes[cursor.at..];
        let mut nul = None;
        for (index, &byte) in remaining.iter().take(256).enumerate() {
            session.items(1)?;
            if byte == 0 {
                nul = Some(index);
                break;
            }
        }
        let Some(length) = nul else {
            return session.fail(Error::State);
        };
        session.items(length)?;
        require(remaining[..length] == *label, session)?;
        // Source read_label rounds the first-NUL offset to four bytes.
        let padded = length + ((4 - length % 4) % 4);
        cursor.take(padded, session)?;
        require(cursor.u32(session)? == 0xFEFF, session)?;
        require(cursor.u32(session)? == 2, session)?;
        let _unused = cursor.u32(session)?;
        Ok(cursor)
    }

    pub(super) fn take<A: document::Admission>(
        &mut self,
        length: usize,
        session: &mut Session<'_, A>,
    ) -> Step<&'a [u8]> {
        session.items(1)?;
        let Some(end) = self.at.checked_add(length) else {
            return session.fail(Error::Overflow);
        };
        let Some(bytes) = self.bytes.get(self.at..end) else {
            return session.fail(Error::State);
        };
        self.at = end;
        Ok(bytes)
    }

    pub(super) fn u32<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        let bytes = self.take(4, session)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn peek_u32<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let Some(end) = self.at.checked_add(4) else {
            return session.fail(Error::Overflow);
        };
        let Some(bytes) = self.bytes.get(self.at..end) else {
            return session.fail(Error::State);
        };
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn usize<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        let value = self.u32(session)?;
        match usize::try_from(value) {
            Ok(value) => Ok(value),
            Err(_) => session.fail(Error::Overflow),
        }
    }

    pub(super) fn optional_id<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<u32>> {
        let value = self.u32(session)?;
        if value == u32::MAX {
            Ok(None)
        } else {
            require(value < i32::MAX as u32, session)?;
            Ok(Some(value))
        }
    }

    pub(super) fn words<A: document::Admission>(
        &mut self,
        count: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Words<'a>> {
        self.aligned(session)?;
        let Some(length) = count.checked_mul(4) else {
            return session.fail(Error::Overflow);
        };
        Ok(Words(self.take(length, session)?))
    }

    pub(super) fn position(&self) -> usize {
        self.at
    }

    fn aligned<A: document::Admission>(&self, session: &mut Session<'_, A>) -> Step<()> {
        require(self.bytes[self.at..].as_ptr().align_offset(4) == 0, session)
    }
}

impl Words<'_> {
    pub(super) fn len(&self) -> usize {
        self.0.len() / 4
    }

    pub(super) fn get<A: document::Admission>(
        &self,
        index: usize,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let Some(at) = index.checked_mul(4) else {
            return session.fail(Error::Overflow);
        };
        let Some(end) = at.checked_add(4) else {
            return session.fail(Error::Overflow);
        };
        let Some(bytes) = self.0.get(at..end) else {
            return session.fail(Error::State);
        };
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn bytes(&self) -> &[u8] {
        self.0
    }
}

pub(super) fn require<A: document::Admission>(
    condition: bool,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    if condition {
        Ok(())
    } else {
        session.fail(Error::State)
    }
}
