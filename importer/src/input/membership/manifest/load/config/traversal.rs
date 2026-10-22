// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Recorded normal config traversal; no native acquisition or current authority.

use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
};

use super::super::super::{session::Session, storage, Error, Step};
use super::super::path;
use super::{definition::Definition, files::Files, merge::tree::Tree};
use crate::{
    cargo,
    input::membership::{host, observed::discovery::Filename, Error as InputError},
};

/// Pinned load_values_from + walk_tree. The caller supplies governed pwd/home/
/// search-stop context and owners which outlive the returned merged tree.
pub(super) fn load<'a, 'image: 'a>(
    mut files: Files<'a, 'image>,
    pwd: &Path,
    home: &Path,
    search_stop: Option<&Path>,
    cwd: &Path,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Tree<'a, 'a>> {
    load_with_files(&mut files, pwd, home, search_stop, cwd, limits, session)
}

pub(super) fn load_with_files<'a, 'image: 'a>(
    files: &mut Files<'a, 'image>,
    pwd: &Path,
    home: &Path,
    search_stop: Option<&Path>,
    cwd: &Path,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Tree<'a, 'a>> {
    let mut result = Tree::empty(Definition::BuiltIn, session)?;
    // load_values_from owns home before entering walk_tree/ancestors.
    let home = path::join(Path::new(""), home, session)?;
    let mut seen = storage::Buffer::new();
    let test_root = session.environment(b"__CARGO_TEST_ROOT")?;
    if let Some(bytes) = test_root {
        session.string(bytes.len())?;
    }
    let test_root = test_root.and_then(|bytes| std::str::from_utf8(bytes).ok());
    let stop = test_root.map(Path::new).or(search_stop);
    let stop = stop
        .map(|stop| path::join(Path::new(""), stop, session))
        .transpose()?;
    let mut next = Some(pwd);
    while let Some(current) = next {
        session.items(1)?;
        session.string(current.as_os_str().len())?;
        next = current.parent();
        if let Some(stop) = stop.as_deref()
            && equal(current, stop, session)?
        {
            next = None;
        }
        let root = path::join(current, Path::new(".cargo"), session)?;
        files.reset_seen();
        if let Some(value) =
            files.load_discovered(&root, Filename::Config, true, true, cwd, limits, session)?
        {
            result = result.merge(value, false, session)?;
        }
        // The callback's full file/include conversion and merge must finish
        // before even asking for this canonicalization observation.
        let canonical = canonical(root, session)?;
        if !contains(seen.as_slice(), &canonical, session)? {
            seen.push(canonical, session)?;
        }
    }
    session.string(home.as_os_str().len())?;
    let resolved_home = if home.as_os_str().as_bytes().contains(&0) {
        None
    } else if !home.is_absolute() {
        return session.fail(Error::Host(InputError::Unobserved));
    } else {
        match session.canonicalize(home.as_os_str().as_bytes())? {
            Ok(bytes) => Some(path::join(
                Path::new(""),
                Path::new(OsStr::from_bytes(bytes)),
                session,
            )?),
            Err(_) => None,
        }
    };
    // unwrap_or evaluates home.to_path_buf even after canonical success.
    let fallback_home = path::join(Path::new(""), &home, session)?;
    let canonical_home = resolved_home.unwrap_or(fallback_home);
    if !contains(seen.as_slice(), &canonical_home, session)?
        && !contains(seen.as_slice(), &home, session)?
    {
        files.reset_seen();
        if let Some(value) =
            files.load_discovered(&home, Filename::Config, true, true, cwd, limits, session)?
        {
            result = result.merge(value, false, session)?;
        }
    }
    Ok(result)
}

fn canonical(raw: PathBuf, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<PathBuf> {
    session.string(raw.as_os_str().len())?;
    if raw.as_os_str().as_bytes().contains(&0) {
        // std's local InvalidInput selects the original spelling, without I/O.
        return Ok(raw);
    }
    if !raw.is_absolute() {
        // Relative query/CWD association is still unimplemented, not a pinned
        // source rejection or permission to narrow supported configurations.
        return session.fail(Error::Host(InputError::Unobserved));
    }
    match session.canonicalize(raw.as_os_str().as_bytes())? {
        Ok(bytes) => path::join(Path::new(""), Path::new(OsStr::from_bytes(bytes)), session),
        Err(_) => Ok(raw),
    }
}

fn contains<A: rawr_toml::document::Admission>(
    seen: &[PathBuf],
    path: &Path,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    for previous in seen {
        if equal(previous, path, session)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn equal<A: rawr_toml::document::Admission>(
    left: &Path,
    right: &Path,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    session.items(1)?;
    session.string(left.as_os_str().len())?;
    session.string(right.as_os_str().len())?;
    Ok(left == right)
}

#[cfg(test)]
mod tests;
