// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 Builder::build (410–617), preserving state order.
use super::super::finalize::{self, captures, storage};
use super::*;

impl Builder {
    pub(in super::super) fn finalize<A: Admission>(
        &self,
        anchored: Id,
        unanchored: Id,
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<finalize::Nfa, Failure>> {
        session.check()?;
        if self.pattern.is_some() {
            return session.fail(Error::State);
        }
        let mut output = finalize::Building::new(reverse, session)?;
        let mut empties = Buffer::new();
        let mut remap = storage::repeat(Id::ZERO, self.states.len(), session)?;
        output.graph.anchored = anchored;
        output.graph.unanchored = unanchored;
        output.graph.starts = storage::copy(self.starts.as_slice(), session)?;
        output.graph.groups = match captures::Groups::build(self.captures.as_slice(), session)? {
            Ok(groups) => groups,
            Err(error) => return Ok(Err(Failure::Captures(error))),
        };
        for (index, state) in self.states.as_slice().iter().enumerate() {
            session.items(1)?;
            let id = Id(index as u32);
            let state = match state {
                State::Empty { next } => {
                    empties.push((id, *next), session)?;
                    continue;
                }
                State::ByteRange { trans } => finalize::State::ByteRange { trans: *trans },
                State::Sparse { transitions } => match transitions.as_slice() {
                    [] => finalize::State::Fail,
                    [trans] => finalize::State::ByteRange { trans: *trans },
                    transitions => finalize::State::Sparse {
                        transitions: storage::copy(transitions, session)?,
                    },
                },
                State::Look { look, next } => finalize::State::Look {
                    look: *look,
                    next: *next,
                },
                State::CaptureStart {
                    pattern,
                    group,
                    next,
                }
                | State::CaptureEnd {
                    pattern,
                    group,
                    next,
                } => {
                    let mut slot = output.graph.groups.slot(*pattern, *group, session)?;
                    if matches!(state, State::CaptureEnd { .. }) {
                        slot = match slot.checked_add(1).filter(|slot| *slot < i32::MAX as u32) {
                            Some(slot) => slot,
                            None => return session.fail(Error::State),
                        };
                    }
                    finalize::State::Capture {
                        next: *next,
                        pattern: *pattern,
                        group: *group,
                        slot,
                    }
                }
                State::Union { alternates } | State::UnionReverse { alternates } => {
                    let reversed = matches!(state, State::UnionReverse { .. });
                    match alternates.as_slice() {
                        [] => finalize::State::Fail,
                        [next] => {
                            empties.push((id, *next), session)?;
                            remap[index] = *next;
                            continue;
                        }
                        [first, second] => {
                            let (alt1, alt2) = if reversed {
                                (*second, *first)
                            } else {
                                (*first, *second)
                            };
                            finalize::State::BinaryUnion { alt1, alt2 }
                        }
                        alternates => {
                            let mut alternates = storage::copy(alternates, session)?;
                            if reversed {
                                session.items(alternates.len())?;
                                alternates.reverse();
                            }
                            finalize::State::Union { alternates }
                        }
                    }
                }
                State::Fail => finalize::State::Fail,
                State::Match { pattern } => finalize::State::Match { pattern: *pattern },
            };
            remap[index] = output.add(state, session)?;
        }
        let mut remapped = storage::repeat(false, self.states.len(), session)?;
        for &(empty, next) in empties.as_slice() {
            session.items(1)?;
            if remapped[empty.index()] {
                continue;
            }
            // The pinned compiler never emits a cycle made solely of gotos.
            // Every traversal is charged; malformed private state is not input.
            let mut target = next;
            while let Some(next) = self.goto(target, session)? {
                target = next;
            }
            let Some(&target) = remap.get(target.index()) else {
                return session.fail(Error::State);
            };
            remap[empty.index()] = target;
            remapped[empty.index()] = true;
            let mut cursor = next;
            while let Some(next) = self.goto(cursor, session)? {
                remap[cursor.index()] = target;
                remapped[cursor.index()] = true;
                cursor = next;
            }
        }
        output.remap(&remap, session)?;
        output.finish(session).map(Ok)
    }

    fn goto<A: Admission>(&self, id: Id, session: &mut Session<'_, A>) -> Step<Option<Id>> {
        session.items(1)?;
        let Some(state) = self.states.as_slice().get(id.index()) else {
            return session.fail(Error::State);
        };
        Ok(match state {
            State::Empty { next } => Some(*next),
            State::Union { alternates } | State::UnionReverse { alternates }
                if alternates.len() == 1 =>
            {
                Some(alternates.as_slice()[0])
            }
            _ => None,
        })
    }
}
