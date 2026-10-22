// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Pinned HIR dispatch with admitted, nonrecursive continuations.
use super::super::translate::view::{Children, Kind, View};
use super::builder::{Builder, Failure, Fragment, Pending, Repeat, Suffix, Trie};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

mod batch;
mod startup;
#[cfg(test)]
mod tests;

pub(super) struct Compiler {
    builder: Builder,
    forward: Pending,
    suffix: Suffix,
    reverse: bool,
    _trie: startup::Initial,
}
enum Frame<'a> {
    Concat {
        children: Children<'a>,
        previous: Option<Fragment>,
    },
    Alternation {
        children: Children<'a>,
        state: Alternative,
    },
    Repeat(Repeat),
}
enum Alternative {
    First,
    Second(Fragment),
    Rest(Fragment),
}
impl Compiler {
    /// Complete the pinned batch and finalizer in the same original Session.
    /// This is immutable NFA data, not an executable regex or source witness.
    pub(super) fn build<A: Admission>(
        &mut self,
        expressions: &[super::super::translate::Expression],
        size_limit: Option<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<super::finalize::Nfa, Failure>> {
        let starts = match self.many(expressions, size_limit, session)? {
            Ok(starts) => starts,
            Err(error) => return Ok(Err(error)),
        };
        self.finalize(starts.anchored, starts.unanchored, session)
    }

    pub(super) fn finalize<A: Admission>(
        &self,
        anchored: super::builder::Id,
        unanchored: super::builder::Id,
        session: &mut Session<'_, A>,
    ) -> Step<Result<super::finalize::Nfa, Failure>> {
        self.builder
            .finalize(anchored, unanchored, self.reverse, session)
    }

    /// Retain the pinned constructor's eager RangeTrie allocation even though
    /// Core's shrink=false paths never populate it.
    pub(super) fn new<A: Admission>(reverse: bool, session: &mut Session<'_, A>) -> Step<Self> {
        let builder = Builder::new();
        let forward = Pending::new();
        let trie = startup::Initial::new(session)?;
        let suffix = Suffix::new();
        Ok(Self {
            builder,
            forward,
            suffix,
            reverse,
            _trie: trie,
        })
    }
    pub(super) fn compile<A: Admission>(
        &mut self,
        root: View<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let mut stack = Buffer::new();
        let mut current = Some(root);
        let mut ready = None;
        loop {
            session.items(1)?;
            if let Some(view) = current.take() {
                let result = match view.kind(session)? {
                    Kind::Empty => self.builder.empty(session)?,
                    Kind::Literal(bytes) => self.builder.literal(bytes, self.reverse, session)?,
                    Kind::Bytes(class) => self.builder.byte_class(class, session)?,
                    Kind::Unicode(class) if self.reverse => {
                        self.builder
                            .unicode_reverse(class, &mut self.suffix, session)?
                    }
                    Kind::Unicode(class) => {
                        self.builder
                            .unicode_forward(class, &mut self.forward, session)?
                    }
                    Kind::Anchor { end } => self.builder.anchor(end, self.reverse, session)?,
                    Kind::Repetition {
                        min,
                        max,
                        greedy,
                        child,
                    } => {
                        let minimum = if min == 0 && max.is_none() {
                            child.minimum_len(session)?
                        } else {
                            None
                        };
                        let pending = match self
                            .builder
                            .begin_repeat(min, max, greedy, minimum, session)?
                        {
                            Ok(p) => p,
                            Err(e) => return Ok(Err(e)),
                        };
                        stack.push(Frame::Repeat(pending), session)?;
                        current = Some(child);
                        continue;
                    }
                    Kind::Concat(mut children) => match children.next(self.reverse, session)? {
                        None => self.builder.empty(session)?,
                        Some(child) => {
                            stack.push(
                                Frame::Concat {
                                    children,
                                    previous: None,
                                },
                                session,
                            )?;
                            current = Some(child);
                            continue;
                        }
                    },
                    Kind::Alternation(mut children) => {
                        let mut scan = children.clone();
                        let mut literals = 0;
                        while let Some(child) = scan.next(false, session)? {
                            if child.is_literal(session)? {
                                literals += 1;
                            }
                        }
                        if literals > 1 && literals == children.len() {
                            let mut trie = Trie::new(self.reverse, session)?;
                            while let Some(child) = children.next(false, session)? {
                                let Kind::Literal(bytes) = child.kind(session)? else {
                                    return session.fail(Error::State);
                                };
                                if let Err(e) = trie.add(bytes, session)? {
                                    return Ok(Err(e));
                                }
                            }
                            trie.compile(&mut self.builder, session)?
                        } else {
                            match children.next(false, session)? {
                                None => self.builder.fail(session)?,
                                Some(child) => {
                                    stack.push(
                                        Frame::Alternation {
                                            children,
                                            state: Alternative::First,
                                        },
                                        session,
                                    )?;
                                    current = Some(child);
                                    continue;
                                }
                            }
                        }
                    }
                };
                ready = Some(match result {
                    Ok(f) => f,
                    Err(e) => return Ok(Err(e)),
                });
            }
            session.items(1)?;
            let Some(compiled) = ready.take() else {
                return session.fail(Error::State);
            };
            let Some(frame) = stack.pop() else {
                return Ok(Ok(compiled));
            };
            match frame {
                Frame::Repeat(pending) => {
                    ready = Some(
                        match self.builder.finish_repeat(pending, compiled, session)? {
                            Ok(f) => f,
                            Err(e) => return Ok(Err(e)),
                        },
                    );
                }
                Frame::Concat {
                    mut children,
                    previous,
                } => {
                    let combined = match previous {
                        None => compiled,
                        Some(mut previous) => {
                            if let Err(e) =
                                self.builder.patch(previous.end, compiled.start, session)?
                            {
                                return Ok(Err(e));
                            }
                            previous.end = compiled.end;
                            previous
                        }
                    };
                    if let Some(child) = children.next(self.reverse, session)? {
                        stack.push(
                            Frame::Concat {
                                children,
                                previous: Some(combined),
                            },
                            session,
                        )?;
                        current = Some(child);
                    } else {
                        ready = Some(combined);
                    }
                }
                Frame::Alternation {
                    mut children,
                    state,
                } => {
                    let state = match state {
                        Alternative::First => Alternative::Second(compiled),
                        Alternative::Second(first) => Alternative::Rest(
                            match self.builder.alternate_pair(first, compiled, session)? {
                                Ok(f) => f,
                                Err(e) => return Ok(Err(e)),
                            },
                        ),
                        Alternative::Rest(alternatives) => {
                            if let Err(e) =
                                self.builder
                                    .alternate_next(&alternatives, compiled, session)?
                            {
                                return Ok(Err(e));
                            }
                            Alternative::Rest(alternatives)
                        }
                    };
                    if let Some(child) = children.next(false, session)? {
                        stack.push(Frame::Alternation { children, state }, session)?;
                        current = Some(child);
                    } else {
                        ready = Some(match state {
                            Alternative::Second(f) | Alternative::Rest(f) => f,
                            Alternative::First => return session.fail(Error::State),
                        });
                    }
                }
            }
        }
    }
}
