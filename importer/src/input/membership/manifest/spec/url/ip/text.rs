//! Inline canonical IP spelling: at most eight four-digit pieces and seven colons.

use rawr_toml::document;

use crate::input::membership::manifest::{session, Error, Step};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) struct Text {
    bytes: [u8; 39],
    length: usize,
}

impl Text {
    pub(super) fn render<A: document::Admission>(
        emit: impl Fn(&mut dyn FnMut(u8)),
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut length = 0;
        emit(&mut |_| length += 1);
        if length > 39 {
            return session.fail(Error::State);
        }
        session.string(length)?;
        session.items(1)?;
        let mut text = Self {
            bytes: [0; 39],
            length: 0,
        };
        emit(&mut |byte| {
            text.bytes[text.length] = byte;
            text.length += 1;
        });
        Ok(text)
    }

    pub(in super::super) fn as_str(&self) -> &str {
        // Private emitters produce only ASCII decimal/hex digits, dots or colons.
        std::str::from_utf8(&self.bytes[..self.length]).expect("IP display is ASCII")
    }
}
