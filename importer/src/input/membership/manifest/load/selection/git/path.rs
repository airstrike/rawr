// Copyright the gitoxide contributors. MIT OR Apache-2.0.
// See PROVENANCE.md and LICENSE-*.

//! gix native lexical paths, preserving spelling and conditional ownership.

use std::{
    borrow::Cow,
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};

use rawr_toml::document;

use crate::input::membership::manifest::{load, session::Session, Error, Step};

pub(super) mod cursor;
pub(super) mod file;

/// Shape classification only; no repository existence or trust follows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Submodule,
    LinkedWorktree,
    Common,
}

/// gix-path 0.12.1 normalize. In particular, the no-parent branch is an
/// identity operation, and walking above an absolute root fails.
pub(super) fn normalize<'a, A: document::Admission>(
    input: Cow<'a, Path>,
    current_dir: &Path,
    session: &mut Session<'_, A>,
) -> Step<Option<Cow<'a, Path>>> {
    session.string(input.as_os_str().len())?;
    session.items(input.as_os_str().len())?;
    if !input
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Ok(Some(input));
    }
    let mut current_dir_option = Some(current_dir);
    let was_relative = input.is_relative();
    session.items(input.as_os_str().len())?;
    let components = input.components();
    let mut output = cursor::Cursor::empty(session)?;
    for component in components {
        session.items(1)?;
        if let Component::ParentDir = component {
            session.items(output.path().as_os_str().len())?;
            let path_was_dot = output.path() == Path::new(".");
            if output.path().as_os_str().is_empty() || path_was_dot {
                let Some(directory) = current_dir_option.take() else {
                    return Ok(None);
                };
                output.push(directory, session)?;
            }
            if !output.pop(session)? {
                return Ok(None);
            }
        } else {
            output.push(Path::new(component.as_os_str()), session)?;
        }
    }
    let same_as_current = if output.path().as_os_str().is_empty() {
        true
    } else {
        session.string(current_dir.as_os_str().len())?;
        let Some(work) = output
            .path()
            .as_os_str()
            .len()
            .checked_add(current_dir.as_os_str().len())
        else {
            return session.fail(Error::Overflow);
        };
        session.items(work)?;
        output.path() == current_dir
    };
    if same_as_current && was_relative {
        Ok(Some(Cow::Borrowed(Path::new("."))))
    } else {
        Ok(Some(Cow::Owned(output.into_path())))
    }
}

/// Retain an already owned normalization result; copy only the borrowed branch
/// reached by the source's subsequent Cow::into_owned call.
pub(super) fn into_owned<A: document::Admission>(
    path: Cow<'_, Path>,
    session: &mut Session<'_, A>,
) -> Step<PathBuf> {
    session.check()?;
    match path {
        Cow::Owned(path) => Ok(path),
        Cow::Borrowed(path) => {
            session.items(path.as_os_str().len())?;
            load::path::copy(path, session)
        }
    }
}

pub(super) fn repository_kind<A: document::Admission>(
    git_dir: &Path,
    session: &mut Session<'_, A>,
) -> Step<Option<Kind>> {
    session.string(git_dir.as_os_str().len())?;
    session.items(git_dir.as_os_str().len())?;
    if git_dir.file_name() == Some(OsStr::new(".git")) {
        return Ok(Some(Kind::Common));
    }
    let mut last_component = None;
    session.items(git_dir.as_os_str().len())?;
    for component in git_dir.components().rev().skip(1) {
        session.items(1)?;
        if component.as_os_str() == OsStr::new(".git") {
            break;
        }
        last_component = Some(component.as_os_str());
    }
    // The source deliberately ignores whether its search found a `.git`.
    // Keep the remembered-component rule even for relative spellings.
    Ok(match last_component {
        Some(component) if component == OsStr::new("modules") => Some(Kind::Submodule),
        Some(component) if component == OsStr::new("worktrees") => Some(Kind::LinkedWorktree),
        _ => None,
    })
}

pub(super) fn without_dot_git_dir<A: document::Admission>(
    mut path: PathBuf,
    session: &mut Session<'_, A>,
) -> Step<PathBuf> {
    session.string(path.as_os_str().len())?;
    let Some(work) = path.as_os_str().len().checked_mul(3) else {
        return session.fail(Error::Overflow);
    };
    session.items(work)?;
    if path.file_name().and_then(OsStr::to_str) == Some(".git") {
        path.pop();
    }
    Ok(path)
}
