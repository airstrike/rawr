use std::fs;
use std::path::{Component, Path, PathBuf};

use super::super::{Budget, Result};
use super::{io, Error};
use crate::store;

pub(super) fn input(path: &Path, budget: &mut Budget<'_>) -> Result<()> {
    let bytes =
        store::path::input_allocation(path).map_err(|error| budget.failure(error.into()))?;
    if bytes != 0 {
        budget.storage(1, bytes)?;
    }
    Ok(())
}

pub(in crate::input) fn metadata(
    path: &Path,
    no_follow: bool,
    budget: &mut Budget<'_>,
) -> Result<fs::Metadata> {
    budget.paths(1)?;
    input(path, budget)?;
    if no_follow {
        fs::symlink_metadata(path)
    } else {
        fs::metadata(path)
    }
    .map_err(|source| io("inspect freshness path", source))
}

pub(in crate::input) fn joined(
    parent: &Path,
    child: &Path,
    budget: &mut Budget<'_>,
) -> Result<PathBuf> {
    let bytes = parent
        .as_os_str()
        .len()
        .checked_add(child.as_os_str().len())
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| budget.overflow())?;
    budget.storage(1, bytes)?;
    let mut output = PathBuf::new();
    output
        .try_reserve_exact(bytes)
        .map_err(|error| budget.failure(super::super::Error::Allocation(error)))?;
    output.push(parent);
    output.push(child);
    Ok(output)
}

fn absolute(path: &Path, base: &Path, budget: &mut Budget<'_>) -> Result<PathBuf> {
    if path.is_absolute() {
        return budget.owned_path(path.as_os_str().as_encoded_bytes());
    }
    if !base.is_absolute() {
        return Err(Error::Policy("relative observation requires an absolute owner base").into());
    }
    joined(base, path, budget)
}

pub(in crate::input) fn normalized(
    path: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<PathBuf> {
    let absolute = absolute(path, base, budget)?;
    normalize(&absolute, budget)
}

fn normalize(path: &Path, budget: &mut Budget<'_>) -> Result<PathBuf> {
    budget.storage(1, path.as_os_str().len())?;
    let mut output = PathBuf::new();
    output
        .try_reserve_exact(path.as_os_str().len())
        .map_err(|error| budget.failure(super::super::Error::Allocation(error)))?;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !output.pop() {
                    return Err(Error::Policy("path-escape").into());
                }
            }
        }
    }
    Ok(output)
}

pub(in crate::input) fn canonical(
    path: &Path,
    base: &Path,
    directory: bool,
    budget: &mut Budget<'_>,
) -> Result<PathBuf> {
    let absolute = absolute(path, base, budget)?;
    let normalized = normalize(&absolute, budget)?;
    budget.paths(1)?;
    input(&absolute, budget)?;
    let output = store::path::canonical_allocation();
    // The original heap account covers the libc allocation and copied native
    // output; the copy is owned once there, the probe above on paths.
    budget.admit(2, output - std::mem::size_of::<PathBuf>())?;
    let canonical = if !directory && crate::input::canonical::regular_file(&normalized) {
        normalized.clone()
    } else {
        fs::canonicalize(&absolute).map_err(|source| io("canonicalize freshness path", source))?
    };
    let kind = match metadata(&canonical, false, budget) {
        Ok(metadata) => {
            if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            }
        }
        Err(super::super::Error::Observation(Error::Io { .. })) => false,
        Err(error) => return Err(error),
    };
    if !kind || canonical != normalized {
        return Err(Error::Policy(if directory {
            "must be a canonical directory without symlink traversal"
        } else {
            "must be a canonical regular file without symlink traversal"
        })
        .into());
    }
    Ok(canonical)
}

pub(super) fn reject_symlinks(path: &Path, budget: &mut Budget<'_>) -> Result<()> {
    budget.storage(1, path.as_os_str().len())?;
    let mut current = PathBuf::new();
    current
        .try_reserve_exact(path.as_os_str().len())
        .map_err(|error| budget.failure(super::super::Error::Allocation(error)))?;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => continue,
        }
        match metadata(&current, true, budget) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Policy("symlink-input").into())
            }
            Ok(_) => {}
            Err(super::super::Error::Observation(Error::Io { source, .. }))
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                break
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn text(path: &Path, budget: &mut Budget<'_>) -> Result<Box<str>> {
    let text = path
        .to_str()
        .ok_or(Error::Policy("input path is not valid Unicode"))?;
    budget.limited("input path bytes", crate::input::LINE_BYTES, text.len())?;
    budget.owned_text(text)
}

pub(super) fn terminated(path: &Path, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let length = bytes
        .len()
        .checked_add(1)
        .ok_or_else(|| budget.overflow())?;
    let mut output = budget.owned(length)?;
    output.extend_from_slice(bytes);
    output.push(0);
    Ok(output)
}
