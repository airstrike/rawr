// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned normalize_toml's dependency/lint tail, mod.rs:453–585.
//! Connected after target groups through all_targets::Owner.

use std::path::Path;

use super::super::super::{
    schema::{dependency::inheritable, lints, manifest, platform},
    session::Session,
    storage, Diagnostic, Error, Step, Tag,
};
use super::super::{config, features::Features, roots, Warnings};
use super::{dependencies, lint};
use crate::{cargo, input::membership::host};

/// Already-associated inputs for this substage, not a target-completion claim.
pub(in super::super) struct Context<'a, 'd> {
    pub(in super::super) package_name: &'a str,
    pub(in super::super) package_root: &'a Path,
    pub(in super::super) edition: u16,
    pub(in super::super) features: &'a Features<'a, 'd>,
    pub(in super::super) public_dependency: bool,
}

pub(in super::super) struct Maps<'a, 'd> {
    normal: dependencies::Complete<'a, 'd>,
    development: dependencies::Complete<'a, 'd>,
    build: dependencies::Complete<'a, 'd>,
}
impl<'a, 'd> Maps<'a, 'd> {
    pub(in super::super) fn get(
        &self,
        kind: dependencies::Kind,
    ) -> Option<&[dependencies::Dependency<'a, 'd>]> {
        match kind {
            dependencies::Kind::Normal => self.normal.dependencies(),
            dependencies::Kind::Development => self.development.dependencies(),
            dependencies::Kind::Build => self.build.dependencies(),
        }
    }
}
pub(in super::super) struct Platform<'a, 'd> {
    name: &'a str,
    maps: Maps<'a, 'd>,
}
impl<'a, 'd> Platform<'a, 'd> {
    pub(in super::super) fn name(&self) -> &str {
        self.name
    }
    pub(in super::super) fn maps(&self) -> &Maps<'a, 'd> {
        &self.maps
    }
}

/// Completion of this tail only. Earlier target stages and later real/virtual
/// conversion remain mandatory before any successful Loaded authority exists.
pub(in super::super) struct Complete<'a, 'd> {
    maps: Maps<'a, 'd>,
    platforms: storage::Buffer<Platform<'a, 'd>>,
    lints: lint::Complete<'a, 'd>,
    hints: Option<&'a manifest::Value<'d>>,
    badges: Option<&'a manifest::Value<'d>>,
    workspace: Option<roots::Root<'a, 'd>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    pub(in super::super) fn maps(&self) -> &Maps<'a, 'd> {
        &self.maps
    }
    pub(in super::super) fn platforms(&self) -> Option<&[Platform<'a, 'd>]> {
        (!self.platforms.as_slice().is_empty()).then(|| self.platforms.as_slice())
    }
    pub(in super::super) fn lints(&self) -> Option<&'a lints::Table<'d>> {
        self.lints.table()
    }
    pub(in super::super) fn hints(&self) -> Option<&'a manifest::Value<'d>> {
        self.hints
    }
    pub(in super::super) fn badges(&self) -> Option<&'a manifest::Value<'d>> {
        self.badges
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
    Dependencies(dependencies::Workspace<'a, 'd>),
    Lints(lint::Workspace<'a, 'd>),
}
#[derive(Clone, Copy)]
enum Stage {
    Top(usize),
    Platform(usize, usize),
    Lints,
}
struct Cursor<'a, 'd> {
    original: &'a manifest::Original<'d>,
    context: Context<'a, 'd>,
    stage: Stage,
    top: [Option<dependencies::Complete<'a, 'd>>; 3],
    active: [Option<dependencies::Complete<'a, 'd>>; 3],
    platforms: storage::Buffer<Platform<'a, 'd>>,
    workspace: Option<roots::Root<'a, 'd>>,
}

pub(in super::super) fn normalize<'a, 'd, 'image: 'config, 'config>(
    original: &'a manifest::Original<'d>,
    context: Context<'a, 'd>,
    workspace: Option<roots::Root<'a, 'd>>,
    warnings: &mut Warnings,
    owner: &mut config::Owner<'config, 'image>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Progress<'a, 'd>> {
    session.check()?;
    session.items(1)?;
    Cursor {
        original,
        context,
        stage: Stage::Top(0),
        top: [None, None, None],
        active: [None, None, None],
        platforms: storage::Buffer::new(),
        workspace,
    }
    .run(warnings, owner, limits, session)
}

impl<'a, 'd> Workspace<'a, 'd> {
    pub(in super::super) fn failure_context<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        match &self.pending {
            Pending::Dependencies(request) => request.failure_context(session),
            Pending::Lints(_) => super::target::formatted(
                format_args!(
                    "error inheriting `lints` from workspace root manifest's `workspace.lints`"
                ),
                session,
            ),
        }
    }
    /// Only the declaration selected for this manifest by the existing lazy
    /// source-cache/full-load branch satisfies this request.
    pub(in super::super) fn resume_declared<'image: 'config, 'config>(
        mut self,
        root: roots::Root<'a, 'd>,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>> {
        session.check()?;
        self.cursor.workspace = Some(root);
        match self.pending {
            Pending::Dependencies(request) => {
                match request.resume_declared(root, warnings, owner, limits, session)? {
                    dependencies::Progress::Complete(done) => self.cursor.accept(done, session)?,
                    dependencies::Progress::Workspace(request) => {
                        return Ok(Progress::Workspace(Workspace {
                            cursor: self.cursor,
                            pending: Pending::Dependencies(request),
                        }))
                    }
                }
            }
            Pending::Lints(request) => {
                return self
                    .cursor
                    .finish(request.resume_declared(root, session)?, session)
                    .map(Progress::Complete)
            }
        }
        self.cursor.run(warnings, owner, limits, session)
    }
}

