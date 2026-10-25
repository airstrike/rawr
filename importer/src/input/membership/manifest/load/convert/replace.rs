// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Cargo's per-replacement conversion, after key parsing and URL defaulting.
use super::{dependency, platform, source};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, normalize, Warnings},
            schema::dependency::{direct, Field},
            session::Session,
            spec::Spec,
            Step,
        },
    },
};
use std::path::Path;
pub(in super::super) mod list;

pub(in super::super) struct InvalidKey {
    pub(in super::super) context: String,
    pub(in super::super) cause: crate::input::membership::manifest::Diagnostic,
}

/// Presence, including an empty table, controls Cargo's exclusion check.
pub(in super::super) fn check_tables<A: rawr_toml::document::Admission>(
    original: &crate::input::membership::manifest::schema::manifest::Original<'_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    use crate::input::membership::manifest::schema::manifest::Field;
    session.items(1)?;
    if original.get(Field::Patch).is_some() && original.get(Field::Replace).is_some() {
        return normalize::target::formatted(
            format_args!("cannot specify both [replace] and [patch]"),
            session,
        )
        .map(Err);
    }
    Ok(Ok(()))
}

/// Preserve Cargo's contextual key error and its underlying typed cause.
pub(in super::super) fn key<A: rawr_toml::document::Admission>(
    spelling: &str,
    span: Option<crate::input::membership::manifest::Span>,
    session: &mut Session<'_, A>,
) -> Step<Result<Spec, InvalidKey>> {
    use crate::input::membership::manifest::Stop;
    let mut spec = match Spec::parse(spelling, span, session) {
        Ok(spec) => spec,
        Err(Stop::Refused) => return Err(Stop::Refused),
        Err(Stop::Rejected(cause)) => {
            let context = normalize::target::formatted(format_args!(
                "replacements must specify a valid semver version to replace, but `{spelling}` does not"
            ), session)?;
            return Ok(Err(InvalidKey { context, cause }));
        }
    };
    spec.default_registry(session)?;
    Ok(Ok(spec))
}

/// The caller owns the parsed/defaulted spec across any pending native work.
/// Replace/patch exclusion and key parsing precede this stage.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn entry<'a, 'd: 'a, 'p, 's, 'image: 's>(
    spec: &'a Spec,
    declaration: &'a direct::Direct<'d>,
    manifest: &Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    platform: Option<&platform::Platform<'p>>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<dependency::Progress<'a, 'p, 's>, String>> {
    session.items(1)?;
    let specified = match declaration.kind() {
        direct::Kind::Simple(_) => true,
        direct::Kind::Detailed(value) => value.get(Field::Version).is_some(),
    };
    if specified {
        let display = spec.display(session)?;
        return normalize::target::formatted(
            format_args!(
                "replacements cannot specify a version requirement, but found one for `{display}`"
            ),
            session,
        )
        .map(Err);
    }
    let converted = match dependency::convert(
        dependency::input::Input::Direct {
            name: spec.name(),
            value: declaration,
        },
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
    let mut converted = match converted {
        dependency::Progress::Canonical(pending) => {
            return Ok(Ok(dependency::Progress::Canonical(pending)))
        }
        dependency::Progress::Complete(value) => value,
    };
    let Some(version) = spec.full_version(session)? else {
        let display = spec.display(session)?;
        return normalize::target::formatted(
            format_args!(
                "replacements must specify a version to replace, but `{display}` does not"
            ),
            session,
        )
        .map(Err);
    };
    let fields = converted.fields();
    let name = fields.rename().unwrap_or(fields.name());
    super::dependencies::replacement_entry(declaration, name, warnings, session)?;
    converted.replace_version(&version, session)?;
    Ok(Ok(dependency::Progress::Complete(converted)))
}
