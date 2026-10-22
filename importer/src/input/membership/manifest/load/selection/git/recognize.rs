// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Pinned is_git wrapper and body over supplied facts. Native current-directory
//! acquisition and command-history association remain external.

use std::{
    borrow::Cow,
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};

use rawr_toml::document;

use super::{head, loose::store, path, predicate, repository, whitespace};
use crate::input::membership::manifest::{io, load, session::Session, Error, Step};
use crate::input::membership::{host, observed};

/// Source wrapper and body errors. These variants move existing owned payloads.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Failure {
    #[error("HEAD search failed: {0}")]
    FindHeadRef(store::Failure),
    #[error("HEAD is missing")]
    MissingHead,
    #[error("HEAD resolved to misplaced reference {name:?}")]
    MisplacedHead { name: Vec<u8> },
    #[error("objects directory is missing: {missing:?}")]
    MissingObjectsDirectory { missing: PathBuf },
    #[error("common directory file failed: {missing:?}: {source}")]
    MissingCommonDir {
        missing: PathBuf,
        source: io::Failure,
    },
    #[error("refs directory is missing: {missing:?}")]
    MissingRefsDirectory { missing: PathBuf },
    #[error("gitdir file failed: {0}")]
    GitFile(path::file::Failure),
    #[error("metadata failed for {path:?}: {source}")]
    Metadata { source: io::Failure, path: PathBuf },
    #[error("current directory failed: {0}")]
    CurrentDir(io::Failure),
}

enum Kind {
    MaybeRepo,
    Submodule,
    LinkedWorkTreeDir,
    WorkTreeGitDir { work_dir: PathBuf },
}

/// Source is_git wrapper: acquire followed metadata before a distinct owned
/// current-directory occurrence, then retain that cwd through recognition.
pub(super) fn git<'image, 'budget>(
    git_dir: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'image, 'budget>>,
    cwd: impl FnOnce(
        &mut Session<'_, host::Host<'image, 'budget>>,
    ) -> Step<Result<PathBuf, io::Failure>>,
) -> Step<Result<repository::Kind, Failure>> {
    let metadata = match predicate::metadata(git_dir, true, session)? {
        Ok(metadata) => metadata,
        Err(source) => {
            session.items(git_dir.as_os_str().len())?;
            let path = load::path::copy(git_dir, session)?;
            return Ok(Err(Failure::Metadata { source, path }));
        }
    };
    let cwd = match cwd(session)? {
        Ok(cwd) => cwd,
        Err(error) => return Ok(Err(Failure::CurrentDir(error))),
    };
    session.check()?;
    with_metadata(git_dir, metadata, &cwd, whitespace, session)
}

