// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Pinned default unanchored, Standard DFA conversion of the completed NFA.
use rawr_toml::document::Admission;

use super::{
    noncontiguous, prefilter, state_id,
    storage::{self, Buffer},
    Abort, Failure, Trial,
};
use crate::input::membership::manifest::{session::Session, Error, Step};

#[cfg(test)]
mod tests;

const DEAD: u32 = 0;
const FAIL: u32 = 1;

/// Owns the converted table and match rows; the input NFA stays borrowed.
pub(in super::super) struct Dfa {
    trans: Buffer<u32>,
    matches: Rows,
    matches_memory_usage: usize,
    pattern_lens: Buffer<u32>,
    prefilter: Option<prefilter::Prefilter>,
    state_len: usize,
    alphabet_len: usize,
    stride2: usize,
    classes: [u8; 256],
    minimum: usize,
    maximum: usize,
    special: noncontiguous::Special,
}

// The source allocates an exact outer Vec of empty Vecs before copying pattern
// lengths. Rows never grows after this allocation. Each inner Buffer retains
// its actual amortized capacity, as the source does not shrink inner match rows.
struct Rows(Vec<Buffer<u32>>);

impl Dfa {
    /// `dfa::Builder::build_from_noncontiguous`, default one-start branch.
    pub(super) fn build<A: Admission>(
        nfa: &noncontiguous::Nfa,
        session: &mut Session<'_, A>,
    ) -> Trial<Self> {
        session.items(256)?;
        let classes = *nfa.classes();
        let state_len = nfa.states().len();
        let alphabet_len = usize::from(classes[255]) + 1;
        let (trans_len, stride2) = geometry(state_len, alphabet_len)?;
        let num_match_states = (nfa.special().max_match as usize)
            .checked_sub(1)
            .expect("completed NFA includes its FAIL sentinel");
        let mut dfa = Self {
            trans: Buffer::filled(trans_len, DEAD, session)?,
            matches: Rows::new(num_match_states, session)?,
            matches_memory_usage: 0,
            pattern_lens: Buffer::copy(nfa.pattern_lens(), session)?,
            prefilter: nfa.prefilter().map(|p| p.try_clone(session)).transpose()?,
            state_len,
            alphabet_len,
            stride2,
            classes,
            minimum: nfa.minimum(),
            maximum: nfa.maximum(),
            special: noncontiguous::Special::default(),
        };
        dfa.finish(nfa, session)?;
        dfa.trans.compact(session)?;
        dfa.pattern_lens.compact(session)?;
        // Source matches.shrink_to_fit is a no-op: its outer length and
        // capacity are unchanged since the exact initial allocation.
        session.check()?;
        Ok(dfa)
    }

    fn finish<A: Admission>(
        &mut self,
        nfa: &noncontiguous::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        for (old, state) in nfa.states().iter().enumerate() {
            session.items(1)?;
            let new = (old << self.stride2) as u32;
            if state.is_match() {
                self.set_matches(nfa, old as u32, new, session)?;
            }
            self.sparse(nfa, old as u32, new, state.fail(), session)?;
        }
        session.items(4)?;
        let old = nfa.special();
        self.special.max_special = old.max_special << self.stride2;
        self.special.max_match = old.max_match << self.stride2;
        self.special.start = old.start << self.stride2;
        self.special.anchored = DEAD;
        Ok(())
    }

    fn set_matches<A: Admission>(
        &mut self,
        nfa: &noncontiguous::Nfa,
        old: u32,
        new: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let index = ((new as usize) >> self.stride2)
            .checked_sub(2)
            .expect("NFA match states follow DEAD and FAIL");
        let mut patterns = nfa.matches(old);
        let mut at_least_one = false;
        while let Some(pattern) = patterns.next(session)? {
            self.matches.0[index].push(pattern, session)?;
            self.matches_memory_usage += std::mem::size_of::<u32>();
            at_least_one = true;
        }
        assert!(at_least_one, "match state must have non-empty pids");
        Ok(())
    }

