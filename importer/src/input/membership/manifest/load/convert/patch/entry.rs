// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Source-ordered patch warning, dependency and manifest-location ownership.
use super::super::{dependency, platform, source};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, normalize, path, Warnings},
            session::Session,
            Step,
        },
    },
};
use std::path::{Path, PathBuf};

pub(in super::super::super) struct Entry<'p, 's> {
    dependency: dependency::Converted<'p, 's>,
    location: PathBuf,
}
impl<'p, 's> Entry<'p, 's> {
    pub(in super::super::super) fn dependency(&self) -> &dependency::Converted<'p, 's> {
        &self.dependency
    }
    pub(in super::super::super) fn location(&self) -> &Path {
        &self.location
    }
}
pub(in super::super::super) enum Progress<'a, 'p, 's> {
    Complete(Entry<'p, 's>),
    Canonical(Pending<'a, 'p, 's>),
}
pub(in super::super::super) struct Pending<'a, 'p, 's> {
    dependency: dependency::Pending<'a, 'p, 's>,
}
impl Pending<'_, '_, '_> {
    pub(in super::super::super) fn spelling(&self) -> &str {
        self.dependency.spelling()
    }
}

/// Registry selection and replace/patch exclusion precede this per-entry stage.
pub(in super::super::super) fn convert<'a, 'd: 'a, 'p, 's, 'image: 's>(
    declaration: &'a normalize::Dependency<'a, 'd>,
    registry: &str,
    manifest: &'a Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    platform: Option<&platform::Platform<'p>>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'p, 's>, String>> {
    super::super::dependencies::patch_entry(declaration, registry, warnings, session)?;
    let converted = match dependency::convert(
        declaration,
        manifest,
        parent,
        sources,
        config,
        normalize::dependencies::Kind::Normal,
        platform,
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    Ok(Ok(match converted {
        dependency::Progress::Canonical(dependency) => Progress::Canonical(Pending { dependency }),
        dependency::Progress::Complete(dependency) => {
            let location = path::copy(manifest, session)?;
            Progress::Complete(Entry {
                dependency,
                location,
            })
        }
    }))
}
