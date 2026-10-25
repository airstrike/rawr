//! Recorded configuration files and depth-first include merge continuations.

use rawr_toml::document;
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use super::super::super::{
    session::Session,
    storage::{self, slots},
    Diagnostic, Error, Step, Tag,
};
use super::super::path;
use super::{
    definition::Definition,
    merge::tree::{include::Include, Tree},
    values::Values,
};
use crate::{cargo, input::membership::host};

#[derive(Clone, Copy)]
pub(super) enum Why {
    Discovery,
    Cli,
}

/// The caller scopes these owners outside all returned trees. The visited
/// set deliberately survives successive file loads until a new Files is formed.
pub(super) struct Files<'a, 'image> {
    paths: slots::Writer<'a, PathBuf>,
    documents: slots::Writer<'a, document::Document<'image>>,
    seen: storage::Buffer<&'a Path>,
}

struct Frame<'a> {
    value: Tree<'a, 'a>,
    root: Tree<'a, 'a>,
    includes: std::vec::IntoIter<Include<'a, 'a>>,
}

impl<'a, 'image: 'a> Files<'a, 'image> {
    /// Normal discovered files each start a new include-cycle set, while their
    /// path/document owners continue to back values already merged by callers.
    pub(super) fn reset_seen(&mut self) {
        self.seen = storage::Buffer::new();
    }

    pub(super) fn from_owners(
        paths: slots::Writer<'a, PathBuf>,
        documents: slots::Writer<'a, document::Document<'image>>,
    ) -> Self {
        Self {
            paths,
            documents,
            seen: storage::Buffer::new(),
        }
    }

    pub(super) fn load(
        &mut self,
        entry: &Path,
        includes: bool,
        why: Why,
        cwd: &Path,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Tree<'a, 'a>> {
        let entry = path::join(Path::new(""), entry, session)?;
        let value = self.read(entry, why, limits, session)?;
        if !includes {
            return Ok(value);
        }
        self.includes(value, why, cwd, limits, session)
    }

    /// Compose one reached physical selection with complete file/include load.
    /// No canonicalization or ancestor/home traversal is performed here. The
    /// caller still scopes a fresh Files visited set where pinned Cargo does.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_discovered(
        &mut self,
        directory: &Path,
        filename: crate::input::membership::observed::discovery::Filename,
        warn: bool,
        includes: bool,
        cwd: &Path,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<Tree<'a, 'a>>> {
        let Some(selected) = super::discovery::select(directory, filename, warn, session)? else {
            return Ok(None);
        };
        self.load(&selected, includes, Why::Discovery, cwd, limits, session)
            .map(Some)
    }

    /// Also reached by inline CLI configuration, after complete conversion.
    /// The CLI argument layer reaches this second pass with an independent set,
    /// without resetting the set shared by its complete file-argument loads.
    pub(super) fn includes_fresh(
        &mut self,
        value: Tree<'a, 'a>,
        why: Why,
        cwd: &Path,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Tree<'a, 'a>> {
        let shared = std::mem::replace(&mut self.seen, storage::Buffer::new());
        let result = self.includes(value, why, cwd, limits, session);
        self.seen = shared;
        result
    }

    /// Reached after complete conversion; caller chooses the visited-set scope.
    pub(super) fn includes(
        &mut self,
        value: Tree<'a, 'a>,
        why: Why,
        cwd: &Path,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Tree<'a, 'a>> {
        let mut frames = storage::Buffer::new();
        frames.push(Self::frame(value, session)?, session)?;
        loop {
            session.items(1)?;
            let Some(mut frame) = frames.pop() else {
                return session.fail(Error::State);
            };
            if let Some(include) = frame.includes.next() {
                let resolved = include.resolve(cwd, session)?;
                frames.push(frame, session)?;
                if let Some(path) = resolved {
                    let value = self.read(path, why, limits, session)?;
                    frames.push(Self::frame(value, session)?, session)?;
                }
                continue;
            }
            let complete = frame.root.merge(frame.value, true, session)?;
            match frames.pop() {
                Some(mut parent) => {
                    parent.root = parent.root.merge(complete, true, session)?;
                    frames.push(parent, session)?;
                }
                None => return Ok(complete),
            }
        }
    }

    fn frame<A: document::Admission>(
        mut value: Tree<'a, 'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Frame<'a>> {
        let includes = value.includes(true, session)?.into_iter();
        let root = Tree::empty(value.definition(), session)?;
        Ok(Frame {
            value,
            root,
            includes,
        })
    }

    fn read(
        &mut self,
        path: PathBuf,
        why: Why,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Tree<'a, 'a>> {
        session.check()?;
        session.string(path.as_os_str().len())?;
        for previous in self.seen.as_slice() {
            session.items(1)?;
            session.string(previous.as_os_str().len())?;
            // Path equality compares components; raw byte equality is wrong
            // for redundant separators and internal current-directory parts.
            if *previous == path {
                return session.reject(Diagnostic::at(Tag::ConfigCycle, None));
            }
        }
        let path = self.paths.push(path, session)?.as_path();
        self.seen.push(path, session)?;
        // Like read_to_string's native path conversion, fail before any read.
        // Optional includes have already handled this predicate as false.
        if path.as_os_str().as_bytes().contains(&0) {
            return session.fail(Error::ConfigPathNul);
        }
        let text = session.text(path.as_os_str().as_bytes(), limits)?;
        let document = session.parse_text(text, limits)?;
        let document = self.documents.push(document, session)?;
        let definition = match why {
            Why::Discovery => Definition::Path(path),
            Why::Cli => Definition::Cli(Some(path)),
        };
        let values = Values::from_document(document, definition, session)?;
        Tree::from_values(values, session)
    }
}

#[cfg(test)]
mod tests;
