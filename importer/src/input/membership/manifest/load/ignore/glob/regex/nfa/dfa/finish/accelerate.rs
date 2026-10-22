// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! dense::DFA::accelerate and State::accelerate, in source operation order.
use super::{
    initial, ordered, remap, set_max, set_patterns, validate, within, Admission, Array, Error,
    Session, Step, MAX_ACCEL, MAX_MATCH, MAX_START, MIN_ACCEL, MIN_MATCH, MIN_START, QUIT,
};

pub(super) fn run<A: Admission>(
    dense: &mut initial::Initial,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    let stride = dense.stride() as u32;
    let states = dense.transitions.values.len() >> dense.stride2;
    if states <= 2 {
        return Ok(());
    }
    let mut accels = ordered::Map::new();
    let (mut cmatch, mut cstart, mut cnormal) = (0, 0, 0);
    for index in 0..states {
        session.items(1)?;
        let id = (index << dense.stride2) as u32;
        if let Some(accel) = state(dense, id, session)? {
            accels.insert(id, accel, session)?;
            if within(id, dense.special[MIN_MATCH], dense.special[MAX_MATCH]) {
                cmatch += 1;
            } else if within(id, dense.special[MIN_START], dense.special[MAX_START]) {
                cstart += 1;
            } else {
                if id == 0 || id == dense.special[QUIT] {
                    return session.fail(Error::State);
                }
                cnormal += 1;
            }
        }
    }
    if accels.len() == 0 {
        return Ok(());
    }
    let original_len = accels.len();
    let mut remap = remap::Remap::new(dense, session)?;
    let mut matches = pattern_map(dense, session)?;
    dense.special[MIN_ACCEL] = i32::MAX as u32 - 1;
    dense.special[MAX_ACCEL] = 0;
    if cmatch > 0 && dense.special[MIN_MATCH] != 0 {
        let mut next = dense.special[MAX_MATCH];
        let mut current = next;
        while current >= dense.special[MIN_MATCH] {
            session.items(1)?;
            if let Some(accel) = accels.remove(current, session)? {
                accels.insert(next, accel, session)?;
                update(dense, next);
                if current != next {
                    remap.swap(dense, current, next, session)?;
                    let Some(current_patterns) = matches.remove(current, session)? else {
                        return session.fail(Error::State);
                    };
                    let Some(next_patterns) = matches.remove(next, session)? else {
                        return session.fail(Error::State);
                    };
                    matches.insert(current, next_patterns, session)?;
                    matches.insert(next, current_patterns, session)?;
                }
                next -= stride;
            }
            current -= stride;
        }
    }
    if cnormal > 0 {
        let mut next_start = dense.special[MIN_START];
        let mut current = ((states - 1) << dense.stride2) as u32;
        let mut next_normal = dense.special[MAX_START] + stride;
        while current >= next_normal {
            session.items(1)?;
            if let Some(accel) = accels.remove(current, session)? {
                remap.swap(dense, next_start, current, session)?;
                remap.swap(dense, next_normal, current, session)?;
                if let Some(other) = accels.remove(next_normal, session)? {
                    accels.insert(current, other, session)?;
                }
                if let Some(other) = accels.remove(next_start, session)? {
                    accels.insert(next_normal, other, session)?;
                }
                accels.insert(next_start, accel, session)?;
                update(dense, next_start);
                dense.special[MIN_START] += stride;
                dense.special[MAX_START] += stride;
                next_start += stride;
                next_normal += stride;
            }
            if !accels.contains(current, session)? {
                current -= stride;
            }
        }
    }
    if cstart > 0 {
        let mut next = dense.special[MIN_START];
        let mut current = next;
        while current <= dense.special[MAX_START] {
            session.items(1)?;
            if let Some(accel) = accels.remove(current, session)? {
                remap.swap(dense, current, next, session)?;
                accels.insert(next, accel, session)?;
                update(dense, next);
                next += stride;
            }
            current += stride;
        }
    }
    remap.apply(dense, session)?;
    // Upstream unwrap is an invariant: neither state nor pattern count changes.
    if set_patterns(dense, &matches, session)?.is_err() {
        return session.fail(Error::State);
    }
    set_max(dense);
    validate(dense, session)?;
    if ((dense.special[MAX_ACCEL] - dense.special[MIN_ACCEL]) / stride + 1) as usize != original_len
    {
        return session.fail(Error::State);
    }
    let mut previous = None;
    for (id, bytes) in accels.into_iter() {
        session.items(1)?;
        if previous.is_some_and(|previous| previous + stride != id) {
            return session.fail(Error::State);
        }
        previous = Some(id);
        // Accels::add extends two native-endian u32 words in one reserve.
        dense.accelerators.reserve(2, session)?;
        session.items(2)?;
        dense
            .accelerators
            .values
            .push(u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        dense
            .accelerators
            .values
            .push(u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]));
        session.items(1)?;
        dense.accelerators.values[0] += 1;
    }
    Ok(())
}

fn update(dense: &mut initial::Initial, id: u32) {
    dense.special[MIN_ACCEL] = dense.special[MIN_ACCEL].min(id);
    dense.special[MAX_ACCEL] = dense.special[MAX_ACCEL].max(id);
}

fn pattern_map<A: Admission>(
    dense: &initial::Initial,
    session: &mut Session<'_, A>,
) -> Step<ordered::Map<Array<u32>>> {
    let mut map = ordered::Map::new();
    for (index, pair) in dense
        .matches
        .slices
        .values
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
    {
        session.items(1)?;
        let mut patterns = Array::new();
        for &id in &dense.matches.patterns.values[pair[0] as usize..(pair[0] + pair[1]) as usize] {
            patterns.push(id, session)?;
        }
        map.insert(
            dense.special[MIN_MATCH] + (index << dense.stride2) as u32,
            patterns,
            session,
        )?;
    }
    Ok(map)
}

pub(super) fn state<A: Admission>(
    dense: &initial::Initial,
    id: u32,
    session: &mut Session<'_, A>,
) -> Step<Option<[u8; 8]>> {
    let mut accel = [0; 8];
    for class in 0..usize::from(dense.classes[255]) + 2 {
        session.items(1)?;
        if dense.transitions.values[id as usize + class] == id {
            continue;
        }
        // ByteClassElements scans the byte domain for each class. EOI is
        // visited last and is never an accelerator needle.
        for byte in 0..=255usize {
            session.items(1)?;
            if usize::from(dense.classes[byte]) != class {
                continue;
            }
            session.items(1)?;
            if accel[0] >= 3 || byte == usize::from(b' ') {
                return Ok(None);
            }
            accel[0] += 1;
            accel[usize::from(accel[0])] = byte as u8;
        }
        session.items(1)?;
    }
    Ok((accel[0] != 0).then_some(accel))
}
