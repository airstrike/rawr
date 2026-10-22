// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Cargo's default-run check after Summary feature warnings.
use super::{target::configure::Kind, Targets};
use crate::input::membership::manifest::{
    load::{
        formatted,
        normalize::{library, package, suggest},
    },
    schema::package::Field,
    session::Session,
    Error, Step,
};
use rawr_toml::document;

pub(in super::super) fn validate<A: document::Admission>(
    package: &package::View<'_, '_>,
    targets: &Targets<'_, '_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.items(1)?;
    let Some(value) = package.get(Field::DefaultRun) else {
        return Ok(Ok(()));
    };
    let Some(run) = library::text(value) else {
        return session.fail(Error::State);
    };
    for target in targets.ordinary.as_slice() {
        session.items(1)?;
        if target.kind() == Kind::Bin {
            session.items(run.len())?;
            session.items(target.name().len())?;
            if target.name() == run {
                return Ok(Ok(()));
            }
        }
    }
    session.items(targets.ordinary.len())?;
    let closest = suggest::select(
        run,
        targets
            .ordinary
            .as_slice()
            .iter()
            .filter(|target| target.kind() == Kind::Bin)
            .map(|target| target.name()),
        session,
    )?;
    let help = match closest {
        Some(name) => formatted(
            format_args!("\n\nhelp: a target with a similar name exists: `{name}`"),
            session,
        )?,
        None => String::new(),
    };
    formatted(
        format_args!("default-run target `{run}` not found{help}"),
        session,
    )
    .map(Err)
}
