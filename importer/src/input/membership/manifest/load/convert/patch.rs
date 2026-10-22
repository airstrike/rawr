// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Patch registry selection, before per-entry warnings and dependency conversion.
pub(in super::super) mod entry;
pub(in super::super) mod groups;
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, normalize::target},
            scalar,
            session::Session,
            spec::url::Url,
            Step, Stop,
        },
    },
};

pub(in super::super) fn registry<'s, 'image: 's>(
    spelling: &str,
    config: &mut config::Owner<'s, 'image>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Url, String>> {
    session.check()?;
    session.string(spelling.len())?;
    session.items(1)?;
    if spelling == "crates-io" {
        return super::url::parse("https://github.com/rust-lang/crates.io-index", session);
    }
    // RegistryName rejects URL spellings before any configuration read.
    if let Ok(name) = scalar::Name::parse(spelling, scalar::Kind::Registry) {
        match config.registry_index(name, limits, session) {
            Ok(Ok(url)) => return Ok(Ok(url)),
            Ok(Err(_)) | Err(Stop::Rejected(_)) => {}
            Err(stop @ Stop::Refused) => return Err(stop),
        }
    }
    let cause = match super::url::parse(spelling, session)? {
        Ok(url) => return Ok(Ok(url)),
        Err(cause) => cause,
    };
    let hint = if spelling == "crates" {
        "\nFor crates.io, use [patch.crates-io] (with a dash)"
    } else {
        ""
    };
    target::formatted(
        format_args!("[patch] entry `{spelling}` should be a URL or registry name{hint}: {cause}"),
        session,
    )
    .map(Err)
}