impl<'a, 'd> Cursor<'a, 'd> {
    fn platform(&self, index: usize) -> Option<(&'a str, &'a platform::Platform<'d>)> {
        match self.original.get(manifest::Field::Target) {
            Some(manifest::Value::Platforms(platforms)) => platforms
                .entry(index)
                .map(|entry| (entry.origin().text.as_str(), entry.value())),
            _ => None,
        }
    }
    fn accept<A: rawr_toml::document::Admission>(
        &mut self,
        done: dependencies::Complete<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.workspace = done.workspace();
        match self.stage {
            Stage::Top(index) => {
                let Some(slot) = self.top.get_mut(index) else {
                    return session.fail(Error::State);
                };
                *slot = Some(done);
                self.stage = if index == 2 {
                    Stage::Platform(0, 0)
                } else {
                    Stage::Top(index + 1)
                };
            }
            Stage::Platform(index, kind) => {
                let Some(slot) = self.active.get_mut(kind) else {
                    return session.fail(Error::State);
                };
                *slot = Some(done);
                if kind == 2 {
                    let Some((name, _)) = self.platform(index) else {
                        return session.fail(Error::State);
                    };
                    let maps = maps(std::mem::take(&mut self.active), session)?;
                    self.platforms.push(Platform { name, maps }, session)?;
                    self.stage = Stage::Platform(index + 1, 0);
                } else {
                    self.stage = Stage::Platform(index, kind + 1);
                }
            }
            Stage::Lints => return session.fail(Error::State),
        }
        Ok(())
    }
    fn run<'image: 'config, 'config>(
        mut self,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Progress<'a, 'd>> {
        session.check()?;
        loop {
            session.items(1)?;
            let (original, index) = match self.stage {
                Stage::Top(index) => (self.top_map(index, warnings, session)?, index),
                Stage::Platform(index, kind) => {
                    let Some((name, platform)) = self.platform(index) else {
                        self.stage = Stage::Lints;
                        continue;
                    };
                    (
                        platform_map(
                            platform,
                            kind,
                            name,
                            self.context.edition,
                            warnings,
                            session,
                        )?,
                        kind,
                    )
                }
                Stage::Lints => {
                    let original = match self.original.get(manifest::Field::Lints) {
                        Some(manifest::Value::Lints(lints)) => Some(lints),
                        None => None,
                        _ => return session.fail(Error::State),
                    };
                    let done = match lint::normalize(original, session)? {
                        lint::Progress::Complete(done) => done,
                        lint::Progress::Workspace(request) => match self.workspace {
                            Some(root) => request.resume_declared(root, session)?,
                            None => {
                                return Ok(Progress::Workspace(Workspace {
                                    cursor: self,
                                    pending: Pending::Lints(request),
                                }))
                            }
                        },
                    };
                    return self.finish(done, session).map(Progress::Complete);
                }
            };
            let kind = match index {
                0 => dependencies::Kind::Normal,
                1 => dependencies::Kind::Development,
                2 => dependencies::Kind::Build,
                _ => return session.fail(Error::State),
            };
            let context = dependencies::Context {
                package_root: self.context.package_root,
                edition: self.context.edition,
                features: self.context.features,
                public_dependency: self.context.public_dependency,
                kind,
            };
            match dependencies::normalize(
                original,
                context,
                self.workspace,
                warnings,
                owner,
                limits,
                session,
            )? {
                dependencies::Progress::Complete(done) => self.accept(done, session)?,
                dependencies::Progress::Workspace(request) => {
                    return Ok(Progress::Workspace(Workspace {
                        cursor: self,
                        pending: Pending::Dependencies(request),
                    }))
                }
            }
        }
    }
    fn top_map<A: rawr_toml::document::Admission>(
        &self,
        index: usize,
        warnings: &mut Warnings,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&'a inheritable::Map<'d>>> {
        let (new, old, label) = match index {
            0 => (manifest::Field::Dependencies, None, "dependencies"),
            1 => (
                manifest::Field::DevDependencies,
                Some(manifest::Field::DevDependencies2),
                "dev-dependencies",
            ),
            2 => (
                manifest::Field::BuildDependencies,
                Some(manifest::Field::BuildDependencies2),
                "build-dependencies",
            ),
            _ => return session.fail(Error::State),
        };
        let new = self.original.get(new);
        let old = old.and_then(|field| self.original.get(field));
        alias(
            old.is_some(),
            new.is_some(),
            label,
            self.context.package_name,
            "package",
            self.context.edition,
            warnings,
            session,
        )?;
        match new.or(old) {
            Some(manifest::Value::Dependencies(map)) => Ok(Some(map)),
            None => Ok(None),
            _ => session.fail(Error::State),
        }
    }
    fn finish<A: rawr_toml::document::Admission>(
        self,
        lints: lint::Complete<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<Complete<'a, 'd>> {
        session.items(1)?;
        Ok(Complete {
            maps: maps(self.top, session)?,
            platforms: self.platforms,
            lints,
            hints: self.original.get(manifest::Field::Hints),
            badges: self.original.get(manifest::Field::Badges),
            workspace: self.workspace,
        })
    }
}
fn maps<'a, 'd, A: rawr_toml::document::Admission>(
    slots: [Option<dependencies::Complete<'a, 'd>>; 3],
    session: &mut Session<'_, A>,
) -> Step<Maps<'a, 'd>> {
    let [Some(normal), Some(development), Some(build)] = slots else {
        return session.fail(Error::State);
    };
    Ok(Maps {
        normal,
        development,
        build,
    })
}
fn platform_map<'a, 'd, A: rawr_toml::document::Admission>(
    platform: &'a platform::Platform<'d>,
    index: usize,
    name: &str,
    edition: u16,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a inheritable::Map<'d>>> {
    let (new, old, label) = match index {
        0 => (platform::Field::Dependencies, None, "dependencies"),
        1 => (
            platform::Field::DevDependencies,
            Some(platform::Field::DevDependencies2),
            "dev-dependencies",
        ),
        2 => (
            platform::Field::BuildDependencies,
            Some(platform::Field::BuildDependencies2),
            "build-dependencies",
        ),
        _ => return session.fail(Error::State),
    };
    let new = platform.get(new);
    let old = old.and_then(|field| platform.get(field));
    alias(
        old.is_some(),
        new.is_some(),
        label,
        name,
        "platform target",
        edition,
        warnings,
        session,
    )?;
    Ok(new.or(old))
}
fn alias<A: rawr_toml::document::Admission>(
    old: bool,
    new: bool,
    label: &str,
    name: &str,
    kind: &str,
    edition: u16,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    if !old {
        return Ok(());
    }
    if edition >= 2024 {
        return session.reject(Diagnostic::at(Tag::DependencyTableUnderscore, None));
    }
    let old_label = if label == "dev-dependencies" {
        "dev_dependencies"
    } else {
        "build_dependencies"
    };
    if new {
        warnings.push(
            &[
                "`",
                old_label,
                "` is redundant with `",
                label,
                "`, preferring `",
                label,
                "` in the `",
                name,
                "` ",
                kind,
            ],
            session,
        )
    } else {
        warnings.push(
            &[
                "`",
                old_label,
                "` is deprecated in favor of `",
                label,
                "` and will not work in the 2024 edition\n(in the `",
                name,
                "` ",
                kind,
                ")",
            ],
            session,
        )
    }
}
