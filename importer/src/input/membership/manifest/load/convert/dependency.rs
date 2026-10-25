// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Ordered dependency checks before source-ID conversion, not a dependency proof.

mod artifact;
pub(in super::super) use artifact::target as compile;
mod converted;
pub(in super::super) mod input;
pub(in super::super) use converted::{Converted, Pending, Progress};
pub(in super::super) mod record;
mod source;

use rawr_toml::document;

use super::super::{
    super::{
        schema::{dependency::Field, primitive},
        session::Session,
        Error, Step,
    },
    normalize::{dependencies, inherit, target},
    Warnings,
};

/// Convert one normalized declaration, retaining its prefix across native work.
/// This does not grant whole-manifest or current-input authority.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn convert<'a, 'd: 'a, 'p, 'store, 'image: 'store>(
    dependency: impl Into<input::Input<'a, 'd>>,
    manifest: &std::path::Path,
    parent: super::source::Id<'store>,
    sources: &mut super::source::Owner<'store>,
    config: &mut super::super::config::Owner<'store, 'image>,
    kind: dependencies::Kind,
    platform: Option<&super::platform::Platform<'p>>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &crate::cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, crate::input::membership::host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'p, 'store>, String>> {
    let dependency = dependency.into();
    let prefix = match before_artifact(
        dependency, manifest, parent, sources, config, kind, platform, limits, warnings, session,
    )? {
        Ok(prefix) => prefix,
        Err(error) => return Ok(Err(error)),
    };
    let artifact = match artifact::convert(
        dependency.name(),
        |field| dependency.get(field),
        artifact::Context {
            kind,
            bindeps,
            json_target_spec,
        },
        session,
    )? {
        Ok(artifact) => artifact,
        Err(error) => return Ok(Err(error)),
    };
    Ok(Ok(converted::retain(prefix, artifact)))
}

/// Source-ordered retained prefix; artifact conversion still must complete.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn before_artifact<'a, 'd: 'a, 'p, 'store, 'image: 'store>(
    dependency: impl Into<input::Input<'a, 'd>>,
    manifest: &std::path::Path,
    parent: super::source::Id<'store>,
    sources: &mut super::source::Owner<'store>,
    config: &mut super::super::config::Owner<'store, 'image>,
    kind: dependencies::Kind,
    platform: Option<&super::platform::Platform<'p>>,
    limits: &crate::cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, crate::input::membership::host::Host<'image, '_>>,
) -> Step<Result<record::BeforeArtifact<'p, 'store>, String>> {
    let dependency = dependency.into();
    fields_owned(
        dependency.name(),
        dependency.simple(),
        |field| dependency.get(field),
        warnings,
        session,
        |version, warnings, session| {
            let selected = match source::select(
                dependency.name(),
                |field| dependency.get(field),
                manifest,
                parent,
                sources,
                config,
                limits,
                warnings,
                session,
            )? {
                Ok(selected) => selected,
                Err(error) => return Ok(Err(error)),
            };
            record::retain(
                dependency, version, selected, sources, config, kind, platform, limits, session,
            )
        },
    )
    .map(|result| {
        result.map(|(mut record, promoted)| {
            record.promotion(promoted);
            record
        })
    })
}

pub(in super::super) fn check<A: document::Admission>(
    dependency: &dependencies::Dependency<'_, '_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    fields(
        dependency.name(),
        dependency.simple(),
        |field| dependency.get(field),
        warnings,
        session,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn select<'store, 'image: 'store>(
    dependency: &dependencies::Dependency<'_, '_>,
    manifest: &std::path::Path,
    parent: super::source::Id<'store>,
    sources: &mut super::source::Owner<'store>,
    config: &mut super::super::config::Owner<'store, 'image>,
    limits: &crate::cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, crate::input::membership::host::Host<'image, '_>>,
) -> Step<Result<super::source::Id<'store>, String>> {
    fields_then(
        dependency.name(),
        dependency.simple(),
        |field| dependency.get(field),
        warnings,
        session,
        |_version, warnings, session| {
            source::select(
                dependency.name(),
                |field| dependency.get(field),
                manifest,
                parent,
                sources,
                config,
                limits,
                warnings,
                session,
            )
        },
    )
}

