// Copyright the ripgrep contributors. MIT OR Unlicense.
//! Retained ignore scratch and nearest-parent precedence for Cargo's caller.
use std::{os::unix::ffi::OsStrExt, path::Path};

use rawr_toml::document::Admission;

use super::super::paths::Candidate;
use super::Matcher;
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};

#[cfg(test)]
mod tests;

impl Matcher {
    /// Closed composition of matched_path_or_any_parents(...).is_ignore().
    /// The caller supplies Cargo's package-relative path after strip_prefix.
    pub(in super::super::super) fn is_ignored<A: Admission>(
        &mut self,
        path: &Path,
        is_dir: bool,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if self.set.len() == 0 {
            return Ok(false);
        }
        let mut path = self.root.strip(path, session)?;
        session.items(1)?;
        // The physical caller must retain the pinned strip_prefix domain.
        // A rooted remainder is an internal invocation violation, not an
        // ignore result or an alternate interpretation of a source panic.
        if path.has_root() {
            return session.fail(Error::State);
        }
        if let Some(index) = self.stripped(path, is_dir, session)? {
            return Ok(!self.globs[index].whitelist);
        }
        loop {
            // Unix Path::parent is a finite borrowed component walk. Admit
            // its actual bounded input extent, including the final empty path.
            session.items(path.as_os_str().as_bytes().len())?;
            let Some(parent) = path.parent() else {
                return Ok(false);
            };
            if let Some(index) = self.stripped(parent, true, session)? {
                return Ok(!self.globs[index].whitelist);
            }
            path = parent;
        }
    }

    fn stripped<A: Admission>(
        &mut self,
        path: &Path,
        is_dir: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        if self.set.len() == 0 {
            return Ok(None);
        }
        // This acquisition stays before Candidate::new and preserves
        // the allocation/capacity across direct and parent queries. The
        // exclusive borrow ends on every return; no pool guard escapes.
        session.items(1)?;
        let scratch = self.matches.get_or_insert_with(Buffer::new);
        let candidate = Candidate::prepare(path, session)?;
        self.set.matches_into(&candidate, scratch, session)?;
        for &index in scratch.as_slice().iter().rev() {
            session.items(1)?;
            let Some(glob) = self.globs.get(index) else {
                return session.fail(Error::State);
            };
            if !glob.directory || is_dir {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }
}
