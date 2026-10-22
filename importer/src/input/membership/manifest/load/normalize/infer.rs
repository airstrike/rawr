// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Ordered pinned target inference, without loaded-manifest authority.

use super::super::{
    super::{session::Session, storage, Step},
    path,
};
use crate::input::membership::{host, observed};
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub(in super::super) struct Item {
    name: String,
    path: PathBuf,
}
impl Item {
    #[cfg(test)]
    pub(super) fn fixture(name: &str, path: &str) -> Self {
        Self {
            name: name.to_owned(),
            path: PathBuf::from(path),
        }
    }
    pub(in super::super) fn name(&self) -> &str {
        &self.name
    }
    pub(in super::super) fn path(&self) -> &Path {
        &self.path
    }
    pub(in super::super) fn into_parts(self) -> (String, PathBuf) {
        (self.name, self.path)
    }
}

pub(in super::super) fn bins(
    root: &Path,
    package_name: &str,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<storage::Buffer<Item>> {
    let mut output = storage::Buffer::new();
    let main = path::join(root, Path::new("src/main.rs"), session)?;
    if session.exists(main.as_os_str().as_bytes())? {
        let main = path::join(Path::new(""), Path::new("src/main.rs"), session)?;
        let name = super::target::owned(&[package_name], false, session)?;
        output.push(Item { name, path: main }, session)?;
    }
    append(root, Path::new("src/bin"), &mut output, session)?;
    Ok(output)
}

pub(in super::super) fn directory(
    root: &Path,
    relative: &Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<storage::Buffer<Item>> {
    let mut output = storage::Buffer::new();
    append(root, relative, &mut output, session)?;
    Ok(output)
}

fn append(
    root: &Path,
    relative: &Path,
    output: &mut storage::Buffer<Item>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<()> {
    let directory = path::join(root, relative, session)?;
    let Ok(mut cursor) = session.directory_stream(directory.as_os_str().as_bytes())? else {
        return Ok(());
    };
    while let host::stream::Next::Entry(entry) = session.stream_next(&mut cursor)? {
        session.string(entry.name().len())?;
        let Ok(name) = std::str::from_utf8(entry.name()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let is_directory = session.stream_inspect(entry)? == Ok(observed::Kind::Directory);
        let entry_path = path::join(&directory, Path::new(name), session)?;
        if is_directory {
            let main = path::join(&entry_path, Path::new("main.rs"), session)?;
            let Some(name) = entry_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let name = super::target::owned(&[name], false, session)?;
            if session.exists(main.as_os_str().as_bytes())? {
                let main = relative_path(root, main, session)?;
                output.push(Item { name, path: main }, session)?;
            }
        } else if entry_path
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("rs")
        {
            let Some(stem) = entry_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let name = super::target::owned(&[stem], false, session)?;
            let path = relative_path(root, entry_path, session)?;
            output.push(Item { name, path }, session)?;
        }
    }
    session.stream_finish(cursor)
}

fn relative_path(
    root: &Path,
    value: PathBuf,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<PathBuf> {
    session.string(root.as_os_str().len())?;
    session.string(value.as_os_str().len())?;
    match value.strip_prefix(root) {
        Ok(relative) => path::join(Path::new(""), relative, session),
        Err(_) => Ok(value),
    }
}

#[cfg(test)]
mod tests;
