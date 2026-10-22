// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Original dependency extras, collected before each dependency's warnings.

use rawr_toml::document;

pub(in super::super) mod all;
pub(in super::super) mod gather;
mod sources;

use super::super::{
    super::{schema::dependency, session::Session, storage, Step},
    normalize::target,
    Warnings,
};

/// Original maps in Cargo's conversion order, after target/link validation.
pub(in super::super) fn original<A: document::Admission>(
    original: &super::super::super::schema::manifest::Original<'_>,
    path: &std::path::Path,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    use super::super::super::schema::{manifest, platform};
    session.check()?;
    for (new, old, label) in [
        (manifest::Field::Dependencies, None, "dependencies"),
        (
            manifest::Field::DevDependencies,
            Some(manifest::Field::DevDependencies2),
            "dev-dependencies",
        ),
        (
            manifest::Field::BuildDependencies,
            Some(manifest::Field::BuildDependencies2),
            "build-dependencies",
        ),
    ] {
        let map = match original
            .get(new)
            .or_else(|| old.and_then(|field| original.get(field)))
        {
            Some(manifest::Value::Dependencies(map)) => Some(map),
            None => None,
            _ => return session.fail(super::super::super::Error::State),
        };
        inspect(map, label, warnings, session)?;
    }
    let platforms = match original.get(manifest::Field::Target) {
        Some(manifest::Value::Platforms(platforms)) => platforms,
        None => return Ok(Ok(())),
        _ => return session.fail(super::super::super::Error::State),
    };
    for entry in platforms.entries() {
        session.items(1)?;
        let parsed = match super::platform::parse(entry.origin().text.as_str(), session)? {
            Ok(parsed) => parsed,
            Err(error) => return Ok(Err(error)),
        };
        super::platform::check_attributes(&parsed, warnings, session)?;
        super::platform::check_keywords(&parsed, path, warnings, session)?;
        for (new, old, label) in [
            (platform::Field::Dependencies, None, "dependencies"),
            (
                platform::Field::BuildDependencies,
                Some(platform::Field::BuildDependencies2),
                "build-dependencies",
            ),
            (
                platform::Field::DevDependencies,
                Some(platform::Field::DevDependencies2),
                "dev-dependencies",
            ),
        ] {
            let map = entry
                .value()
                .get(new)
                .or_else(|| old.and_then(|field| entry.value().get(field)));
            inspect_label(
                map,
                format_args!("target.{parsed}.{label}"),
                warnings,
                session,
            )?;
        }
    }
    Ok(Ok(()))
}

/// The caller supplies the source-shaped table label, including any platform.
/// This does not resolve dependencies or establish complete manifest authority.
pub(in super::super) fn inspect<A: document::Admission>(
    original: Option<&dependency::inheritable::Map<'_>>,
    table: &str,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    inspect_label(original, format_args!("{table}"), warnings, session)
}

fn inspect_label<A: document::Admission>(
    original: Option<&dependency::inheritable::Map<'_>>,
    table: std::fmt::Arguments<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    let Some(original) = original else {
        return Ok(());
    };
    for (_, name, dependency) in original.entries() {
        session.items(1)?;
        // Cargo forms the label even for simple values with no unused keys.
        let table = target::formatted(table, session)?;
        let mut unused = storage::Buffer::new();
        match dependency.kind() {
            dependency::inheritable::Kind::Value(value) => match value.kind() {
                dependency::direct::Kind::Simple(_) => {}
                dependency::direct::Kind::Detailed(value) => {
                    for (key, _) in value.extras() {
                        collect(key.text.as_str(), &mut unused, session)?;
                    }
                }
            },
            dependency::inheritable::Kind::Inherit(value) => {
                for (key, _) in value.extras() {
                    collect(key.text.as_str(), &mut unused, session)?;
                }
            }
        }
        // Cargo evaluates unused_keys() to completion before unused_dep_keys.
        // A refused later key must not expose earlier warnings for this entry.
        for key in unused.into_iter() {
            session.items(1)?;
            warnings.push(
                &[
                    "unused manifest key: ",
                    &table,
                    ".",
                    name.text.as_str(),
                    ".",
                    &key,
                ],
                session,
            )?;
        }
    }
    Ok(())
}

fn collect<A: document::Admission>(
    key: &str,
    unused: &mut storage::Buffer<String>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    let key = target::formatted(format_args!("{key}"), session)?;
    unused.push(key, session)?;
    Ok(())
}

pub(in super::super) fn patch_entry<A: document::Admission>(
    dependency: &super::super::normalize::Dependency<'_, '_>,
    registry: &str,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    let table = target::formatted(format_args!("patch.{registry}"), session)?;
    let mut unused = storage::Buffer::new();
    for (key, _) in dependency.extras() {
        collect(key.text.as_str(), &mut unused, session)?;
    }
    for key in unused.into_iter() {
        session.items(1)?;
        warnings.push(
            &[
                "unused manifest key: ",
                &table,
                ".",
                dependency.name.as_str(),
                ".",
                &key,
            ],
            session,
        )?;
    }
    Ok(())
}

pub(in super::super) fn replacement_entry<A: document::Admission>(
    dependency: &dependency::direct::Direct<'_>,
    name: &str,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    let mut unused = storage::Buffer::new();
    if let dependency::direct::Kind::Detailed(value) = dependency.kind() {
        for (key, _) in value.extras() {
            collect(key.text.as_str(), &mut unused, session)?;
        }
    }
    for key in unused.into_iter() {
        session.items(1)?;
        warnings.push(&["unused manifest key: replace.", name, ".", &key], session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
