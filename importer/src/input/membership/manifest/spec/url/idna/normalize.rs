// Adapted from ICU4X 2.2.0. Unicode-3.0; see normalize/LICENSE.
//! Complete pinned UTS46 mapping/NFC continuations over immutable ICU data.

mod compose;
mod data;
mod decompose;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Step, Stop};

/// Incremental transformation state; no domain or URL validity is implied.
pub(super) struct Normalizer<I: Iterator<Item = char>> {
    delegate: I,
    buffer: storage::Buffer<Character>,
    position: usize,
    pending: Option<Pending>,
    unprocessed_starter: Option<char>,
    mode: Mode,
    refused: bool,
}

#[derive(Clone, Copy)]
enum Mode {
    Map,
    Validate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    character: char,
    value: u32,
}

/// Safe scalar storage and an explicit stable-sort tie-break in one real record.
#[derive(Debug)]
struct Character {
    character: char,
    ccc: u8,
    ordinal: usize,
}

impl Character {
    fn new(character: char, ccc: u8) -> Self {
        Self {
            character,
            ccc,
            ordinal: 0,
        }
    }

    fn placeholder(character: char) -> Self {
        Self::new(character, 0xff)
    }

    fn starter(character: char) -> Self {
        Self::new(character, 0)
    }

    fn from_pending(pending: Pending) -> Self {
        Self::new(pending.character, data::ccc(pending.value))
    }
}

/// Map UTS46 statuses, drop ignorables, and normalize to NFC incrementally.
pub(super) fn map_normalize<I: Iterator<Item = char>, A: document::Admission>(
    input: I,
    session: &mut session::Session<'_, A>,
) -> Step<Normalizer<I>> {
    Normalizer::start(input, Mode::Map, session)
}

/// Map and normalize, replacing ignorables for the caller's validity comparison.
pub(super) fn normalize_validate<I: Iterator<Item = char>, A: document::Admission>(
    input: I,
    session: &mut session::Session<'_, A>,
) -> Step<Normalizer<I>> {
    Normalizer::start(input, Mode::Validate, session)
}

impl<I: Iterator<Item = char>> Normalizer<I> {
    fn start<A: document::Admission>(
        delegate: I,
        mode: Mode,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut value = Self {
            delegate,
            buffer: storage::Buffer::new(),
            position: 0,
            pending: Some(Pending {
                character: '\u{ffff}',
                value: 0,
            }),
            unprocessed_starter: None,
            mode,
            refused: false,
        };
        // ICU primes decomposition by consuming a placeholder starter; leading
        // combining input is therefore gathered before construction completes.
        let _ = value.decomposition_next(session)?;
        Ok(value)
    }

    /// Advance with the same original account used for construction.
    pub(super) fn next<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<char>> {
        if let Err(stop) = session.check() {
            self.refused = true;
            return Err(stop);
        }
        if self.refused {
            return Err(Stop::Refused);
        }
        let result = self.composition_next(session);
        if result.is_err() {
            self.refused = true;
        }
        result
    }

    fn push<A: document::Admission>(
        &mut self,
        value: Character,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.buffer.push(value, session)?;
        Ok(())
    }

    fn buffered(&self, index: usize) -> Option<(char, u8)> {
        self.buffer
            .as_slice()
            .get(index)
            .map(|value| (value.character, value.ccc))
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.position = 0;
    }

    fn drain_prefix(&mut self) {
        if self.position == 0 {
            return;
        }
        let length = self.buffer.len();
        for index in self.position..length {
            self.buffer
                .as_mut_slice()
                .swap(index - self.position, index);
        }
        self.buffer.truncate(length - self.position);
        self.position = 0;
    }

    fn remove(&mut self, index: usize) {
        for next in index + 1..self.buffer.len() {
            self.buffer.as_mut_slice().swap(next - 1, next);
        }
        self.buffer.truncate(self.buffer.len() - 1);
    }
}
