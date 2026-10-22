// Copyright the Cargo contributors and Rust Project Developers.
// MIT OR Apache-2.0; license texts in ../path/LICENSE-*.
//! Pinned Cargo closest-name matching with admitted Unicode/DP work.

use rawr_toml::document;

use super::super::super::{session::Session, storage, Error, Step};
use super::infer;

mod properties;
#[cfg(test)]
mod tests;

pub(in super::super) fn closest<'a, A: document::Admission>(
    choice: &str,
    inferred: &'a [infer::Item],
    session: &mut Session<'_, A>,
) -> Step<Option<&'a str>> {
    select(choice, inferred.iter().map(infer::Item::name), session)
}

pub(in super::super) fn select<'a, A: document::Admission>(
    choice: &str,
    candidates: impl Iterator<Item = &'a str>,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a str>> {
    session.check()?;
    let mut best = None;
    for candidate in candidates {
        session.items(1)?;
        if let Some(distance) = distance(choice, candidate, 3, session)? {
            // Iterator::min_by_key keeps the first equally close element.
            if best.is_none_or(|(prior, _)| distance < prior) {
                best = Some((distance, candidate));
            }
        }
    }
    Ok(best.map(|(_, name)| name))
}

fn distance<A: document::Admission>(
    a: &str,
    b: &str,
    limit: usize,
    session: &mut Session<'_, A>,
) -> Step<Option<usize>> {
    let lower_a = lowercase(a, session)?;
    let lower_b = lowercase(b, session)?;
    let mut a = lower_a.as_slice();
    let mut b = lower_b.as_slice();
    if a.len() < b.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let minimum = a.len() - b.len();
    if minimum > limit {
        return Ok(None);
    }
    while let Some(((bc, br), (ac, ar))) = b.split_first().zip(a.split_first()) {
        session.items(1)?;
        if ac != bc {
            break;
        }
        a = ar;
        b = br;
    }
    while let Some(((bc, br), (ac, ar))) = b.split_last().zip(a.split_last()) {
        session.items(1)?;
        if ac != bc {
            break;
        }
        a = ar;
        b = br;
    }
    if b.is_empty() {
        return Ok(Some(minimum));
    }
    let Some(width) = b.len().checked_add(1) else {
        return session.fail(Error::Overflow);
    };
    let mut prior_prior = storage::Buffer::new();
    let mut prior = storage::Buffer::new();
    let mut current = storage::Buffer::new();
    for column in 0..width {
        prior_prior.push(usize::MAX, session)?;
        prior.push(column, session)?;
        current.push(0usize, session)?;
    }
    for i in 1..=a.len() {
        session.items(1)?;
        current.as_mut_slice()[0] = i;
        for j in 1..=b.len() {
            // One admitted recurrence cell before its comparisons/arithmetic.
            session.items(1)?;
            let substitution = usize::from(a[i - 1] != b[j - 1]);
            let cost = (prior.as_slice()[j] + 1)
                .min(current.as_slice()[j - 1] + 1)
                .min(prior.as_slice()[j - 1] + substitution);
            current.as_mut_slice()[j] = cost;
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                current.as_mut_slice()[j] = cost.min(prior_prior.as_slice()[j - 2] + 1);
            }
        }
        [prior_prior, prior, current] = [prior, current, prior_prior];
    }
    let result = prior.as_slice()[b.len()];
    Ok((result <= limit).then_some(result))
}

/// str::to_lowercase's sole contextual mapping is final sigma. Retain its
/// original-string scans while collecting directly into admitted char storage.
fn lowercase<A: document::Admission>(
    text: &str,
    session: &mut Session<'_, A>,
) -> Step<storage::Buffer<char>> {
    session.check()?;
    session.string(text.len())?;
    let mut output = storage::Buffer::new();
    let mut encoded = 0usize;
    for (index, ch) in text.char_indices() {
        session.items(1)?;
        if ch == 'Σ' {
            let before = cased_after_ignorable(text[..index].chars().rev(), session)?;
            let final_sigma =
                before && !cased_after_ignorable(text[index + 'Σ'.len_utf8()..].chars(), session)?;
            append(
                if final_sigma { 'ς' } else { 'σ' },
                &mut encoded,
                &mut output,
                session,
            )?;
        } else {
            // Pinned char mapping is allocation-free; unlike str mapping this
            // iterator alone does not implement contextual sigma.
            for lower in ch.to_lowercase() {
                append(lower, &mut encoded, &mut output, session)?;
            }
        }
    }
    Ok(output)
}
fn append<A: document::Admission>(
    ch: char,
    encoded: &mut usize,
    output: &mut storage::Buffer<char>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let Some(length) = encoded.checked_add(ch.len_utf8()) else {
        return session.fail(Error::Overflow);
    };
    session.string(length)?;
    output.push(ch, session)?;
    *encoded = length;
    Ok(())
}
fn cased_after_ignorable<A: document::Admission>(
    characters: impl Iterator<Item = char>,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    for ch in characters {
        session.items(1)?;
        let ignorable = if ch.is_ascii() {
            matches!(ch, '\'' | '.' | ':' | '^' | '`')
        } else {
            contains(properties::CASE_IGNORABLE, ch, session)?
        };
        if ignorable {
            continue;
        }
        return Ok(ch.is_lowercase()
            || ch.is_uppercase()
            || contains(properties::LT, ch, session)?);
    }
    Ok(false)
}
fn contains<A: document::Admission>(
    ranges: &[(u32, u32)],
    ch: char,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    let code = ch as u32;
    let (mut start, mut end) = (0, ranges.len());
    while start < end {
        session.items(1)?;
        let middle = start + (end - start) / 2;
        let (lower, upper) = ranges[middle];
        if code < lower {
            end = middle
        } else if code >= upper {
            start = middle + 1
        } else {
            return Ok(true);
        }
    }
    Ok(false)
}
