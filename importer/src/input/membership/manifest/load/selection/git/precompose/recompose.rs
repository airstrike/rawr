// Copyright the Rust Project Developers. MIT OR Apache-2.0.
//! Pinned canonical recomposition state machine, including blocked marks.

use super::{buffer, decompose::Decompose, lookup, Admission, Session, Step};
use std::str::Chars;

enum State {
    Composing,
    Purging(usize),
    Finished(usize),
}

pub(super) struct Recompose<'input> {
    iter: Decompose<'input>,
    state: State,
    buffer: buffer::Buffer<char>,
    composee: Option<char>,
    last_class: Option<u8>,
}

impl<'input> Recompose<'input> {
    pub(super) fn from_chars<A: Admission>(
        chars: Chars<'input>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Ok(Self {
            iter: Decompose::from_chars(chars, session)?,
            state: State::Composing,
            buffer: buffer::Buffer::new(session)?,
            composee: None,
            last_class: None,
        })
    }

    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<char>> {
        loop {
            session.items(1)?;
            match self.state {
                State::Composing => {
                    while let Some(ch) = self.iter.next(session)? {
                        let class = lookup::class(ch, session)?;
                        let current = match self.composee {
                            None => {
                                if class != 0 {
                                    return Ok(Some(ch));
                                }
                                self.composee = Some(ch);
                                continue;
                            }
                            Some(current) => current,
                        };
                        match self.last_class {
                            None => match lookup::compose(current, ch, session)? {
                                Some(composed) => {
                                    self.composee = Some(composed);
                                    continue;
                                }
                                None => {
                                    if class == 0 {
                                        self.composee = Some(ch);
                                        return Ok(Some(current));
                                    }
                                    self.buffer.push(ch, session)?;
                                    self.last_class = Some(class);
                                }
                            },
                            Some(last) => {
                                if last >= class {
                                    if class == 0 {
                                        self.composee = Some(ch);
                                        self.last_class = None;
                                        self.state = State::Purging(0);
                                        return Ok(Some(current));
                                    }
                                    self.buffer.push(ch, session)?;
                                    self.last_class = Some(class);
                                    continue;
                                }
                                match lookup::compose(current, ch, session)? {
                                    Some(composed) => {
                                        self.composee = Some(composed);
                                        continue;
                                    }
                                    None => {
                                        self.buffer.push(ch, session)?;
                                        self.last_class = Some(class);
                                    }
                                }
                            }
                        }
                    }
                    self.state = State::Finished(0);
                    if self.composee.is_some() {
                        return Ok(self.composee.take());
                    }
                }
                State::Purging(next) => match self.buffer.values().get(next).copied() {
                    None => {
                        self.buffer.truncate(0, session)?;
                        self.state = State::Composing;
                    }
                    value => {
                        self.state = State::Purging(next + 1);
                        return Ok(value);
                    }
                },
                State::Finished(next) => match self.buffer.values().get(next).copied() {
                    None => {
                        self.buffer.truncate(0, session)?;
                        return Ok(self.composee.take());
                    }
                    value => {
                        self.state = State::Finished(next + 1);
                        return Ok(value);
                    }
                },
            }
        }
    }
}
