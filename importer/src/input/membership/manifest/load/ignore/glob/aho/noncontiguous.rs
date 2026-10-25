// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Complete default Standard, case-sensitive noncontiguous NFA construction.
use rawr_toml::document::Admission;

use super::{
    prefilter, state_id,
    storage::{Buffer, Queue},
    Abort, Failure, Trial, MAX,
};
use crate::input::membership::manifest::{session::Session, Step};

#[cfg(test)]
mod tests;

const DEAD: u32 = 0;
const FAIL: u32 = 1;

pub(in super::super) struct Nfa {
    states: Buffer<State>,
    sparse: Buffer<Transition>,
    dense: Buffer<u32>,
    matches: Buffer<Match>,
    pattern_lens: Buffer<u32>,
    prefilter: Option<prefilter::Prefilter>,
    classes: [u8; 256],
    minimum: usize,
    maximum: usize,
    special: Special,
}

pub(super) struct State {
    sparse: u32,
    dense: u32,
    matches: u32,
    fail: u32,
    depth: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub(super) struct Transition {
    byte: u8,
    next: u32,
    link: u32,
}
#[derive(Clone, Copy, Default)]
struct Match {
    pattern: u32,
    link: u32,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Special {
    pub(super) start: u32,
    pub(super) anchored: u32,
    pub(super) max_match: u32,
    pub(super) max_special: u32,
}

impl State {
    pub(super) fn is_match(&self) -> bool {
        self.matches != 0
    }
    pub(super) fn fail(&self) -> u32 {
        self.fail
    }
    pub(super) fn depth(&self) -> u32 {
        self.depth
    }
}
impl Transition {
    pub(super) fn byte(self) -> u8 {
        self.byte
    }
    pub(super) fn next(self) -> u32 {
        self.next
    }
}

pub(super) struct Transitions<'a> {
    nfa: &'a Nfa,
    link: u32,
}
impl Transitions<'_> {
    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Transition>> {
        session.items(1)?;
        if self.link == 0 {
            return Ok(None);
        }
        let transition = self.nfa.sparse.slice()[self.link as usize];
        self.link = transition.link;
        Ok(Some(transition))
    }
}
pub(super) struct Matches<'a> {
    nfa: &'a Nfa,
    link: u32,
}
impl Matches<'_> {
    pub(super) fn next<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<Option<u32>> {
        session.items(1)?;
        if self.link == 0 {
            return Ok(None);
        }
        let matched = self.nfa.matches.slice()[self.link as usize];
        self.link = matched.link;
        Ok(Some(matched.pattern))
    }
}

// The source Compiler drops its prefilter builder before its NFA on failure.
// Keeping those fields together also retains the builder's candidate needle
// through final NFA compaction on success.
struct Compiler {
    prefilter: prefilter::Builder,
    nfa: Nfa,
    boundaries: [u128; 2],
}

