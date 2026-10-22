//! Governed absolute Unicode --config file arguments, then normal config merge.

use std::{os::unix::ffi::OsStrExt as _, path::Path};

use super::super::super::{session::Session, Diagnostic, Error, Step, Tag};
use super::super::path;
use super::{
    definition::Definition,
    files::{Files, Why},
    merge::tree::Tree,
    traversal,
};
use crate::{
    cargo,
    input::membership::{host, Error as InputError},
};

/// Borrowed governed context, not an arbitrary GlobalContext implementation.
#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub(super) pwd: &'a Path,
    pub(super) home: &'a Path,
    pub(super) search_stop: Option<&'a Path>,
    pub(super) cwd: &'a Path,
}

/// Arguments must be the actual governed package argv after Cargo's Unicode
/// parsing. This route emits absolute file paths, not successful inline TOML.
/// Already-initialized normal values avoid replaying GlobalContext::values().
/// Supplying them does not assert when their source initialization happened.
pub(super) fn load<'a, 'image: 'a>(
    mut files: Files<'a, 'image>,
    arguments: &[&str],
    initialized: Option<Tree<'a, 'a>>,
    context: Context<'_>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Tree<'a, 'a>> {
    load_with_files(&mut files, arguments, initialized, context, limits, session)
}

pub(super) fn load_with_files<'a, 'image: 'a>(
    files: &mut Files<'a, 'image>,
    arguments: &[&str],
    initialized: Option<Tree<'a, 'a>>,
    context: Context<'_>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Tree<'a, 'a>> {
    session.check()?;
    // configure calls merge_cli_args only for nonempty cli_config.
    let cli = if arguments.is_empty() {
        None
    } else {
        files.reset_seen();
        Some(table(files, arguments, context.cwd, limits, session)?)
    };
    // cli_args_as_table completes before the values_mut() initialization point.
    let normal = match initialized {
        Some(normal) => normal,
        None => traversal::load_with_files(
            files,
            context.pwd,
            context.home,
            context.search_stop,
            context.cwd,
            limits,
            session,
        )?,
    };
    match cli {
        Some(cli) => normal.merge(cli, true, session),
        None => Ok(normal),
    }
}

fn table<'a, 'image: 'a>(
    files: &mut Files<'a, 'image>,
    arguments: &[&str],
    cwd: &Path,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Tree<'a, 'a>> {
    let mut result = Tree::empty(Definition::Cli(None), session)?;
    for argument in arguments {
        session.items(1)?;
        let argument_path = Path::new(argument);
        let path = path::join(cwd, argument_path, session)?;
        session.string(argument.len())?;
        if !argument_path.is_absolute() {
            // This route's argv association is incomplete, not a Cargo input
            // restriction or a fabricated inline parsing outcome.
            return session.fail(Error::Host(InputError::Unobserved));
        }
        session.string(path.as_os_str().len())?;
        let exists = if path.as_os_str().as_bytes().contains(&0) {
            // Source Path::exists locally fails native conversion. It does not
            // query recorded metadata or invent a native errno for InvalidInput.
            false
        } else {
            session.exists(path.as_os_str().as_bytes())?
        };
        if !exists {
            // Every governed argument begins with '/'. DocumentMut parsing
            // rejects it before deserialization/credential checks. This does
            // not assert the parser's internal first error or failure timing.
            return session.reject(Diagnostic::at(Tag::ConfigCliParse, None));
        }
        let value = files.load(&path, true, Why::Cli, cwd, limits, session)?;
        let value = files.includes_fresh(value, Why::Cli, cwd, limits, session)?;
        result = result.merge(value, true, session)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
