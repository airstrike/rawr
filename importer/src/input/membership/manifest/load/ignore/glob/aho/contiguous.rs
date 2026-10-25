// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Default contiguous conversion from the completed, borrowed noncontiguous NFA.
//!
//! Correspondence: aho-corasick 1.1.4 `nfa/contiguous.rs:937–1007`
//! (build_from_noncontiguous) and `:486–511,686–819` (state encoding/remap).
use rawr_toml::document::Admission;

use super::{noncontiguous, prefilter, state_id, storage::Buffer, Trial};
use crate::input::membership::manifest::{session::Session, Error, Step};

mod query;

#[cfg(test)]
mod tests;

const DEAD: u32 = 0;
const FAIL: u32 = 1;
const DENSE: u32 = 0xFF;
const ONE: u32 = 0xFE;
const MAX_SPARSE: usize = 127;

pub(in super::super) struct Nfa {
    repr: Buffer<u32>,
    pattern_lens: Buffer<u32>,
    state_len: usize,
    prefilter: Option<prefilter::Prefilter>,
    alphabet_len: usize,
    classes: [u8; 256],
    minimum: usize,
    maximum: usize,
    special: noncontiguous::Special,
}

impl Nfa {
    /// The predecessor remains owned by the caller through success or failure.
    pub(super) fn build<A: Admission>(
        predecessor: &noncontiguous::Nfa,
        session: &mut Session<'_, A>,
    ) -> Trial<Self> {
        session.items(1)?;
        let classes = *predecessor.classes();
        let mut remap = Buffer::filled(predecessor.states().len(), DEAD, session)?;
        // Preserve the source's field construction and destruction order.
        let mut nfa = Self {
            repr: Buffer::new(),
            pattern_lens: Buffer::copy(predecessor.pattern_lens(), session)?,
            state_len: predecessor.states().len(),
            prefilter: predecessor
                .prefilter()
                .map(|prefilter| prefilter.try_clone(session))
                .transpose()?,
            alphabet_len: usize::from(classes[255]) + 1,
            classes,
            minimum: predecessor.minimum(),
            maximum: predecessor.maximum(),
            special: noncontiguous::Special::default(),
        };
        for (index, state) in predecessor.states().iter().enumerate() {
            session.items(1)?;
            if index == FAIL as usize {
                remap.slice_mut()[index] = FAIL;
                continue;
            }
            let id = write(
                predecessor,
                index as u32,
                state,
                &nfa.classes,
                &mut nfa.repr,
                state.depth() < 2,
                session,
            )?;
            remap.slice_mut()[index] = id;
        }
        for &id in remap.slice() {
            session.items(1)?;
            if id == FAIL {
                continue;
            }
            remap_state(
                nfa.alphabet_len,
                remap.slice(),
                &mut nfa.repr.slice_mut()[id as usize..],
                session,
            )?;
        }
        let old = predecessor.special();
        session.items(4)?;
        nfa.special.max_special = remap.slice()[old.max_special as usize];
        nfa.special.max_match = remap.slice()[old.max_match as usize];
        nfa.special.start = remap.slice()[old.start as usize];
        nfa.special.anchored = remap.slice()[old.anchored as usize];
        nfa.repr.compact(session)?;
        nfa.pattern_lens.compact(session)?;
        Ok(nfa)
    }

    // These borrowed views supply the later closed overlapping-query owner.
    // No alternate transition algorithm or generic Automaton is introduced.
    pub(super) fn repr(&self) -> &[u32] {
        self.repr.slice()
    }

    pub(super) fn pattern_lens(&self) -> &[u32] {
        self.pattern_lens.slice()
    }

    pub(super) fn state_len(&self) -> usize {
        self.state_len
    }

    pub(super) fn prefilter(&self) -> Option<&prefilter::Prefilter> {
        self.prefilter.as_ref()
    }

    pub(super) fn alphabet_len(&self) -> usize {
        self.alphabet_len
    }

    pub(super) fn classes(&self) -> &[u8; 256] {
        &self.classes
    }

    pub(super) fn minimum(&self) -> usize {
        self.minimum
    }

    pub(super) fn maximum(&self) -> usize {
        self.maximum
    }

    pub(super) fn special(&self) -> &noncontiguous::Special {
        &self.special
    }
}

