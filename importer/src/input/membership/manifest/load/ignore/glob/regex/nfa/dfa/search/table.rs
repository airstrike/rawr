// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Dense Automaton accessors and the pinned accel::find_fwd helper boundary.
use super::{Admission, Error, Failure, Initial, Session, Step};

#[cfg(test)]
std::thread_local! {
    // Observe entry to the actual helper dispatch in refusal tests only.
    static HELPER_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;

const MAX: usize = 0;
const MIN_MATCH: usize = 2;
const MAX_MATCH: usize = 3;
const MIN_ACCEL: usize = 4;
const MAX_ACCEL: usize = 5;
const MIN_START: usize = 6;
const MAX_START: usize = 7;

pub(super) fn special(dense: &Initial, id: u32) -> bool {
    id <= dense.special[MAX]
}

fn within(dense: &Initial, id: u32, minimum: usize, maximum: usize) -> bool {
    dense.special[minimum] != 0 && dense.special[minimum] <= id && id <= dense.special[maximum]
}

pub(super) fn matched(dense: &Initial, id: u32) -> bool {
    within(dense, id, MIN_MATCH, MAX_MATCH)
}

pub(super) fn accelerated(dense: &Initial, id: u32) -> bool {
    within(dense, id, MIN_ACCEL, MAX_ACCEL)
}

pub(super) fn start_state(dense: &Initial, id: u32) -> bool {
    within(dense, id, MIN_START, MAX_START)
}

pub(super) fn start<A: Admission>(
    dense: &Initial,
    session: &mut Session<'_, A>,
) -> Step<Result<u32, Failure>> {
    session.items(1)?;
    if !dense.has_closed_query_config() {
        return session.fail(Error::State);
    }
    if dense.starts.anchored_only {
        return Ok(Err(Failure::UnsupportedAnchored));
    }
    // Input::new(bytes): no look-behind, unanchored group + Start::Text.
    // The universal-start cache is a prefilter restart optimization, not a
    // replacement for start_state_forward's source table lookup.
    let Some(&id) = dense.starts.table.values.get(2) else {
        return session.fail(Error::State);
    };
    if matched(dense, id) {
        return session.fail(Error::State);
    }
    Ok(Ok(id))
}

pub(super) fn next<A: Admission>(
    dense: &Initial,
    id: u32,
    byte: Option<u8>,
    session: &mut Session<'_, A>,
) -> Step<u32> {
    session.items(1)?;
    let class = match byte {
        Some(byte) => usize::from(dense.classes[usize::from(byte)]),
        None => usize::from(dense.classes[255]) + 1,
    };
    let Some(index) = (id as usize).checked_add(class) else {
        return session.fail(Error::Overflow);
    };
    match dense.transitions.values.get(index) {
        Some(&next) => Ok(next),
        None => session.fail(Error::State),
    }
}

fn index<A: Admission>(dense: &Initial, id: u32, session: &mut Session<'_, A>) -> Step<usize> {
    session.items(1)?;
    if !matched(dense, id) {
        return session.fail(Error::State);
    }
    let state = ((id - dense.special[MIN_MATCH]) as usize) >> dense.stride2;
    match state.checked_mul(2) {
        Some(index) => Ok(index),
        None => session.fail(Error::Overflow),
    }
}

pub(super) fn match_len<A: Admission>(
    dense: &Initial,
    id: u32,
    session: &mut Session<'_, A>,
) -> Step<usize> {
    let index = index(dense, id, session)?;
    match dense.matches.slices.values.get(index + 1) {
        Some(&length) => Ok(length as usize),
        None => session.fail(Error::State),
    }
}

pub(super) fn pattern<A: Admission>(
    dense: &Initial,
    id: u32,
    which: usize,
    session: &mut Session<'_, A>,
) -> Step<u32> {
    let index = index(dense, id, session)?;
    let Some(&offset) = dense.matches.slices.values.get(index) else {
        return session.fail(Error::State);
    };
    let Some(&length) = dense.matches.slices.values.get(index + 1) else {
        return session.fail(Error::State);
    };
    if which >= length as usize {
        return session.fail(Error::State);
    }
    let Some(position) = (offset as usize).checked_add(which) else {
        return session.fail(Error::Overflow);
    };
    match dense.matches.patterns.values.get(position) {
        Some(&pattern) => Ok(pattern),
        None => session.fail(Error::State),
    }
}

pub(super) fn accelerate<A: Admission>(
    dense: &Initial,
    id: u32,
    bytes: &[u8],
    at: usize,
    session: &mut Session<'_, A>,
) -> Step<usize> {
    // One logical accelerator-search event, before the actual safe helper.
    // Its finite internal lane bound is proof, not another byte-work debit.
    session.items(1)?;
    if !accelerated(dense, id) {
        return session.fail(Error::State);
    }
    let index = ((id - dense.special[MIN_ACCEL]) as usize) >> dense.stride2;
    let Some(&count) = dense.accelerators.values.first() else {
        return session.fail(Error::State);
    };
    if index >= count as usize {
        return session.fail(Error::State);
    }
    let Some(position) = index.checked_mul(2).and_then(|index| index.checked_add(1)) else {
        return session.fail(Error::Overflow);
    };
    let Some(word) = dense.accelerators.values.get(position) else {
        return session.fail(Error::State);
    };
    let encoded = word.to_ne_bytes();
    let length = usize::from(encoded[0]);
    if !(1..=3).contains(&length) {
        return session.fail(Error::State);
    }
    // Accel has at most three needles. Read its packed native-endian word
    // without temporary heap storage, then call the selected pinned helper.
    let Some(suffix) = bytes.get(at..) else {
        return session.fail(Error::State);
    };
    #[cfg(test)]
    HELPER_CALLS.with(|calls| calls.set(calls.get() + 1));
    let found = match length {
        1 => memchr::memchr(encoded[1], suffix),
        2 => memchr::memchr2(encoded[1], encoded[2], suffix),
        3 => memchr::memchr3(encoded[1], encoded[2], encoded[3], suffix),
        _ => return session.fail(Error::State),
    };
    Ok(found.map_or(bytes.len(), |offset| at + offset))
}
