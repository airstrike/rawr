// Copyright 2012-2015 The Rust Project Developers.
// Copyright the Cargo contributors. MIT OR Apache-2.0.
// See path/PROVENANCE.md and path/LICENSE-*.

//! Pinned lexical native paths. No operation observes the filesystem.

use std::alloc::Layout;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use rawr_toml::document;

use super::super::{session, storage, Error, Step};

#[cfg(test)]
mod tests;

/// Semantic failures of Cargo's inherited relative-path conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Invalid {
    #[error("path could not be made relative to the package root")]
    Relative,
    #[error("resolved path is not UTF-8")]
    Utf8,
}

/// Native Unix Path::join spelling, including absolute replacement and an empty
/// suffix's trailing separator. Parent components remain unnormalized.
pub(super) fn join<A: document::Admission>(
    root: &Path,
    path: &Path,
    session: &mut session::Session<'_, A>,
) -> Step<PathBuf> {
    session.check()?;
    let suffix = path.as_os_str().as_bytes();
    let prefix = if path.is_absolute() {
        &[][..]
    } else {
        root.as_os_str().as_bytes()
    };
    let separator = prefix.last().is_some_and(|&byte| byte != b'/');
    let Some(length) = prefix
        .len()
        .checked_add(usize::from(separator))
        .and_then(|length| length.checked_add(suffix.len()))
    else {
        return session.fail(Error::Overflow);
    };
    let mut bytes = allocate(length, session)?;
    bytes.extend_from_slice(prefix);
    if separator {
        bytes.push(b'/');
    }
    bytes.extend_from_slice(suffix);
    Ok(native(bytes))
}

/// cargo_util::paths::normalize_path, using admitted borrowed component slots.
pub(super) fn normalize<A: document::Admission>(
    path: &Path,
    session: &mut session::Session<'_, A>,
) -> Step<PathBuf> {
    session.check()?;
    session.string(path.as_os_str().len())?;
    let mut components = storage::Buffer::new();
    for component in path.components() {
        match component {
            // Prefix never occurs under the pinned supported Unix cfg.
            Component::Prefix(_) => return session.fail(Error::State),
            Component::RootDir => {
                components.clear();
                components.push(component, session)?;
            }
            Component::CurDir => {}
            Component::ParentDir => match components.as_slice().last() {
                Some(Component::ParentDir) | None => {
                    components.push(component, session)?;
                }
                Some(Component::Normal(_)) => {
                    components.pop();
                }
                Some(Component::RootDir) => {}
                _ => return session.fail(Error::State),
            },
            Component::Normal(_) => {
                components.push(component, session)?;
            }
        }
    }
    assemble(components.as_slice(), session)
}

/// Exact pathdiff 0.2.3 component branches, without its unaccounted Vec/collect.
pub(super) fn diff<A: document::Admission>(
    path: &Path,
    base: &Path,
    session: &mut session::Session<'_, A>,
) -> Step<Option<PathBuf>> {
    session.check()?;
    session.string(path.as_os_str().len())?;
    session.string(base.as_os_str().len())?;
    if path.is_absolute() != base.is_absolute() {
        return if path.is_absolute() {
            copy(path, session).map(Some)
        } else {
            Ok(None)
        };
    }
    let mut left = path.components();
    let mut right = base.components();
    let mut components = storage::Buffer::new();
    loop {
        match (left.next(), right.next()) {
            (None, None) => break,
            (Some(a), None) => {
                components.push(a, session)?;
                for component in left.by_ref() {
                    components.push(component, session)?;
                }
                break;
            }
            (None, _) => {
                components.push(Component::ParentDir, session)?;
            }
            (Some(a), Some(b)) if components.len() == 0 && a == b => {}
            (Some(a), Some(Component::CurDir)) => {
                components.push(a, session)?;
            }
            (Some(_), Some(Component::ParentDir)) => return Ok(None),
            (Some(a), Some(_)) => {
                components.push(Component::ParentDir, session)?;
                for _ in right {
                    components.push(Component::ParentDir, session)?;
                }
                components.push(a, session)?;
                for component in left.by_ref() {
                    components.push(component, session)?;
                }
                break;
            }
        }
    }
    assemble(components.as_slice(), session).map(Some)
}

/// The single-script branch starts with UTF-8, so lexical components cannot
/// produce non-UTF-8 output. Ownership transfers without reallocation.
pub(super) fn normalize_text<A: document::Admission>(
    text: &str,
    session: &mut session::Session<'_, A>,
) -> Step<String> {
    let path = normalize(Path::new(text), session)?;
    match String::from_utf8(path.into_os_string().into_vec()) {
        Ok(text) => Ok(text),
        Err(_) => session.fail(Error::State),
    }
}

/// Cargo resolve_relative_path: join old root, normalize, diff against the new
/// root, then check UTF-8. It neither normalizes the base nor canonicalizes it.
pub(super) fn rebase<A: document::Admission>(
    old_root: &Path,
    new_root: &Path,
    relative: &str,
    session: &mut session::Session<'_, A>,
) -> Step<Result<String, Invalid>> {
    let joined = join(old_root, Path::new(relative), session)?;
    let normalized = normalize(&joined, session)?;
    let Some(relative) = diff(&normalized, new_root, session)? else {
        return invalid(Invalid::Relative, session);
    };
    match String::from_utf8(relative.into_os_string().into_vec()) {
        Ok(text) => Ok(Ok(text)),
        Err(_) => invalid(Invalid::Utf8, session),
    }
}

fn invalid<A: document::Admission, T>(
    error: Invalid,
    session: &mut session::Session<'_, A>,
) -> Step<Result<T, Invalid>> {
    session.items(1)?;
    Ok(Err(error))
}

pub(super) fn copy<A: document::Admission>(
    path: &Path,
    session: &mut session::Session<'_, A>,
) -> Step<PathBuf> {
    let source = path.as_os_str().as_bytes();
    let mut bytes = allocate(source.len(), session)?;
    bytes.extend_from_slice(source);
    Ok(native(bytes))
}

fn assemble<A: document::Admission>(
    components: &[Component<'_>],
    session: &mut session::Session<'_, A>,
) -> Step<PathBuf> {
    let mut length = 0usize;
    let mut last = None;
    let mut capacity = 0;
    for component in components {
        let part = component.as_os_str().as_bytes();
        if part.starts_with(b"/") {
            length = 0;
            last = None;
        }
        let Some(next) = length
            .checked_add(usize::from(last.is_some_and(|byte| byte != b'/')))
            .and_then(|length| length.checked_add(part.len()))
        else {
            return session.fail(Error::Overflow);
        };
        length = next;
        capacity = capacity.max(length);
        if let Some(&byte) = part.last() {
            last = Some(byte);
        }
    }
    let mut bytes = allocate(capacity, session)?;
    for component in components {
        let part = component.as_os_str().as_bytes();
        if part.starts_with(b"/") {
            bytes.clear();
        }
        if bytes.last().is_some_and(|&byte| byte != b'/') {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(part);
    }
    if bytes.len() != length {
        return session.fail(Error::State);
    }
    Ok(native(bytes))
}

fn allocate<A: document::Admission>(
    length: usize,
    session: &mut session::Session<'_, A>,
) -> Step<Vec<u8>> {
    session.string(length)?;
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.items(1)?;
    if length != 0 {
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    let mut bytes = Vec::new();
    if let Err(error) = bytes.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    if length != 0 {
        storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
    }
    Ok(bytes)
}

fn native(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}
