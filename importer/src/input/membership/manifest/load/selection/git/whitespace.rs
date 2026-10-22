// Copyright (c) 2014 The Rust Project Developers. MIT OR Apache-2.0.
//! Retained fixed reverse-whitespace helper. This is not a Cargo-process
//! association: its eventual caller must establish that owner and its history.

use std::alloc::Layout;

use rawr_toml::document;

use crate::input::membership::manifest::{session::Session, Error, Step};

mod dfa;
mod wire;

/// Inline storage to be retained by the complete modeled Cargo-process owner.
/// No Clone/Default/reset path can silently create an independent ready helper.
pub(super) struct Owner {
    state: State,
}

enum State {
    Uninitialized,
    Ready(dfa::Dfa<'static>),
    Refused,
}

#[repr(C)]
struct Aligned {
    _align: [u32; 0],
    bytes: [u8; 3232],
}

static TABLE: Aligned = Aligned {
    _align: [],
    bytes: *include_bytes!("whitespace/whitespace_anchored_rev.littleendian.dfa"),
};

impl Owner {
    /// Admit the real inline owner slot. Empty storage proves no cold producer.
    pub(super) fn admit<A: document::Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self {
            state: State::Uninitialized,
        })
    }

    /// Include this full layout in a containing owner's actual heap allocation,
    /// if it has one. Inline use does not invent an allocation or a Box charge.
    pub(super) fn layout() -> Layout {
        Layout::new::<Self>()
    }

    /// ByteSlice::trim_end: arbitrary bytes, whole input, anchored, reverse and
    /// earliest=false. The returned slice borrows only the caller's input.
    pub(super) fn trim_end<'input, A: document::Admission>(
        &mut self,
        bytes: &'input [u8],
        session: &mut Session<'_, A>,
    ) -> Step<&'input [u8]> {
        let result = self.end(bytes, session);
        match result {
            Ok(end) => Ok(&bytes[..end]),
            Err(error) => {
                // Also poison a ready helper after search refusal. A new
                // Session cannot turn spent work into a fresh attempt.
                self.state = State::Refused;
                Err(error)
            }
        }
    }

    fn end<A: document::Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.check()?;
        match self.state {
            State::Refused => return session.fail(Error::State),
            State::Uninitialized => {
                // Publish only the complete validated value. A refused decode
                // leaves no usable representation and is never retried.
                self.state = State::Refused;
                let value = dfa::Dfa::decode(&TABLE.bytes, session)?;
                self.state = State::Ready(value);
            }
            State::Ready(_) => {}
        }
        let State::Ready(value) = &self.state else {
            return session.fail(Error::State);
        };
        value.end(bytes, session)
    }
}

#[cfg(test)]
mod tests;
