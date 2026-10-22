// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned lints_inherit_with and InheritableFields::lints; see lint/PROVENANCE.md.

use rawr_toml::document;

use super::super::super::{
    schema::{lints, workspace},
    session::Session,
    Diagnostic, Step, Tag,
};
use super::super::roots;

#[cfg(test)]
mod tests;

/// A normalized lint table only. This cannot stand for completed loading.
pub(in super::super) struct Complete<'a, 'd> {
    table: Option<&'a lints::Table<'d>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    /// A present table has had workspace inheritance removed, even when empty.
    pub(in super::super) fn table(&self) -> Option<&'a lints::Table<'d>> {
        self.table
    }
}

pub(in super::super) enum Progress<'a, 'd> {
    Complete(Complete<'a, 'd>),
    Workspace(Workspace<'a, 'd>),
}

/// The caller must resolve this through its existing lazy workspace machinery.
pub(in super::super) struct Workspace<'a, 'd> {
    original: &'a lints::Inherited<'d>,
}

pub(in super::super) fn normalize<'a, 'd, A: document::Admission>(
    original: Option<&'a lints::Inherited<'d>>,
    session: &mut Session<'_, A>,
) -> Step<Progress<'a, 'd>> {
    session.check()?;
    if let Some(original) = original.filter(|value| value.workspace()) {
        // Overrides reject before the lazy workspace getter is reached.
        if original.table().tools().len() != 0 {
            return session.reject(Diagnostic::at(
                Tag::LintsOverride,
                original.declared().and_then(|key| key.span),
            ));
        }
        session.items(1)?;
        return Ok(Progress::Workspace(Workspace { original }));
    }
    session.items(1)?;
    Ok(Progress::Complete(Complete {
        table: original.map(lints::Inherited::table),
    }))
}

impl<'a, 'd> Workspace<'a, 'd> {
    pub(in super::super) fn resume_declared<A: document::Admission>(
        self,
        root: roots::Root<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<Complete<'a, 'd>> {
        session.check()?;
        let Some(workspace::Value::Lints(table)) = root
            .declaration
            .and_then(|root| root.get(workspace::Field::Lints))
        else {
            return session.reject(Diagnostic::at(
                Tag::LintsInheritedMissing,
                self.original.declared().and_then(|key| key.span),
            ));
        };
        session.items(1)?;
        Ok(Complete { table: Some(table) })
    }
}
