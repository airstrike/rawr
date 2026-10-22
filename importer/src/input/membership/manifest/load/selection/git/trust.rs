// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Pinned Unix ownership trust over recorded metadata and command identity.

use std::{alloc::Layout, ffi::OsString, os::unix::ffi::OsStringExt, path::Path};

use rawr_toml::document;

use super::predicate;
use crate::input::membership::{
    host,
    manifest::{io, session::Session, Error, Step},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Level {
    Reduced,
    Full,
}

pub(super) fn from_path(
    path: &Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Level, io::Failure>> {
    let owner = match predicate::metadata(path, false, session)? {
        Ok(metadata) => metadata.owner,
        Err(error) => return Ok(Err(error)),
    };
    let effective = session.identity()?;
    if owner == effective {
        return Ok(Ok(Level::Full));
    }

    // var_os owns a byte copy before to_str and decimal parsing. Non-Unicode
    // values fail locally, with no VarError allocation and no trimming.
    let sudo = if let Some(bytes) = session.environment(b"SUDO_UID")? {
        session.string(bytes.len())?;
        session.items(1)?;
        let Ok(layout) = Layout::array::<u8>(bytes.len()) else {
            return session.fail(Error::Overflow);
        };
        if !bytes.is_empty() {
            session.admit(document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
        }
        let mut copy = Vec::new();
        if let Err(error) = copy.try_reserve_exact(bytes.len()) {
            return session.fail(Error::Allocation(error));
        }
        session.items(bytes.len())?;
        copy.extend_from_slice(bytes);
        let value = OsString::from_vec(copy);
        session.items(bytes.len())?;
        let parsed = match value.to_str() {
            Some(text) => {
                session.items(text.len())?;
                text.parse::<u32>().ok()
            }
            None => None,
        };
        drop(value);
        parsed
    } else {
        None
    };
    Ok(Ok(if sudo == Some(owner) {
        Level::Full
    } else {
        Level::Reduced
    }))
}

#[cfg(test)]
mod tests;
