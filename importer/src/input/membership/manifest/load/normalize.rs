//! Patch-first and package normalization, ending before full conversion.
pub(super) mod all_targets;
pub(super) mod dependencies;
pub(super) mod files;
pub(super) mod infer;
pub(super) mod inherit;
pub(super) mod library;
pub(super) mod lint;
pub(super) mod merge;
mod name;
pub(super) mod package;
pub(super) mod resolve;
pub(super) mod suggest;
pub(super) mod tail;
pub(super) mod target;
pub(super) mod targets;
pub(super) mod validate;
pub(super) mod virtual_fields;

use super::super::{
    schema::{
        dependency::{self, direct},
        manifest, primitive,
    },
    session::Session,
    storage, value, Diagnostic, Error, Step, Tag,
};
use super::{config, path, roots, workspace, Preparing};
use crate::{cargo, input::membership::host};
use std::{os::unix::ffi::OsStringExt, path::Path};

/// A transformed field never exposes the consumed base or stale original path.
pub(super) enum Field<'a, 'd> {
    Original(&'a primitive::Value<'d>),
    Path(&'a str),
}
pub(super) struct Dependency<'a, 'd> {
    original: &'a direct::Direct<'d>,
    replacement: Option<String>,
    pub(super) name: &'a primitive::Name<'d>,
}
impl<'a, 'd> Dependency<'a, 'd> {
    pub(super) fn get(&self, field: dependency::Field) -> Option<Field<'_, 'd>> {
        if self.replacement.is_some() {
            if field == dependency::Field::Base {
                return None;
            }
            if field == dependency::Field::Path {
                return self.replacement.as_deref().map(Field::Path);
            }
        }
        match self.original.kind() {
            direct::Kind::Detailed(details) => details.get(field).map(Field::Original),
            direct::Kind::Simple(_) => None,
        }
    }
    pub(super) fn fields(&self) -> impl Iterator<Item = (dependency::Field, Field<'_, 'd>)> {
        let details = match self.original.kind() {
            direct::Kind::Detailed(details) => Some(details),
            _ => None,
        };
        details
            .into_iter()
            .flat_map(|details| details.fields())
            .filter_map(|entry| self.get(entry.field()).map(|value| (entry.field(), value)))
    }
    pub(super) fn simple(&self) -> Option<&str> {
        match self.original.kind() {
            direct::Kind::Simple(text) => Some(text.as_str()),
            _ => None,
        }
    }
    pub(super) fn extras(&self) -> impl Iterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        let details = match self.original.kind() {
            direct::Kind::Detailed(details) => Some(details),
            _ => None,
        };
        details.into_iter().flat_map(|details| details.extras())
    }
}
pub(super) struct Source<'a, 'd> {
    pub(super) name: &'a str,
    dependencies: storage::Buffer<Dependency<'a, 'd>>,
}
impl<'a, 'd> Source<'a, 'd> {
    pub(super) fn dependencies(&self) -> &[Dependency<'a, 'd>] {
        self.dependencies.as_slice()
    }
}
/// Carries the retained original/configuration into the next (package) stage.
/// Construction proves only that every patch passed normalization.
pub(super) struct Patches<'a, 'd> {
    preparing: Preparing<'a, 'd>,
    patch: Option<storage::Buffer<Source<'a, 'd>>>,
    workspace: Option<roots::Root<'a, 'd>>,
}
impl<'a, 'd> Patches<'a, 'd> {
    pub(super) fn original(&self) -> &'a manifest::Original<'d> {
        self.preparing.original
    }
    pub(super) fn warnings(&self) -> &super::Warnings {
        &self.preparing.configured.warnings
    }
    /// Retain lazy inheritance across the following package stages.
    pub(super) fn workspace(&self) -> Option<&Path> {
        self.workspace.map(|root| root.directory)
    }
    pub(super) fn patch(&self) -> Option<&[Source<'a, 'd>]> {
        self.patch.as_ref().map(|patch| patch.as_slice())
    }
}
pub(super) enum Progress<'a, 'd> {
    Patches(Patches<'a, 'd>),
    Workspace(Workspace<'a, 'd>),
}
/// Exact lazy load_inheritable_fields input. A member miss requires full loading;
/// preparing another manifest is not sufficient to satisfy this request.
pub(super) struct Workspace<'a, 'd> {
    cursor: Cursor<'a, 'd>,
    pending: Dependency<'a, 'd>,
    child: &'a str,
}
impl<'a, 'd> Workspace<'a, 'd> {
    pub(super) fn failure_context<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        let Some(source) = self.cursor.active.as_ref() else {
            return session.fail(Error::State);
        };
        target::formatted(
            format_args!(
                "resolving path for patch of ({}) for source ({})",
                self.pending.name.as_str(),
                source.name
            ),
            session,
        )
    }
    pub(super) fn manifest(&self) -> &'a Path {
        self.cursor.preparing.path
    }
    pub(super) fn explicit_root(&self) -> Option<&str> {
        match &self.cursor.preparing.configured.workspace {
            workspace::Configuration::Member { root } => root.as_ref().map(|root| root.as_str()),
            _ => None,
        }
    }
    /// Caller supplies the declaration selected for this exact manifest by the
    /// source cache branch, or by successful full recursive loading. A bare
    /// pathname or another Reader::prepare result does not satisfy this API.
    /// Reuses the suspended dependency and memoizes the root; no config reread.
    pub(super) fn resume_declared<'image, 'config>(
        mut self,
        root: roots::Root<'a, 'd>,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>>
    where
        'image: 'config,
    {
        session.check()?;
        session.items(1)?;
        self.cursor.workspace = Some(root);
        self.pending.replacement = Some(join(root.directory, self.child, session)?);
        self.cursor.push(self.pending, session)?;
        self.cursor.run(owner, limits, session)
    }
}
struct Cursor<'a, 'd> {
    preparing: Preparing<'a, 'd>,
    patch: Option<storage::Buffer<Source<'a, 'd>>>,
    source: usize,
    active: Option<Source<'a, 'd>>,
    dependency: usize,
    workspace: Option<roots::Root<'a, 'd>>,
}
impl<'a, 'd> Preparing<'a, 'd> {
    pub(super) fn normalize_patches<'image, 'config>(
        self,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>>
    where
        'image: 'config,
    {
        session.items(1)?;
        let patch = match self.original.get(manifest::Field::Patch) {
            Some(manifest::Value::Patch(_)) => Some(storage::Buffer::new()),
            None => None,
            _ => return session.fail(Error::State),
        };
        Cursor {
            preparing: self,
            patch,
            source: 0,
            active: None,
            dependency: 0,
            workspace: None,
        }
        .run(owner, limits, session)
    }
}
impl<'a, 'd> Cursor<'a, 'd> {
    fn push<A: rawr_toml::document::Admission>(
        &mut self,
        dependency: Dependency<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(source) = self.active.as_mut() else {
            return session.fail(Error::State);
        };
        source.dependencies.push(dependency, session)?;
        self.dependency += 1;
        Ok(())
    }
    fn run<'image, 'config>(
        mut self,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>>
    where
        'image: 'config,
    {
        session.check()?;
        let original = self.preparing.original;
        if let Some(manifest::Value::Patch(patch)) = original.get(manifest::Field::Patch) {
            while let Some(source) = patch.entry(self.source) {
                if self.active.is_none() {
                    session.items(1)?;
                    self.active = Some(Source {
                        name: source.origin().text.as_str(),
                        dependencies: storage::Buffer::new(),
                    });
                }
                while let Some((name, _, original)) = source.value().entry(self.dependency) {
                    session.items(1)?;
                    let mut pending = Dependency {
                        original,
                        replacement: None,
                        name,
                    };
                    if let direct::Kind::Detailed(details) = original.kind()
                        && let Some(primitive::Value::Name(base)) =
                            details.get(dependency::Field::Base)
                    {
                        let Some(primitive::Value::String(child)) =
                            details.get(dependency::Field::Path)
                        else {
                            return session.reject(Diagnostic::at(
                                Tag::PatchBaseWithoutPath,
                                original.span(),
                            ));
                        };
                        let configured = configured_base(
                            base,
                            child.as_str(),
                            original.span(),
                            owner,
                            &self.preparing.configured.features,
                            limits,
                            session,
                        )?;
                        if let Some(path) = configured {
                            pending.replacement = Some(path);
                        } else if let Some(root) = self.workspace {
                            pending.replacement =
                                Some(join(root.directory, child.as_str(), session)?);
                        } else if let workspace::Configuration::Root {
                            directory,
                            declaration,
                        } = self.preparing.configured.workspace
                        {
                            session.items(1)?;
                            self.workspace = Some(roots::Root {
                                directory,
                                declaration,
                            });
                            pending.replacement = Some(join(directory, child.as_str(), session)?);
                        } else {
                            session.items(1)?;
                            return Ok(Progress::Workspace(Workspace {
                                cursor: self,
                                pending,
                                child: child.as_str(),
                            }));
                        }
                    }
                    self.push(pending, session)?;
                }
                let Some(completed) = self.active.take() else {
                    return session.fail(Error::State);
                };
                let Some(output) = self.patch.as_mut() else {
                    return session.fail(Error::State);
                };
                output.push(completed, session)?;
                self.source += 1;
                self.dependency = 0;
            }
        }
        session.items(1)?;
        Ok(Progress::Patches(Patches {
            preparing: self.preparing,
            patch: self.patch,
            workspace: self.workspace,
        }))
    }
}
/// Shared configured-base precedence. None requests Cargo's lazy workspace root;
/// only the caller knows which declaration-cache branch satisfies that request.
fn configured_base<'image, 'config>(
    base: &primitive::Name<'_>,
    child: &str,
    span: Option<super::super::Span>,
    owner: &mut config::Owner<'config, 'image>,
    features: &super::features::Features<'_, '_>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Option<String>>
where
    'image: 'config,
{
    match owner.path_base(base, features, limits, session)? {
        Some(root) => join(&root, child, session).map(Some),
        None if base.as_str() == "workspace" => Ok(None),
        None => session.reject(Diagnostic::at(Tag::PatchBaseUndefined, span)),
    }
}

fn join<A: rawr_toml::document::Admission>(
    root: &Path,
    child: &str,
    session: &mut Session<'_, A>,
) -> Step<String> {
    let joined = path::join(root, Path::new(child), session)?;
    if joined.to_str().is_none() {
        return session.source_joined_dependency_path();
    }
    // Transfer the already admitted native allocation; there is no second heap copy.
    match String::from_utf8(joined.into_os_string().into_vec()) {
        Ok(text) => Ok(text),
        Err(_) => session.fail(Error::State),
    }
}
