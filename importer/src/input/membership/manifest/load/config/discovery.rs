//! Cargo get_file_path's physical selector over checked acquisition records.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use super::super::super::{session::Session, Error, Step};
use super::super::path;
use crate::input::membership::{
    host,
    observed::discovery::{Filename, Selection},
    Error as InputError,
};

/// Selection retains acquisition facts and exact selected spelling. This does
/// not attest hypothetical current warning success or perform walk_tree.
pub(super) fn select(
    directory: &Path,
    filename: Filename,
    warn: bool,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<PathBuf>> {
    // Pinned get_file_path constructs both spellings before either attempted
    // acquisition, even if warn=false or local native conversion will fail.
    let legacy = path::join(directory, Path::new(filename.legacy()), session)?;
    let toml = path::join(directory, Path::new(filename.toml()), session)?;
    session.string(directory.as_os_str().len())?;
    if directory.as_os_str().as_bytes().contains(&0) {
        // Both fixed suffixes are NUL-free. The reached legacy open conversion
        // and fallback exists conversion each fail locally, without native I/O.
        // Do not send this through observed::Path or invent an OS errno.
        return Ok(None);
    }
    if !directory.is_absolute() {
        // This is incomplete replay, never Cargo's semantic answer or a source
        // restriction. The observation domain is absolute; integrating callers
        // must establish their governed directory/CWD domain separately.
        return session.fail(Error::Host(InputError::Unobserved));
    }
    let transcript = session.discovery(directory.as_os_str().as_bytes(), filename, warn)?;
    Ok(match transcript.selection() {
        Selection::Legacy => Some(legacy),
        Selection::Toml => Some(toml),
        Selection::None => None,
    })
}

#[cfg(test)]
mod tests;
