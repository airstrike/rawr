// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: path/LICENSE-MIT and path/LICENSE-APACHE beside this module.

//! Lazy candidate discovery; a present candidate still requires full loading.

use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use rawr_toml::document;

use super::super::{session::Session, Error, Step};
use super::{path, roots};
use crate::input::membership::host;

pub(super) enum Discovery<'p> {
    Cached(PathBuf),
    Candidates(Candidates<'p>),
}

/// Cargo's inheritable_from_path cache branch. A miss requires full loading.
pub(super) enum Lookup<'a, 'd> {
    Cached(roots::Root<'a, 'd>),
    Load(PathBuf),
}
pub(super) fn lookup<'a, 'd, A: document::Admission>(
    manifest: PathBuf,
    roots: &roots::Roots<'a, 'd>,
    session: &mut Session<'_, A>,
) -> Step<Lookup<'a, 'd>> {
    session.check()?;
    let Some(directory) = manifest.parent() else {
        return session.fail(Error::State);
    };
    match roots.get(directory, session)? {
        Some(root) => Ok(Lookup::Cached(root)),
        None => Ok(Lookup::Load(manifest)),
    }
}

/// Cargo's complete cached pass precedes creation of find_root_iter, including
/// its __CARGO_TEST_ROOT read. Cargo home is the containing context's value.
pub(super) fn begin<'p, 'image: 'p>(
    manifest: &'p Path,
    home: &'p Path,
    roots: &roots::Roots<'_, '_>,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Discovery<'p>> {
    if let Some(path) = roots.ancestor(manifest, session)? {
        return Ok(Discovery::Cached(path));
    }
    // std::env::var(...).ok(): absent and non-Unicode values both mean no stop.
    // Missing recorded observations are terminal, never treated as absence.
    let stop = session
        .environment(b"__CARGO_TEST_ROOT")?
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(Path::new);
    Candidates::new(manifest, home, stop, session).map(Discovery::Candidates)
}

pub(super) struct Candidates<'p> {
    current: Option<&'p Path>,
    stop: Option<&'p Path>,
    previous: Option<&'p Path>,
    home: &'p Path,
    done: bool,
}

impl<'p> Candidates<'p> {
    fn new<A: document::Admission>(
        manifest: &'p Path,
        home: &'p Path,
        stop: Option<&'p Path>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut candidates = Self {
            current: Some(manifest),
            stop,
            previous: None,
            home,
            done: false,
        };
        // paths::ancestors(...).skip(2), before LookBehind is applied.
        candidates.advance();
        candidates.advance();
        Ok(candidates)
    }

    fn advance(&mut self) -> Option<&'p Path> {
        let current = self.current?;
        self.current = if self.stop == Some(current) {
            None
        } else {
            current.parent()
        };
        Some(current)
    }

    pub(super) fn next(
        &mut self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Option<PathBuf>> {
        self.next_with(session, |path, session| {
            session.exists(path.as_os_str().as_bytes())
        })
    }

    fn next_with<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
        mut exists: impl FnMut(&Path, &mut Session<'_, A>) -> Step<bool>,
    ) -> Step<Option<PathBuf>> {
        session.check()?;
        while !self.done {
            let Some(current) = self.advance() else {
                self.done = true;
                break;
            };
            let previous = self.previous.replace(current);
            if current.ends_with("target/package") || previous == Some(self.home) {
                self.done = true;
                break;
            }
            let candidate = path::join(current, Path::new("Cargo.toml"), session)?;
            if exists(&candidate, session)? {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
