// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Cargo util/toml/mod.rs:1294–1397, before embedded-field validation.

use super::super::super::{
    schema::{manifest, package::Field, primitive},
    session::Session,
    Error, Step,
};
use super::super::{
    features::{Feature, Features},
    normalize::{library, package, target},
    Warnings,
};
use super::edition;
use crate::input::membership::host;
use std::{os::unix::ffi::OsStrExt, path::Path};

pub(in super::super) enum Failure {
    Edition,
    Semantic(String),
}

/// Borrowed inputs from normalized real-manifest conversion, not a new parser.
pub(in super::super) struct Context<'a, 'd> {
    pub(in super::super) original: &'a manifest::Original<'d>,
    pub(in super::super) package: &'a package::View<'a, 'd>,
    pub(in super::super) root: &'a Path,
    pub(in super::super) features: &'a Features<'a, 'd>,
}

pub(in super::super) fn check(
    context: Context<'_, '_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<edition::Edition, Failure>> {
    session.check()?;
    if !session.is_dir(context.root.as_os_str().as_bytes())? {
        return target::formatted(
            format_args!(
                "package root '{}' is not a directory",
                context.root.display()
            ),
            session,
        )
        .map(|text| Err(Failure::Semantic(text)));
    }
    let Some(name) = context.package.get(Field::Name).and_then(library::text) else {
        return session.fail(Error::State);
    };
    session.items(name.len())?;
    if name.contains(':') {
        context.features.require(Feature::OpenNamespaces, session)?;
    }
    let rust = match context.package.get(Field::RustVersion) {
        Some(package::Value::Original(primitive::Value::Rust(version))) => Some(version),
        None => None,
        _ => return session.fail(Error::State),
    };
    let selected = edition::select(
        context.package.get(Field::Edition).and_then(library::text),
        rust,
        warnings,
        session,
    )?;
    let selected = match selected {
        Ok(edition) => edition,
        Err(_) => return Ok(Err(Failure::Edition)),
    };
    if !selected.is_stable() {
        let version = match context.package.get(Field::Version) {
            Some(package::Value::Original(primitive::Value::Version(version))) => {
                target::formatted(format_args!("@{version}"), session)?
            }
            None => String::new(),
            _ => return session.fail(Error::State),
        };
        let hint = rust
            .map(|rust| {
                target::formatted(
                    format_args!("help: {name}{version} requires rust {rust}"),
                    session,
                )
            })
            .transpose()?;
        context
            .features
            .require_with_hint(Feature::UnstableEditions, hint.as_deref(), session)?;
    }
    if context.original.get(manifest::Field::Project).is_some() {
        if selected.at_least_2024() {
            return target::formatted(
                format_args!(
                    "`[project]` is not supported as of the 2024 Edition, please use `[package]`"
                ),
                session,
            )
            .map(|text| Err(Failure::Semantic(text)));
        }
        warnings.push(
            &["`[project]` is deprecated in favor of `[package]`"],
            session,
        )?;
    }
    if context.package.get(Field::Metabuild).is_some() {
        context.features.require(Feature::Metabuild, session)?;
    }
    Ok(Ok(selected))
}
