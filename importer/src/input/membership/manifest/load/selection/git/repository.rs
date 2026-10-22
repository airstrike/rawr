// Copyright the gitoxide contributors. MIT OR Apache-2.0.
// See PROVENANCE.md and LICENSE-*.

//! The pinned discovery result's owned-path transformations.

use std::{borrow::Cow, path};

use rawr_toml::document;

use crate::input::membership::manifest::{load, session::Session, Step};

/// Source recognition result before repository opening and configuration.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Kind {
    PossiblyBare,
    WorkTree {
        linked_git_dir: Option<path::PathBuf>,
    },
    WorkTreeGitDir {
        work_dir: path::PathBuf,
    },
    Submodule {
        git_dir: path::PathBuf,
    },
    SubmoduleGitDir,
}

/// Source-owned discovery paths, before repository opening/configuration.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Path {
    LinkedWorkTree {
        work_dir: path::PathBuf,
        git_dir: path::PathBuf,
    },
    WorkTree(path::PathBuf),
    Repository(path::PathBuf),
}

impl Path {
    pub(super) fn from_dot_git_dir<A: document::Admission>(
        dir: path::PathBuf,
        kind: Kind,
        current_dir: &path::Path,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Self>> {
        session.check()?;
        let result = match kind {
            Kind::Submodule { git_dir } => {
                // Preserve struct-field evaluation order: git dir before work dir.
                let Some(git_dir) =
                    super::path::normalize(Cow::Owned(git_dir), current_dir, session)?
                else {
                    return Ok(None);
                };
                let git_dir = super::path::into_owned(git_dir, session)?;
                let Some(dir) = trailing_parent(dir, current_dir, session)? else {
                    return Ok(None);
                };
                Self::LinkedWorkTree {
                    git_dir,
                    work_dir: super::path::without_dot_git_dir(dir, session)?,
                }
            }
            Kind::SubmoduleGitDir => Self::Repository(dir),
            Kind::WorkTreeGitDir { work_dir } => Self::LinkedWorkTree {
                git_dir: dir,
                work_dir,
            },
            Kind::WorkTree { linked_git_dir } => match linked_git_dir {
                Some(git_dir) => {
                    let Some(dir) = trailing_parent(dir, current_dir, session)? else {
                        return Ok(None);
                    };
                    Self::LinkedWorkTree {
                        git_dir,
                        work_dir: super::path::without_dot_git_dir(dir, session)?,
                    }
                }
                None => {
                    let Some(mut dir) = trailing_parent(dir, current_dir, session)? else {
                        return Ok(None);
                    };
                    session.items(dir.as_os_str().len())?;
                    dir.pop();
                    let work_dir = if dir.as_os_str().is_empty() {
                        load::path::copy(path::Path::new("."), session)?
                    } else {
                        dir
                    };
                    Self::WorkTree(work_dir)
                }
            },
            Kind::PossiblyBare => Self::Repository(dir),
        };
        session.items(1)?;
        Ok(Some(result))
    }

    pub(super) fn into_parts<A: document::Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<(path::PathBuf, Option<path::PathBuf>)> {
        session.check()?;
        match self {
            Self::LinkedWorkTree { work_dir, git_dir } => Ok((git_dir, Some(work_dir))),
            Self::WorkTree(work_dir) => {
                let git_dir = super::path::file::join(&work_dir, path::Path::new(".git"), session)?;
                Ok((git_dir, Some(work_dir)))
            }
            Self::Repository(repository) => Ok((repository, None)),
        }
    }
}

fn trailing_parent<A: document::Admission>(
    dir: path::PathBuf,
    current_dir: &path::Path,
    session: &mut Session<'_, A>,
) -> Step<Option<path::PathBuf>> {
    session.string(dir.as_os_str().len())?;
    session.items(dir.as_os_str().len())?;
    if !matches!(
        dir.components().next_back(),
        Some(path::Component::ParentDir)
    ) {
        return Ok(Some(dir));
    }
    match super::path::normalize(Cow::Owned(dir), current_dir, session)? {
        Some(dir) => super::path::into_owned(dir, session).map(Some),
        None => Ok(None),
    }
}
