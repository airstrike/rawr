// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! One pinned normalize_dependencies call, not complete manifest loading.

use std::path::Path;

use super::super::super::{
    schema::{
        dependency::{inheritable, Field},
        primitive,
    },
    session::Session,
    storage, value, Diagnostic, Error, Step, Stop, Tag,
};
use super::super::{
    config,
    features::{Feature, Features},
    roots, Warnings,
};
use super::inherit;
use crate::{cargo, input::membership::host};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(in super::super) enum Kind {
    Normal,
    Development,
    Build,
}

/// Inputs from the eventual closed caller, never acquired from ambient state.
pub(in super::super) struct Context<'a, 'd> {
    pub(in super::super) package_root: &'a Path,
    pub(in super::super) edition: u16,
    pub(in super::super) features: &'a Features<'a, 'd>,
    pub(in super::super) public_dependency: bool,
    pub(in super::super) kind: Kind,
}

pub(in super::super) struct Dependency<'a, 'd> {
    name: &'a primitive::Name<'d>,
    inherited: inherit::View<'a, 'd>,
    remove_public: bool,
    path: Option<String>,
}
impl<'a, 'd> Dependency<'a, 'd> {
    pub(in super::super) fn name(&self) -> &str {
        self.name.as_str()
    }
    pub(in super::super) fn simple(&self) -> Option<&str> {
        self.inherited.simple()
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<inherit::Value<'_, 'd>> {
        match field {
            Field::Public if self.remove_public => None,
            Field::Base if self.path.is_some() => None,
            Field::Path if self.path.is_some() => self.path.as_deref().map(inherit::Value::Text),
            _ => self.inherited.get(field),
        }
    }
    pub(in super::super) fn extras(
        &self,
    ) -> impl Iterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        self.inherited.extras()
    }
}

pub(in super::super) struct Complete<'a, 'd> {
    dependencies: Option<storage::Buffer<Dependency<'a, 'd>>>,
    workspace: Option<roots::Root<'a, 'd>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    pub(in super::super) fn dependencies(&self) -> Option<&[Dependency<'a, 'd>]> {
        self.dependencies.as_ref().map(|items| items.as_slice())
    }
    pub(in super::super) fn workspace(&self) -> Option<roots::Root<'a, 'd>> {
        self.workspace
    }
}
pub(in super::super) enum Progress<'a, 'd> {
    Complete(Complete<'a, 'd>),
    Workspace(Workspace<'a, 'd>),
}
pub(in super::super) struct Workspace<'a, 'd> {
    cursor: Cursor<'a, 'd>,
    pending: Pending<'a, 'd>,
}
enum Pending<'a, 'd> {
    Inherit(&'a primitive::Name<'d>, inherit::Workspace<'a, 'd>),
    Base(Dependency<'a, 'd>),
}
impl Workspace<'_, '_> {
    pub(in super::super) fn failure_context<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        match &self.pending {
            Pending::Inherit(name, _) => {
                let name = name.as_str();
                super::target::formatted(format_args!("error inheriting `{name}` from workspace root manifest's `workspace.dependencies.{name}`"), session)
            }
            Pending::Base(dependency) => super::target::formatted(
                format_args!("resolving path dependency {}", dependency.name()),
                session,
            ),
        }
    }
}
struct Cursor<'a, 'd> {
    original: Option<&'a inheritable::Map<'d>>,
    context: Context<'a, 'd>,
    complete: Complete<'a, 'd>,
    index: usize,
}

