// Copyright the Rust Project Developers and gitoxide contributors. MIT OR Apache-2.0.
//! Pinned NFC path transformation with its original two normalization buffers.

use std::{borrow::Cow, path::Path};

use crate::input::membership::manifest::{session::Session, Step};
use rawr_toml::document::Admission;

mod buffer;
mod decompose;
mod lookup;
mod recompose;
#[rustfmt::skip]
mod tables;

use recompose::Recompose;

#[derive(Clone, Copy, Eq, PartialEq)]
enum IsNormalized {
    Yes,
    No,
    Maybe,
}

/// gix-utils::precompose_path preserves the original Cow on invalid UTF-8 or NFC.
pub(super) fn path<'path, A: Admission>(
    path: Cow<'path, Path>,
    session: &mut Session<'_, A>,
) -> Step<Cow<'path, Path>> {
    session.items(path.as_os_str().len())?;
    let Some(text) = path.to_str() else {
        return Ok(path);
    };
    if normalized(text, session)? {
        return Ok(path);
    }
    // String::from_iter<char> creates an empty String, reserves the iterator's
    // default lower bound (zero), then pushes each encoded scalar separately.
    let mut output = buffer::Text::new();
    let mut normalized = Recompose::from_chars(text.chars(), session)?;
    while let Some(ch) = normalized.next(session)? {
        output.push(ch, session)?;
    }
    // The iterator is retired by String::extend before String becomes PathBuf.
    drop(normalized);
    Ok(Cow::Owned(output.into_string().into()))
}

fn normalized<A: Admission>(text: &str, session: &mut Session<'_, A>) -> Step<bool> {
    let mut last_class = 0;
    let mut result = IsNormalized::Yes;
    let mut chars = text.chars();
    loop {
        session.items(1)?;
        let Some(ch) = chars.next() else { break };
        if ch <= '\u{7f}' {
            last_class = 0;
            continue;
        }
        let class = lookup::class(ch, session)?;
        if last_class > class && class != 0 {
            return Ok(false);
        }
        session.items(1)?;
        match tables::qc_nfc(ch) {
            IsNormalized::No => return Ok(false),
            IsNormalized::Maybe => result = IsNormalized::Maybe,
            IsNormalized::Yes => {}
        }
        last_class = class;
    }
    if result == IsNormalized::Yes {
        return Ok(true);
    }
    // Iterator::eq pulls the original (left) char before the normalized right.
    let mut original = text.chars();
    let mut normalized = Recompose::from_chars(text.chars(), session)?;
    loop {
        session.items(1)?;
        let left = original.next();
        let right = normalized.next(session)?;
        match (left, right) {
            (Some(left), Some(right)) if left == right => {}
            (None, None) => return Ok(true),
            _ => return Ok(false),
        }
    }
}

#[cfg(test)]
mod tests;