impl Nfa {
    pub(super) fn states(&self) -> &[State] {
        self.states.slice()
    }
    pub(super) fn classes(&self) -> &[u8; 256] {
        &self.classes
    }
    pub(super) fn pattern_lens(&self) -> &[u32] {
        self.pattern_lens.slice()
    }
    pub(super) fn minimum(&self) -> usize {
        self.minimum
    }
    pub(super) fn maximum(&self) -> usize {
        self.maximum
    }
    pub(super) fn special(&self) -> &Special {
        &self.special
    }
    pub(super) fn prefilter(&self) -> Option<&prefilter::Prefilter> {
        self.prefilter.as_ref()
    }
    pub(super) fn transitions(&self, state: u32) -> Transitions<'_> {
        Transitions {
            nfa: self,
            link: self.states.slice()[state as usize].sparse,
        }
    }
    pub(super) fn matches(&self, state: u32) -> Matches<'_> {
        Matches {
            nfa: self,
            link: self.states.slice()[state as usize].matches,
        }
    }

    /// Pinned unanchored next_state, used by the default DFA conversion.
    pub(super) fn next_state<A: Admission>(
        &self,
        mut state: u32,
        byte: u8,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        loop {
            let next = self.follow(state, byte, session)?;
            if next != FAIL {
                return Ok(next);
            }
            state = self.states.slice()[state as usize].fail;
        }
    }

    pub(super) fn build<A: Admission>(
        patterns: &[String],
        session: &mut Session<'_, A>,
    ) -> Trial<Self> {
        // Compiler::new constructs this builder before any NFA storage.
        let prefilter = prefilter::Builder::new(session)?;
        let mut classes = [0; 256];
        for (byte, class) in classes.iter_mut().enumerate() {
            session.items(1)?;
            *class = byte as u8;
        }
        let nfa = Self {
            states: Buffer::new(),
            sparse: Buffer::new(),
            dense: Buffer::new(),
            matches: Buffer::new(),
            pattern_lens: Buffer::new(),
            prefilter: None,
            classes,
            minimum: usize::MAX,
            maximum: 0,
            special: Special::default(),
        };
        let mut compiler = Compiler {
            prefilter,
            nfa,
            boundaries: [0; 2],
        };
        let Compiler {
            nfa,
            prefilter,
            boundaries,
        } = &mut compiler;
        nfa.sparse.push(Transition::default(), session)?;
        nfa.matches.push(Match::default(), session)?;
        nfa.dense.push(DEAD, session)?;
        nfa.alloc_state(0, session)?;
        nfa.alloc_state(0, session)?;
        nfa.special.start = nfa.alloc_state(0, session)?;
        nfa.special.anchored = nfa.alloc_state(0, session)?;
        nfa.init_full(nfa.special.start, FAIL, session)?;
        nfa.init_full(nfa.special.anchored, FAIL, session)?;
        nfa.init_full(DEAD, DEAD, session)?;
        nfa.trie(patterns, prefilter, boundaries, session)?;
        nfa.states.compact(session)?;
        let mut class = 0;
        for byte in 0..256 {
            session.items(1)?;
            nfa.classes[byte] = class;
            if byte != 255 && boundaries[byte / 128] & (1u128 << (byte % 128)) != 0 {
                class += 1;
            }
        }
        nfa.anchored_start(session)?;
        let mut link = nfa.states.slice()[nfa.special.start as usize].sparse;
        while link != 0 {
            session.items(1)?;
            let transition = &mut nfa.sparse.slice_mut()[link as usize];
            if transition.next == FAIL {
                transition.next = nfa.special.start;
            }
            link = transition.link;
        }
        nfa.densify(session)?;
        nfa.failures(session)?;
        // close_start_state_loop_for_leftmost is inert for Standard.
        nfa.shuffle(session)?;
        nfa.prefilter = prefilter.build(session)?;
        nfa.special.max_special = if nfa.prefilter.is_some() {
            nfa.special.anchored
        } else {
            nfa.special.max_match
        };
        nfa.sparse.compact(session)?;
        nfa.dense.compact(session)?;
        nfa.matches.compact(session)?;
        nfa.pattern_lens.compact(session)?;
        Ok(compiler.nfa)
    }

    fn trie<A: Admission>(
        &mut self,
        patterns: &[String],
        prefilter: &mut prefilter::Builder,
        boundaries: &mut [u128; 2],
        session: &mut Session<'_, A>,
    ) -> Trial<()> {
        for (index, pattern) in patterns.iter().enumerate() {
            session.items(1)?;
            let (pid, length) = pattern_fields(index, pattern.len())?;
            session.string(pattern.len())?;
            self.minimum = self.minimum.min(pattern.len());
            self.maximum = self.maximum.max(pattern.len());
            self.pattern_lens.push(length, session)?;
            prefilter.add(pattern.as_bytes(), session)?;
            let mut previous = self.special.start;
            for (depth, byte) in pattern.bytes().enumerate() {
                session.items(1)?;
                if byte > 0 {
                    let end = byte as usize - 1;
                    boundaries[end / 128] |= 1u128 << (end % 128);
                }
                boundaries[byte as usize / 128] |= 1u128 << (byte as usize % 128);
                let next = self.follow(previous, byte, session)?;
                if next != FAIL {
                    previous = next;
                } else {
                    let next = self.alloc_state(depth, session)?;
                    self.add_transition(previous, byte, next, session)?;
                    previous = next;
                }
            }
            self.add_match(previous, pid, session)?;
        }
        Ok(())
    }

    fn alloc_state<A: Admission>(
        &mut self,
        depth: usize,
        session: &mut Session<'_, A>,
    ) -> Trial<u32> {
        let id = state_id(self.states.len())?;
        self.states.push(
            State {
                sparse: 0,
                dense: 0,
                matches: 0,
                fail: self.special.start,
                depth: depth as u32,
            },
            session,
        )?;
        Ok(id)
    }

    fn alloc_transition<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<u32> {
        let id = state_id(self.sparse.len())?;
        self.sparse.push(Transition::default(), session)?;
        Ok(id)
    }

    fn init_full<A: Admission>(
        &mut self,
        state: u32,
        next: u32,
        session: &mut Session<'_, A>,
    ) -> Trial<()> {
        let mut previous = 0;
        for byte in 0..=255 {
            session.items(1)?;
            let link = self.alloc_transition(session)?;
            self.sparse.slice_mut()[link as usize] = Transition {
                byte,
                next,
                link: 0,
            };
            if previous == 0 {
                self.states.slice_mut()[state as usize].sparse = link;
            } else {
                self.sparse.slice_mut()[previous as usize].link = link;
            }
            previous = link;
        }
        Ok(())
    }

    fn add_transition<A: Admission>(
        &mut self,
        state: u32,
        byte: u8,
        next: u32,
        session: &mut Session<'_, A>,
    ) -> Trial<()> {
        let head = self.states.slice()[state as usize].sparse;
        if head == 0 || byte < self.sparse.slice()[head as usize].byte {
            let link = self.alloc_transition(session)?;
            self.sparse.slice_mut()[link as usize] = Transition {
                byte,
                next,
                link: head,
            };
            self.states.slice_mut()[state as usize].sparse = link;
            return Ok(());
        }
        if byte == self.sparse.slice()[head as usize].byte {
            self.sparse.slice_mut()[head as usize].next = next;
            return Ok(());
        }
        let mut previous = head;
        let mut following = self.sparse.slice()[head as usize].link;
        while following != 0 && byte > self.sparse.slice()[following as usize].byte {
            session.items(1)?;
            previous = following;
            following = self.sparse.slice()[following as usize].link;
        }
        if following == 0 || byte < self.sparse.slice()[following as usize].byte {
            let link = self.alloc_transition(session)?;
            self.sparse.slice_mut()[link as usize] = Transition {
                byte,
                next,
                link: following,
            };
            self.sparse.slice_mut()[previous as usize].link = link;
        } else {
            self.sparse.slice_mut()[following as usize].next = next;
        }
        Ok(())
    }

    fn follow<A: Admission>(
        &self,
        state: u32,
        byte: u8,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let state = &self.states.slice()[state as usize];
        if state.dense != 0 {
            return Ok(
                self.dense.slice()[state.dense as usize + self.classes[byte as usize] as usize]
            );
        }
        let mut link = state.sparse;
        while link != 0 {
            session.items(1)?;
            let transition = self.sparse.slice()[link as usize];
            if byte <= transition.byte {
                return Ok(if byte == transition.byte {
                    transition.next
                } else {
                    FAIL
                });
            }
            link = transition.link;
        }
        Ok(FAIL)
    }

    fn add_match<A: Admission>(
        &mut self,
        state: u32,
        pattern: u32,
        session: &mut Session<'_, A>,
    ) -> Trial<()> {
        let mut link = self.states.slice()[state as usize].matches;
        while self.matches.slice()[link as usize].link != 0 {
            session.items(1)?;
            link = self.matches.slice()[link as usize].link;
        }
        let next = state_id(self.matches.len())?;
        self.matches.push(Match::default(), session)?;
        self.matches.slice_mut()[next as usize].pattern = pattern;
        if link == 0 {
            self.states.slice_mut()[state as usize].matches = next;
        } else {
            self.matches.slice_mut()[link as usize].link = next;
        }
        Ok(())
    }

    fn copy_matches<A: Admission>(
        &mut self,
        source: u32,
        destination: u32,
        session: &mut Session<'_, A>,
    ) -> Trial<()> {
        session.items(1)?;
        let mut destination_link = self.states.slice()[destination as usize].matches;
        while self.matches.slice()[destination_link as usize].link != 0 {
            session.items(1)?;
            destination_link = self.matches.slice()[destination_link as usize].link;
        }
        let mut source_link = self.states.slice()[source as usize].matches;
        while source_link != 0 {
            session.items(1)?;
            let link = state_id(self.matches.len())?;
            self.matches.push(
                Match {
                    pattern: self.matches.slice()[source_link as usize].pattern,
                    link: 0,
                },
                session,
            )?;
            if destination_link == 0 {
                self.states.slice_mut()[destination as usize].matches = link;
            } else {
                self.matches.slice_mut()[destination_link as usize].link = link;
            }
            destination_link = link;
            source_link = self.matches.slice()[source_link as usize].link;
        }
        Ok(())
    }

    fn anchored_start<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let mut source = self.states.slice()[self.special.start as usize].sparse;
        let mut destination = self.states.slice()[self.special.anchored as usize].sparse;
        while source != 0 {
            session.items(1)?;
            let transition = self.sparse.slice()[source as usize];
            self.sparse.slice_mut()[destination as usize].next = transition.next;
            source = transition.link;
            destination = self.sparse.slice()[destination as usize].link;
        }
        self.copy_matches(self.special.start, self.special.anchored, session)?;
        self.states.slice_mut()[self.special.anchored as usize].fail = DEAD;
        Ok(())
    }

    fn densify<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let alphabet = self.classes[255] as usize + 1;
        for state in 0..self.states.len() {
            session.items(1)?;
            if state <= FAIL as usize || self.states.slice()[state].depth >= 3 {
                continue;
            }
            let dense = state_id(self.dense.len())?;
            self.dense.reserve(alphabet, session)?;
            for _ in 0..alphabet {
                self.dense.push(FAIL, session)?;
            }
            let mut link = self.states.slice()[state].sparse;
            while link != 0 {
                session.items(1)?;
                let transition = self.sparse.slice()[link as usize];
                let index = dense as usize + self.classes[transition.byte as usize] as usize;
                self.dense.slice_mut()[index] = transition.next;
                link = transition.link;
            }
            self.states.slice_mut()[state].dense = dense;
        }
        Ok(())
    }

    fn failures<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let mut queue = Queue::new();
        // queued_set is inert when ascii_case_insensitive=false; every trie
        // edge therefore has one unique child and no BTreeSet is constructed.
        let mut link = self.states.slice()[self.special.start as usize].sparse;
        while link != 0 {
            session.items(1)?;
            let transition = self.sparse.slice()[link as usize];
            link = transition.link;
            if transition.next != self.special.start {
                queue.push(transition.next, session)?;
            }
        }
        while let Some(state) = queue.pop(session)? {
            let mut link = self.states.slice()[state as usize].sparse;
            while link != 0 {
                session.items(1)?;
                let transition = self.sparse.slice()[link as usize];
                link = transition.link;
                queue.push(transition.next, session)?;
                let mut fail = self.states.slice()[state as usize].fail;
                while self.follow(fail, transition.byte, session)? == FAIL {
                    fail = self.states.slice()[fail as usize].fail;
                }
                fail = self.follow(fail, transition.byte, session)?;
                self.states.slice_mut()[transition.next as usize].fail = fail;
                self.copy_matches(fail, transition.next, session)?;
            }
            self.copy_matches(self.special.start, state, session)?;
        }
        Ok(())
    }

    fn shuffle<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        let mut map = Buffer::indices(self.states.len(), session)?;
        let mut next = 4;
        for index in 4..self.states.len() {
            session.items(1)?;
            if self.states.slice()[index].matches == 0 {
                continue;
            }
            self.states.slice_mut().swap(index, next);
            map.slice_mut().swap(index, next);
            next += 1;
        }
        let anchored = next - 1;
        self.states.slice_mut().swap(3, anchored);
        map.slice_mut().swap(3, anchored);
        let start = next - 2;
        self.states.slice_mut().swap(2, start);
        map.slice_mut().swap(2, start);
        self.special.max_match = (next - 3) as u32;
        self.special.start = start as u32;
        self.special.anchored = anchored as u32;
        if self.states.slice()[anchored].matches != 0 {
            self.special.max_match = anchored as u32;
        }
        let oldmap = Buffer::copy(map.slice(), session)?;
        for index in 0..self.states.len() {
            session.items(1)?;
            let mut new = oldmap.slice()[index];
            if new as usize == index {
                continue;
            }
            loop {
                session.items(1)?;
                let id = oldmap.slice()[new as usize];
                if id as usize == index {
                    map.slice_mut()[index] = new;
                    break;
                }
                new = id;
            }
        }
        let alphabet = self.classes[255] as usize + 1;
        for state in self.states.slice_mut() {
            session.items(1)?;
            state.fail = map.slice()[state.fail as usize];
            let mut link = state.sparse;
            while link != 0 {
                session.items(1)?;
                let transition = &mut self.sparse.slice_mut()[link as usize];
                transition.next = map.slice()[transition.next as usize];
                link = transition.link;
            }
            if state.dense != 0 {
                for next in &mut self.dense.slice_mut()[state.dense as usize..][..alphabet] {
                    session.items(1)?;
                    *next = map.slice()[*next as usize];
                }
            }
        }
        Ok(())
    }
}

fn pattern_fields(index: usize, length: usize) -> Trial<(u32, u32)> {
    if index > MAX {
        return Err(Abort::Semantic(Failure::PatternId { attempted: index }));
    }
    if length > MAX {
        return Err(Abort::Semantic(Failure::PatternLength {
            pattern: index as u32,
            length,
        }));
    }
    Ok((index as u32, length as u32))
}
