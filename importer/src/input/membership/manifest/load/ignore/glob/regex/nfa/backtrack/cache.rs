// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned lazy Cache ownership, reset, and per-search visited-bitset setup.
use super::super::{builder::Id, storage::Array};
use super::{Admission, Backtrack, Engine, Error, Session, Step};
use std::num::NonZeroUsize;

pub(in super::super) struct Lazy(pub(super) Option<Cache>);
pub(super) struct Cache {
    pub(super) stack: Array<Frame>,
    pub(super) visited: Visited,
}

#[derive(Clone, Copy)]
pub(super) enum Frame {
    Step {
        sid: Id,
        at: usize,
    },
    RestoreCapture {
        slot: u32,
        offset: Option<NonZeroUsize>,
    },
}

pub(super) struct Visited {
    pub(super) bitset: Array<usize>,
    pub(super) stride: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TooLong {
    pub(super) length: usize,
}

impl Lazy {
    pub(super) fn none() -> Self {
        Self(None)
    }

    pub(super) fn get<A: Admission>(
        &mut self,
        engine: &Engine,
        session: &mut Session<'_, A>,
    ) -> Step<&mut Cache> {
        session.items(1)?;
        if self.0.is_none() {
            self.0 = Some(Cache::new(engine, session)?);
        }
        match self.0.as_mut() {
            Some(cache) => Ok(cache),
            None => session.fail(Error::State),
        }
    }

    pub(in super::super) fn reset<A: Admission>(
        &mut self,
        wrapper: &Backtrack,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if let Some(engine) = wrapper.0.as_ref() {
            // As upstream, resetting an empty lazy cache first initializes it.
            self.get(engine, session)?.reset(engine, session)?;
        }
        Ok(())
    }
}

impl Cache {
    fn new<A: Admission>(engine: &Engine, session: &mut Session<'_, A>) -> Step<Self> {
        let stack = Array::new();
        let mut visited = Visited {
            bitset: Array::new(),
            stride: 0,
        };
        visited.reset(engine, session)?;
        Ok(Self { stack, visited })
    }

    fn reset<A: Admission>(&mut self, engine: &Engine, session: &mut Session<'_, A>) -> Step<()> {
        self.visited.reset(engine, session)
    }

    pub(super) fn setup<A: Admission>(
        &mut self,
        engine: &Engine,
        span_len: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), TooLong>> {
        session.items(self.stack.values.len())?;
        self.stack.values.clear();
        self.visited.setup(engine, span_len, session)
    }
}

impl Visited {
    pub(super) const BLOCK_SIZE: usize = usize::BITS as usize;

    fn reset<A: Admission>(&mut self, _: &Engine, session: &mut Session<'_, A>) -> Step<()> {
        session.items(self.bitset.values.len())?;
        self.bitset.values.clear();
        // Source reset keeps stride and allocations, and never clears stack.
        Ok(())
    }

    fn setup<A: Admission>(
        &mut self,
        engine: &Engine,
        span_len: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), TooLong>> {
        session.items(1)?;
        self.stride = match span_len.checked_add(1) {
            Some(stride) => stride,
            None => return session.fail(Error::Overflow),
        };
        let Some(needed) = engine.nfa.0.states.len().checked_mul(self.stride) else {
            return Ok(Err(TooLong { length: span_len }));
        };
        let Some(maximum) = engine.visited_capacity().checked_mul(8) else {
            return session.fail(Error::Overflow);
        };
        if needed > maximum {
            return Ok(Err(TooLong { length: span_len }));
        }
        let blocks = needed.div_ceil(Self::BLOCK_SIZE);
        session.items(self.bitset.values.len().saturating_sub(blocks))?;
        self.bitset.values.truncate(blocks);
        // The source clears retained blocks before a possible resize/allocation.
        session.items(self.bitset.values.len())?;
        self.bitset.values.fill(0);
        if blocks > self.bitset.values.len() {
            self.bitset.resize(blocks, 0, session)?;
        }
        Ok(Ok(()))
    }

    pub(super) fn insert<A: Admission>(
        &mut self,
        sid: Id,
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if at >= self.stride {
            return session.fail(Error::State);
        }
        let Some(index) = sid
            .index()
            .checked_mul(self.stride)
            .and_then(|index| index.checked_add(at))
        else {
            return session.fail(Error::Overflow);
        };
        let Some(block) = self.bitset.values.get_mut(index / Self::BLOCK_SIZE) else {
            return session.fail(Error::State);
        };
        let bit = 1 << (index % Self::BLOCK_SIZE);
        if *block & bit != 0 {
            return Ok(false);
        }
        *block |= bit;
        Ok(true)
    }
}