pub(in super::super) fn normalize<'a, 'd, 'image, 'config>(
    original: Option<&'a inheritable::Map<'d>>,
    context: Context<'a, 'd>,
    workspace: Option<roots::Root<'a, 'd>>,
    warnings: &mut Warnings,
    owner: &mut config::Owner<'config, 'image>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Progress<'a, 'd>>
where
    'image: 'config,
{
    session.check()?;
    session.items(1)?;
    Cursor {
        original,
        context,
        index: 0,
        complete: Complete {
            dependencies: original.map(|_| storage::Buffer::new()),
            workspace,
        },
    }
    .run(warnings, owner, limits, session)
}

impl<'a, 'd> Workspace<'a, 'd> {
    pub(in super::super) fn resume_declared<'image, 'config>(
        mut self,
        root: roots::Root<'a, 'd>,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>>
    where
        'image: 'config,
    {
        session.check()?;
        self.cursor.complete.workspace = Some(root);
        match self.pending {
            Pending::Inherit(name, request) => {
                let view = request.resume_declared(root, warnings, session)?;
                let dependency = self
                    .cursor
                    .finish(name, view, warnings, owner, limits, session)?;
                // The same declared root also satisfies a workspace path base.
                self.cursor.push(dependency, session)?;
            }
            Pending::Base(mut dependency) => {
                let Some(inherit::Value::Original(primitive::Value::String(child))) =
                    dependency.get(Field::Path)
                else {
                    return session.fail(Error::State);
                };
                dependency.path = Some(super::join(root.directory, child.as_str(), session)?);
                self.cursor.push(dependency, session)?;
            }
        }
        self.cursor.run(warnings, owner, limits, session)
    }
}

impl<'a, 'd> Cursor<'a, 'd> {
    fn push<A: rawr_toml::document::Admission>(
        &mut self,
        dependency: Dependency<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(output) = self.complete.dependencies.as_mut() else {
            return session.fail(Error::State);
        };
        output.push(dependency, session)?;
        self.index += 1;
        Ok(())
    }
    fn run<'image, 'config>(
        mut self,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>>
    where
        'image: 'config,
    {
        session.check()?;
        while let Some((name, _, original)) = self.original.and_then(|map| map.entry(self.index)) {
            let view = match inherit::dependency(
                name.as_str(),
                original,
                self.context.package_root,
                self.context.edition,
                session,
            )? {
                inherit::Progress::Direct(view) => view,
                inherit::Progress::Workspace(request) => match self.complete.workspace {
                    Some(root) => request.resume_declared(root, warnings, session)?,
                    None => {
                        return Ok(Progress::Workspace(Workspace {
                            cursor: self,
                            pending: Pending::Inherit(name, request),
                        }))
                    }
                },
            };
            let dependency = self.finish(name, view, warnings, owner, limits, session)?;
            if dependency.get(Field::Base).is_some() {
                return Ok(Progress::Workspace(Workspace {
                    cursor: self,
                    pending: Pending::Base(dependency),
                }));
            }
            self.push(dependency, session)?;
        }
        session.items(1)?;
        Ok(Progress::Complete(self.complete))
    }
    fn finish<'image, 'config>(
        &self,
        name: &'a primitive::Name<'d>,
        view: inherit::View<'a, 'd>,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Dependency<'a, 'd>>
    where
        'image: 'config,
    {
        let mut dependency = Dependency {
            name,
            inherited: view,
            remove_public: false,
            path: None,
        };
        post_inherit(&mut dependency, &self.context, warnings, session)?;
        if let Some(inherit::Value::Original(primitive::Value::Name(base))) =
            dependency.get(Field::Base)
        {
            let Some(inherit::Value::Original(primitive::Value::String(child))) =
                dependency.get(Field::Path)
            else {
                return session.reject(Diagnostic::at(Tag::PatchBaseWithoutPath, None));
            };
            dependency.path = match super::configured_base(
                base,
                child.as_str(),
                None,
                owner,
                self.context.features,
                limits,
                session,
            )? {
                Some(path) => Some(path),
                None => match self.complete.workspace {
                    Some(root) => Some(super::join(root.directory, child.as_str(), session)?),
                    None => None,
                },
            };
        }
        Ok(dependency)
    }
}

fn post_inherit<A: rawr_toml::document::Admission>(
    dependency: &mut Dependency<'_, '_>,
    context: &Context<'_, '_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    if dependency.get(Field::DefaultFeatures2).is_some() {
        if context.edition >= 2024 {
            return session.reject(Diagnostic::at(
                Tag::DependencyDefaultFeaturesUnderscore,
                None,
            ));
        }
        if dependency.get(Field::DefaultFeatures).is_some() {
            warnings.push(&["`default_features` is redundant with `default-features`, preferring `default-features` in the `", dependency.name(), "` dependency"], session)?;
        } else {
            warnings.push(&["`default_features` is deprecated in favor of `default-features` and will not work in the 2024 edition\n(in the `", dependency.name(), "` dependency)"], session)?;
        }
    }
    if dependency.get(Field::Public).is_some() {
        let enabled = match context.features.require(Feature::PublicDependency, session) {
            Ok(()) => true,
            Err(Stop::Rejected(_)) => false,
            Err(stop @ Stop::Refused) => return Err(stop),
        };
        match context.kind {
            Kind::Normal if !enabled && !context.public_dependency => {
                warnings.push(
                    &[
                        "ignoring `public` on dependency ",
                        dependency.name(),
                        ", pass `-Zpublic-dependency` to enable support for it",
                    ],
                    session,
                )?;
                dependency.remove_public = true;
            }
            Kind::Development | Kind::Build => {
                if enabled || context.public_dependency {
                    return session.reject(Diagnostic::at(Tag::DependencyPublicKind, None));
                }
                warnings.push(
                    &[
                        "'public' specifier can only be used on regular dependencies, not ",
                        match context.kind {
                            Kind::Development => "dev-dependencies",
                            Kind::Build => "build-dependencies",
                            Kind::Normal => "dependencies",
                        },
                    ],
                    session,
                )?;
                dependency.remove_public = true;
            }
            _ => {}
        }
    }
    Ok(())
}
