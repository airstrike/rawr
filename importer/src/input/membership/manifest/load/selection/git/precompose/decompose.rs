// Copyright the Rust Project Developers. MIT OR Apache-2.0.
//! Canonical decomposition, stable combining-class order and retained capacity.

use super::{buffer, lookup, Admission, Session, Step};
use std::{iter::Fuse, ops::Range, str::Chars};

enum Kind {
    Canonical,
    _Compatible,
}

pub(super) struct Decompose<'input> {
    _kind: Kind,
    chars: Fuse<Chars<'input>>,
    buffer: buffer::Buffer<(u8, char)>,
    ready: Range<usize>,
}

impl<'input> Decompose<'input> {
    pub(super) fn from_chars<A: Admission>(
        chars: Chars<'input>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Ok(Self {
            _kind: Kind::Canonical,
            chars: chars.fuse(),
            buffer: buffer::Buffer::new(session)?,
            ready: 0..0,
        })
    }

    fn push<A: Admission>(&mut self, ch: char, session: &mut Session<'_, A>) -> Step<()> {
        let class = lookup::class(ch, session)?;
        if class == 0 {
            self.sort_pending(session)?;
            self.buffer.push((class, ch), session)?;
            self.ready.end = self.buffer.values().len();
        } else {
            self.buffer.push((class, ch), session)?;
        }
        Ok(())
    }

    fn sort_pending<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        buffer::sort(&mut self.buffer.values_mut()[self.ready.end..], session)
    }

    fn reset<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        let pending = self.buffer.values().len() - self.ready.end;
        session.items(pending)?;
        for index in 0..pending {
            self.buffer.values_mut()[index] = self.buffer.values()[index + self.ready.end];
        }
        self.buffer.truncate(pending, session)?;
        self.ready = 0..0;
        Ok(())
    }

    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<char>> {
        while self.ready.end == 0 {
            session.items(1)?;
            match self.chars.next() {
                Some(ch) => {
                    lookup::canonical(ch, session, |part, session| self.push(part, session))?
                }
                None => {
                    if self.buffer.values().is_empty() {
                        return Ok(None);
                    }
                    self.sort_pending(session)?;
                    self.ready.end = self.buffer.values().len();
                    break;
                }
            }
        }
        session.items(1)?;
        let ch = self.buffer.values()[self.ready.start].1;
        let next = self.ready.start + 1;
        if next == self.ready.end {
            self.reset(session)?;
        } else {
            self.ready.start = next;
        }
        Ok(Some(ch))
    }
}