fn write<A: Admission>(
    predecessor: &noncontiguous::Nfa,
    old_id: u32,
    old: &noncontiguous::State,
    classes: &[u8; 256],
    destination: &mut Buffer<u32>,
    force_dense: bool,
    session: &mut Session<'_, A>,
) -> Trial<u32> {
    // The source checks the starting offset, not each appended word or the
    // final representation length. Its semantic limit is SmallIndex::MAX.
    let id = state_id(destination.len())?;
    let mut count = 0;
    let mut transitions = predecessor.transitions(old_id);
    while transitions.next(session)?.is_some() {
        count += 1;
    }
    let kind = if force_dense || count > MAX_SPARSE {
        DENSE
    } else if count == 1 && !old.is_match() {
        ONE
    } else {
        count as u32
    };
    if kind == DENSE {
        destination.push(kind, session)?;
        destination.push(old.fail(), session)?;
        write_dense(predecessor, old_id, classes, destination, session)?;
    } else if kind == ONE {
        let Some(transition) = predecessor.transitions(old_id).next(session)? else {
            return session.fail(Error::State).map_err(Into::into);
        };
        let class = u32::from(classes[transition.byte() as usize]);
        destination.push(kind | (class << 8), session)?;
        destination.push(old.fail(), session)?;
        destination.push(transition.next(), session)?;
    } else {
        destination.push(kind, session)?;
        destination.push(old.fail(), session)?;
        write_sparse(predecessor, old_id, classes, destination, session)?;
    }
    if old.is_match() {
        let mut count = 0usize;
        let mut matches = predecessor.matches(old_id);
        while matches.next(session)?.is_some() {
            count += 1;
        }
        if count == 1 {
            let Some(pattern) = predecessor.matches(old_id).next(session)? else {
                return session.fail(Error::State).map_err(Into::into);
            };
            destination.push((1 << 31) | pattern, session)?;
        } else {
            destination.push(count as u32, session)?;
            let mut matches = predecessor.matches(old_id);
            while let Some(pattern) = matches.next(session)? {
                destination.push(pattern, session)?;
            }
        }
    }
    Ok(id)
}

fn write_sparse<A: Admission>(
    predecessor: &noncontiguous::Nfa,
    old_id: u32,
    classes: &[u8; 256],
    destination: &mut Buffer<u32>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let (mut chunk, mut length) = ([0; 4], 0);
    let mut transitions = predecessor.transitions(old_id);
    while let Some(transition) = transitions.next(session)? {
        chunk[length] = classes[transition.byte() as usize];
        length += 1;
        if length == 4 {
            destination.push(u32::from_ne_bytes(chunk), session)?;
            chunk = [0; 4];
            length = 0;
        }
    }
    if length != 0 {
        let repeat = chunk[length - 1];
        while length < 4 {
            session.items(1)?;
            chunk[length] = repeat;
            length += 1;
        }
        destination.push(u32::from_ne_bytes(chunk), session)?;
    }
    let mut transitions = predecessor.transitions(old_id);
    while let Some(transition) = transitions.next(session)? {
        destination.push(transition.next(), session)?;
    }
    Ok(())
}

fn write_dense<A: Admission>(
    predecessor: &noncontiguous::Nfa,
    old_id: u32,
    classes: &[u8; 256],
    destination: &mut Buffer<u32>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let start = destination.len();
    let alphabet = usize::from(classes[255]) + 1;
    destination.reserve(alphabet, session)?;
    for _ in 0..alphabet {
        destination.push(FAIL, session)?;
    }
    let mut transitions = predecessor.transitions(old_id);
    while let Some(transition) = transitions.next(session)? {
        destination.slice_mut()[start + usize::from(classes[transition.byte() as usize])] =
            transition.next();
    }
    Ok(())
}

fn remap_state<A: Admission>(
    alphabet: usize,
    remap: &[u32],
    state: &mut [u32],
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    let kind = state[0] & 0xFF;
    state[1] = remap[state[1] as usize];
    let (start, length) = if kind == DENSE {
        (2, alphabet)
    } else if kind == ONE {
        (2, 1)
    } else {
        let length = kind as usize;
        (2 + words(length), length)
    };
    for next in &mut state[start..][..length] {
        session.items(1)?;
        *next = remap[*next as usize];
    }
    Ok(())
}

fn words(transitions: usize) -> usize {
    (transitions >> 2) + usize::from(!transitions.is_multiple_of(4))
}
