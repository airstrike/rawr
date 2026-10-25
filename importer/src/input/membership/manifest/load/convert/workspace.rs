//! Virtual-manifest conversion; excludes real-package and target checks.
use super::{finish, patch::groups as patch, profiles, replace::list as replace, resolver, source};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, features::Features, normalize, workspace::Configuration, Warnings},
            schema::{manifest, primitive, workspace},
            session::Session,
            spec::Spec,
            storage::{self, slots},
            Error, Step,
        },
    },
};
use std::path::Path;

pub(in super::super) struct Complete<'a, 's> {
    pub(in super::super) replacements: storage::Buffer<replace::Entry<'a, 'a, 's>>,
    pub(in super::super) patches: patch::Map<'a, 's>,
    pub(in super::super) resolver: Option<resolver::Behavior>,
}
pub(in super::super) enum Progress<'a, 's> {
    Complete(Complete<'a, 's>),
    Replacement,
    Patch,
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn convert<'a, 'd: 'a, 's: 'a, 'image: 's>(
    original: &'a manifest::Original<'d>,
    patches: Option<&'a [normalize::Source<'a, 'd>]>,
    workspace: &'a Configuration<'a, 'd>,
    specs: &mut slots::Writer<'a, Spec>,
    manifest: &'a Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    bindeps: bool,
    json_target_spec: bool,
    profile_context: profiles::Context<'_>,
    features: &Features<'_, '_>,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 's>, replace::Failure>> {
    let replacements = match replace::convert(
        original,
        specs,
        manifest,
        parent,
        sources,
        config,
        None,
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Err(error) => return Ok(Err(error)),
        Ok(replace::Progress::Complete(value)) => value,
        Ok(replace::Progress::Canonical(_)) => return Ok(Ok(Progress::Replacement)),
    };
    let patches = match patch::convert(
        patches,
        manifest,
        parent,
        sources,
        config,
        None,
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
        Ok(patch::Progress::Complete(value)) => value,
        Ok(patch::Progress::Canonical(_)) => return Ok(Ok(Progress::Patch)),
    };
    if let Some(value) = original.get(manifest::Field::Profile) {
        let manifest::Value::Profiles(profiles) = value else {
            return session.fail(Error::State);
        };
        if let Err(error) = profiles::validate(
            profiles,
            features,
            profile_context.unstable,
            profile_context.cargo_version,
            warnings,
            session,
        )? {
            return Ok(Err(replace::Failure::Conversion(error)));
        }
    }
    session.items(1)?;
    let selected = match original.get(manifest::Field::Workspace) {
        None => None,
        Some(manifest::Value::Workspace(value)) => match value.get(workspace::Field::Resolver) {
            None => None,
            Some(workspace::Value::Local(primitive::Value::String(value))) => Some(value.as_str()),
            _ => return session.fail(Error::State),
        },
        _ => return session.fail(Error::State),
    };
    let resolver = match resolver::select(None, selected, session)? {
        Ok(value) => value,
        Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
    };
    if matches!(workspace, Configuration::Member { .. }) {
        return super::super::formatted(
            format_args!("virtual manifests must be configured with [workspace]"),
            session,
        )
        .map(|message| Err(replace::Failure::Conversion(message)));
    }
    finish::warnings(None, original, warnings, session)?;
    Ok(Ok(Progress::Complete(Complete {
        replacements,
        patches,
        resolver,
    })))
}
