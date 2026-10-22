//! Consecutive dependency conversion, source consistency and compiler lint flags.
use super::super::{patch::groups as patch, replace::list as replace, source};
use super::gather;
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, normalize, Warnings},
            schema::manifest,
            session::Session,
            spec::Spec,
            storage::{self, slots},
            Step,
        },
    },
};
use std::path::Path;

pub(in super::super::super) struct Complete<'a, 's> {
    pub(in super::super::super) prepared: Prepared<'a, 's>,
    pub(in super::super::super) kinds: super::super::kinds::Ready,
    pub(in super::super::super) contents: Option<super::super::contents::Owned>,
}
pub(in super::super::super) struct Prepared<'a, 's> {
    pub(in super::super::super) features: super::super::summary::Features,
    pub(in super::super::super) identity: Option<super::super::package::Id<'a, 's>>,
    pub(in super::super::super) publish: Option<super::super::publish::Policy>,
    pub(in super::super::super) metadata: Option<super::super::metadata::Metadata>,
    pub(in super::super::super) hints: Option<Hints>,
    pub(in super::super::super) lint_flags: super::super::lints::flags::Flags,
    pub(in super::super::super) ordinary: gather::Collected<'a, 's>,
    pub(in super::super::super) replacements: storage::Buffer<replace::Entry<'a, 'a, 's>>,
    pub(in super::super::super) patches: patch::Map<'a, 's>,
}
pub(in super::super::super) struct Hints {
    pub(in super::super::super) mostly_unused: Option<super::super::value::Owned>,
}
pub(in super::super::super) enum Progress<'a, 'd, 's> {
    Complete(Complete<'a, 's>),
    Compile {
        pending: super::super::kinds::Pending<'a, 'd>,
    },
    Ordinary,
    Replacement,
    Patch,
}

pub(in super::super::super) fn convert<'a, 'd: 'a, 's: 'a, 'image: 's>(
    tail: &'a normalize::tail::Complete<'a, 'd>,
    original: &'a manifest::Original<'d>,
    package: Option<&'a normalize::package::View<'_, 'd>>,
    targets: &super::super::Targets<'_, '_>,
    patches: Option<&'a [normalize::Source<'a, 'd>]>,
    specs: &mut slots::Writer<'a, Spec>,
    packages: &mut super::super::package::Owner<'a, 's>,
    manifest: &'a Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    bindeps: bool,
    json_target_spec: bool,
    lint_context: crate::input::membership::manifest::load::lints::Context,
    profile_context: super::super::profiles::Context<'_>,
    cargo_features: &crate::input::membership::manifest::load::features::Features<'_, '_>,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'd, 's>, replace::Failure>> {
    let ordinary = match gather::convert(
        tail,
        manifest,
        parent,
        sources,
        config,
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
        Ok(gather::Progress::Canonical(_)) => return Ok(Ok(Progress::Ordinary)),
        Ok(gather::Progress::Complete(value)) => value,
    };
    let replacements = match replace::convert(
        original,
        specs,
        manifest,
        parent,
        sources,
        config,
        ordinary.platform(),
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Err(error) => return Ok(Err(error)),
        Ok(replace::Progress::Canonical(_)) => return Ok(Ok(Progress::Replacement)),
        Ok(replace::Progress::Complete(value)) => value,
    };
    let patches = match patch::convert(
        patches,
        manifest,
        parent,
        sources,
        config,
        ordinary.platform(),
        bindeps,
        json_target_spec,
        limits,
        warnings,
        session,
    )? {
        Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
        Ok(patch::Progress::Canonical(_)) => return Ok(Ok(Progress::Patch)),
        Ok(patch::Progress::Complete(value)) => value,
    };
    if let Err(error) = super::sources::check(&ordinary, sources, session)? {
        return Ok(Err(replace::Failure::Conversion(error)));
    }
    let lint_flags = match super::super::lints::convert(
        tail.lints(),
        lint_context.cargo_lints,
        lint_context.nightly_features_allowed,
        warnings,
        session,
    )? {
        Ok(flags) => flags,
        Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
    };
    let hints = match tail.hints() {
        None => None,
        Some(manifest::Value::Hints(hints)) => {
            session.items(1)?;
            Some(Hints {
                mostly_unused: hints
                    .mostly_unused()
                    .map(|value| super::super::value::copy(value, session))
                    .transpose()?,
            })
        }
        _ => return session.fail(crate::input::membership::manifest::Error::State),
    };
    let metadata = package
        .map(|package| super::super::metadata::retain(package, tail.badges(), session))
        .transpose()?;
    if let Some(value) = original.get(manifest::Field::Profile) {
        let manifest::Value::Profiles(profiles) = value else {
            return session.fail(crate::input::membership::manifest::Error::State);
        };
        if let Err(error) = super::super::profiles::validate(
            profiles,
            cargo_features,
            profile_context.unstable,
            profile_context.cargo_version,
            warnings,
            session,
        )? {
            return Ok(Err(replace::Failure::Conversion(error)));
        }
    }
    let publish = match package {
        Some(package) => match super::super::publish::convert(package, session)? {
            Ok(policy) => Some(policy),
            Err(error) => return Ok(Err(replace::Failure::Conversion(error))),
        },
        None => None,
    };
    let identity = package
        .map(|package| {
            super::super::package::normalized(package, packages, parent, sources, session)
        })
        .transpose()?;
    if let Err(error) = super::super::summary::dependencies(ordinary.as_slice(), session)? {
        return Ok(Err(replace::Failure::Conversion(error)));
    }
    // normalize_features preserves the original table unchanged.
    let feature_table = match original.get(manifest::Field::Features) {
        None => None,
        Some(manifest::Value::Features(value)) => Some(value),
        _ => return session.fail(crate::input::membership::manifest::Error::State),
    };
    let features =
        match super::super::summary::features(feature_table, ordinary.as_slice(), session)? {
            Ok(value) => value,
            Err(super::super::summary::Failure::Message(message)) => {
                return Ok(Err(replace::Failure::Conversion(message)))
            }
            Err(super::super::summary::Failure::Missing(missing)) => {
                return Ok(Err(replace::Failure::MissingFeatureDependency(missing)))
            }
        };
    features.warn(warnings, session)?;
    if let Some(package) = package
        && let Err(error) = super::super::run::validate(package, targets, session)?
    {
        return Ok(Err(replace::Failure::Conversion(error)));
    }
    let prepared = Prepared {
        features,
        identity,
        publish,
        metadata,
        hints,
        lint_flags,
        ordinary,
        replacements,
        patches,
    };
    match super::super::kinds::convert(package, json_target_spec, session)? {
        Err(error) => Ok(Err(replace::Failure::Conversion(error))),
        Ok(super::super::kinds::Progress::Ready(kinds)) => {
            let contents = package
                .map(|package| super::super::contents::retain(package, session))
                .transpose()?;
            super::super::finish::warnings(package, original, warnings, session)?;
            if let Err(error) = super::super::finish::gates(
                contents
                    .as_ref()
                    .is_some_and(|contents| contents.teapot.is_some()),
                kinds.default.is_some() || kinds.forced.is_some(),
                cargo_features,
                profile_context.cargo_version,
                session,
            )? {
                return Ok(Err(replace::Failure::FeatureGate(error)));
            }
            Ok(Ok(Progress::Complete(Complete {
                prepared,
                kinds,
                contents,
            })))
        }
        Ok(super::super::kinds::Progress::Pending(pending)) => {
            Ok(Ok(Progress::Compile { pending }))
        }
    }
}
