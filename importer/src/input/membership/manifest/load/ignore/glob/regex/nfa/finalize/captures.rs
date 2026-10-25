// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 GroupInfo::new, for emitted unnamed group zero.
use super::{storage, Admission, Buffer, Error, Session, Step};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) enum Failure {
    TooManyPatterns(usize),
    MissingGroups(u32),
    FirstNamed(u32),
    TooManyGroups { pattern: u32, groups: usize },
}

/// No named or explicit groups can be emitted. Preserve the ordered ownership
/// of the empty name maps with their exact storage layout. Their opaque bytes
/// are never read: the reached lookup is group-zero slot arithmetic only, so
/// unused randomized hash seeds cannot affect this emitted-grammar projection.
pub(in super::super) struct Groups {
    pub(super) ranges: Vec<(u32, u32)>,
    _name_maps: Vec<EmptyMap>,
    _names: Vec<Vec<Option<Arc<str>>>>,
    // GroupInfo's memory_extra; the shared Groups is admitted at its full size.
    #[allow(dead_code)]
    pub(super) memory_extra: usize,
}
struct EmptyMap {
    _storage: std::mem::MaybeUninit<std::collections::HashMap<Arc<str>, u32>>,
}

impl Groups {
    /// The closed emitter registers exactly the implicit group per pattern.
    pub(in super::super) fn slot_len(&self) -> usize {
        self.ranges.len() * 2
    }

    pub(super) fn empty<A: Admission>(session: &mut Session<'_, A>) -> Step<Arc<Self>> {
        storage::shared(
            Self {
                ranges: Vec::new(),
                _name_maps: Vec::new(),
                _names: Vec::new(),
                memory_extra: 0,
            },
            session,
        )
    }

    pub(in super::super) fn build<A: Admission>(
        captures: &[Buffer<Option<Arc<str>>>],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Arc<Self>, Failure>> {
        let mut ranges = Buffer::new();
        let mut maps = Buffer::new();
        let mut names = Buffer::new();
        let mut memory_extra = 0_usize;
        for (pattern, groups) in captures.iter().enumerate() {
            session.items(1)?;
            if pattern >= i32::MAX as usize {
                return Ok(Err(Failure::TooManyPatterns(pattern)));
            }
            let pattern = pattern as u32;
            match groups.as_slice().first() {
                None => return Ok(Err(Failure::MissingGroups(pattern))),
                Some(Some(_)) => return Ok(Err(Failure::FirstNamed(pattern))),
                Some(None) => {}
            }
            ranges.push((0, 0), session)?;
            maps.push(
                EmptyMap {
                    _storage: std::mem::MaybeUninit::uninit(),
                },
                session,
            )?;
            let mut first = storage::vector(1, session)?;
            session.items(1)?;
            first.push(None);
            names.push(first, session)?;
            memory_extra = match memory_extra.checked_add(std::mem::size_of::<Option<Arc<str>>>()) {
                Some(bytes) => bytes,
                None => return session.fail(Error::Overflow),
            };
            session.items(1)?;
            if groups.len() != 1 {
                // An internal producer-contract failure, never a glob rejection.
                return session.fail(Error::State);
            }
        }
        let Some(offset) = ranges.len().checked_mul(2) else {
            return session.fail(Error::Overflow);
        };
        for (pattern, range) in ranges.as_mut_slice().iter_mut().enumerate() {
            session.items(1)?;
            if offset >= i32::MAX as usize {
                return Ok(Err(Failure::TooManyGroups {
                    pattern: pattern as u32,
                    groups: 1,
                }));
            }
            *range = (offset as u32, offset as u32);
        }
        let ranges = storage::compact(ranges, session)?;
        let maps = storage::compact(maps, session)?;
        let names = storage::compact(names, session)?;
        storage::shared(
            Self {
                ranges,
                _name_maps: maps,
                _names: names,
                memory_extra,
            },
            session,
        )
        .map(Ok)
    }

    pub(in super::super) fn slot<A: Admission>(
        &self,
        pattern: u32,
        group: u32,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        if group != 0 || pattern as usize >= self.ranges.len() {
            return session.fail(Error::State);
        }
        Ok(pattern * 2)
    }
}
