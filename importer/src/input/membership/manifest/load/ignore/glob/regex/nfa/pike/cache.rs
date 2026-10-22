// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! PikeVMCache::get/reset, Cache::new/reset and ActiveStates/SlotTable storage.
use super::super::builder::Id;
use super::{storage::Array, Admission, Error, Pike, Session, Step};
use std::num::NonZeroUsize;

pub(in super::super) struct Lazy(pub(super) Option<Cache>);
pub(super) struct Cache {
    // The source NonMaxUsize wraps NonZeroUsize. No offsets are interpreted by
    // this lifecycle slice; using its representation keeps optional-slot size.
    pub(super) stack: Array<Frame>,
    pub(super) curr: Active,
    pub(super) next: Active,
}
#[derive(Clone, Copy)]
pub(super) enum Frame {
    Explore(Id),
    Restore {
        slot: u32,
        offset: Option<NonZeroUsize>,
    },
}
pub(super) struct Active {
    pub(super) length: usize,
    pub(super) dense: Array<Id>,
    pub(super) sparse: Array<Id>,
    pub(super) slots: Slots,
}
pub(super) struct Slots {
    pub(super) table: Array<Option<NonZeroUsize>>,
    pub(super) per_state: usize,
    pub(super) for_captures: usize,
}

impl Lazy {
    pub(super) fn none() -> Self {
        Self(None)
    }

    pub(super) fn get<A: Admission>(
        &mut self,
        engine: &Pike,
        session: &mut Session<'_, A>,
    ) -> Step<&mut Cache> {
        session.items(1)?;
        if self.0.is_none() {
            // Publish only after both ActiveStates finish construction.
            self.0 = Some(Cache::new(engine, session)?);
        }
        match self.0.as_mut() {
            Some(cache) => Ok(cache),
            None => session.fail(Error::State),
        }
    }

    pub(in super::super) fn reset<A: Admission>(
        &mut self,
        engine: &Pike,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        // The source initializes a previously empty wrapper, then resets it.
        self.get(engine, session)?.reset(engine, session)
    }
}

impl Cache {
    fn new<A: Admission>(engine: &Pike, session: &mut Session<'_, A>) -> Step<Self> {
        let stack = Array::new();
        let curr = Active::new(engine, session)?;
        let next = Active::new(engine, session)?;
        Ok(Self { stack, curr, next })
    }

    fn reset<A: Admission>(&mut self, engine: &Pike, session: &mut Session<'_, A>) -> Step<()> {
        self.curr.reset(engine, session)?;
        self.next.reset(engine, session)
        // Cache::reset does not clear the epsilon stack. Search setup will.
    }
}

impl Active {
    fn new<A: Admission>(engine: &Pike, session: &mut Session<'_, A>) -> Step<Self> {
        let mut active = Self {
            length: 0,
            dense: Array::new(),
            sparse: Array::new(),
            slots: Slots {
                table: Array::new(),
                per_state: 0,
                for_captures: 0,
            },
        };
        // SparseSet::new(0) reaches resize(0) before ActiveStates::reset.
        active.resize_set(0, session)?;
        active.reset(engine, session)?;
        Ok(active)
    }

    fn resize_set<A: Admission>(
        &mut self,
        length: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if length > i32::MAX as usize {
            return session.fail(Error::State);
        }
        self.length = 0;
        self.dense.resize(length, Id::ZERO, session)?;
        self.sparse.resize(length, Id::ZERO, session)
    }

    fn reset<A: Admission>(&mut self, engine: &Pike, session: &mut Session<'_, A>) -> Step<()> {
        self.resize_set(engine.nfa.0.states.len(), session)?;
        self.slots.reset(engine, session)
    }
}

impl Slots {
    fn reset<A: Admission>(&mut self, engine: &Pike, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        self.per_state = engine.nfa.0.groups.slot_len();
        let Some(implicit) = engine.nfa.0.starts.len().checked_mul(2) else {
            return session.fail(Error::Overflow);
        };
        self.for_captures = self.per_state.max(implicit);
        let Some(length) = engine
            .nfa
            .0
            .states
            .len()
            .checked_mul(self.per_state)
            .and_then(|length| length.checked_add(self.for_captures))
        else {
            return session.fail(Error::Overflow);
        };
        self.table.resize(length, None, session)
    }
}
