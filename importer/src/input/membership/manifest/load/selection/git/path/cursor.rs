// Copyright the gitoxide contributors and Rust Project Developers.
// MIT OR Apache-2.0; see ../PROVENANCE.md.
//! Source PathBuf mutation with retained admitted capacity, never allocator slack.

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use rawr_toml::document;

use crate::input::membership::manifest::{load, session::Session, Step};

pub(in super::super) struct Cursor {
    bytes: Vec<u8>,
    capacity: usize,
}

impl Cursor {
    pub(in super::super) fn copy<A: document::Admission>(
        path: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(path.as_os_str().len())?;
        let copy = load::path::copy(path, session)?;
        Ok(Self {
            bytes: copy.into_os_string().into_vec(),
            capacity: path.as_os_str().len(),
        })
    }

    pub(in super::super) fn empty<A: document::Admission>(
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        Ok(Self {
            bytes: Vec::new(),
            capacity: 0,
        })
    }

    pub(in super::super) fn path(&self) -> &Path {
        Path::new(OsStr::from_bytes(&self.bytes))
    }

    pub(in super::super) fn into_path(self) -> PathBuf {
        PathBuf::from(OsString::from_vec(self.bytes))
    }

    pub(in super::super) fn push<A: document::Admission>(
        &mut self,
        suffix: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if suffix.is_absolute() {
            self.bytes.clear();
        } else if self.bytes.last().is_some_and(|&byte| byte != b'/') {
            super::file::append(&mut self.bytes, &mut self.capacity, b"/", session)?;
        }
        super::file::append(
            &mut self.bytes,
            &mut self.capacity,
            suffix.as_os_str().as_bytes(),
            session,
        )
    }

    pub(in super::super) fn pop<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(self.bytes.len())?;
        let Some(parent) = self.path().parent() else {
            return Ok(false);
        };
        let length = parent.as_os_str().len();
        self.bytes.truncate(length);
        Ok(true)
    }
}
