// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Actual intermediate character ownership and its complete UTF-8 byte extent.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Step};

pub(super) struct Characters {
    values: storage::Buffer<char>,
    bytes: usize,
}

impl Characters {
    pub(super) const fn new() -> Self {
        Self {
            values: storage::Buffer::new(),
            bytes: 0,
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        c: char,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let Some(bytes) = self.bytes.checked_add(c.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        session.string(bytes)?;
        self.values.push(c, session)?;
        self.bytes = bytes;
        Ok(())
    }

    pub(super) fn truncate(&mut self, length: usize) {
        // All removed scalar widths were included in the checked running sum.
        for c in &self.values.as_slice()[length..] {
            self.bytes -= c.len_utf8();
        }
        self.values.truncate(length);
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }
    pub(super) fn as_slice(&self) -> &[char] {
        self.values.as_slice()
    }
}