/// Interpret the supplied original followed metadata and cwd. This does not
/// acquire those facts, associate relative native queries, or open a repository.
pub(super) fn with_metadata(
    git_dir: &Path,
    metadata: observed::Metadata,
    cwd: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<repository::Kind, Failure>> {
    session.items(1)?;
    let dot_git = if metadata.kind == observed::Kind::File {
        match path::file::from_gitdir_file(git_dir, whitespace, session)? {
            Ok(directory) => Cow::Owned(directory),
            Err(error) => return Ok(Err(Failure::GitFile(error))),
        }
    } else {
        Cow::Borrowed(git_dir)
    };

    if let Err(error) = head::check(dot_git.as_ref(), session)? {
        session.items(1)?;
        return Ok(Err(match error {
            head::Failure::MissingHead => Failure::MissingHead,
            head::Failure::MisplacedHead { name } => Failure::MisplacedHead { name },
            head::Failure::Find(error) => Failure::FindHeadRef(error),
        }));
    }

    // This second branch uses ORIGINAL metadata, never metadata for dot_git.
    let (common_dir, kind) = if metadata.kind == observed::Kind::File {
        let common_dir = path::file::join(&dot_git, Path::new("commondir"), session)?;
        match path::file::from_plain_file(&common_dir, whitespace, session)? {
            Some(Err(source)) => {
                session.items(1)?;
                return Ok(Err(Failure::MissingCommonDir {
                    missing: common_dir,
                    source,
                }));
            }
            Some(Ok(common_dir)) => {
                let common_dir = path::file::join(&dot_git, &common_dir, session)?;
                (Cow::Owned(common_dir), Kind::LinkedWorkTreeDir)
            }
            None => (clone(&dot_git, session)?, Kind::Submodule),
        }
    } else {
        let common_dir = path::file::join(&dot_git, Path::new("commondir"), session)?;
        // Result::ok consumes and drops source errors here. Step refusal has
        // already escaped through ?, so it can never become MaybeRepo.
        let parsed =
            path::file::from_plain_file(&common_dir, whitespace, session)?.and_then(Result::ok);
        let paired = match parsed {
            Some(common) => {
                // Keep this temporary through the whole source chained
                // and_then/map expression, including stripping terminal .git.
                let gitdir_file = path::file::join(&dot_git, Path::new("gitdir"), session)?;
                let worktree = path::file::from_plain_file_relative_to_file(
                    &gitdir_file,
                    whitespace,
                    session,
                )?
                .and_then(Result::ok);
                let result = match worktree {
                    Some(worktree) => {
                        let work_dir = path::without_dot_git_dir(worktree, session)?;
                        Some((work_dir, common))
                    }
                    None => {
                        drop(common);
                        None
                    }
                };
                drop(gitdir_file);
                result
            }
            None => None,
        };
        match paired {
            Some((work_dir, common_dir)) => {
                let common_dir = path::file::join(&dot_git, &common_dir, session)?;
                (Cow::Owned(common_dir), Kind::WorkTreeGitDir { work_dir })
            }
            None => (clone(&dot_git, session)?, Kind::MaybeRepo),
        }
    };

    {
        let objects = path::file::join(&common_dir, Path::new("objects"), session)?;
        if !predicate::directory(&objects, session)? {
            session.items(1)?;
            return Ok(Err(Failure::MissingObjectsDirectory { missing: objects }));
        }
    }
    {
        let refs = path::file::join(&common_dir, Path::new("refs"), session)?;
        if !predicate::directory(&refs, session)? {
            session.items(1)?;
            return Ok(Err(Failure::MissingRefsDirectory { missing: refs }));
        }
    }

    let result = match kind {
        Kind::LinkedWorkTreeDir => repository::Kind::WorkTree {
            linked_git_dir: Some(path::into_owned(dot_git, session)?),
        },
        Kind::WorkTreeGitDir { work_dir } => repository::Kind::WorkTreeGitDir { work_dir },
        Kind::Submodule => repository::Kind::Submodule {
            git_dir: path::into_owned(dot_git, session)?,
        },
        Kind::MaybeRepo => {
            let conformed = conform(git_dir, cwd, session)?;
            classify(&conformed, session)?
        }
    };
    // Keep common_dir (and any unconsumed dot_git) alive through final guessing,
    // exactly as the source's outer locals. No configuration/bare proof follows.
    session.items(1)?;
    Ok(Ok(result))
}

fn clone<'a, A: document::Admission>(
    path: &Cow<'a, Path>,
    session: &mut Session<'_, A>,
) -> Step<Cow<'a, Path>> {
    session.items(1)?;
    match path {
        Cow::Borrowed(path) => Ok(Cow::Borrowed(*path)),
        Cow::Owned(path) => {
            scans(path, 1, session)?;
            load::path::copy(path, session).map(Cow::Owned)
        }
    }
}

fn conform<'a, A: document::Admission>(
    git_dir: &'a Path,
    cwd: &Path,
    session: &mut Session<'_, A>,
) -> Step<Cow<'a, Path>> {
    scans(git_dir, 1, session)?;
    if git_dir == Path::new(".") {
        // Path equality compares components, so this includes ./ and .//./.
        // The source realpath loop reaches CurDir only: no symlink predicates,
        // links, limits, parent failures or semantic realpath fallback exists.
        let real = path::file::push_empty(cwd, session)?;
        scans(git_dir, 1, session)?;
        for component in git_dir.components() {
            session.items(1)?;
            if component != Component::CurDir {
                return session.fail(Error::State);
            }
        }
        Ok(Cow::Owned(real))
    } else {
        // The helper admits its component processing and variable scans.
        Ok(
            path::normalize(Cow::Borrowed(git_dir), cwd, session)?
                .unwrap_or(Cow::Borrowed(git_dir)),
        )
    }
}

fn classify(path: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<repository::Kind> {
    // bare() must probe index first, even if file_name is already .git.
    let bare = {
        let index = path::file::join(path, Path::new("index"), session)?;
        if predicate::exists(&index, session)? {
            false
        } else {
            scans(path, 1, session)?;
            path.file_name() != Some(OsStr::new(".git"))
        }
    };
    let possibly_bare = if bare {
        true
    } else {
        scans(path, 2, session)?;
        path.extension() == Some(OsStr::new("git"))
    };
    if possibly_bare {
        return Ok(repository::Kind::PossiblyBare);
    }
    if path::repository_kind(path, session)?.is_some_and(|kind| kind == path::Kind::Submodule) {
        return Ok(repository::Kind::SubmoduleGitDir);
    }
    scans(path, 1, session)?;
    if path.file_name() == Some(OsStr::new(".git")) {
        Ok(repository::Kind::WorkTree {
            linked_git_dir: None,
        })
    } else {
        Ok(repository::Kind::PossiblyBare)
    }
}

fn scans<A: document::Admission>(
    path: &Path,
    passes: usize,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.string(path.as_os_str().len())?;
    let Some(work) = path.as_os_str().len().checked_mul(passes) else {
        return session.fail(Error::Overflow);
    };
    session.items(work)
}

#[cfg(test)]
mod tests;
