//! Option<ConfigRelativePath> over already loaded, completely merged tables.

use rawr_toml::document;
use std::path::{Path, PathBuf};

use super::super::super::{session::Session, Diagnostic, Step, Tag};
use super::{
    definition::Definition,
    key::Key,
    merge::tree::{lookup::Found, Tree},
};
use crate::input::membership::host;

mod origin;
pub(super) mod string;

/// Supplied only at the loader's reached values()/credential-values boundary.
/// This read-only bundle does not perform discovery or authorize eager loading.
pub(super) struct Tables<'a> {
    ordinary: &'a Tree<'a, 'a>,
    credentials: Option<&'a Tree<'a, 'a>>,
}

pub(super) struct Relative<'a> {
    text: &'a str,
    origin: origin::Origin<'a>,
}

impl Relative<'_> {
    pub(super) fn resolve<A: document::Admission>(
        &self,
        cwd: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<PathBuf> {
        self.origin.definition().resolve(self.text, cwd, session)
    }
}

impl<'a> Tables<'a> {
    pub(super) fn from_trees(
        ordinary: &'a Tree<'a, 'a>,
        credentials: Option<&'a Tree<'a, 'a>>,
    ) -> Self {
        Self {
            ordinary,
            credentials,
        }
    }

    fn cv<A: document::Admission>(
        &self,
        key: &Key<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Found<'a>>> {
        session.check()?;
        if let Some(credentials) = self.credentials
            && let Some(value) = credentials.find(key, session)?
        {
            return Ok(Some(value));
        }
        self.ordinary.find(key, session)
    }

    pub(super) fn optional_path<'r, 'image: 'r>(
        &'r self,
        key: &'r Key<'_>,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<Relative<'r>>> {
        optional_path(key, session, |key, session| self.cv(key, session))
    }
}

/// The transparent ConfigRelativePath wrapper adds only later path resolution.
pub(super) fn optional_path<'r, 'image: 'r>(
    key: &'r Key<'_>,
    session: &mut Session<'_, host::Host<'image, '_>>,
    cv: impl FnMut(&Key<'_>, &mut Session<'_, host::Host<'image, '_>>) -> Step<Option<Found<'r>>>,
) -> Step<Option<Relative<'r>>> {
    string::get(key, session, cv).map(|value| {
        value.map(|value| Relative {
            text: value.text,
            origin: value.origin,
        })
    })
}

#[cfg(test)]
mod tests;
