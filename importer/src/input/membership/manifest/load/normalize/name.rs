// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Embedded native file-stem naming; see name/PROVENANCE.md.

use std::{alloc::Layout, os::unix::ffi::OsStrExt, path::Path};

use rawr_toml::document;

use super::super::super::{scalar, session::Session, storage, Error, Step};

/// The caller reaches this only for an embedded package without a declared name.
/// A successful result owns its validated text, without borrowing itself.
pub(super) fn embedded<A: document::Admission>(
    manifest_file: &Path,
    session: &mut Session<'_, A>,
) -> Step<String> {
    session.check()?;
    let Some(stem) = manifest_file.file_stem() else {
        // Cargo's loader has already enforced the presence of a file name.
        return session.fail(Error::State);
    };
    sanitize(stem.as_bytes(), session)
}

fn sanitize<A: document::Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<String> {
    session.check()?;
    let mut lossy_length = 0usize;
    for ch in lossy(bytes) {
        let Some(next) = lossy_length.checked_add(ch.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        lossy_length = next;
    }
    session.string(lossy_length)?;
    // ASCII underscore is unchanged by UTF-8 replacement, even in invalid input.
    let placeholder = if bytes.contains(&b'_') { '_' } else { '-' };
    let mut length = 0usize;
    visit(bytes, placeholder, |ch| {
        let Some(next) = length.checked_add(ch.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        length = next;
        Ok(())
    })?;
    session.string(length)?;
    session.items(1)?;
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
    visit(bytes, placeholder, |ch| {
        output.push(ch);
        Ok(())
    })?;
    // Match the source's PackageName::new after sanitizing, without retaining
    // a borrowed Name alongside its owned backing String.
    if scalar::Name::parse(&output, scalar::Kind::Package).is_err() {
        return session.fail(Error::State);
    }
    Ok(output)
}

/// Same invalid-sequence grouping as String::from_utf8_lossy, without ownership.
fn lossy(bytes: &[u8]) -> impl Iterator<Item = char> + '_ {
    bytes.utf8_chunks().flat_map(|chunk| {
        chunk
            .valid()
            .chars()
            .chain((!chunk.invalid().is_empty()).then_some('\u{fffd}'))
    })
}

fn visit(bytes: &[u8], placeholder: char, mut emit: impl FnMut(char) -> Step<()>) -> Step<()> {
    let mut chars = lossy(bytes).peekable();
    let mut started = false;
    while let Some(ch) = chars.next() {
        if ch == ':' && chars.peek() == Some(&':') {
            chars.next();
            if !started {
                for ch in "package".chars() {
                    emit(ch)?;
                }
            }
            emit(':')?;
            emit(':')?;
            started = false;
        } else if !started {
            if (unicode_ident::is_xid_start(ch) || ch == '_') && !ch.is_ascii_digit() {
                emit(ch)?;
                started = true;
            }
        } else {
            emit(if unicode_ident::is_xid_continue(ch) || ch == '-' {
                ch
            } else {
                placeholder
            })?;
        }
    }
    if !started {
        for ch in "package".chars() {
            emit(ch)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