    /// Source sparse_iter order, with its single closed transition consumer.
    fn sparse<A: Admission>(
        &mut self,
        nfa: &noncontiguous::Nfa,
        old: u32,
        new: u32,
        fail: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut previous = None;
        let mut byte = 0usize;
        let mut transitions = nfa.transitions(old);
        while let Some(transition) = transitions.next(session)? {
            while byte < usize::from(transition.byte()) {
                session.items(1)?;
                let representative = byte as u8;
                let class = self.classes[byte];
                byte += 1;
                if previous != Some(class) {
                    self.transition(nfa, new, fail, representative, class, FAIL, session)?;
                    previous = Some(class);
                }
            }
            session.items(1)?;
            let representative = transition.byte();
            let class = self.classes[representative as usize];
            byte += 1;
            if previous != Some(class) {
                self.transition(
                    nfa,
                    new,
                    fail,
                    representative,
                    class,
                    transition.next(),
                    session,
                )?;
                previous = Some(class);
            }
        }
        for byte in byte..=255 {
            session.items(1)?;
            let representative = byte as u8;
            let class = self.classes[byte];
            if previous != Some(class) {
                self.transition(nfa, new, fail, representative, class, FAIL, session)?;
                previous = Some(class);
            }
        }
        Ok(())
    }

    fn transition<A: Admission>(
        &mut self,
        nfa: &noncontiguous::Nfa,
        state: u32,
        fail: u32,
        byte: u8,
        class: u8,
        mut next: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if next == FAIL {
            next = if fail == DEAD {
                DEAD
            } else {
                nfa.next_state(fail, byte, session)?
            };
        }
        self.trans.slice_mut()[state as usize + usize::from(class)] = next << self.stride2;
        Ok(())
    }

    pub(super) fn start(&self) -> u32 {
        self.special.start
    }
    pub(super) fn is_special(&self, state: u32) -> bool {
        state <= self.special.max_special
    }
    pub(super) fn is_dead(&self, state: u32) -> bool {
        state == DEAD
    }
    pub(super) fn is_match(&self, state: u32) -> bool {
        !self.is_dead(state) && state <= self.special.max_match
    }
    pub(super) fn is_start(&self, state: u32) -> bool {
        state == self.special.start || state == self.special.anchored
    }
    pub(super) fn patterns_len(&self) -> usize {
        self.pattern_lens.len()
    }
    pub(super) fn pattern_len(&self, pattern: u32) -> usize {
        self.pattern_lens.slice()[pattern as usize] as usize
    }
    pub(super) fn minimum(&self) -> usize {
        self.minimum
    }
    pub(super) fn maximum(&self) -> usize {
        self.maximum
    }
    pub(super) fn match_len(&self, state: u32) -> usize {
        debug_assert!(self.is_match(state));
        self.matches.0[((state as usize) >> self.stride2) - 2].len()
    }
    pub(super) fn match_pattern(&self, state: u32, index: usize) -> u32 {
        debug_assert!(self.is_match(state));
        self.matches.0[((state as usize) >> self.stride2) - 2].slice()[index]
    }
    pub(super) fn prefilter(&self) -> Option<&prefilter::Prefilter> {
        self.prefilter.as_ref()
    }

    /// One admitted table transition; IDs already contain their row stride.
    pub(super) fn next_state<A: Admission>(
        &self,
        state: u32,
        byte: u8,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let class = self.classes[byte as usize];
        Ok(self.trans.slice()[state as usize + usize::from(class)])
    }

    /// Source logical reporting, separate from actual Buffer layout admission.
    pub(super) fn memory_usage(&self) -> usize {
        self.trans.len() * std::mem::size_of::<u32>()
            + self.matches.0.len() * std::mem::size_of::<Vec<u32>>()
            + self.matches_memory_usage
            + self.pattern_lens.len() * std::mem::size_of::<u32>()
            + self.prefilter.as_ref().map_or(0, |p| p.memory_usage())
    }
}

impl Rows {
    fn new<A: Admission>(count: usize, session: &mut Session<'_, A>) -> Step<Self> {
        storage::admit::<Buffer<u32>, A>(count, session)?;
        let mut rows = Vec::new();
        if let Err(error) = rows.try_reserve_exact(count) {
            return session.fail(Error::Allocation(error));
        }
        session.items(count)?;
        for _ in 0..count {
            rows.push(Buffer::new());
        }
        Ok(Self(rows))
    }
}

// Source checks the final row's premultiplied ID, not the table's length.
// A completed NFA has at least four states, each bounded by SmallIndex::MAX.
fn geometry(states: usize, alphabet: usize) -> Trial<(usize, usize)> {
    let stride2 = alphabet.next_power_of_two().trailing_zeros() as usize;
    let Some(trans_len) = states.checked_shl(stride2 as u32) else {
        return Err(Abort::Semantic(Failure::StateId {
            attempted: usize::MAX,
        }));
    };
    let last = trans_len
        .checked_sub(1 << stride2)
        .expect("completed NFA has at least four states");
    state_id(last)?;
    Ok((trans_len, stride2))
}
