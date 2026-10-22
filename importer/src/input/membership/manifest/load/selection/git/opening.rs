// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Discovery through local configuration. This is not a completed repository.

use super::{config, path, precompose, trust, upwards, whitespace};
use crate::input::membership::{
    host,
    manifest::{io, session::Session, Step},
};
use rawr_toml::document::Admission;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

pub(super) struct Local {
    git_dir: PathBuf,
    worktree: Option<PathBuf>,
    common_dir: Option<PathBuf>,
    current_dir: PathBuf,
    trust: trust::Level,
    stage: config::Stage,
}

impl Local {
    fn precomposed<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Self> {
        if self.stage.precompose_unicode() {
            // The first call consumes its original owned path; the next three
            // borrow their retained owners and replace only a changed spelling.
            self.git_dir = precompose::path(Cow::Owned(self.git_dir), session)?.into_owned();
            if let Some(common) = self.common_dir.as_mut()
                && let Cow::Owned(path) = precompose::path(Cow::Borrowed(common), session)?
            {
                *common = path;
            }
            if let Some(worktree) = self.worktree.as_mut()
                && let Cow::Owned(path) = precompose::path(Cow::Borrowed(worktree), session)?
            {
                *worktree = path;
            }
            if let Cow::Owned(path) = precompose::path(Cow::Borrowed(&self.current_dir), session)? {
                self.current_dir = path;
            }
        }
        Ok(self)
    }
}

#[derive(Debug)]
pub(super) enum Failure {
    Discover(upwards::Failure),
    CommonDirectory(io::Failure),
    Configuration(config::Failure),
}

/// The same caller-owned occurrence provider spans upwards and opening. No
/// native/default provider, cached cwd substitution or process association.
pub(super) fn discover<'image, 'budget>(
    root: upwards::Root<'_>,
    whitespace: &mut whitespace::Owner,
    current_dir: &mut impl FnMut(
        upwards::Request<'_>,
        &mut Session<'_, host::Host<'image, 'budget>>,
    ) -> Step<Result<PathBuf, io::Failure>>,
    session: &mut Session<'_, host::Host<'image, 'budget>>,
) -> Step<Result<Local, Failure>> {
    let (repository, trust) = match upwards::discover(root, whitespace, current_dir, session)? {
        Ok(found) => found,
        Err(error) => return Ok(Err(Failure::Discover(error))),
    };
    let (git_dir, worktree) = repository.into_parts(session)?;
    // Both default trust-map entries have lossy=false, lenient=true and empty
    // CLI/API vectors. Selecting them introduces no heap allocation. Their
    // later ODB/permission effects are not represented as completed here.
    session.items(1)?;
    let cwd = match current_dir(upwards::Request::Opening, session)? {
        Ok(cwd) => cwd,
        Err(error) => return Ok(Err(Failure::Discover(upwards::Failure::CurrentDir(error)))),
    };
    session.check()?;
    let common_dir = {
        let file = path::file::join(&git_dir, Path::new("commondir"), session)?;
        match path::file::from_plain_file(&file, whitespace, session)? {
            None => None,
            Some(Err(error)) => return Ok(Err(Failure::CommonDirectory(error))),
            Some(Ok(common)) => Some(path::file::join(&git_dir, &common, session)?),
        }
    };
    let stage = match config::Stage::local(
        common_dir.as_deref().unwrap_or(&git_dir),
        &git_dir,
        trust,
        session,
    )? {
        Ok(stage) => stage,
        Err(error) => return Ok(Err(Failure::Configuration(error))),
    };
    Local {
        git_dir,
        worktree,
        common_dir,
        current_dir: cwd,
        trust,
        stage,
    }
    .precomposed(session)
    .map(Ok)
}

#[cfg(test)]
mod tests;
