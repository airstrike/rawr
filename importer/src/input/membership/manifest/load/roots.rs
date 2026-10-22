// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: path/LICENSE-MIT and path/LICENSE-APACHE beside this module.

//! Pinned Cargo declaration cache, not a cache of successfully loaded manifests.
//! See roots/PROVENANCE.md for source order and key distinctions.

use std::path::{Path, PathBuf};

use rawr_toml::document;

use super::super::{
    schema::{primitive, workspace},
    session::Session,
    storage, Error, Step,
};
use super::path;

#[derive(Clone, Copy)]
pub(super) struct Root<'a, 'd> {
    pub(super) directory: &'a Path,
    pub(super) declaration: Option<&'a workspace::Workspace<'d>>,
}

struct Entry<'a, 'd> {
    key: &'a Path,
    root: Root<'a, 'd>,
}

pub(super) struct Roots<'a, 'd>(storage::Buffer<Entry<'a, 'd>>);

impl<'a, 'd> Roots<'a, 'd> {
    pub(super) const fn new() -> Self {
        Self(storage::Buffer::new())
    }

    /// Keys are supplied by the source call site. In particular, do not turn
    /// inheritable_from_path's post-load manifest-file key into a directory key.
    pub(super) fn insert<A: document::Admission>(
        &mut self,
        key: &'a Path,
        root: Root<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        match self
            .0
            .as_slice()
            .binary_search_by(|entry| entry.key.cmp(key))
        {
            Ok(index) => {
                session.items(1)?;
                self.0.as_mut_slice()[index].root = root;
            }
            Err(index) => {
                self.0.push(Entry { key, root }, session)?;
                self.0.as_mut_slice()[index..].rotate_right(1);
            }
        }
        Ok(())
    }

    pub(super) fn get<A: document::Admission>(
        &self,
        key: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Root<'a, 'd>>> {
        session.check()?;
        Ok(self
            .0
            .as_slice()
            .binary_search_by(|entry| entry.key.cmp(key))
            .ok()
            .map(|index| self.0.as_slice()[index].root))
    }

    /// This is only the first, cache-based branch of workspace discovery.
    /// A miss must continue to Cargo's candidate iterator and full loader.
    pub(super) fn ancestor<A: document::Admission>(
        &self,
        manifest: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Option<PathBuf>> {
        session.check()?;
        for current in manifest.ancestors().skip(1) {
            if let Some(root) = self.get(current, session)?
                && !root.excluded(manifest, session)?
            {
                return path::join(current, Path::new("Cargo.toml"), session).map(Some);
            }
        }
        Ok(None)
    }
}

impl Root<'_, '_> {
    pub(super) fn excluded<A: document::Admission>(
        self,
        manifest: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        // Cargo evaluates excludes first, then members even if none excluded.
        // These are lexical prefixes, not glob-expanded workspace membership.
        let excluded = self.prefix(workspace::Field::Exclude, manifest, session)?;
        let member = self.prefix(workspace::Field::Members, manifest, session)?;
        Ok(!member && excluded)
    }

    fn prefix<A: document::Admission>(
        self,
        field: workspace::Field,
        manifest: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        let Some(declaration) = self.declaration else {
            return Ok(false);
        };
        let values = match declaration.get(field) {
            Some(workspace::Value::Local(primitive::Value::Strings(values))) => values,
            None => return Ok(false),
            _ => return session.fail(Error::State),
        };
        for value in values.as_slice() {
            let joined = path::join(self.directory, Path::new(value.as_str()), session)?;
            if manifest.starts_with(joined) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