fn fields<'a, 'd: 'a, A: document::Admission>(
    name: &str,
    simple: Option<&str>,
    get: impl Fn(Field) -> Option<inherit::Value<'a, 'd>>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    fields_then(name, simple, get, warnings, session, |_, _, _| Ok(Ok(())))
}

fn fields_then<'a, 'd: 'a, R, A: document::Admission>(
    name: &str,
    simple: Option<&str>,
    get: impl Fn(Field) -> Option<inherit::Value<'a, 'd>>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
    after: impl FnOnce(Option<&str>, &mut Warnings, &mut Session<'_, A>) -> Step<Result<R, String>>,
) -> Step<Result<R, String>> {
    fields_owned(name, simple, get, warnings, session, after)
        .map(|result| result.map(|(value, _promoted)| value))
}

fn fields_owned<'a, 'd: 'a, R, A: document::Admission>(
    name: &str,
    simple: Option<&str>,
    get: impl Fn(Field) -> Option<inherit::Value<'a, 'd>>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
    after: impl FnOnce(Option<&str>, &mut Warnings, &mut Session<'_, A>) -> Step<Result<R, String>>,
) -> Step<Result<(R, Option<String>), String>> {
    session.check()?;
    // Cargo promotes a simple string by cloning it into detailed.version.
    let promoted = match simple {
        Some(version) => Some(target::formatted(format_args!("{version}"), session)?),
        None => None,
    };
    let version = match promoted.as_deref() {
        Some(version) => Some(version),
        None => text(get(Field::Version), session)?,
    };
    let git = get(Field::Git).is_some();
    if version.is_none() && get(Field::Path).is_none() && !git {
        return target::formatted(format_args!("dependency ({name}) specified without providing a local path, Git repository, version, or workspace dependency to use"), session).map(Err);
    }
    if let Some(version) = version {
        session.string(version.len())?;
        session.items(1)?;
        if version.contains('+') {
            warnings.push(&["version requirement `", version, "` for dependency `", name, "` includes semver metadata which will be ignored, removing the metadata is recommended to avoid confusion"], session)?;
        }
    }
    if !git {
        for (field, label) in [
            (Field::Branch, "branch"),
            (Field::Tag, "tag"),
            (Field::Rev, "rev"),
        ] {
            session.items(1)?;
            if get(field).is_some() {
                return target::formatted(
                    format_args!("key `{label}` is ignored for dependency ({name})."),
                    session,
                )
                .map(Err);
            }
        }
    }
    match get(Field::Features) {
        None => {}
        Some(inherit::Value::Original(primitive::Value::Strings(values))) => {
            for value in values.as_slice() {
                if let Err(error) = feature(name, value.as_str(), session)? {
                    return Ok(Err(error));
                }
            }
        }
        Some(inherit::Value::Features(values)) => {
            for value in values.iter() {
                if let Err(error) = feature(name, value, session)? {
                    return Ok(Err(error));
                }
            }
        }
        _ => return session.fail(Error::State),
    }
    after(version, warnings, session).map(|result| result.map(|value| (value, promoted)))
}

fn text<'a, 'd, A: document::Admission>(
    value: Option<inherit::Value<'a, 'd>>,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a str>> {
    match value {
        None => Ok(None),
        Some(inherit::Value::Text(text)) => Ok(Some(text)),
        Some(inherit::Value::Original(primitive::Value::String(text))) => Ok(Some(text.as_str())),
        _ => session.fail(Error::State),
    }
}

fn feature<A: document::Admission>(
    name: &str,
    feature: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.string(feature.len())?;
    session.items(1)?;
    if feature.contains('/') {
        return target::formatted(format_args!("feature `{feature}` in dependency `{name}` is not allowed to contain slashes\nIf you want to enable features of a transitive dependency, the direct dependency needs to re-export those features from the `[features]` table."), session).map(Err);
    }
    if feature.starts_with("dep:") {
        return target::formatted(format_args!("feature `{feature}` in dependency `{name}` is not allowed to use explicit `dep:` syntax\nIf you want to enable an optional dependency, specify the name of the optional dependency without the `dep:` prefix, or specify a feature from the dependency's `[features]` table that enables the optional dependency."), session).map(Err);
    }
    Ok(Ok(()))
}

#[cfg(test)]
mod tests;
