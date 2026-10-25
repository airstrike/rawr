// Copyright the Cargo contributors. MIT OR Apache-2.0.
//! Cargo's explicit-include physical selection branch. Git discovery remains a
//! separate prerequisite for packages without includes; no guessed repository
//! result or partial selector authority is supplied here.

use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use super::{driver, formatted, ignore, path, Warnings};
use crate::input::membership::{
    host,
    manifest::{session::Session, storage::Buffer, Error, Step},
    observed,
};

mod git;
mod walk;

pub(super) enum Failure {
    Rule,
    Compile,
    Walk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    File,
    Symlink,
    Other,
}

pub(super) struct Entry {
    pub(super) path: PathBuf,
    pub(super) kind: Kind,
    pub(super) under_symlink_dir: bool,
}

/// Owned physical facts in source encounter order, with reached warning text.
/// This stage is not current source or complete Cargo-command authority.
pub(super) struct Files<'package, 'inputs> {
    package: &'package driver::Package<'inputs>,
    entries: Buffer<Entry>,
    warnings: Warnings,
}

impl Files<'_, '_> {
    pub(super) fn entries(&self) -> &[Entry] {
        self.entries.as_slice()
    }
    pub(super) fn warnings(&self) -> &[String] {
        self.warnings.as_slice()
    }

    pub(super) fn project(
        &self,
        include_lockfile: bool,
        vcs: bool,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<crate::input::membership::archive::Projection> {
        let mut selected = Buffer::new();
        for entry in self.entries.as_slice() {
            selected.push(entry.path.as_os_str().as_bytes(), session)?;
        }
        self.package
            .project(selected.as_slice(), include_lockfile, vcs, session)
    }
}

/// None leaves the discovery-dependent branch unresolved. It never means an
/// empty selection or a filesystem fallback for a package without includes.
pub(super) fn included<'package, 'inputs>(
    package: &'package driver::Package<'inputs>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Option<Files<'package, 'inputs>>, Failure>> {
    session.check()?;
    if package.include.is_empty() {
        return Ok(Ok(None));
    }
    if !package.root.is_absolute() {
        return session.fail(Error::State);
    }

    // Pinned _list_files constructs both owners in this order, even though
    // explicit includes make the exclude query branch unreachable.
    let mut exclude_builder = ignore::Builder::prepare(package.root, session)?;
    for rule in package.exclude {
        if exclude_builder.add(rule, session)?.is_err() {
            return Ok(Err(Failure::Rule));
        }
    }
    let mut exclude = match exclude_builder.build(session)? {
        Ok(matcher) => matcher,
        Err(_) => return Ok(Err(Failure::Compile)),
    };
    let mut include_builder = ignore::Builder::prepare(package.root, session)?;
    for rule in package.include {
        if include_builder.add(rule, session)?.is_err() {
            return Ok(Err(Failure::Rule));
        }
    }
    let mut include = match include_builder.build(session)? {
        Ok(matcher) => matcher,
        Err(_) => return Ok(Err(Failure::Compile)),
    };
    let mut filter = Filter {
        root: package.root,
        _exclude: &mut exclude,
        include: &mut include,
    };
    let mut entries = Buffer::new();
    let mut warnings = Warnings::new();
    if append(
        package.root,
        &mut entries,
        true,
        &mut filter,
        &mut warnings,
        session,
    )?
    .is_err()
    {
        return Ok(Err(Failure::Walk));
    }
    Ok(Ok(Some(Files {
        package,
        entries,
        warnings,
    })))
}

struct Filter<'root, 'matchers> {
    root: &'root Path,
    // Borrow the source locals: include matcher/builder then exclude matcher/
    // builder must retire in that order, including their captured engine owners.
    _exclude: &'matchers mut ignore::compiled::Matcher,
    include: &'matchers mut ignore::compiled::Matcher,
}
impl Filter<'_, '_> {
    fn accepts(
        &mut self,
        candidate: &Path,
        directory: bool,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<bool> {
        session.items(1)?;
        session.string(candidate.as_os_str().len())?;
        let Ok(relative) = candidate.strip_prefix(self.root) else {
            return Ok(false);
        };
        if matches!(
            relative.as_os_str().as_bytes(),
            b"Cargo.toml" | b"Cargo.lock"
        ) || directory
        {
            return Ok(true);
        }
        self.include.is_ignored(relative, false, session)
    }
}

fn append(
    root: &Path,
    entries: &mut Buffer<Entry>,
    is_root: bool,
    filter: &mut Filter<'_, '_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<(), walk::Failure>> {
    let mut walk = walk::Walk::new(root, is_root, session)?;
    let mut current_symlink_dir: Option<PathBuf> = None;
    while let Some(result) = walk.next(session)? {
        match result {
            Ok(entry) => {
                let directory = entry.kind == observed::Kind::Directory;
                let at_root = is_root && entry.depth == 0;
                let mut accepted = at_root || filter.accepts(&entry.path, directory, session)?;
                if accepted && directory {
                    if !at_root {
                        let manifest = path::join(&entry.path, Path::new("Cargo.toml"), session)?;
                        accepted = !session.exists(manifest.as_os_str().as_bytes())?;
                    }
                    if accepted
                        && is_root
                        && entry.depth == 1
                        && entry.path.file_name().and_then(|name| name.to_str()) == Some("target")
                    {
                        accepted = false;
                    }
                }
                if !accepted {
                    if directory {
                        walk.skip_current_dir(session)?;
                    }
                    continue;
                }

                if !current_symlink_dir
                    .as_ref()
                    .is_some_and(|parent| entry.path.starts_with(parent))
                {
                    current_symlink_dir = if directory && entry.followed {
                        Some(path::copy(&entry.path, session)?)
                    } else {
                        None
                    };
                }
                if matches!(entry.kind, observed::Kind::File | observed::Kind::Symlink) {
                    let kind = if entry.followed || entry.kind == observed::Kind::Symlink {
                        Kind::Symlink
                    } else {
                        Kind::File
                    };
                    entries.push(
                        Entry {
                            path: entry.path,
                            kind,
                            under_symlink_dir: current_symlink_dir.is_some(),
                        },
                        session,
                    )?;
                }
            }
            Err(walk::Failure::Loop {
                ancestor, child, ..
            }) => {
                let text = formatted(
                    format_args!(
                        "File system loop found: {} points to an ancestor {}",
                        child.display(),
                        ancestor.display(),
                    ),
                    session,
                )?;
                warnings.push_formatted(text, session)?;
            }
            Err(failure @ walk::Failure::Io { path: None, .. }) => return Ok(Err(failure)),
            Err(walk::Failure::Io {
                path: Some(error_path),
                ..
            }) => {
                let directory = session.is_dir(error_path.as_os_str().as_bytes())?;
                if filter.accepts(&error_path, directory, session)? {
                    entries.push(
                        Entry {
                            path: path::copy(&error_path, session)?,
                            kind: Kind::Other,
                            under_symlink_dir: false,
                        },
                        session,
                    )?;
                }
            }
        }
    }
    walk.finish(session)?;
    Ok(Ok(()))
}
