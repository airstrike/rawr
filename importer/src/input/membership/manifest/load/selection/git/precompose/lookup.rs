// Copyright the Rust Project Developers. MIT OR Apache-2.0.
//! Pinned constant-space Unicode lookups and canonical Hangul arithmetic.

use super::{tables, Admission, Session, Step};
use crate::input::membership::manifest::Error;

fn hash(key: u32, salt: u32, n: usize) -> usize {
    let y = key.wrapping_add(salt).wrapping_mul(2654435769);
    let y = y ^ key.wrapping_mul(0x31415926);
    (((y as u64) * (n as u64)) >> 32) as usize
}

fn lookup<K: Copy, V>(
    key: u32,
    salts: &[u16],
    values: &[K],
    key_of: impl Fn(K) -> u32,
    value_of: impl Fn(K) -> V,
    default: V,
) -> V {
    let salt = u32::from(salts[hash(key, 0, salts.len())]);
    let pair = values[hash(key, salt, salts.len())];
    if key == key_of(pair) {
        value_of(pair)
    } else {
        default
    }
}

pub(super) fn class<A: Admission>(ch: char, session: &mut Session<'_, A>) -> Step<u8> {
    session.items(1)?;
    Ok(lookup(
        ch as u32,
        tables::CANONICAL_COMBINING_CLASS_SALT,
        tables::CANONICAL_COMBINING_CLASS_KV,
        |pair| pair >> 8,
        |pair| (pair & 0xff) as u8,
        0,
    ))
}

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;
const S_LAST: u32 = S_BASE + S_COUNT - 1;
const L_LAST: u32 = L_BASE + L_COUNT - 1;
const V_LAST: u32 = V_BASE + V_COUNT - 1;
const T_LAST: u32 = T_BASE + T_COUNT - 1;
const T_FIRST: u32 = T_BASE + 1;

fn scalar<A: Admission>(value: u32, session: &mut Session<'_, A>) -> Step<char> {
    // The exact source's checked Hangul bounds imply a Unicode scalar. Use
    // the safe constructor instead of introducing its unchecked conversion.
    match char::from_u32(value) {
        Some(ch) => Ok(ch),
        None => session.fail(Error::State),
    }
}

pub(super) fn canonical<A: Admission>(
    ch: char,
    session: &mut Session<'_, A>,
    mut emit: impl FnMut(char, &mut Session<'_, A>) -> Step<()>,
) -> Step<()> {
    session.items(1)?;
    if ch <= '\u{7f}' {
        return emit(ch, session);
    }
    let code = ch as u32;
    if (S_BASE..S_BASE + S_COUNT).contains(&code) {
        let index = code - S_BASE;
        emit(scalar(L_BASE + index / N_COUNT, session)?, session)?;
        emit(
            scalar(V_BASE + (index % N_COUNT) / T_COUNT, session)?,
            session,
        )?;
        let trailing = index % T_COUNT;
        if trailing > 0 {
            emit(scalar(T_BASE + trailing, session)?, session)?;
        }
        return Ok(());
    }
    session.items(1)?;
    let decomposition = lookup(
        code,
        tables::CANONICAL_DECOMPOSED_SALT,
        tables::CANONICAL_DECOMPOSED_KV,
        |pair| pair.0,
        |pair| Some(pair.1),
        None,
    );
    if let Some((start, len)) = decomposition {
        for &part in &tables::CANONICAL_DECOMPOSED_CHARS[usize::from(start)..][..usize::from(len)] {
            emit(part, session)?;
        }
        return Ok(());
    }
    emit(ch, session)
}

pub(super) fn compose<A: Admission>(
    a: char,
    b: char,
    session: &mut Session<'_, A>,
) -> Step<Option<char>> {
    session.items(1)?;
    let (left, right) = (a as u32, b as u32);
    match (left, right) {
        (L_BASE..=L_LAST, V_BASE..=V_LAST) => {
            let index = (left - L_BASE) * N_COUNT + (right - V_BASE) * T_COUNT;
            return scalar(S_BASE + index, session).map(Some);
        }
        (S_BASE..=S_LAST, T_FIRST..=T_LAST) if (left - S_BASE).is_multiple_of(T_COUNT) => {
            return scalar(left + (right - T_BASE), session).map(Some);
        }
        _ => {}
    }
    session.items(1)?;
    Ok(if a < '\u{10000}' && b < '\u{10000}' {
        lookup(
            left << 16 | right,
            tables::COMPOSITION_TABLE_SALT,
            tables::COMPOSITION_TABLE_KV,
            |pair| pair.0,
            |pair| Some(pair.1),
            None,
        )
    } else {
        tables::composition_table_astral(a, b)
    })
}
