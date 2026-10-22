// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned dense::DFA shuffle and build_from_nfa finishing operations.
use super::super::{finalize::storage, storage::Array};
use super::{initial, Admission, Error, Session, Step};
use crate::input::membership::manifest::storage::Buffer;

mod accelerate;
mod ordered;
mod remap;
#[cfg(test)]
mod tests;

const MAX: usize = 0;
const QUIT: usize = 1;
const MIN_MATCH: usize = 2;
const MAX_MATCH: usize = 3;
const MIN_ACCEL: usize = 4;
const MAX_ACCEL: usize = 5;
const MIN_START: usize = 6;
const MAX_START: usize = 7;

pub(super) fn shuffle<A: Admission>(
    dense: &mut initial::Initial,
    matches: Buffer<(u32, Array<u32>)>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), initial::Failure>> {
    session.items(1)?;
    let stride = dense.stride() as u32;
    dense.special[QUIT] = stride;
    if dense.transitions.values.len() <= 2 * dense.stride() {
        set_max(dense);
        return Ok(Ok(()));
    }
    let mut matches = ordered::Map::from_sorted(matches);
    let mut starts = ordered::Map::new();
    for &id in &dense.starts.table.values {
        session.items(1)?;
        if id == 0 {
            continue;
        }
        if matches.contains(id, session)? {
            return session.fail(Error::State);
        }
        starts.insert(id, (), session)?;
    }
    let mut remap = remap::Remap::new(dense, session)?;
    if matches.len() == 0 {
        dense.special[MIN_MATCH] = 0;
        dense.special[MAX_MATCH] = 0;
    } else {
        let mut next = 2 * stride;
        let mut new_matches = ordered::Map::new();
        dense.special[MIN_MATCH] = next;
        for (id, patterns) in matches.into_iter() {
            session.items(1)?;
            remap.swap(dense, next, id, session)?;
            new_matches.insert(next, patterns, session)?;
            if starts.contains(next, session)? {
                starts.remove(next, session)?;
                starts.insert(id, (), session)?;
            }
            next += stride;
        }
        matches = new_matches;
        dense.special[MAX_MATCH] = dense.special[MIN_MATCH].max(next - stride);
    }
    let mut next = if dense.special[MIN_MATCH] == 0 {
        2 * stride
    } else {
        dense.special[MAX_MATCH] + stride
    };
    dense.special[MIN_START] = next;
    for (id, ()) in starts.into_iter() {
        session.items(1)?;
        remap.swap(dense, next, id, session)?;
        next += stride;
    }
    dense.special[MAX_START] = dense.special[MIN_START].max(next - stride);
    remap.apply(dense, session)?;
    if let Err(error) = set_patterns(dense, &matches, session)? {
        return Ok(Err(error));
    }
    set_max(dense);
    validate(dense, session)?;
    Ok(Ok(()))
}

/// Runner scratch has already dropped when this continuation is reached.
pub(super) fn rest<A: Admission>(
    dense: &mut initial::Initial,
    session: &mut Session<'_, A>,
) -> Step<()> {
    // Minimize is disabled; acceleration is enabled in the pinned defaults.
    accelerate::run(dense, session)?;
    session.items(1)?;
    // Core has no prefilter, so specialized start states are disabled.
    dense.special[MAX] = dense.special[QUIT]
        .max(dense.special[MAX_MATCH])
        .max(dense.special[MAX_ACCEL]);
    dense.special[MIN_START] = 0;
    dense.special[MAX_START] = 0;
    if !dense.starts.anchored_only {
        dense.starts.universal_unanchored = universal(&dense.starts.table.values[..6], session)?;
    }
    dense.starts.universal_anchored = universal(&dense.starts.table.values[6..12], session)?;
    // Match the four Vec::shrink_to_fit calls and their order. Accelerators
    // retain their growth extent. There is no additional source size check.
    compact(&mut dense.transitions, session)?;
    compact(&mut dense.starts.table, session)?;
    compact(&mut dense.matches.slices, session)?;
    compact(&mut dense.matches.patterns, session)?;
    Ok(())
}

fn universal<A: Admission>(group: &[u32], session: &mut Session<'_, A>) -> Step<Option<u32>> {
    session.items(1)?;
    let first = group[0];
    for &id in &group[1..] {
        session.items(1)?;
        if id != first {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

fn compact<A: Admission>(array: &mut Array<u32>, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if array.admitted > array.values.len() {
        let values = storage::copy(&array.values, session)?;
        *array = Array {
            admitted: values.len(),
            values,
        };
    }
    Ok(())
}

fn pattern_offset(length: usize) -> Result<u32, initial::Failure> {
    if length >= i32::MAX as usize {
        return Err(initial::Failure::MatchPatternIds);
    }
    Ok(length as u32)
}

fn set_patterns<A: Admission>(
    dense: &mut initial::Initial,
    map: &ordered::Map<Array<u32>>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), initial::Failure>> {
    let mut slices = Array::new();
    let mut patterns = Array::new();
    for (_, ids) in map.entries() {
        session.items(1)?;
        let start = match pattern_offset(patterns.values.len()) {
            Ok(start) => start,
            Err(error) => return Ok(Err(error)),
        };
        slices.push(start, session)?;
        slices.push(ids.values.len() as u32, session)?;
        for &id in &ids.values {
            patterns.push(id, session)?;
        }
    }
    dense.matches.slices = slices;
    dense.matches.patterns = patterns;
    Ok(Ok(()))
}

fn set_max(dense: &mut initial::Initial) {
    dense.special[MAX] = dense.special[QUIT]
        .max(dense.special[MAX_MATCH])
        .max(dense.special[MAX_ACCEL])
        .max(dense.special[MAX_START]);
}

fn within(id: u32, low: u32, high: u32) -> bool {
    id != 0 && low <= id && id <= high
}

fn validate<A: Admission>(dense: &initial::Initial, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    let s = &dense.special;
    for (low, high) in [
        (MIN_MATCH, MAX_MATCH),
        (MIN_ACCEL, MAX_ACCEL),
        (MIN_START, MAX_START),
    ] {
        session.items(1)?;
        if (s[low] == 0) != (s[high] == 0)
            || s[low] > s[high]
            || (s[low] != 0 && s[QUIT] >= s[low])
            || s[MAX] < s[high]
        {
            return session.fail(Error::State);
        }
    }
    for (left, right) in [
        (MIN_MATCH, MIN_ACCEL),
        (MIN_MATCH, MIN_START),
        (MIN_ACCEL, MIN_START),
    ] {
        session.items(1)?;
        if s[left] != 0 && s[right] != 0 && s[left] > s[right] {
            return session.fail(Error::State);
        }
    }
    if s[MAX] < s[QUIT]
        || s[MAX] as usize >> dense.stride2 >= dense.transitions.values.len() >> dense.stride2
    {
        return session.fail(Error::State);
    }
    Ok(())
}
