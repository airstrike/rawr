//! Source-ordered read entry into retained pre-normalization state.
//! Complete normalization and real/virtual manifest conversion remain required.

use super::super::{
    schema::manifest::Original, session::Session, storage::slots::Writer, value, Diagnostic, Step,
    Tag,
};
use super::{convert::source, embedded, roots::Roots, Context, Preparing};
use crate::{cargo, input::membership::host};
use rawr_toml::document::Document;
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

pub(super) fn is_embedded(path: &Path) -> bool {
    let extension = path.extension();
    extension == Some(OsStr::new("rs")) || extension.is_none()
}

pub(super) struct Body<'image> {
    raw: &'image str,
    expanded: Option<String>,
}
impl Body<'_> {
    fn text(&self) -> &str {
        self.expanded.as_deref().unwrap_or(self.raw)
    }
}

/// Owners live outside this cursor in dependency order, as required.
/// Growing one tier cannot move a value already borrowed by a later tier.
pub(super) struct Reader<'body, 'document, 'original, 'image> {
    pub(super) bodies: Writer<'body, Body<'image>>,
    pub(super) documents: Writer<'document, Document<'body>>,
    pub(super) originals: Writer<'original, Original<'document>>,
}
/// The exact source identity supplied to this reached manifest read. Complete
/// normalization, conversion and diagnostic attachment still follow.
pub(super) struct Sourced<'a, 'd, 's> {
    pub(super) source: source::Id<'s>,
    pub(super) preparing: Preparing<'a, 'd>,
}
pub(super) struct Normalizing<'a, 'd, 'owner, 's> {
    pub(super) source: source::Id<'s>,
    pub(super) progress: super::driver::Progress<'a, 'd, 'owner>,
}
impl<'a, 'd: 'a, 's> Sourced<'a, 'd, 's> {
    pub(super) fn normalize<'owner, 'image: 'config, 'config>(
        self,
        targets: &mut Writer<'owner, super::normalize::all_targets::Owner<'a, 'd>>,
        config: &mut super::config::Owner<'config, 'image>,
        public_dependency: bool,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Normalizing<'a, 'd, 'owner, 's>, String>>
    where
        'a: 'owner,
    {
        super::driver::start(
            self.preparing,
            targets,
            config,
            public_dependency,
            limits,
            session,
        )
        .map(|result| {
            result.map(|progress| Normalizing {
                source: self.source,
                progress,
            })
        })
    }
}

impl<'body, 'document, 'original, 'image> Reader<'body, 'document, 'original, 'image>
where
    'image: 'body,
    'body: 'document,
    'document: 'original,
{
    /// Workspace loader call-site order: SourceId::for_manifest_path precedes
    /// read_manifest. A source error must not read or publish the manifest.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin<'s>(
        &mut self,
        path: &'original Path,
        script: bool,
        context: Context<'original>,
        sources: &mut source::Owner<'s>,
        roots: &mut Roots<'original, 'document>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Sourced<'original, 'document, 's>, String>> {
        match self.begin_staged(path, script, context, sources, roots, limits, session)? {
            Ok(sourced) => Ok(Ok(sourced)),
            Err(Failure {
                cause: Cause::Message(message),
                ..
            }) => Ok(Err(message)),
            Err(Failure {
                cause: Cause::Rejected(diagnostic),
                ..
            }) => Err(super::super::Stop::Rejected(diagnostic)),
        }
    }
    pub(super) fn prepare(
        &mut self,
        path: &'original Path,
        script: bool,
        context: Context<'original>,
        roots: &mut Roots<'original, 'document>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Preparing<'original, 'document>> {
        let (original, is_embedded) = self.interpret(path, script, limits, session)?;
        super::prepare(original, path, is_embedded, context, roots, session)
    }
    /// Read, parse and deserialize before Cargo's manifest-context closure.
    /// Configuration and early workspace publication occur only afterwards.
    pub(super) fn interpret(
        &mut self,
        path: &'original Path,
        script: bool,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<(&'original Original<'document>, bool)> {
        // Classifying the pathname has no I/O. In the pin it precedes reading.
        let is_embedded = is_embedded(path);
        let raw = session.text(path.as_os_str().as_bytes(), limits)?;
        let expanded = if is_embedded {
            if !script {
                return session.reject(Diagnostic::at(Tag::ScriptRequired, None));
            }
            Some(embedded::expand(raw, session)?)
        } else {
            None
        };
        let body = self.bodies.push(Body { raw, expanded }, session)?;
        let document = session.parse_text(body.text(), limits)?;
        let document = self.documents.push(document, session)?;
        let original = Original::interpret(value::Input::root(document), session)?;
        let original = self.originals.push(original, session)?;
        Ok((original, is_embedded))
    }
}

#[cfg(test)]
mod tests;

mod begin;
pub(super) use begin::{Cause, Failure, Stage};
